// Copyright (C) 2026 Steven P. Collins. All rights reserved.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::PatternInfo;
// use crate::block_tree::{BlockIndex, BlockTree}; // Unused imports removed

use crate::scratchpad::{Candidate, Scratchpad};
use crate::space::SimulationSpace;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

pub struct SnapshotStore {
    store: RwLock<HashMap<u64, Arc<Vec<u8>>>>,
}

impl SnapshotStore {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for SnapshotStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotStore {
    pub fn insert(&self, generation: u64, data: Arc<Vec<u8>>) {
        if let Ok(mut lock) = self.store.write() {
            lock.insert(generation, data);

            // Prune old snapshots to prevent unbounded memory growth
            // Keep last 1000 generations (increased from 200 to handle fast-running engines)
            if generation > 1000 {
                lock.remove(&(generation - 1000));
            }
        }
    }

    pub fn get(&self, generation: u64) -> Option<Arc<Vec<u8>>> {
        if let Ok(lock) = self.store.read() {
            lock.get(&generation).cloned()
        } else {
            None
        }
    }

    pub fn get_latest(&self) -> Option<(u64, Arc<Vec<u8>>)> {
        if let Ok(lock) = self.store.read() {
            lock.keys()
                .max()
                .cloned()
                .and_then(|generation| lock.get(&generation).map(|data| (generation, data.clone())))
        } else {
            None
        }
    }
}

pub enum Tasks {
    SpreadBatch(usize, usize),
    CommitBatch(usize, usize),
}

pub enum EngineCommand {
    Start,
    StartGenerations(u64),
    Step,
    Reset,
    Seed(String),
    SeedAndStart(String, u64),
}

pub enum IoTask {
    Snapshot {
        generation: u64,
        cells: Vec<((i128, i128), u8)>,
        telemetry: crate::Telemetry,
    },
}

#[derive(Debug, Clone)]
pub struct SnapshotRecord {
    pub x: i128,
    pub y: i128,
    pub state: u8,
}

pub trait EngineSubscriber: Send + Sync {
    fn on_snapshot_available(
        &self,
        // generation: u64,
        data: Arc<Vec<u8>>,
        telemetry: crate::Telemetry,
    ) -> bool;
}

pub type SimulationEngine = Engine;

// ...

pub struct WorkQueue {
    queue: crossbeam_deque::Injector<Tasks>,
    _stealers: Vec<crossbeam_deque::Stealer<Tasks>>,
    in_flight_count: AtomicUsize,
}

impl WorkQueue {
    pub fn new(size: usize) -> Self {
        let queue = crossbeam_deque::Injector::new();
        let stealers = Vec::with_capacity(size);
        Self {
            queue,
            _stealers: stealers,
            in_flight_count: AtomicUsize::new(0),
        }
    }

    pub fn enqueue(&self, task: Tasks) {
        self.in_flight_count.fetch_add(1, Ordering::SeqCst);
        self.queue.push(task);
    }

    pub fn enqueue_batch(&self, tasks: Vec<Tasks>) {
        self.in_flight_count
            .fetch_add(tasks.len(), Ordering::SeqCst);
        for task in tasks {
            self.queue.push(task);
        }
    }

    pub fn purge(&self) {
        loop {
            if let crossbeam_deque::Steal::Empty = self.queue.steal() {
                break;
            }
        }
        self.in_flight_count.store(0, Ordering::SeqCst);
    }

    pub fn in_flight_count(&self) -> usize {
        self.in_flight_count.load(Ordering::SeqCst)
    }
}

pub struct Telemetry {
    _start_time: std::time::Instant,
    last_tick: std::time::Instant,
    pub gps: f64,
    pub work_rate_ema: f64,
    pub net_rate_ema: f64,
    pub last_work_count: u64,
    pub last_net_count: i64,
    pub last_generation: u64,
}

impl Telemetry {
    pub fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            _start_time: now,
            last_tick: now,
            gps: 0.0,
            work_rate_ema: 0.0,
            net_rate_ema: 0.0,
            last_work_count: 0,
            last_net_count: 0,
            last_generation: 0,
        }
    }

    /// Resets the last tick to the current time.
    /// Used when starting or resuming the engine to avoid measuring idle time in the GPS.
    pub fn reset(&mut self) {
        self.last_tick = std::time::Instant::now();
    }
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Telemetry {
    pub fn update(&mut self, generation: u64, work_count: u64, net_count: i64) {
        let now = std::time::Instant::now();

        // Special Case: Generation 0 is an initialization/reset event.
        // We do not want to report a fictitious GPS based on pattern load time.
        if generation == 0 {
            self.gps = 0.0;
            self.work_rate_ema = 0.0;
            self.net_rate_ema = 0.0;
            self.last_tick = now;
            return;
        }

        let delta = now.duration_since(self.last_tick).as_secs_f64();

        if delta > 0.0 && generation > self.last_generation {
            self.last_generation = generation;
            let cycles = 1.0;
            let instantaneous_gps = cycles / delta;

            // EMA for GPS
            let alpha = 0.1;
            if self.gps == 0.0 {
                self.gps = instantaneous_gps;
            } else {
                self.gps = self.gps * (1.0 - alpha) + instantaneous_gps * alpha;
            }

            // Work and Net are per-generation (reset each step).
            // So the value passed IS the delta.
            let work_delta = work_count;
            let net_delta = net_count;

            let instantaneous_work = work_delta as f64 / delta;
            let instantaneous_net = net_delta as f64 / delta;

            // EMA for Work
            if self.work_rate_ema == 0.0 {
                self.work_rate_ema = instantaneous_work;
            } else {
                self.work_rate_ema =
                    self.work_rate_ema * (1.0 - alpha) + instantaneous_work * alpha;
            }

            // EMA for Net
            if self.net_rate_ema == 0.0 {
                self.net_rate_ema = instantaneous_net;
            } else {
                self.net_rate_ema = self.net_rate_ema * (1.0 - alpha) + instantaneous_net * alpha;
            }

            self.last_work_count = work_count;
            self.last_net_count = net_count;
        } else {
            // Delta is 0 (too fast?), skip update or assume instant?
        }
        self.last_tick = now;
    }
}

pub struct Engine {
    pub space: Arc<SimulationSpace>,
    pub work_queue: Arc<WorkQueue>,
    pub in_flight_count: AtomicUsize,
    pub stop_signal: Arc<AtomicBool>,
    pub generation: AtomicU64,
    pub target_generation: AtomicU64,
    pub living_count: AtomicU64,
    pub work: AtomicU64,
    pub net: AtomicI64,
    pub dead_block_count: AtomicU64, // Metric for pruning trigger
    pub scratchpad: Scratchpad,
    pub pool_size: usize,
    pub stopping: AtomicBool,
    pub tainted: AtomicBool,
    pub subscribers: Mutex<Vec<Arc<dyn EngineSubscriber>>>,
    pub record_buffers: Vec<Vec<Mutex<Vec<SnapshotRecord>>>>,
    pub epoch: AtomicU64,
    pub telemetry: Mutex<Telemetry>,
    #[allow(clippy::type_complexity)]
    pub current_generation_bounds: Mutex<Option<((i128, i128), (i128, i128))>>,
    pub transition_lock: Mutex<()>,

    // Synchronization for phases
    pub phase_counter: AtomicUsize,

    // Thread-local buffers for commit phase to avoid reallocation
    pub commit_buffers: Vec<crate::scratchpad::CachePadded<UnsafeCell<CommitBuffer>>>,

    pub snapshots: SnapshotStore,
    pub patterns: Mutex<Vec<PatternInfo>>,
    pub active_pattern: Mutex<Option<String>>,
    pub io_tx: std::sync::mpsc::SyncSender<IoTask>,
    pub io_pool_rx: crossbeam_channel::Receiver<Vec<((i128, i128), u8)>>,

    // Command Serialization
    pub command_tx: crossbeam_channel::Sender<EngineCommand>,
    pub command_rx: crossbeam_channel::Receiver<EngineCommand>,
}

pub struct CommitBuffer {
    pub incoming: Vec<Candidate>,
    pub coords: Vec<(i128, i128)>, // Kept for legacy compatibility if needed
}

unsafe impl Sync for Engine {}

impl Engine {
    pub fn new(space: Arc<SimulationSpace>, pool_size: usize) -> Arc<Self> {
        let bucket_count = space.storage().buckets.len();
        let buffer_count = 2; // Double buffered recording
        let mut record_buffers = Vec::with_capacity(buffer_count);
        for _ in 0..buffer_count {
            let mut bucket_buffers = Vec::with_capacity(bucket_count);
            for _ in 0..bucket_count {
                bucket_buffers.push(Mutex::new(Vec::with_capacity(1024)));
            }
            record_buffers.push(bucket_buffers);
        }

        // Initialize commit buffers
        let mut commit_buffers = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            commit_buffers.push(crate::scratchpad::CachePadded::new(UnsafeCell::new(
                CommitBuffer {
                    incoming: Vec::with_capacity(4096),
                    coords: Vec::with_capacity(1024),
                },
            )));
        }

        let initial_pop = space.total_population();

        let (io_tx, io_rx) = std::sync::mpsc::sync_channel(2);
        let (io_pool_tx, io_pool_rx) = crossbeam_channel::unbounded();
        let (command_tx, command_rx) = crossbeam_channel::unbounded();

        let engine = Arc::new(Self {
            space,
            work_queue: Arc::new(WorkQueue::new(pool_size)),
            in_flight_count: AtomicUsize::new(0),
            stop_signal: Arc::new(AtomicBool::new(false)),
            generation: AtomicU64::new(0),
            target_generation: AtomicU64::new(u64::MAX),
            living_count: AtomicU64::new(initial_pop),
            work: AtomicU64::new(0),
            net: AtomicI64::new(0), // AtomicI64
            dead_block_count: AtomicU64::new(0),
            scratchpad: Scratchpad::new(pool_size, bucket_count),
            pool_size,
            stopping: AtomicBool::new(true),
            tainted: AtomicBool::new(false),
            subscribers: Mutex::new(Vec::new()),
            record_buffers,
            epoch: AtomicU64::new(0),
            telemetry: Mutex::new(Telemetry::new()),
            phase_counter: AtomicUsize::new(0),
            commit_buffers,
            snapshots: SnapshotStore::new(),
            patterns: Mutex::new(Vec::new()),
            active_pattern: Mutex::new(None),
            current_generation_bounds: Mutex::new(None),
            transition_lock: Mutex::new(()),
            io_tx,
            io_pool_rx,
            command_tx,
            command_rx,
        });

        for i in 0..pool_size {
            let engine_clone = engine.clone();
            std::thread::Builder::new()
                .name(format!("Worker-{}", i))
                .spawn(move || {
                    Self::run_worker(engine_clone, i);
                })
                .expect("Failed to spawn engine worker thread");
        }

        let io_engine_clone = engine.clone();
        std::thread::Builder::new()
            .name("Worker-IO".to_string())
            .spawn(move || {
                Self::run_io_worker(io_engine_clone, io_rx, io_pool_tx);
            })
            .expect("Failed to spawn IO thread");

        engine
    }

    pub fn add_subscriber(&self, subscriber: Arc<dyn EngineSubscriber>) {
        self.subscribers.lock().unwrap().push(subscriber);
    }

    pub fn notify_subscribers(&self, packet: Arc<Vec<u8>>, telemetry: crate::Telemetry) {
        let mut subscribers = self.subscribers.lock().unwrap();
        subscribers.retain(|sub| sub.on_snapshot_available(packet.clone(), telemetry));
    }

    // Legacy methods usually expected by main.rs / tests
    pub fn start(&self) {
        let _guard = self.transition_lock.lock().unwrap();
        if self.stopping.load(Ordering::SeqCst) {
            self.stopping.store(false, Ordering::SeqCst);
            let _ = self.command_tx.send(EngineCommand::Start);
            // We can't use Arc<Self> here easily, but process_commands takes it.
            // However, we can use a temporary trick or just let the worker pick it up
            // since Start initiates background work anyway.
            // For Start, async is fine.
        }
    }
    pub fn stop(&self) {
        // Immediate set without lock to ensure responsiveness
        self.stopping.store(true, Ordering::SeqCst);
    }
    pub fn step(&self) {
        let _guard = self.transition_lock.lock().unwrap();
        if self.stopping.load(Ordering::SeqCst) {
            self.stopping.store(false, Ordering::SeqCst);
            let _ = self.command_tx.send(EngineCommand::Step);
            // Step initiates work, workers will pick it up.
        }
    }
    pub fn reset(&self) {
        let _guard = self.transition_lock.lock().unwrap();
        if self.stopping.load(Ordering::SeqCst) {
            let _ = self.command_tx.send(EngineCommand::Reset);
            self.drain_command_queue_locked();
        }
    }
    pub fn seed(&self, pattern: String) {
        let _guard = self.transition_lock.lock().unwrap();
        if self.stopping.load(Ordering::SeqCst) {
            let _ = self.command_tx.send(EngineCommand::Seed(pattern));
            self.drain_command_queue_locked();
        }
    }
    pub fn start_generations(&self, count: u64) {
        let _guard = self.transition_lock.lock().unwrap();
        if self.stopping.load(Ordering::SeqCst) {
            self.stopping.store(false, Ordering::SeqCst);
            let _ = self.command_tx.send(EngineCommand::StartGenerations(count));
        }
    }
    pub fn seed_and_start(&self, pattern: String, target_gen: Option<u64>) {
        let _guard = self.transition_lock.lock().unwrap();
        if self.stopping.load(Ordering::SeqCst) {
            let target = target_gen.unwrap_or(u64::MAX);
            self.stopping.store(false, Ordering::SeqCst);
            let _ = self
                .command_tx
                .send(EngineCommand::SeedAndStart(pattern, target));
        }
    }

    /// Update the simulation space with a new pattern synchronously.
    ///
    /// This is a convenience method for tests to ensure the pattern is
    /// loaded and `living_count` is updated before proceeding.
    pub fn seed_sync(&self, x: i128, y: i128, pattern: String) {
        self.space.clear();
        self.space.seed_from_rle(x, y, &pattern);
        let pop = self.space.total_population();
        self.living_count.store(pop, Ordering::SeqCst);
    }

    /// Place a single alive cell at `(x, y)` and increment `living_count`.
    ///
    /// This is the proper engine-level API for inserting individual cells.
    /// Unlike calling `storage().insert()` directly, this keeps `living_count`
    /// in sync so `capture_state` reports accurate population figures.
    ///
    /// Must only be called while the engine is stopped.
    pub fn place_cell(&self, x: i128, y: i128) {
        let mask = {
            let guard = self.space.read();
            guard.current_state_mask()
        };
        self.space.storage().insert(crate::cell::Cell::new(
            x,
            y,
            crate::cell::CellState::Alive,
            mask,
        ));
        self.living_count.fetch_add(1, Ordering::SeqCst);
    }
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    pub fn get_catalog(&self) -> Vec<PatternInfo> {
        self.patterns.lock().unwrap().clone()
    }

    pub fn set_target_generation(&self, target: u64) {
        self.target_generation.store(target, Ordering::SeqCst);
    }

    pub fn register_pattern(&self, pattern: crate::PatternInfo) {
        if let Ok(mut lock) = self.patterns.lock() {
            lock.push(pattern);
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
            && self.work_queue.in_flight_count() == 0
            && self.command_rx.is_empty()
    }

    pub fn shutdown(&self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }

    pub fn abort(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.work_queue.purge();
    }

    pub fn mark_tainted(&self) {
        self.tainted.store(true, Ordering::SeqCst);
    }

    pub fn work_queue_in_flight(&self) -> usize {
        self.work_queue.in_flight_count()
    }

    pub fn wait_for_quiescence(&self, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if self.work_queue_in_flight() == 0 && self.is_stopped() {
                // Check once more after a tiny sleep to avoid transient 0s
                std::thread::sleep(std::time::Duration::from_millis(1));
                if self.work_queue_in_flight() == 0 {
                    return true;
                }
            }
            std::thread::yield_now();
        }
        false
    }

    pub fn get_cells_in_rect(
        &self,
        start: (i128, i128),
        end: (i128, i128),
    ) -> Vec<((i128, i128), u8)> {
        let guard = self.space.mask.read();
        let mut out = Vec::new();
        self.space.storage().collect_in_rect(
            start,
            end,
            guard.current_state_mask(),
            guard.last_state_mask(),
            guard.next_state_mask(),
            &mut out,
        );
        out
    }

    pub fn run(engine: Arc<Self>) {
        Self::run_worker(engine, 0);
    }

    pub fn run_worker(engine: Arc<Self>, thread_idx: usize) {
        let local_queue = crossbeam_deque::Worker::new_fifo();
        loop {
            let task = local_queue.pop().or_else(|| {
                std::iter::repeat_with(|| {
                    engine
                        .work_queue
                        .queue
                        .steal_batch_and_pop(&local_queue)
                        .or_else(|| engine.work_queue.queue.steal())
                })
                .find(|s| !s.is_retry())
                .and_then(|s| s.success())
            });

            if let Some(task) = task {
                Self::process_task(&engine, task, thread_idx);
                engine
                    .work_queue
                    .in_flight_count
                    .fetch_sub(1, Ordering::SeqCst);
            } else {
                if engine.stop_signal.load(Ordering::Relaxed) {
                    break;
                }

                // If no work, check for commands if we are the "designated" worker
                // or just if there's no tasks in flight anywhere
                if engine.work_queue_in_flight() == 0 {
                    engine.process_commands();
                }

                std::thread::yield_now();
            }
        }
    }

    fn process_task(engine: &Arc<Self>, task: Tasks, thread_idx: usize) {
        match task {
            Tasks::SpreadBatch(start, end) => {
                Self::spread_bucket(start, end, engine, thread_idx);
            }
            Tasks::CommitBatch(start, end) => {
                Self::commit_bucket(start, end, engine, thread_idx);
            }
        }
    }

    pub fn process_commands(&self) {
        if self.command_rx.is_empty() {
            return;
        }

        if let Ok(_guard) = self.transition_lock.try_lock() {
            self.drain_command_queue_locked();
        }
    }

    fn drain_command_queue_locked(&self) {
        while let Ok(cmd) = self.command_rx.try_recv() {
            let in_flight = self.work_queue_in_flight();

            match cmd {
                EngineCommand::Start | EngineCommand::StartGenerations(_) | EngineCommand::Step => {
                    if self.stopping.load(Ordering::SeqCst) {
                        continue;
                    }

                    match cmd {
                        EngineCommand::StartGenerations(count) => {
                            let current = self.generation.load(Ordering::SeqCst);
                            self.target_generation
                                .store(current + count, Ordering::SeqCst);
                        }
                        EngineCommand::Step => {
                            let current = self.generation.load(Ordering::SeqCst);
                            self.target_generation.store(current + 1, Ordering::SeqCst);
                        }
                        EngineCommand::Start => {
                            self.target_generation.store(u64::MAX, Ordering::SeqCst);
                        }
                        _ => {}
                    }

                    if in_flight == 0 {
                        self.initiate_spread();
                    }
                }
                EngineCommand::Reset => {
                    if !self.stopping.load(Ordering::SeqCst) {
                        continue;
                    }
                    self.space.clear();
                    self.space.mask.reset();
                    self.generation.store(0, Ordering::SeqCst);
                    let maybe_rle = self.active_pattern.lock().unwrap().clone();
                    if let Some(rle) = maybe_rle {
                        self.space.seed_from_rle(0, 0, &rle);
                    }
                    let pop = self.space.total_population();
                    self.living_count.store(pop, Ordering::SeqCst);
                    self.stopping.store(true, Ordering::SeqCst);
                    *self.current_generation_bounds.lock().unwrap() = self.space.bounds();
                    self.capture_state(false);
                }
                EngineCommand::Seed(pattern_input) => {
                    if !self.stopping.load(Ordering::SeqCst) {
                        continue;
                    }
                    self.space.clear();
                    self.space.mask.reset();
                    self.generation.store(0, Ordering::SeqCst);
                    let rle = {
                        let patterns = self.patterns.lock().unwrap();
                        patterns
                            .iter()
                            .find(|p| p.name == pattern_input)
                            .map(|p| p.rle.clone())
                            .unwrap_or(pattern_input)
                    };
                    *self.active_pattern.lock().unwrap() = Some(rle.clone());
                    self.space.seed_from_rle(0, 0, &rle);
                    let pop = self.space.total_population();
                    self.living_count.store(pop, Ordering::SeqCst);
                    self.stopping.store(true, Ordering::SeqCst);
                    *self.current_generation_bounds.lock().unwrap() = self.space.bounds();
                    self.capture_state(false);
                }
                EngineCommand::SeedAndStart(pattern_input, target_gen) => {
                    if self.stopping.load(Ordering::SeqCst) {
                        continue;
                    }
                    self.space.clear();
                    self.space.mask.reset();
                    self.generation.store(0, Ordering::SeqCst);
                    let rle = {
                        let patterns = self.patterns.lock().unwrap();
                        patterns
                            .iter()
                            .find(|p| p.name == pattern_input)
                            .map(|p| p.rle.clone())
                            .unwrap_or(pattern_input)
                    };
                    *self.active_pattern.lock().unwrap() = Some(rle.clone());
                    self.space.seed_from_rle(0, 0, &rle);
                    let pop = self.space.total_population();
                    self.living_count.store(pop, Ordering::SeqCst);
                    self.target_generation.store(target_gen, Ordering::SeqCst);
                    self.stopping.store(false, Ordering::SeqCst);
                    *self.current_generation_bounds.lock().unwrap() = self.space.bounds();
                    self.telemetry.lock().unwrap().reset();
                    // Emit a Gen-0 snapshot so subscribers see the initial state
                    // before the first generation completes.
                    self.capture_state(true);
                    self.initiate_spread();
                }
            }
        }
    }

    fn initiate_spread(&self) {
        if self.stopping.load(Ordering::SeqCst) {
            return;
        }

        let bucket_count = self.space.storage().buckets.len();
        let total_batches = std::cmp::max(1, self.pool_size * 4);
        let batch_size = bucket_count.div_ceil(total_batches);

        let mut tasks = Vec::new();
        for i in (0..bucket_count).step_by(batch_size) {
            let end = std::cmp::min(i + batch_size, bucket_count);
            tasks.push(Tasks::SpreadBatch(i, end));
        }

        // Initialize phase counter before enqueuing
        self.scratchpad.clear(self.generation());
        // Reset per-step metrics
        self.work.store(0, Ordering::SeqCst);

        self.phase_counter.store(tasks.len(), Ordering::SeqCst);
        self.work_queue.enqueue_batch(tasks);
    }

    fn spread_bucket(
        bucket_start: usize,
        bucket_end: usize,
        engine: &Arc<Self>,
        thread_idx: usize,
    ) {
        let storage = engine.space.storage();
        let masks = engine.space.read();
        let current_mask = masks.current_state_mask();
        let read_idx = match current_mask {
            1 => 0,
            2 => 1,
            4 => 2,
            _ => 0,
        };

        const COL_0: u64 = 0x0101010101010101;
        const COL_7: u64 = 0x8080808080808080;

        for bucket_idx in bucket_start..bucket_end {
            if bucket_idx >= storage.buckets.len() {
                break;
            }
            let bucket = storage.buckets[bucket_idx]
                .read()
                .unwrap_or_else(|e| e.into_inner());

            for node in &bucket.arena.nodes {
                let bx = node.bx;
                let by = node.by;
                let block = &node.block;

                if block.boards[read_idx] != 0 {
                    let center = block.boards[read_idx];

                    // North Edge (Row 0) -> Send to N (bx, by-1)
                    if (center & 0xFF) != 0 {
                        engine.scratchpad.push_candidate(
                            thread_idx,
                            bx,
                            by - 1,
                            (center & 0xFF) << 56,
                            1,
                        ); // 1 = From South
                    }

                    // South Edge (Row 7) -> Send to S (bx, by+1)
                    if ((center >> 56) & 0xFF) != 0 {
                        engine.scratchpad.push_candidate(
                            thread_idx,
                            bx,
                            by + 1,
                            (center >> 56) & 0xFF,
                            0,
                        ); // 0 = From North
                    }

                    // West Edge (Col 0) -> Send to W (bx-1, by)
                    // Receiver needs it shifted to Col 7 (0x80)
                    if (center & COL_0) != 0 {
                        engine.scratchpad.push_candidate(
                            thread_idx,
                            bx - 1,
                            by,
                            (center & COL_0) << 7,
                            3,
                        ); // 3 = From East
                    }

                    // East Edge (Col 7) -> Send to E (bx+1, by)
                    // Receiver needs it shifted to Col 0 (0x01)
                    if (center & COL_7) != 0 {
                        engine.scratchpad.push_candidate(
                            thread_idx,
                            bx + 1,
                            by,
                            (center & COL_7) >> 7,
                            2,
                        ); // 2 = From West
                    }

                    // Corners
                    // NW (0,0) -> Send to NW (bx-1, by-1)
                    // Receiver needs SE (Bit 63). 0->63. << 63.
                    if (center & 1) != 0 {
                        engine
                            .scratchpad
                            .push_candidate(thread_idx, bx - 1, by - 1, 1 << 63, 7); // 7 = SE from NW
                    }
                    // NE (7,0) -> Send to NE (bx+1, by-1)
                    // Receiver needs SW (Bit 56). 7->56. << 49.
                    if (center & 128) != 0 {
                        engine
                            .scratchpad
                            .push_candidate(thread_idx, bx + 1, by - 1, 1u64 << 56, 6); // 6 = SW from NE
                    }
                    // SW (0,7) -> Send to SW (bx-1, by+1)
                    // Receiver needs NE (Bit 7). 56->7. >> 49.
                    if (center & (1 << 56)) != 0 {
                        engine
                            .scratchpad
                            .push_candidate(thread_idx, bx - 1, by + 1, 1u64 << 7, 5); // 5 = NE from SW
                    }
                    // SE (7,7) -> Send to SE (bx+1, by+1)
                    // Receiver needs NW (Bit 0). 63->0. >> 63.
                    if (center & (1 << 63)) != 0 {
                        engine
                            .scratchpad
                            .push_candidate(thread_idx, bx + 1, by + 1, 1, 4); // 4 = NW from SE
                    }
                }
            }
        }

        // Transition Logic safe against races
        let prev = engine.phase_counter.fetch_sub(1, Ordering::SeqCst);
        if prev == 1 {
            engine.initiate_commit();
        }
    }

    fn initiate_commit(&self) {
        // Reset counters for the new generation calculation
        self.net.store(0, Ordering::SeqCst);
        self.dead_block_count.store(0, Ordering::SeqCst);
        *self.current_generation_bounds.lock().unwrap() = None;

        let bucket_count = self.space.storage().buckets.len();
        let total_batches = std::cmp::max(1, self.pool_size * 4);
        let batch_size = bucket_count.div_ceil(total_batches);

        let mut tasks = Vec::new();
        for i in (0..bucket_count).step_by(batch_size) {
            let end = std::cmp::min(i + batch_size, bucket_count);
            tasks.push(Tasks::CommitBatch(i, end));
        }

        self.phase_counter.store(tasks.len(), Ordering::SeqCst);
        self.work_queue.enqueue_batch(tasks);
    }

    fn commit_bucket(
        bucket_start: usize,
        bucket_end: usize,
        engine: &Arc<Self>,
        thread_idx: usize,
    ) {
        let storage = engine.space.storage();
        let (current_idx, next_idx) = {
            let masks = engine.space.read();
            let current_mask = masks.current_state_mask();
            let next_mask = masks.next_state_mask();

            let current_idx = match current_mask {
                1 => 0,
                2 => 1,
                4 => 2,
                _ => 0,
            };
            let next_idx = match next_mask {
                1 => 0,
                2 => 1,
                4 => 2,
                _ => 0,
            };
            (current_idx, next_idx)
        };

        #[derive(Default, Clone, Copy)]
        struct Neighbors {
            n: u64,
            s: u64,
            e: u64,
            w: u64,
            nw: u64,
            ne: u64,
            sw: u64,
            se: u64,
        }

        let buffer = unsafe { &mut *engine.commit_buffers[thread_idx].value.get() };
        let mut local_bounds: Option<((i128, i128), (i128, i128))> = None;

        let mut total_work = 0;
        let mut total_dead = 0;
        let mut total_born = 0;
        let mut total_died = 0;

        let generation = engine.generation();
        let should_shrink = (generation as usize % engine.pool_size) == thread_idx;

        for bucket_idx in bucket_start..bucket_end {
            if bucket_idx >= storage.buckets.len() {
                break;
            }

            if should_shrink && buffer.incoming.capacity() > 16384 && buffer.incoming.len() < 4096 {
                buffer.incoming.shrink_to_fit();
            }
            buffer.incoming.clear();
            engine
                .scratchpad
                .get_column_into(bucket_idx, &mut buffer.incoming);

            buffer.incoming.sort_unstable_by(|a, b| {
                if a.y == b.y {
                    a.x.cmp(&b.x)
                } else {
                    a.y.cmp(&b.y)
                }
            });

            let mut bucket = storage.buckets[bucket_idx]
                .write()
                .unwrap_or_else(|e| e.into_inner());

            let mut node_neighbors = Vec::with_capacity(bucket.arena.nodes.len());
            node_neighbors.resize(bucket.arena.nodes.len(), Neighbors::default());

            let mut current_pos = (i128::MIN, i128::MIN);
            let mut msg_target_idx: Option<usize> = None;

            for msg in &buffer.incoming {
                if (msg.x, msg.y) != current_pos {
                    current_pos = (msg.x, msg.y);
                    let idx = bucket.ensure_block(msg.x, msg.y) as usize;
                    if idx >= node_neighbors.len() {
                        node_neighbors.resize(idx + 1, Neighbors::default());
                    }
                    msg_target_idx = Some(idx);
                }

                if let Some(idx) = msg_target_idx {
                    let entry = &mut node_neighbors[idx];
                    match msg.mask {
                        0 => entry.n |= msg.payload,
                        1 => entry.s |= msg.payload,
                        2 => entry.w |= msg.payload,
                        3 => entry.e |= msg.payload,
                        4 => entry.nw |= msg.payload,
                        5 => entry.ne |= msg.payload,
                        6 => entry.sw |= msg.payload,
                        7 => entry.se |= msg.payload,
                        _ => {}
                    }
                }
            }

            for (idx, node) in bucket.arena.nodes.iter_mut().enumerate() {
                let neighbors = if idx < node_neighbors.len() {
                    node_neighbors[idx]
                } else {
                    Neighbors::default()
                };

                // Correctly pass corner data and Capture Metrics
                let (_pop, born, died, work, is_dead) = node.block.step(
                    neighbors.n,
                    neighbors.s,
                    neighbors.w,
                    neighbors.e,
                    neighbors.nw,
                    neighbors.ne,
                    neighbors.sw,
                    neighbors.se,
                    current_idx,
                    next_idx,
                );

                if let Some(((bx1, by1), (bx2, by2))) = node.block.exact_bounds_in_state(next_idx) {
                    let world_x1 = (node.bx << 3) + bx1;
                    let world_y1 = (node.by << 3) + by1;
                    let world_x2 = (node.bx << 3) + bx2;
                    let world_y2 = (node.by << 3) + by2;

                    if let Some(((lx1, ly1), (lx2, ly2))) = local_bounds {
                        local_bounds = Some((
                            (lx1.min(world_x1), ly1.min(world_y1)),
                            (lx2.max(world_x2), ly2.max(world_y2)),
                        ));
                    } else {
                        local_bounds = Some(((world_x1, world_y1), (world_x2, world_y2)));
                    }
                }

                total_born += born as u64;
                total_died += died as u64;
                total_work += work as u64;
                if is_dead {
                    total_dead += 1;
                }
            }
        }

        // Atomically update metrics
        // Living Count: Incremental update (Safe against drift if logic is correct)
        engine.living_count.fetch_add(total_born, Ordering::SeqCst);
        engine.living_count.fetch_sub(total_died, Ordering::SeqCst);

        // NET: Born - Died (Delta for this step)
        let net_change = (total_born as i64) - (total_died as i64);
        engine.net.fetch_add(net_change, Ordering::SeqCst);

        engine.work.fetch_add(total_work, Ordering::SeqCst);
        engine
            .dead_block_count
            .fetch_add(total_dead, Ordering::SeqCst);

        // Merge local bounds into global snapshot
        if let Some(((l_x1, l_y1), (l_x2, l_y2))) = local_bounds {
            let mut g_bounds = engine.current_generation_bounds.lock().unwrap();
            if let Some(((g_x1, g_y1), (g_x2, g_y2))) = *g_bounds {
                *g_bounds = Some((
                    (g_x1.min(l_x1), g_y1.min(l_y1)),
                    (g_x2.max(l_x2), g_y2.max(l_y2)),
                ));
            } else {
                *g_bounds = Some(((l_x1, l_y1), (l_x2, l_y2)));
            }
        }

        let prev = engine.phase_counter.fetch_sub(1, Ordering::SeqCst);
        if prev == 1 {
            // Generation is complete
            let curr_gen = engine.generation.load(Ordering::SeqCst);
            let target = engine.target_generation.load(Ordering::SeqCst);
            if curr_gen + 1 >= target {
                engine.stopping.store(true, Ordering::SeqCst);
            }

            // Advance generation to commit the data we just calculated
            engine.space.advance_generation();
            engine.generation.fetch_add(1, Ordering::SeqCst);

            let stopping_at_capture = engine.stopping.load(Ordering::SeqCst);
            engine.capture_state(!stopping_at_capture);

            // Process commands (Reset, Seed, etc)
            engine.process_commands();

            if !engine.stopping.load(Ordering::SeqCst) {
                engine.initiate_spread();
            } else if !stopping_at_capture {
                // If we said we were running at line 1082 but now we are stopping
                // (due to a command or target gen reach), emit the actual stopped state.
                engine.capture_state(false);
            }
        }
    }

    fn capture_state(&self, is_running: bool) {
        let generation = self.generation.load(Ordering::SeqCst);
        let work = self.work.load(Ordering::SeqCst); // Accumulator
        let net = self.net.load(Ordering::SeqCst);

        // Check for Pruning Trigger
        let dead_blocks = self.dead_block_count.load(Ordering::SeqCst);
        if dead_blocks > 1000 {
            // Prune dead blocks to maintain performance
            self.space.prune();
            // Reset dead block count
            self.dead_block_count.store(0, Ordering::SeqCst);
        }

        // Update Telemetry
        self.telemetry.lock().unwrap().update(generation, work, net);

        // Extract cells using Memory Pooling
        let mut vec = self.io_pool_rx.try_recv().unwrap_or_else(|_| Vec::new());
        let living_count = self.living_count.load(Ordering::SeqCst) as usize;

        // Ensure capacity with 25% headroom if reallocation is needed
        if vec.capacity() < living_count {
            let required_cap = living_count.saturating_add(living_count / 4);
            // capacity is smaller than length needed. subtract length just to be safe for reserve API
            let reserve_amount = required_cap.saturating_sub(vec.len());
            vec.reserve(reserve_amount);
        }

        self.space.collect_all_states_into(&mut vec);

        // Capture telemetry synchronously
        let (gps, work_rate, net_rate) = {
            let t = self.telemetry.lock().unwrap();
            (t.gps, t.work_rate_ema, t.net_rate_ema)
        };
        let bounds = *self.current_generation_bounds.lock().unwrap();
        let telemetry = crate::Telemetry {
            generation,
            population: living_count as u64,
            is_running,
            gps,
            work_rate,
            net_rate,
            bounds: crate::Telemetry::to_cartesian_bounds(bounds),
        };

        // Fire to async I/O worker
        let _ = self.io_tx.send(IoTask::Snapshot {
            generation,
            cells: vec,
            telemetry,
        });
    }

    fn run_io_worker(
        engine: Arc<Self>,
        io_rx: std::sync::mpsc::Receiver<IoTask>,
        io_pool_tx: crossbeam_channel::Sender<Vec<((i128, i128), u8)>>,
    ) {
        while let Ok(IoTask::Snapshot {
            generation,
            mut cells,
            telemetry,
        }) = io_rx.recv()
        {
            // Serialize
            let packet_data = crate::encode_binary_packet(generation, &cells);
            let packet = Arc::new(packet_data);

            // Store
            engine.snapshots.insert(generation, packet.clone());

            // Notify
            engine.notify_subscribers(packet, telemetry);

            // Recycle memory block
            cells.clear();
            let _ = io_pool_tx.send(cells);
        }
    }
}
