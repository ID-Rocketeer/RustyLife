use crate::cell::{Cell, CellState};
use crate::scratchpad::{CachePadded, Candidate, Scratchpad};
use crate::space::SimulationSpace;
use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;

/// Internal tasks executed by the engine's worker thread pool.
#[derive(Debug)]
pub enum Tasks {
    /// Initial signal to start the simulation loop.
    Start,
    /// Start the simulation loop and run for N generations.
    StartGenerations(u64),
    /// Stop the simulation loop after the current generation.
    Stop,
    /// Execute a single generation step.
    Step,
    /// Shutdown the worker thread.
    Quit,

    /// Unified 2-Pass Architecture:
    /// 1. Spread: Identify living cells and notify their 8 neighbors.
    SpreadBatch(usize, usize),
    /// 2. Commit: Create nodes, apply counts, and update states in bucket trees.
    CommitBatch(usize, usize),

    /// Reset the simulation state.
    Reset,
    /// Seed the simulation with a named pattern.
    Seed(String),
    /// Seed the simulation with a named pattern and immediately start.
    SeedAndStart(String, Option<u64>),
}

/// Tasks dedicated to the background I/O thread.
#[derive(Debug, Clone)]
pub struct SnapshotRecord {
    pub x: i128,
    pub y: i128,
    pub state: u8,
}

/// Thread-local buffer for commit operations.
/// Owned exclusively by the worker thread with the corresponding index.
pub struct CommitBuffer {
    pub incoming: Vec<Candidate>,
    pub coords: Vec<(i128, i128)>,
}

impl Default for CommitBuffer {
    fn default() -> Self {
        Self {
            incoming: Vec::with_capacity(1024),
            coords: Vec::with_capacity(1024),
        }
    }
}

pub struct PaddedBuffer {
    pub inner: CachePadded<UnsafeCell<CommitBuffer>>,
}

// SAFETY: access is guarded by thread_idx in worker loop
unsafe impl Sync for PaddedBuffer {}

pub enum IoTask {
    /// Generate a binary snapshot and notifies subscribers.
    Snapshot {
        generation: u64,
        living_count: u64,
        is_running: bool,
        /// Captured records grouped by bucket.
        buckets: Vec<Vec<SnapshotRecord>>,
        /// The index of the record buffer this snapshot came from.
        buffer_idx: usize,
        /// Generation Session ID to prevent ghost snapshots.
        epoch: u64,
        gps: f64,
        work_rate: f64,
        net_rate: f64,
    },
    /// Return the used bucket vectors back to the engine's pool.
    ReturnBuffers {
        buckets: Vec<Vec<SnapshotRecord>>,
        buffer_idx: usize,
    },
    /// Shutdown the I/O thread.
    Quit,
}

impl Tasks {
    pub fn should_purge(&self) -> bool {
        match self {
            Tasks::Start
            | Tasks::StartGenerations(_)
            | Tasks::Step
            | Tasks::SpreadBatch(_, _)
            | Tasks::CommitBatch(_, _) => true,
            Tasks::Stop
            | Tasks::Quit
            | Tasks::Reset
            | Tasks::Seed(_)
            | Tasks::SeedAndStart(_, _) => false,
        }
    }
}

pub struct WorkQueue {
    queue: Mutex<VecDeque<Tasks>>,
    condvar: Condvar,
    in_flight_count: AtomicUsize,
}

impl WorkQueue {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            condvar: Condvar::new(),
            in_flight_count: AtomicUsize::new(0),
        }
    }

    pub fn enqueue(&self, item: Tasks) {
        {
            let mut queue_inner = self.queue.lock().unwrap();
            self.in_flight_count.fetch_add(1, Ordering::SeqCst);
            queue_inner.push_back(item);
        } // Drop lock
        self.condvar.notify_one();
    }

    pub fn enqueue_batch(&self, items: Vec<Tasks>) {
        if items.is_empty() {
            return;
        }
        let count = items.len();
        {
            let mut queue_inner = self.queue.lock().unwrap();
            self.in_flight_count.fetch_add(count, Ordering::SeqCst);
            for item in items {
                queue_inner.push_back(item);
            }
        } // Drop lock before notifying

        self.condvar.notify_all();
    }

    pub fn purge(&self) {
        let mut queue_inner = self.queue.lock().unwrap();
        let initial_len = queue_inner.len();
        queue_inner.retain(|t| !t.should_purge());
        let final_len = queue_inner.len();
        let cleared = initial_len - final_len;
        self.in_flight_count.fetch_sub(cleared, Ordering::SeqCst);
    }

    pub fn in_flight_count(&self) -> usize {
        self.in_flight_count.load(Ordering::SeqCst)
    }

    pub fn dequeue_or_idle(&self) -> Tasks {
        let mut queue_inner = self.queue.lock().unwrap();
        loop {
            if let Some(item) = queue_inner.pop_front() {
                return item;
            }
            queue_inner = self.condvar.wait(queue_inner).unwrap();
        }
    }

    pub fn finish_work(&self) -> usize {
        let count = self.in_flight_count.fetch_sub(1, Ordering::SeqCst) - 1;
        if count == 0 {
            // Notify all when in-flight drops to zero (Phase completion)
            // This is critical for the main thread waiting on completion.
            self.condvar.notify_all();
        }
        count
    }
}

/// Internal telemetry data for rate calculations
pub struct Telemetry {
    pub start_time: std::time::Instant,
    pub last_tick: std::time::Instant,
    pub last_gen: u64,
    pub gps: f64,
    pub work_rate_ema: f64,
    pub net_rate_ema: f64,
}

impl Telemetry {
    pub fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            start_time: now,
            last_tick: now,
            last_gen: 0,
            gps: 0.0,
            work_rate_ema: 0.0,
            net_rate_ema: 0.0,
        }
    }

    pub fn update(&mut self, current_gen: u64, work: u64, net: i64) {
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64();

        // Update at most 10 times a second to ensure stability or on every step?
        // Let's update every step but smooth heavy.
        if dt > 0.0 {
            let limit_dt = dt.max(0.001); // Prevent div by zero
            let gen_diff = current_gen.saturating_sub(self.last_gen) as f64;
            let current_gps = gen_diff / limit_dt;

            // Simple Moving Average / EMA
            let alpha = 0.1; // Smooth factor
            self.gps = current_gps * alpha + self.gps * (1.0 - alpha);

            // Work Rate = Work (Events) / dt
            let current_work_rate = work as f64 / limit_dt;
            self.work_rate_ema = current_work_rate * alpha + self.work_rate_ema * (1.0 - alpha);

            // Net Rate = Net (Change) / dt
            let current_net_rate = net as f64 / limit_dt;
            self.net_rate_ema = current_net_rate * alpha + self.net_rate_ema * (1.0 - alpha);

            self.last_tick = now;
            self.last_gen = current_gen;
        }
    }
}

/// Interface for external components to observe simulation progress.
pub trait EngineSubscriber: Send + Sync {
    /// Notified when a new snapshot is ready in memory.
    ///
    /// The `generation` and its serialized `data` buffer.
    /// Returns `false` if the subscriber wants the simulation to stop.
    fn on_snapshot_available(&self, generation: u64, data: Arc<Vec<u8>>) -> bool;
}

/// Thread-safe circular buffer for generation snapshots.
pub struct SnapshotStore {
    snapshots: RwLock<std::collections::BTreeMap<u64, Arc<Vec<u8>>>>,
    pool: Mutex<Vec<Vec<u8>>>,
    max_capacity: usize,
    epoch: AtomicU64,
}

impl SnapshotStore {
    fn new(max_capacity: usize) -> Self {
        Self {
            snapshots: RwLock::new(std::collections::BTreeMap::new()),
            pool: Mutex::new(Vec::with_capacity(max_capacity)),
            max_capacity,
            epoch: AtomicU64::new(0),
        }
    }

    pub fn get_buffer(&self, capacity: usize) -> Vec<u8> {
        let mut p = self.pool.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut buf) = p.pop() {
            if buf.capacity() >= capacity {
                buf.clear();
                return buf;
            }
        }
        Vec::with_capacity(capacity)
    }

    pub fn get_latest(&self) -> Option<(u64, Arc<Vec<u8>>)> {
        let g = self.snapshots.read().unwrap_or_else(|e| e.into_inner());
        g.last_key_value().map(|(k, v)| (*k, v.clone()))
    }

    pub fn insert(&self, generation: u64, data: Arc<Vec<u8>>, epoch: u64) {
        // Enforce Epoch: Reject stale snapshots
        if epoch < self.epoch.load(Ordering::SeqCst) {
            return;
        }
        let mut g = self.snapshots.write().unwrap_or_else(|e| e.into_inner());
        // Double check inside lock in case clear() happened just now
        if epoch < self.epoch.load(Ordering::SeqCst) {
            return;
        }
        g.insert(generation, data);

        // Keep last N generations
        if g.len() > self.max_capacity {
            let first_key = *g.keys().next().unwrap();
            if let Some(evicted) = g.remove(&first_key) {
                drop(g);
                if let Ok(vec) = Arc::try_unwrap(evicted) {
                    let mut p = self.pool.lock().unwrap_or_else(|e| e.into_inner());
                    p.push(vec);
                }
            }
        }
    }

    pub fn get(&self, generation: u64) -> Option<Arc<Vec<u8>>> {
        let g = self.snapshots.read().unwrap_or_else(|e| e.into_inner());
        g.get(&generation).cloned()
    }

    pub fn clear(&self, new_epoch: u64) {
        // Update epoch first to block new inserts
        self.epoch.store(new_epoch, Ordering::SeqCst);
        let mut g = self.snapshots.write().unwrap();
        g.clear();
    }

    pub fn latest_generation(&self) -> u64 {
        let g = self.snapshots.read().unwrap_or_else(|e| e.into_inner());
        g.keys().next_back().cloned().unwrap_or(0)
    }

    pub fn get_all_generations(&self) -> Vec<u64> {
        let g = self.snapshots.read().unwrap_or_else(|e| e.into_inner());
        g.keys().cloned().collect()
    }
}

/// The primary orchestrator for the parallel cellular automata simulation.
///
/// `SimulationEngine` manages a thread pool that executes the multi-phase
/// simulation cycle. It handles task orchestration, thread synchronization,
/// and snapshot generation.
pub struct SimulationEngine {
    /// The sparse grid containing the cells.
    pub space: Arc<SimulationSpace>,
    /// Temporary buffer for candidate coordinates during Phase 1 & 2.
    pub scratchpad: Arc<Scratchpad>,
    _worker_handles: Mutex<Vec<thread::JoinHandle<()>>>,
    work_queue: Arc<WorkQueue>,
    pub living_count: AtomicU64,
    pub work: AtomicU64,
    pub net: AtomicI64,
    pub generation: AtomicU64,
    pub telemetry: Mutex<Telemetry>,
    subscribers: Arc<RwLock<Vec<Arc<dyn EngineSubscriber>>>>,
    stopping: AtomicBool,
    tainted: AtomicBool,
    pool_size: usize,
    /// Memory store for recent binary snapshots.
    pub snapshots: Arc<SnapshotStore>,
    /// The last seed pattern applied, used for Reset.
    pub last_seed: Mutex<Option<String>>,
    io_tx: std::sync::mpsc::Sender<IoTask>,
    _io_handle: Mutex<Option<thread::JoinHandle<()>>>,
    transition_lock: Mutex<()>,
    /// Double-buffered snapshot record storage [generation % 2][bucket_idx].
    record_buffers: [Vec<Mutex<Vec<SnapshotRecord>>>; 2],
    /// The target generation to stop at (u64::MAX if unlimited).
    target_generation: AtomicU64,
    /// Counter to invalidate old IO tasks after a Reset/Seed.
    epoch: AtomicU64,
    /// Dynamic registry of available patterns.
    pub patterns: RwLock<Vec<crate::patterns::Pattern>>,
    /// Thread-local scratch buffers for commit operations.
    pub commit_buffers: Vec<PaddedBuffer>,
}

impl SimulationEngine {
    pub fn new(space: Arc<SimulationSpace>, pool_size: usize) -> Arc<Self> {
        let work_queue = Arc::new(WorkQueue::new());
        let bucket_count = space.storage().buckets.len();
        let scratchpad = Arc::new(Scratchpad::new(pool_size, bucket_count));
        let subscribers = Arc::new(RwLock::new(Vec::new()));
        let snapshots = Arc::new(SnapshotStore::new(100)); // Keep last 100 gens
        let (io_tx, io_rx) = std::sync::mpsc::channel::<IoTask>();

        let engine = Arc::new(Self {
            space: space.clone(),
            scratchpad,
            _worker_handles: Mutex::new(Vec::new()),
            work_queue,
            subscribers: subscribers.clone(),
            stopping: AtomicBool::new(true),
            tainted: AtomicBool::new(false),
            pool_size,
            snapshots: snapshots.clone(),
            generation: AtomicU64::new(0),
            living_count: AtomicU64::new(0),
            work: AtomicU64::new(0),
            net: AtomicI64::new(0),
            telemetry: Mutex::new(Telemetry::new()),
            last_seed: Mutex::new(None),
            io_tx,
            _io_handle: Mutex::new(None),
            transition_lock: Mutex::new(()),
            record_buffers: [
                (0..bucket_count)
                    .map(|_| Mutex::new(Vec::with_capacity(1024)))
                    .collect(),
                (0..bucket_count)
                    .map(|_| Mutex::new(Vec::with_capacity(1024)))
                    .collect(),
            ],
            target_generation: AtomicU64::new(u64::MAX),
            epoch: AtomicU64::new(0),
            patterns: RwLock::new(crate::patterns::get_builtin_patterns()),
            commit_buffers: (0..pool_size)
                .map(|_| PaddedBuffer {
                    inner: CachePadded::new(UnsafeCell::new(CommitBuffer::default())),
                })
                .collect(),
        });

        // Setup Background I/O thread
        let engine_for_io = Arc::clone(&engine);

        let io_handle = thread::Builder::new()
            .name("Simulation-IO".into())
            .spawn(move || {
                Self::simulation_io_loop(engine_for_io, io_rx);
            })
            .expect("Failed to spawn IO thread");

        *engine._io_handle.lock().unwrap() = Some(io_handle);

        let mut handles = engine._worker_handles.lock().unwrap();
        for thread_idx in 0..pool_size {
            let engine_arc = Arc::clone(&engine);
            handles.push(
                thread::Builder::new()
                    .name(format!("Worker-{}", thread_idx))
                    .spawn(move || {
                        loop {
                            let task = engine_arc.work_queue.dequeue_or_idle();

                            // NOTE: This catch_unwind is a safety barrier to prevent a single bucket
                            // corruption from deadlocking the entire thread pool. It is normal
                            // for it to appear in the profile trace wrapping actual execution.
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    // eprintln!("Thread {} processing {:?}", thread_idx, task);
                                    match &task {
                                        Tasks::Quit => {
                                            engine_arc.work_queue.finish_work();
                                            return true; // Signal to break loop
                                        }
                                        Tasks::Stop => {
                                            let _lock = engine_arc.transition_lock.lock().unwrap();
                                            engine_arc.stopping.store(true, Ordering::SeqCst);
                                            engine_arc.tainted.store(true, Ordering::SeqCst);
                                            engine_arc.work_queue.purge();
                                            if engine_arc.work_queue.in_flight_count() > 1 {
                                                engine_arc.work_queue.enqueue(Tasks::Stop);
                                            }
                                        }
                                        Tasks::Start => {
                                            // engine.stopping.store(false, Ordering::SeqCst); // REMOVED: Managed by public API
                                            if engine_arc.stopping.load(Ordering::SeqCst) {
                                                // If we were stopped while this task was in queue, abort.
                                                engine_arc.work_queue.finish_work();
                                                return true;
                                            }
                                            if engine_arc.work_queue.in_flight_count() == 1 {
                                                // Eager Advance: Prepare destination generation before work starts
                                                engine_arc.space.advance_generation();
                                                engine_arc
                                                    .generation
                                                    .fetch_add(1, Ordering::SeqCst);

                                                // Do NOT reset target_generation here, as this task is recycled for the internal loop.
                                                // Public start() sets it to MAX. StartGenerations() sets it to specific target.

                                                if engine_arc.tainted.load(Ordering::SeqCst) {
                                                    engine_arc.scratchpad.clear();
                                                    engine_arc.space.repair();
                                                    engine_arc
                                                        .tainted
                                                        .store(false, Ordering::SeqCst);
                                                }
                                                Self::initiate_spread(&engine_arc);
                                            } else {
                                                engine_arc.work_queue.enqueue(Tasks::Start);
                                            }
                                        }
                                        Tasks::StartGenerations(count) => {
                                            let current =
                                                engine_arc.generation.load(Ordering::SeqCst) as u64;
                                            engine_arc
                                                .target_generation
                                                .store(current + count, Ordering::SeqCst);
                                            // engine_arc.stopping.store(false, Ordering::SeqCst); // REMOVED: Managed by public API

                                            if engine_arc.stopping.load(Ordering::SeqCst) {
                                                // If we were stopped while this task was in queue, abort.
                                                engine_arc.work_queue.finish_work();
                                                return true;
                                            }

                                            if engine_arc.work_queue.in_flight_count() == 1 {
                                                // Eager Advance
                                                engine_arc.space.advance_generation();
                                                engine_arc
                                                    .generation
                                                    .fetch_add(1, Ordering::SeqCst);

                                                if engine_arc.tainted.load(Ordering::SeqCst) {
                                                    engine_arc.scratchpad.clear();
                                                    engine_arc.space.repair();
                                                    engine_arc
                                                        .tainted
                                                        .store(false, Ordering::SeqCst);
                                                }
                                                Self::initiate_spread(&engine_arc);
                                            } else {
                                                engine_arc
                                                    .work_queue
                                                    .enqueue(Tasks::StartGenerations(*count));
                                            }
                                        }
                                        Tasks::Step => {
                                            let _lock = engine_arc.transition_lock.lock().unwrap();
                                            if engine_arc.work_queue.in_flight_count() == 1 {
                                                // Eager Advance
                                                engine_arc.space.advance_generation();
                                                engine_arc
                                                    .generation
                                                    .fetch_add(1, Ordering::SeqCst);

                                                if engine_arc.tainted.load(Ordering::SeqCst) {
                                                    engine_arc.scratchpad.clear();
                                                    engine_arc.space.repair();
                                                    engine_arc
                                                        .tainted
                                                        .store(false, Ordering::SeqCst);
                                                }
                                                engine_arc.stopping.store(true, Ordering::SeqCst);
                                                Self::initiate_spread(&engine_arc);
                                            }
                                        }
                                        Tasks::SpreadBatch(start, end) => {
                                            Self::spread_bucket(
                                                *start,
                                                *end,
                                                &engine_arc,
                                                thread_idx,
                                            );
                                        }
                                        Tasks::CommitBatch(start, end) => {
                                            Self::commit_bucket(
                                                *start,
                                                *end,
                                                &engine_arc,
                                                thread_idx,
                                            );
                                        }
                                        Tasks::Reset => {
                                            let _lock = engine_arc.transition_lock.lock().unwrap();
                                            // Race Condition Fix: Reset must ensure exclusivity.
                                            engine_arc.stopping.store(true, Ordering::SeqCst);
                                            engine_arc.work_queue.purge();

                                            if engine_arc.work_queue.in_flight_count() > 1 {
                                                engine_arc.work_queue.enqueue(Tasks::Reset);
                                            } else {
                                                let new_epoch =
                                                    engine_arc.epoch.fetch_add(1, Ordering::SeqCst)
                                                        + 1;
                                                engine_arc.snapshots.clear(new_epoch);
                                                engine_arc.space.clear();
                                                engine_arc.generation.store(0, Ordering::SeqCst);
                                                engine_arc.living_count.store(0, Ordering::SeqCst);
                                                let seed =
                                                    engine_arc.last_seed.lock().unwrap().clone();
                                                if let Some(pattern) = seed {
                                                    Self::apply_seed(&engine_arc, &pattern);
                                                }
                                                Self::trigger_initial_snapshot(&engine_arc);
                                            }
                                        }
                                        Tasks::Seed(pattern) => {
                                            let _lock = engine_arc.transition_lock.lock().unwrap();
                                            // Race Condition Fix: Reset must ensure exclusivity.
                                            // 1. Signal Stop to prevent new work generation.
                                            engine_arc.stopping.store(true, Ordering::SeqCst);
                                            // 2. Purge existing work (Step, Spread, Commit).
                                            engine_arc.work_queue.purge();

                                            // 3. Wait for workers to drain.
                                            // If other tasks are still in flight (e.g. finishing a Spread),
                                            // re-enqueue Seed to try again later.
                                            if engine_arc.work_queue.in_flight_count() > 1 {
                                                engine_arc
                                                    .work_queue
                                                    .enqueue(Tasks::Seed(pattern.clone()));
                                            } else {
                                                // Quiescent State: We are the only active task.
                                                let new_epoch =
                                                    engine_arc.epoch.fetch_add(1, Ordering::SeqCst)
                                                        + 1;
                                                engine_arc.snapshots.clear(new_epoch);
                                                engine_arc.space.clear();
                                                engine_arc.generation.store(0, Ordering::SeqCst);
                                                *engine_arc.last_seed.lock().unwrap() =
                                                    Some(pattern.clone());
                                                Self::apply_seed(&engine_arc, &pattern);
                                                Self::trigger_initial_snapshot(&engine_arc);
                                            }
                                        }
                                        Tasks::SeedAndStart(pattern, target_gen) => {
                                            let _lock = engine_arc.transition_lock.lock().unwrap();
                                            engine_arc.stopping.store(true, Ordering::SeqCst);
                                            engine_arc.work_queue.purge();

                                            if engine_arc.work_queue.in_flight_count() > 1 {
                                                engine_arc.work_queue.enqueue(Tasks::SeedAndStart(
                                                    pattern.clone(),
                                                    *target_gen,
                                                ));
                                            } else {
                                                let new_epoch =
                                                    engine_arc.epoch.fetch_add(1, Ordering::SeqCst)
                                                        + 1;
                                                engine_arc.snapshots.clear(new_epoch);
                                                engine_arc.space.clear();
                                                engine_arc.generation.store(0, Ordering::SeqCst);
                                                engine_arc.living_count.store(0, Ordering::SeqCst);
                                                *engine_arc.last_seed.lock().unwrap() =
                                                    Some(pattern.clone());
                                                Self::apply_seed(&engine_arc, &pattern);
                                                Self::trigger_initial_snapshot(&engine_arc);

                                                // Set target generation if provided
                                                let target = target_gen.unwrap_or(u64::MAX);
                                                engine_arc
                                                    .target_generation
                                                    .store(target, Ordering::SeqCst);

                                                // Auto-start immediately
                                                engine_arc.stopping.store(false, Ordering::SeqCst);
                                                engine_arc.work_queue.enqueue(Tasks::Start);
                                            }
                                        }
                                    };
                                    false // Do not break
                                }));

                            match result {
                                Ok(should_break) => {
                                    if should_break {
                                        break;
                                    }
                                    let remaining = engine_arc.work_queue.finish_work();
                                    if remaining == 0 {
                                        Self::handle_emergent_transition(&engine_arc, &task);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("CRITICAL: Worker Panic caught: {:?}", e);
                                    // Ensure we decrement count to avoid deadlock, though state is likely compromised
                                    let remaining = engine_arc.work_queue.finish_work();
                                    if remaining == 0 {
                                        Self::handle_emergent_transition(&engine_arc, &task);
                                    }
                                }
                            }
                        }
                    })
                    .expect("Failed to spawn worker thread"),
            );
        }
        drop(handles);
        engine
    }

    fn handle_emergent_transition(engine: &Arc<Self>, completed_task: &Tasks) {
        let _lock = engine
            .transition_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Self::handle_transition_internal(engine, completed_task);
    }

    fn handle_transition_internal(engine: &Arc<Self>, completed_task: &Tasks) {
        match completed_task {
            Tasks::Start | Tasks::StartGenerations(_) | Tasks::Step | Tasks::SpreadBatch(_, _) => {
                let in_flight = engine.work_queue.in_flight_count();
                // eprintln!("Transition Check: Task {:?}, InFlight {}", completed_task, in_flight);
                if in_flight == 0 {
                    if !Self::initiate_commit(engine) {
                        Self::handle_transition_internal(engine, &Tasks::CommitBatch(0, 0));
                    }
                }
            }
            Tasks::CommitBatch(_, _) => {
                if engine.work_queue.in_flight_count() == 0 {
                    // Generation cycle complete.
                    // Snapshot the generation that was just committed into the Current mask.
                    let generation_count = engine.generation.load(Ordering::SeqCst);
                    if generation_count == 0 {
                        // Spurious transition or race condition led here without generation advance.
                        return;
                    }

                    // Offload Collected Records
                    // Use the buffer that was just populated during the commit of this generation.
                    let buffer_idx = (generation_count % 2) as usize;
                    let mut captured_buckets =
                        Vec::with_capacity(engine.record_buffers[buffer_idx].len());
                    for m in &engine.record_buffers[buffer_idx] {
                        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
                        let vec = std::mem::replace(&mut *g, Vec::new());
                        captured_buckets.push(vec);
                    }

                    // Update Telemetry
                    let work_count = engine.work.swap(0, Ordering::Relaxed);
                    let net_count = engine.net.swap(0, Ordering::Relaxed);

                    let (gps, work_rate, net_rate) = {
                        let mut tel = engine.telemetry.lock().unwrap();
                        tel.update(generation_count, work_count, net_count);
                        (tel.gps, tel.work_rate_ema, tel.net_rate_ema)
                    };

                    let _ = engine.io_tx.send(IoTask::Snapshot {
                        generation: generation_count as u64,
                        living_count: engine.living_count.load(Ordering::SeqCst),
                        buckets: captured_buckets,
                        is_running: !engine.stopping.load(Ordering::SeqCst),
                        buffer_idx,
                        epoch: engine.epoch.load(Ordering::SeqCst),
                        gps,
                        work_rate,
                        net_rate,
                    });

                    if !engine.stopping.load(Ordering::SeqCst) {
                        let target = engine.target_generation.load(Ordering::SeqCst);
                        if generation_count as u64 >= target {
                            engine.stopping.store(true, Ordering::SeqCst);
                            // We do not enqueue Tasks::Start, effectively stopping here.
                            // But we should notify subscribers that we ARE stopped?
                            // No, IoTask::Snapshot has is_running flag.
                            // But we need to update that flag for the NEXT snapshot or this current one?
                            // The snapshot we just sent (generation_count) had is_running = !stopping.
                            // We read stopping BEFORE writing to IoTx.
                            // If we set stopping=true HERE, the snapshot just sent might have said "Running".
                            // That's fine, the NEXT status update (if any) or query will show stopped.
                        } else {
                            engine.work_queue.enqueue(Tasks::Start);
                        }
                    }
                }
            }
            Tasks::Seed(_) | Tasks::Reset | Tasks::SeedAndStart(_, _) => {
                if !engine.stopping.load(Ordering::SeqCst) {
                    Self::initiate_spread(engine);
                }
            }
            _ => {}
        }
    }

    fn initiate_spread(engine: &Arc<Self>) {
        let bucket_count = engine.space.storage().buckets.len();
        let pool_size = engine.pool_size;

        let total_batches = std::cmp::max(1, pool_size * 4);
        let batch_size = (bucket_count + total_batches - 1) / total_batches;

        let mut tasks = Vec::with_capacity(total_batches);
        for i in (0..bucket_count).step_by(batch_size) {
            let end = std::cmp::min(i + batch_size, bucket_count);
            tasks.push(Tasks::SpreadBatch(i, end));
        }

        if tasks.is_empty() {
            if !Self::initiate_commit(engine) {
                Self::handle_transition_internal(engine, &Tasks::CommitBatch(0, 0));
            }
        } else {
            engine.work_queue.enqueue_batch(tasks);
        }
    }

    fn spread_bucket(
        bucket_start: usize,
        bucket_end: usize,
        engine: &Arc<Self>,
        thread_idx: usize,
    ) {
        let storage = engine.space.storage();
        // Read from last generation's state to propagate into the current generation
        let source_mask = engine.space.mask.read().last_state_mask();

        for bucket_idx in bucket_start..bucket_end {
            if bucket_idx >= storage.buckets.len() {
                break;
            }
            let bucket_lock = storage.buckets[bucket_idx]
                .read()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(root) = bucket_lock.root {
                Self::spread_recursive(&bucket_lock, root, engine, source_mask, thread_idx);
            }
        }
    }

    fn spread_recursive(
        tree: &crate::tree::CellTree,
        idx: crate::tree::NodeIndex,
        engine: &Arc<Self>,
        mask: usize,
        thread_idx: usize,
    ) {
        let node = tree.arena.get(idx);
        if node.cell.state(mask) == CellState::Alive {
            let (x, y) = node.cell.coordinates();
            // Spread to all 8 neighbors (skip self for correct neighbor counting)
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    engine.scratchpad.push_candidate(thread_idx, x + dx, y + dy);
                }
            }
        }

        if let Some(left) = node.left {
            Self::spread_recursive(tree, left, engine, mask, thread_idx);
        }
        if let Some(right) = node.right {
            Self::spread_recursive(tree, right, engine, mask, thread_idx);
        }
    }

    fn initiate_commit(engine: &Arc<Self>) -> bool {
        let bucket_count = engine.scratchpad.bucket_count();
        let pool_size = engine.pool_size;

        let total_batches = std::cmp::max(1, pool_size * 4);
        let batch_size = (bucket_count + total_batches - 1) / total_batches;

        let mut tasks = Vec::with_capacity(total_batches);
        for i in (0..bucket_count).step_by(batch_size) {
            let end = std::cmp::min(i + batch_size, bucket_count);
            tasks.push(Tasks::CommitBatch(i, end));
        }

        if tasks.is_empty() {
            false
        } else {
            // Reset counts before committing
            engine.living_count.store(0, Ordering::SeqCst);
            engine.work_queue.enqueue_batch(tasks);
            true
        }
    }

    fn commit_bucket(
        bucket_start: usize,
        bucket_end: usize,
        engine: &Arc<Self>,
        thread_idx: usize,
    ) {
        let space = &engine.space;
        let storage = space.storage();
        let masks = space.mask.read();
        let cur = masks.current_state_mask();
        let last = masks.last_state_mask();
        let next = masks.next_state_mask();

        // SAFETY: thread_idx is unique to this worker thread and fixed for its lifetime.
        // No other thread accesses this index in commit_buffers.
        let buffer = unsafe { &mut *engine.commit_buffers[thread_idx].inner.value.get() };

        for bucket_idx in bucket_start..bucket_end {
            if bucket_idx >= storage.buckets.len() {
                break;
            }

            buffer.incoming.clear();
            engine
                .scratchpad
                .get_column_into(bucket_idx, &mut buffer.incoming);

            buffer.incoming.sort_unstable_by(|a, b| {
                crate::tree::CellTree::compare_coords((a.x, a.y), (b.x, b.y))
            });

            buffer.coords.clear();
            for c in &buffer.incoming {
                buffer.coords.push((c.x, c.y));
            }

            let mut bucket_lock = storage.buckets[bucket_idx]
                .write()
                .unwrap_or_else(|e| e.into_inner());

            // Unified Commit & Prune:
            // 1. Ensure nodes exist and increment neighbor counts for the SOURCE mask (last)
            // so that calculate_next_state can use the correct neighborhood density.
            bucket_lock.apply_batch(
                &buffer.coords,
                |x, y| Cell::new(x, y, CellState::Dead, last),
                |cell| cell.increment_neighbor_count(last),
            );

            // 2. Perform state update (writing into current mask) and natural pruning.
            let buffer_idx = (engine.generation.load(Ordering::SeqCst) % 2) as usize;
            let mut records = engine.record_buffers[buffer_idx][bucket_idx]
                .lock()
                .unwrap_or_else(|e| e.into_inner());

            bucket_lock.commit_and_prune(last, cur, next, |cell, next_state| {
                let prev_state = cell.state(last);

                // Telemetry: Detect state changes
                if prev_state != next_state {
                    engine.work.fetch_add(1, Ordering::Relaxed);
                    if next_state == CellState::Alive {
                        engine.net.fetch_add(1, Ordering::Relaxed);
                    } else {
                        engine.net.fetch_sub(1, Ordering::Relaxed);
                    }
                }

                if next_state == CellState::Alive {
                    engine.living_count.fetch_add(1, Ordering::Relaxed);
                }
                if let Some(state) = cell.presenter_view(cur, last, next) {
                    let (x, y) = cell.coordinates();
                    records.push(SnapshotRecord { x, y, state });
                }
            });
        }
    }

    pub fn start(&self) {
        // We remove the in_flight_count == 0 check here to allow autostart
        // when a seed is already being applied. The worker-side check for
        // in_flight_count == 1 handles the actual state transition safety.

        self.target_generation.store(u64::MAX, Ordering::SeqCst);
        self.stopping.store(false, Ordering::SeqCst);
        self.work_queue.enqueue(Tasks::Start);
    }

    pub fn set_target_generation(&self, target: u64) {
        self.target_generation.store(target, Ordering::SeqCst);
    }

    pub fn start_generations(&self, count: u64) {
        self.stopping.store(false, Ordering::SeqCst);
        self.work_queue.enqueue(Tasks::StartGenerations(count));
    }

    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        // Synchronous Graceful Stop: wait for the current generation boundary.
        // This ensures the space is in a consistent state for observation.
        let start = std::time::Instant::now();
        while self.work_queue.in_flight_count() > 0 {
            if start.elapsed().as_secs() > 5 {
                eprintln!(
                    "CRITICAL: Engine Stop timed out! Leaked in_flight_count: {}",
                    self.work_queue.in_flight_count()
                );
                break;
            }
            std::thread::yield_now();
        }
    }

    /// Abort the simulation immediately, purging all pending work and tainting
    /// the state. This is useful for emergency resets or testing recovery.
    pub fn abort(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.tainted.store(true, Ordering::SeqCst);
        self.work_queue.purge();
        // Wait for workers to finish current atomized task
        while self.work_queue.in_flight_count() > 0 {
            std::thread::yield_now();
        }
    }

    /// Forcefully run a state repair cycle on the next step or start.
    pub fn mark_tainted(&self) {
        self.tainted.store(true, Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.work_queue.enqueue(Tasks::Reset);
    }

    pub fn seed(&self, pattern: String) {
        self.work_queue.enqueue(Tasks::Seed(pattern));
    }

    pub fn seed_and_start(&self, pattern: String, target_generations: Option<u64>) {
        self.work_queue
            .enqueue(Tasks::SeedAndStart(pattern, target_generations));
    }

    pub fn register_pattern(&self, pattern: crate::patterns::Pattern) {
        let mut patterns = self.patterns.write().unwrap();
        patterns.push(pattern);
        // Sort for UI consistency
        patterns.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn shutdown(&self) {
        self.work_queue.purge();
        let mut tasks = Vec::new();
        for _ in 0..self.pool_size {
            tasks.push(Tasks::Quit);
        }
        self.work_queue.enqueue_batch(tasks);

        let mut handles = self._worker_handles.lock().unwrap();
        for handle in handles.drain(..) {
            let _ = handle.join();
        }
    }

    pub fn get_catalog(&self) -> Vec<crate::PatternInfo> {
        self.patterns
            .read()
            .unwrap()
            .iter()
            .map(|d| crate::PatternInfo {
                name: d.name.clone(),
                description: d.description.clone(),
            })
            .collect()
    }

    fn apply_seed(engine: &Arc<SimulationEngine>, pattern_name: &str) {
        let patterns = engine.patterns.read().unwrap();
        if let Some(pattern) = patterns
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(pattern_name))
        {
            match &pattern.source {
                crate::patterns::PatternSource::Builtin(f) => f(&engine.space, 0, 0),
                crate::patterns::PatternSource::Rle(rle) => engine.space.seed_from_rle(0, 0, rle),
            }
        } else {
            eprintln!("Unknown seed pattern: {}", pattern_name);
        }
    }

    fn trigger_initial_snapshot(engine: &Arc<Self>) {
        let generation_count = engine.generation.load(Ordering::SeqCst);
        let guard = engine.space.mask.read();
        let cur = guard.current_state_mask();
        let last = guard.last_state_mask();
        let next = guard.next_state_mask();
        let mut captured_buckets = Vec::new();
        let mut total_living = 0;
        let storage = engine.space.storage();

        for (_bucket_idx, bucket) in storage.buckets.iter().enumerate() {
            let bucket_lock = bucket.read().unwrap_or_else(|e| e.into_inner());
            let mut records = Vec::new();
            if let Some(root) = bucket_lock.root {
                Self::collect_records_recursive(
                    &bucket_lock,
                    root,
                    cur,
                    last,
                    next,
                    &mut records,
                    &mut total_living,
                );
            }
            captured_buckets.push(records);
        }

        let _ = engine.io_tx.send(IoTask::Snapshot {
            generation: generation_count as u64,
            living_count: total_living,
            buckets: captured_buckets,
            is_running: !engine.stopping.load(Ordering::SeqCst),
            buffer_idx: usize::MAX, // Special value for initial snapshot
            epoch: engine.epoch.load(Ordering::SeqCst),
            gps: 0.0,
            work_rate: 0.0,
            net_rate: 0.0,
        });

        engine.living_count.store(total_living, Ordering::SeqCst);
    }

    fn collect_records_recursive(
        tree: &crate::tree::CellTree,
        idx: crate::tree::NodeIndex,
        cur: usize,
        last: usize,
        next: usize,
        out: &mut Vec<SnapshotRecord>,
        living_count: &mut u64,
    ) {
        let node = tree.arena.get(idx);
        if node.cell.state(cur) == CellState::Alive {
            *living_count += 1;
        }
        if let Some(state) = node.cell.presenter_view(cur, last, next) {
            let (x, y) = node.cell.coordinates();
            out.push(SnapshotRecord { x, y, state });
        }
        if let Some(left) = node.left {
            Self::collect_records_recursive(tree, left, cur, last, next, out, living_count);
        }
        if let Some(right) = node.right {
            Self::collect_records_recursive(tree, right, cur, last, next, out, living_count);
        }
    }

    pub fn step(&self) {
        let start_gen = self.generation();

        // Ensure we are stopped
        self.stopping.store(true, Ordering::SeqCst);

        // Wait for any pending tasks (like an in-flight Stop or previous generation remnants)
        // to clear so that our Step task is guaranteed to be the next thing processed.
        // This is critical for the "Repair Phase" triggered by step/start.
        while self.work_queue.in_flight_count() > 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        self.work_queue.enqueue(Tasks::Step);

        // Wait for generation to advance
        while self.generation() <= start_gen {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    pub fn add_subscriber(&self, subscriber: Arc<dyn EngineSubscriber>) {
        self.subscribers.write().unwrap().push(subscriber);
    }

    pub fn get_cells_in_rect(
        &self,
        min: (i128, i128),
        max: (i128, i128),
    ) -> Vec<((i128, i128), u8)> {
        let mut out = Vec::new();
        let guard = self.space.mask.read();
        let current_mask = guard.current_state_mask();
        let last_mask = guard.last_state_mask();
        let last_last_mask = guard.next_state_mask();
        self.space.storage().collect_in_rect(
            min,
            max,
            current_mask,
            last_mask,
            last_last_mask,
            &mut out,
        );
        out
    }

    pub fn work_queue_in_flight(&self) -> usize {
        self.work_queue.in_flight_count()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst) as u64
    }

    pub fn is_stopped(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    #[inline(never)]
    fn simulation_io_loop(engine: Arc<Self>, rx: std::sync::mpsc::Receiver<IoTask>) {
        while let Ok(task) = rx.recv() {
            match task {
                IoTask::Quit => break,
                IoTask::Snapshot {
                    generation,
                    living_count,
                    is_running,
                    buckets,
                    buffer_idx,
                    epoch,
                    gps,
                    work_rate,
                    net_rate,
                } => {
                    let current_epoch = engine.epoch.load(Ordering::SeqCst);

                    // Filter Ghost Snapshots
                    if epoch != current_epoch {
                        // Don't leak buffers! Return them.
                        if buffer_idx < 2 {
                            let _ = engine.io_tx.send(IoTask::ReturnBuffers {
                                buckets,
                                buffer_idx,
                            });
                        }
                        continue;
                    }
                    let record_count = buckets.iter().map(|b| b.len()).sum::<usize>() as u64;

                    // Construct Header
                    let header = crate::Response::BinaryStateHeader {
                        generation,
                        total_cells: living_count,
                        is_running,
                        record_count,
                        gps,
                        work_rate,
                        net_rate,
                    };
                    let json_header = serde_json::to_vec(&header).unwrap();
                    let json_len = json_header.len() as u32;

                    // Calculate Size: 4 (len) + JSON + Binary(Records * 33) + 4 (CRC)
                    let total_size = 4 + json_header.len() + (record_count as usize * 33) + 4;
                    let mut buffer = engine.snapshots.get_buffer(total_size);

                    let mut hasher = crc32fast::Hasher::new();

                    // 1. Length Prefix
                    buffer.extend_from_slice(&json_len.to_le_bytes());

                    // 2. JSON Header
                    buffer.extend_from_slice(&json_header);

                    // 3. Binary Payload
                    // IMPORTANT: CRC usually covers only binary payload in our design for speed?
                    // Let's check lib.rs... Yes: `crc32fast::hash(&buf[payload_start..payload_end])`
                    // So we hash while writing the cells.

                    let mut write_cell = |val: &[u8]| {
                        buffer.extend_from_slice(val);
                        hasher.update(val);
                    };

                    for bucket_records in &buckets {
                        for rec in bucket_records {
                            let mut buf = [0u8; 33];
                            buf[0..16].copy_from_slice(&rec.x.to_le_bytes());
                            buf[16..32].copy_from_slice(&rec.y.to_le_bytes());
                            buf[32] = rec.state;
                            write_cell(&buf);
                        }
                    }

                    // 4. CRC32
                    let crc = hasher.finalize();
                    buffer.extend_from_slice(&crc.to_le_bytes());

                    let shared_data = Arc::new(buffer);
                    engine
                        .snapshots
                        .insert(generation, shared_data.clone(), epoch);

                    let subs = engine.subscribers.read().unwrap();
                    for s in subs.iter() {
                        s.on_snapshot_available(generation, shared_data.clone());
                    }

                    // Return buffers back to the engine
                    let _ = engine.io_tx.send(IoTask::ReturnBuffers {
                        buckets,
                        buffer_idx,
                    });
                }
                IoTask::ReturnBuffers {
                    buckets,
                    buffer_idx,
                } => {
                    if buffer_idx < 2 {
                        for (i, mut vec) in buckets.into_iter().enumerate() {
                            vec.clear();
                            let mut lock = engine.record_buffers[buffer_idx][i]
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());

                            // CRITICAL FIX: Buffer Recycling Race
                            // Only recycle the buffer if the slot is EMPTY.
                            // If it's not empty, the Engine has already started writing the next generation
                            // into this slot (wrap-around). Overwriting it would corrupt the new data.
                            if lock.is_empty() && lock.capacity() < vec.capacity() {
                                *lock = vec;
                            }
                            // Else: Engine is using it. Discard `vec` (drop) to preserve data integrity.
                        }
                    }
                }
            }
        }
    }
}

impl Drop for SimulationEngine {
    fn drop(&mut self) {
        let _ = self.io_tx.send(IoTask::Quit);
        if let Some(handle) = self._io_handle.lock().unwrap().take() {
            let _ = handle.join();
        }

        self.work_queue.purge();
        for _ in 0..self.pool_size {
            self.work_queue.enqueue(Tasks::Quit);
        }
        let mut handles = self._worker_handles.lock().unwrap();
        for handle in handles.drain(..) {
            let _ = handle.join();
        }
    }
}
