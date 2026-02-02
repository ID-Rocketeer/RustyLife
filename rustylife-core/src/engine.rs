use crate::cell::{Cell, CellState};
use crate::scratchpad::Scratchpad;
use crate::space::SimulationSpace;
use crate::tree::CellNode;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;

/// Internal tasks executed by the engine's worker thread pool.
#[derive(Debug)]
pub enum Tasks {
    /// Initial signal to start the simulation loop.
    Start,
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
}

/// Tasks dedicated to the background I/O thread.
#[derive(Debug, Clone)]
pub struct SnapshotRecord {
    pub x: i128,
    pub y: i128,
    pub state: u8,
}

pub enum IoTask {
    /// Generate a binary snapshot and notifying subscribers.
    Snapshot {
        generation: u64,
        living_count: u64,
        is_running: bool,
        /// Captured records grouped by bucket.
        buckets: Vec<Vec<SnapshotRecord>>,
    },
    /// Shutdown the I/O thread.
    Quit,
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
        let cleared = queue_inner.len();
        queue_inner.clear();
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
    max_capacity: usize,
}

impl SnapshotStore {
    fn new(max_capacity: usize) -> Self {
        Self {
            snapshots: RwLock::new(std::collections::BTreeMap::new()),
            max_capacity,
        }
    }

    pub fn insert(&self, generation: u64, data: Arc<Vec<u8>>) {
        let mut g = self.snapshots.write().unwrap();
        g.insert(generation, data);

        // Keep last N generations
        if g.len() > self.max_capacity {
            let first_key = *g.keys().next().unwrap();
            g.remove(&first_key);
        }
    }

    pub fn get(&self, generation: u64) -> Option<Arc<Vec<u8>>> {
        let g = self.snapshots.read().unwrap();
        g.get(&generation).cloned()
    }

    pub fn latest_generation(&self) -> u64 {
        let g = self.snapshots.read().unwrap();
        g.keys().next_back().cloned().unwrap_or(0)
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
    subscribers: Arc<RwLock<Vec<Arc<dyn EngineSubscriber>>>>,
    stopping: AtomicBool,
    tainted: AtomicBool,
    pool_size: usize,
    /// Memory store for recent binary snapshots.
    pub snapshots: Arc<SnapshotStore>,
    generation: AtomicUsize,
    pub living_count: AtomicU64,
    /// The last seed pattern applied, used for Reset.
    pub last_seed: Mutex<Option<String>>,
    io_tx: std::sync::mpsc::Sender<IoTask>,
    _io_handle: Mutex<Option<thread::JoinHandle<()>>>,
    transition_lock: Mutex<()>,
    /// Double-buffered snapshot record storage [generation % 2][bucket_idx].
    record_buffers: [Vec<Mutex<Vec<SnapshotRecord>>>; 2],
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
            generation: AtomicUsize::new(0),
            living_count: AtomicU64::new(0),
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
        });

        // Setup Background I/O thread
        let engine_for_io = Arc::clone(&engine);

        let io_handle = thread::spawn(move || {
            while let Ok(task) = io_rx.recv() {
                match task {
                    IoTask::Quit => break,
                    IoTask::Snapshot {
                        generation,
                        living_count,
                        is_running,
                        buckets,
                    } => {
                        let record_count = buckets.iter().map(|b| b.len()).sum::<usize>() as u64;
                        let mut buffer = Vec::with_capacity(64 + (record_count as usize * 33));

                        let mut hasher = crc32fast::Hasher::new();
                        let mut write_le = |val: &[u8]| {
                            buffer.extend_from_slice(val);
                            hasher.update(val);
                        };

                        // Header
                        write_le(&generation.to_le_bytes());
                        write_le(&living_count.to_le_bytes());
                        write_le(&[if is_running { 1 } else { 0 }]);
                        write_le(&record_count.to_le_bytes());

                        for bucket_records in buckets {
                            for rec in bucket_records {
                                let mut buf = [0u8; 33];
                                buf[0..16].copy_from_slice(&rec.x.to_le_bytes());
                                buf[16..32].copy_from_slice(&rec.y.to_le_bytes());
                                buf[32] = rec.state;
                                write_le(&buf);
                            }
                        }

                        // CRC32
                        let crc = hasher.finalize();
                        buffer.extend_from_slice(&crc.to_le_bytes());

                        let shared_data = Arc::new(buffer);
                        engine_for_io
                            .snapshots
                            .insert(generation, shared_data.clone());

                        let subs = engine_for_io.subscribers.read().unwrap();
                        for s in subs.iter() {
                            s.on_snapshot_available(generation, shared_data.clone());
                        }
                    }
                }
            }
        });

        *engine._io_handle.lock().unwrap() = Some(io_handle);

        let mut handles = engine._worker_handles.lock().unwrap();
        for thread_idx in 0..pool_size {
            let engine_arc = Arc::clone(&engine);
            handles.push(thread::spawn(move || {
                loop {
                    let task = engine_arc.work_queue.dequeue_or_idle();

                    // NOTE: This catch_unwind is a safety barrier to prevent a single bucket
                    // corruption from deadlocking the entire thread pool. It is normal
                    // for it to appear in the profile trace wrapping actual execution.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        match &task {
                            Tasks::Quit => {
                                engine_arc.work_queue.finish_work();
                                return true; // Signal to break loop
                            }
                            Tasks::Stop => {
                                engine_arc.stopping.store(true, Ordering::SeqCst);
                                engine_arc.tainted.store(true, Ordering::SeqCst);
                                engine_arc.work_queue.purge();
                                if engine_arc.work_queue.in_flight_count() > 1 {
                                    engine_arc.work_queue.enqueue(Tasks::Stop);
                                }
                            }
                            Tasks::Start => {
                                let _lock = engine_arc.transition_lock.lock().unwrap();
                                if engine_arc.work_queue.in_flight_count() == 1 {
                                    if engine_arc.tainted.load(Ordering::SeqCst) {
                                        engine_arc.scratchpad.clear();
                                        engine_arc.space.repair();
                                        engine_arc.tainted.store(false, Ordering::SeqCst);
                                    }
                                    engine_arc.stopping.store(false, Ordering::SeqCst);
                                    Self::initiate_spread(&engine_arc);
                                }
                            }
                            Tasks::Step => {
                                let _lock = engine_arc.transition_lock.lock().unwrap();
                                if engine_arc.work_queue.in_flight_count() == 1 {
                                    if engine_arc.tainted.load(Ordering::SeqCst) {
                                        engine_arc.scratchpad.clear();
                                        engine_arc.space.repair();
                                        engine_arc.tainted.store(false, Ordering::SeqCst);
                                    }
                                    engine_arc.stopping.store(true, Ordering::SeqCst);
                                    Self::initiate_spread(&engine_arc);
                                }
                            }
                            Tasks::SpreadBatch(start, end) => {
                                Self::spread_bucket(*start, *end, &engine_arc, thread_idx);
                            }
                            Tasks::CommitBatch(start, end) => {
                                Self::commit_bucket(*start, *end, &engine_arc);
                            }
                            Tasks::Reset => {
                                let _lock = engine_arc.transition_lock.lock().unwrap();
                                engine_arc.space.clear();
                                engine_arc.generation.store(0, Ordering::SeqCst);
                                engine_arc.living_count.store(0, Ordering::SeqCst);
                                let seed = engine_arc.last_seed.lock().unwrap().clone();
                                if let Some(pattern) = seed {
                                    Self::apply_seed(&engine_arc, &pattern);
                                }
                                Self::trigger_initial_snapshot(&engine_arc);
                            }
                            Tasks::Seed(pattern) => {
                                let _lock = engine_arc.transition_lock.lock().unwrap();
                                engine_arc.space.clear();
                                engine_arc.generation.store(0, Ordering::SeqCst);
                                *engine_arc.last_seed.lock().unwrap() = Some(pattern.clone());
                                Self::apply_seed(&engine_arc, &pattern);
                                Self::trigger_initial_snapshot(&engine_arc);
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
            }));
        }
        drop(handles);
        engine
    }

    fn handle_emergent_transition(engine: &Arc<Self>, completed_task: &Tasks) {
        let _lock = engine.transition_lock.lock().unwrap();
        Self::handle_transition_internal(engine, completed_task);
    }

    fn handle_transition_internal(engine: &Arc<Self>, completed_task: &Tasks) {
        match completed_task {
            Tasks::Start | Tasks::Step | Tasks::SpreadBatch(_, _) => {
                if engine.work_queue.in_flight_count() == 0 {
                    if !Self::initiate_commit(engine) {
                        Self::handle_transition_internal(engine, &Tasks::CommitBatch(0, 0));
                    }
                }
            }
            Tasks::CommitBatch(_, _) => {
                if engine.work_queue.in_flight_count() == 0 {
                    // Generation cycle complete.
                    engine.space.advance_generation();
                    let generation_count = engine.generation.fetch_add(1, Ordering::SeqCst) + 1;

                    // Offload Collected Records
                    // Use the buffer that was just populated during the commit of this generation.
                    let buffer_idx = ((generation_count - 1) % 2) as usize;
                    let captured_buckets: Vec<Vec<SnapshotRecord>> = engine.record_buffers
                        [buffer_idx]
                        .iter()
                        .map(|m| {
                            let mut g = m.lock().unwrap();
                            let vec = g.clone();
                            g.clear();
                            vec
                        })
                        .collect();

                    let _ = engine.io_tx.send(IoTask::Snapshot {
                        generation: generation_count as u64,
                        living_count: engine.living_count.load(Ordering::SeqCst),
                        buckets: captured_buckets,
                        is_running: !engine.stopping.load(Ordering::SeqCst),
                    });

                    if !engine.stopping.load(Ordering::SeqCst) {
                        engine.work_queue.enqueue(Tasks::Start);
                    }
                }
            }
            Tasks::Seed(_) | Tasks::Reset => {
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
        let current_mask = engine.space.mask.read().current_state_mask();

        for bucket_idx in bucket_start..bucket_end {
            if bucket_idx >= storage.buckets.len() {
                break;
            }
            let bucket_lock = storage.buckets[bucket_idx].read().unwrap();
            if let Some(ref node) = bucket_lock.root {
                Self::spread_recursive(node, engine, current_mask, thread_idx);
            }
        }
    }

    fn spread_recursive(node: &CellNode, engine: &Arc<Self>, mask: usize, thread_idx: usize) {
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

        if let Some(ref left) = node.left {
            Self::spread_recursive(left, engine, mask, thread_idx);
        }
        if let Some(ref right) = node.right {
            Self::spread_recursive(right, engine, mask, thread_idx);
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

    fn commit_bucket(bucket_start: usize, bucket_end: usize, engine: &Arc<Self>) {
        let space = &engine.space;
        let storage = space.storage();
        let masks = space.mask.read();
        let cur = masks.current_state_mask();
        let next = masks.next_state_mask();
        let last = masks.last_state_mask();

        for bucket_idx in bucket_start..bucket_end {
            if bucket_idx >= storage.buckets.len() {
                break;
            }

            let mut incoming = engine.scratchpad.get_column(bucket_idx);
            incoming.sort_unstable_by(|a, b| {
                crate::tree::CellTree::compare_coords((a.x, a.y), (b.x, b.y))
            });

            let coords: Vec<(i128, i128)> = incoming.iter().map(|c| (c.x, c.y)).collect();

            let mut bucket_lock = storage.buckets[bucket_idx].write().unwrap();

            // Unified Commit & Prune:
            // 1. Ensure nodes exist and increment neighbor counts
            bucket_lock.apply_batch(
                &coords,
                |x, y| Cell::new(x, y, CellState::Dead, cur),
                |cell| cell.increment_neighbor_count(cur),
            );

            // 2. Perform state update, counting, and natural pruning
            let buffer_idx = (engine.generation.load(Ordering::Relaxed) % 2) as usize;
            let mut records = engine.record_buffers[buffer_idx][bucket_idx]
                .lock()
                .unwrap();

            bucket_lock.commit_and_prune(cur, next, last, |cell, next_state| {
                if next_state == CellState::Alive {
                    engine.living_count.fetch_add(1, Ordering::Relaxed);
                }
                if let Some(state) = cell.presenter_view(next, cur, last) {
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

        self.stopping.store(false, Ordering::SeqCst);
        self.work_queue.enqueue(Tasks::Start);
    }

    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        // Taint the state because we are about to purge pending work
        self.tainted.store(true, Ordering::SeqCst);
        self.work_queue.purge();
        // We still enqueue Stop to ensure any remaining tasks that might pick it up do so,
        // though purge likely cleared it.
        self.work_queue.enqueue(Tasks::Stop);
    }

    pub fn reset(&self) {
        self.work_queue.enqueue(Tasks::Reset);
    }

    pub fn seed(&self, pattern: String) {
        self.work_queue.enqueue(Tasks::Seed(pattern));
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

    fn apply_seed(engine: &Arc<SimulationEngine>, pattern: &str) {
        match pattern {
            "glider" => engine.space.seed_glider(0, 0),
            "blinker" => engine.space.seed_blinker(0, 0),
            "r-pentomino" => engine.space.seed_r_pentomino(0, 0),
            "glider gun" => engine.space.seed_glider_gun(-20, -10),
            "spaceship" => engine.space.seed_spaceship(0, 0),
            "block" => engine.space.seed_block(0, 0),
            "beehive" => engine.space.seed_beehive(0, 0),
            "breeder 1" => engine.space.seed_breeder_1(-400, -150),
            _ => eprintln!("Unknown seed pattern: {}", pattern),
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
            let bucket_lock = bucket.read().unwrap();
            let mut records = Vec::new();
            if let Some(ref root) = bucket_lock.root {
                Self::collect_records_recursive(
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
        });
    }

    fn collect_records_recursive(
        node: &CellNode,
        cur: usize,
        last: usize,
        next: usize,
        out: &mut Vec<SnapshotRecord>,
        living_count: &mut u64,
    ) {
        if node.cell.state(cur) == CellState::Alive {
            *living_count += 1;
        }
        if let Some(state) = node.cell.presenter_view(cur, last, next) {
            let (x, y) = node.cell.coordinates();
            out.push(SnapshotRecord { x, y, state });
        }
        if let Some(ref left) = node.left {
            Self::collect_records_recursive(left, cur, last, next, out, living_count);
        }
        if let Some(ref right) = node.right {
            Self::collect_records_recursive(right, cur, last, next, out, living_count);
        }
    }

    pub fn step(&self) {
        let start_gen = self.generation();
        if self.work_queue.in_flight_count() == 0 {
            self.stopping.store(true, Ordering::SeqCst);
            self.work_queue.enqueue(Tasks::Step);
            // Wait for generation to advance
            while self.generation() <= start_gen {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
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
