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

    /// V3 Staged Architecture:
    /// 1. Scout: Identify potential future living cells.
    ScoutBatch(usize, usize),
    /// 2. Consolidate: Create new dead cells at potential future sites.
    ConsolidateBatch(usize, usize),
    /// 3a. Scatter: Generate neighbor count updates for neighbors.
    ScatterNeighbors(usize), // Scatter is triggered by non-empty Consolidate, usually sparse. Keep per-bucket?
    /// 3b. Gather: Apply neighbor count updates to this bucket.
    GatherNeighbors(usize), // Gather needs to happen for all buckets? No, only those with updates?
    /// 4. UpdateState: Calculate next state based on counts.
    UpdateStateBatch(usize, usize),
    /// Reset the simulation state.
    Reset,
    /// Seed the simulation with a named pattern.
    Seed(String),
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
    /// Notified when a new snapshot file is ready.
    ///
    /// The `path` provides the location of the binary-encoded generation snapshot.
    /// Returns `false` if the subscriber wants the simulation to stop.
    fn on_snapshot_available(&self, path: std::path::PathBuf) -> bool;
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
    /// Directory where binary snapshots are staged.
    pub staging_dir: std::path::PathBuf,
    generation: AtomicUsize,
    /// Count of living cells (Population) in the generation we are currently calculating (Phase 4).
    /// Maps to `total_cells` in the binary snapshot header.
    pub living_count: AtomicU64,
    /// Total count of serialized cell records for the snapshot.
    ///
    /// Includes all 4 visible lifecycle states (Newly Alive, Alive, Dying, Newly Dead).
    /// Maps to `record_count` in the binary snapshot header.
    pub record_count: AtomicU64,
    /// The last seed pattern applied, used for Reset.
    pub last_seed: Mutex<Option<String>>,
}

impl SimulationEngine {
    pub fn new(space: Arc<SimulationSpace>, pool_size: usize) -> Arc<Self> {
        let staging_dir = std::env::current_dir().unwrap().join("staging");
        Self::new_with_staging(space, pool_size, staging_dir)
    }

    pub fn new_with_staging(
        space: Arc<SimulationSpace>,
        pool_size: usize,
        staging_dir: std::path::PathBuf,
    ) -> Arc<Self> {
        let work_queue = Arc::new(WorkQueue::new());
        let bucket_count = space.storage().buckets.len();
        let scratchpad = Arc::new(Scratchpad::new(pool_size, bucket_count));
        let subscribers = Arc::new(RwLock::new(Vec::new()));

        if !staging_dir.exists() {
            std::fs::create_dir_all(&staging_dir).expect("Failed to create staging directory");
        } else {
            // Wipe the staging directory to prevent protocol version mismatch panics
            // and stale snapshots from interfering with the new run.
            for entry in std::fs::read_dir(&staging_dir).unwrap() {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }

        let engine = Arc::new(Self {
            space,
            scratchpad,
            _worker_handles: Mutex::new(Vec::new()),
            work_queue,
            subscribers,
            stopping: AtomicBool::new(true),
            tainted: AtomicBool::new(false),
            pool_size,
            staging_dir,
            generation: AtomicUsize::new(0),
            living_count: AtomicU64::new(0),
            record_count: AtomicU64::new(0),
            last_seed: Mutex::new(None),
        });

        let mut handles = engine._worker_handles.lock().unwrap();
        for thread_idx in 0..pool_size {
            let engine_arc = Arc::clone(&engine);
            handles.push(thread::spawn(move || {
                loop {
                    let task = engine_arc.work_queue.dequeue_or_idle();

                    // Wrap execution in catch_unwind to prevent partially failed phases from deadlocking the engine
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        match &task {
                            Tasks::Quit => {
                                engine_arc.work_queue.finish_work();
                                return true; // Signal to break loop
                            }
                            Tasks::Stop => {
                                engine_arc.stopping.store(true, Ordering::SeqCst);
                                engine_arc.work_queue.purge();
                                if engine_arc.work_queue.in_flight_count() > 1 {
                                    engine_arc.work_queue.enqueue(Tasks::Stop);
                                }
                            }
                            Tasks::Start => {
                                if engine_arc.work_queue.in_flight_count() == 1 {
                                    engine_arc.stopping.store(false, Ordering::SeqCst);
                                    engine_arc.tainted.store(false, Ordering::SeqCst);
                                    Self::initiate_scouting(&engine_arc);
                                }
                            }
                            Tasks::Step => {
                                if engine_arc.work_queue.in_flight_count() == 1 {
                                    engine_arc.stopping.store(true, Ordering::SeqCst);
                                    engine_arc.tainted.store(false, Ordering::SeqCst);
                                    Self::initiate_scouting(&engine_arc);
                                }
                            }
                            Tasks::ScoutBatch(start, end) => {
                                for i in *start..*end {
                                    // Optimization: Move the "is_empty" check here (Parallel!)
                                    // But wait, scout_bucket checks `root` anyway? No.
                                    // `scout_bucket` calls `scout_recursive`.
                                    // We should add the check inside scout_bucket or here.
                                    // Adding it here avoids function call overhead.
                                    if engine_arc.space.storage().buckets[i]
                                        .read()
                                        .unwrap()
                                        .root
                                        .is_some()
                                    {
                                        Self::scout_bucket(i, &engine_arc, thread_idx);
                                    }
                                }
                            }
                            Tasks::ConsolidateBatch(start, end) => {
                                for i in *start..*end {
                                    Self::consolidate_bucket(&engine_arc, i);
                                }
                            }
                            Tasks::ScatterNeighbors(bucket_idx) => {
                                Self::scatter_neighbors(*bucket_idx, &engine_arc, thread_idx);
                            }
                            Tasks::GatherNeighbors(bucket_idx) => {
                                Self::gather_neighbors(*bucket_idx, &engine_arc);
                            }
                            Tasks::UpdateStateBatch(start, end) => {
                                for i in *start..*end {
                                    if engine_arc.space.storage().buckets[i]
                                        .read()
                                        .unwrap()
                                        .root
                                        .is_some()
                                    {
                                        Self::run_state_update_in_bucket(i, &engine_arc);
                                    }
                                }
                            }
                            Tasks::Reset => {
                                engine_arc.space.clear();
                                engine_arc.generation.store(0, Ordering::SeqCst);
                                engine_arc.living_count.store(0, Ordering::SeqCst);
                                engine_arc.record_count.store(0, Ordering::SeqCst);
                                let seed = engine_arc.last_seed.lock().unwrap().clone();
                                if let Some(pattern) = seed {
                                    Self::apply_seed(&engine_arc, &pattern);
                                }
                                Self::trigger_initial_snapshot(&engine_arc);
                            }
                            Tasks::Seed(pattern) => {
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
        match completed_task {
            Tasks::Start | Tasks::Step | Tasks::ScoutBatch(_, _) => {
                // If we hit this, it means all scouting tasks (if any) are done.
                if !Self::initiate_consolidation(engine) {
                    // No consolidation work? Jump to scatter
                    Self::handle_emergent_transition(engine, &Tasks::ConsolidateBatch(0, 0));
                }
            }
            Tasks::ConsolidateBatch(_, _) => {
                if engine.work_queue.in_flight_count() == 0 {
                    // Move to Scatter (previously initiate_neighbor_count_updates)
                    // We need a helper for initiating scatter
                    if !Self::initiate_scatter_neighbors(engine) {
                        // No living cells to scatter? Jump to Gather (which will also be empty)
                        Self::handle_emergent_transition(
                            engine,
                            &Tasks::ScatterNeighbors(usize::MAX),
                        );
                    }
                }
            }
            Tasks::ScatterNeighbors(_) => {
                if engine.work_queue.in_flight_count() == 0 {
                    // All scattering done. Now initiate Gather.
                    if !Self::initiate_gather_neighbors(engine) {
                        Self::handle_emergent_transition(
                            engine,
                            &Tasks::GatherNeighbors(usize::MAX),
                        );
                    }
                }
            }
            Tasks::GatherNeighbors(_) => {
                if engine.work_queue.in_flight_count() == 0 {
                    // Gather done. Reset counts.
                    engine.living_count.store(0, Ordering::SeqCst);
                    engine.record_count.store(0, Ordering::SeqCst);

                    if !Self::initiate_state_updates(&engine) {
                        // All dead?
                        Self::handle_emergent_transition(engine, &Tasks::UpdateStateBatch(0, 0));
                    }
                }
            }
            Tasks::UpdateStateBatch(_, _) => {
                // Generation cycle complete.
                engine.space.advance_generation();
                let generation_count = engine.generation.fetch_add(1, Ordering::SeqCst) + 1;

                // Phase 5: Snapshot Staging
                let tmp_path = engine
                    .staging_dir
                    .join(format!("gen_{}.tmp", generation_count));
                let bin_path = engine
                    .staging_dir
                    .join(format!("gen_{}.bin", generation_count));

                if let Err(e) = engine.space.encode_to_file(
                    &tmp_path,
                    generation_count as u64,
                    engine.living_count.load(Ordering::SeqCst),
                    !engine.stopping.load(Ordering::SeqCst),
                    engine.record_count.load(Ordering::SeqCst),
                ) {
                    eprintln!("Failed to write snapshot: {}", e);
                } else {
                    // Atomic rename
                    if let Err(e) = std::fs::rename(&tmp_path, &bin_path) {
                        eprintln!("Failed to rename snapshot: {}", e);
                    }
                }

                let keep_running = {
                    let subs = engine.subscribers.read().unwrap();
                    let mut all_true = true;
                    for s in subs.iter() {
                        if !s.on_snapshot_available(bin_path.clone()) {
                            all_true = false;
                        }
                    }
                    all_true
                };

                // Windows Grooming: Cleanup old files
                if generation_count > 5 {
                    let old_gen = generation_count - 5;
                    let old_path = engine.staging_dir.join(format!("gen_{}.bin", old_gen));
                    if old_path.exists() {
                        // Best effort delete (Windows might have it locked)
                        let _ = std::fs::remove_file(old_path);
                    }
                }

                if keep_running && !engine.stopping.load(Ordering::SeqCst) {
                    engine.work_queue.enqueue(Tasks::Start);
                }
            }
            _ => {}
        }
    }

    fn initiate_scouting(engine: &Arc<Self>) {
        let bucket_count = engine.space.storage().buckets.len();
        let pool_size = engine.pool_size;

        // Target ~4 batches per thread for good load balancing
        let total_batches = std::cmp::max(1, pool_size * 4);
        let batch_size = (bucket_count + total_batches - 1) / total_batches;

        let mut tasks = Vec::with_capacity(total_batches);
        for i in (0..bucket_count).step_by(batch_size) {
            let end = std::cmp::min(i + batch_size, bucket_count);
            tasks.push(Tasks::ScoutBatch(i, end));
        }

        if tasks.is_empty() {
            // Should not happen unless 0 buckets
            if !Self::initiate_consolidation(engine) {
                Self::handle_emergent_transition(engine, &Tasks::ConsolidateBatch(0, 0));
            }
        } else {
            engine.work_queue.enqueue_batch(tasks);
        }
    }

    fn scout_bucket(bucket_idx: usize, engine: &Arc<Self>, thread_idx: usize) {
        let storage = engine.space.storage();
        if bucket_idx >= storage.buckets.len() {
            return;
        }

        let bucket_lock = storage.buckets[bucket_idx].read().unwrap();
        let current_mask = engine.space.mask.read().current_state_mask();
        if let Some(ref node) = bucket_lock.root {
            Self::scout_recursive(node, engine, current_mask, thread_idx);
        }
    }

    fn scout_recursive(node: &CellNode, engine: &Arc<Self>, mask: usize, thread_idx: usize) {
        if node.cell.state(mask) == CellState::Alive {
            let (x, y) = node.cell.coordinates();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    engine.scratchpad.push_candidate(thread_idx, x + dx, y + dy);
                }
            }
        }

        if let Some(ref left) = node.left {
            Self::scout_recursive(left, engine, mask, thread_idx);
        }
        if let Some(ref right) = node.right {
            Self::scout_recursive(right, engine, mask, thread_idx);
        }
    }

    fn initiate_consolidation(engine: &Arc<Self>) -> bool {
        let bucket_count = engine.scratchpad.bucket_count();
        let pool_size = engine.pool_size;

        // Target ~4 batches per thread
        let total_batches = std::cmp::max(1, pool_size * 4);
        let batch_size = (bucket_count + total_batches - 1) / total_batches;

        let mut tasks = Vec::with_capacity(total_batches);
        for i in (0..bucket_count).step_by(batch_size) {
            let end = std::cmp::min(i + batch_size, bucket_count);
            tasks.push(Tasks::ConsolidateBatch(i, end));
        }

        if tasks.is_empty() {
            false
        } else {
            engine.work_queue.enqueue_batch(tasks);
            true
        }
    }

    fn consolidate_bucket(engine: &Arc<Self>, bucket_idx: usize) {
        let storage = engine.space.storage();
        if bucket_idx >= storage.buckets.len() {
            return;
        }

        let mut candidates = engine.scratchpad.get_column(bucket_idx);

        candidates.sort_unstable();
        candidates.dedup();

        let coords_vec: Vec<(i128, i128)> = candidates.iter().map(|c| (c.x, c.y)).collect();

        let mut bucket = storage.buckets[bucket_idx].write().unwrap();
        let current_mask = engine.space.mask.read().current_state_mask();
        bucket.merge_and_rebuild(&coords_vec, current_mask);
    }

    fn initiate_state_updates(engine: &Arc<Self>) -> bool {
        let bucket_count = engine.space.storage().buckets.len();
        let pool_size = engine.pool_size;

        let total_batches = std::cmp::max(1, pool_size * 4);
        let batch_size = (bucket_count + total_batches - 1) / total_batches;

        let mut tasks = Vec::with_capacity(total_batches);
        for i in (0..bucket_count).step_by(batch_size) {
            let end = std::cmp::min(i + batch_size, bucket_count);
            tasks.push(Tasks::UpdateStateBatch(i, end));
        }

        if tasks.is_empty() {
            false
        } else {
            engine.work_queue.enqueue_batch(tasks);
            true
        }
    }

    pub fn start(&self) {
        if self.work_queue.in_flight_count() == 0 {
            // Check if we were stopped aggressively
            if self.tainted.load(Ordering::SeqCst) {
                // We need to repair the state before starting.
                // We can't call self.repair() directly because it needs an Arc<Self>
                // Hack: We assume the caller has an Arc, but here we only have &self.
                // However, repair() is internal helpers. We can make a static version or
                // just inline the logic since we have access to fields.
                self.scratchpad.clear();
                self.space.repair();
                self.tainted.store(false, Ordering::SeqCst);
            }

            self.stopping.store(false, Ordering::SeqCst);
            self.work_queue.enqueue(Tasks::Start);
        }
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
        let tmp_path = engine
            .staging_dir
            .join(format!("gen_{}.tmp", generation_count));
        let bin_path = engine
            .staging_dir
            .join(format!("gen_{}.bin", generation_count));

        // Gather statistics for the initial state
        let mut cells = Vec::new();
        let guard = engine.space.mask.read();
        engine.space.storage().collect_all(
            guard.current_state_mask(),
            guard.last_state_mask(),
            guard.next_state_mask(),
            &mut cells,
        );
        let living = cells.iter().filter(|(_, s)| (*s & 0b10) != 0).count() as u64;
        let records = cells.len() as u64;

        if let Ok(_) = engine.space.encode_to_file(
            &tmp_path,
            generation_count as u64,
            living,
            !engine.stopping.load(Ordering::SeqCst),
            records,
        ) {
            let _ = std::fs::rename(&tmp_path, &bin_path);
            let subs = engine.subscribers.read().unwrap();
            for s in subs.iter() {
                let _ = s.on_snapshot_available(bin_path.clone());
            }
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

    fn initiate_scatter_neighbors(engine: &Arc<Self>) -> bool {
        let storage = engine.space.storage();
        let mut tasks = Vec::new();
        for i in 0..storage.buckets.len() {
            if storage.buckets[i].read().unwrap().root.is_some() {
                tasks.push(Tasks::ScatterNeighbors(i));
            }
        }
        if tasks.is_empty() {
            false
        } else {
            engine.work_queue.enqueue_batch(tasks);
            true
        }
    }

    fn scatter_neighbors(bucket_idx: usize, engine: &Arc<Self>, thread_idx: usize) {
        let storage = engine.space.storage();
        if bucket_idx >= storage.buckets.len() {
            return;
        }

        let bucket_lock = storage.buckets[bucket_idx].read().unwrap();
        let current_mask = engine.space.mask.read().current_state_mask();
        if let Some(ref node) = bucket_lock.root {
            Self::scatter_recursive(node, engine, current_mask, thread_idx);
        }
    }

    fn scatter_recursive(node: &CellNode, engine: &Arc<Self>, mask: usize, thread_idx: usize) {
        if node.cell.state(mask) == CellState::Alive {
            let (x, y) = node.cell.coordinates();
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
            Self::scatter_recursive(left, engine, mask, thread_idx);
        }
        if let Some(ref right) = node.right {
            Self::scatter_recursive(right, engine, mask, thread_idx);
        }
    }

    fn initiate_gather_neighbors(engine: &Arc<Self>) -> bool {
        let mut tasks = Vec::new();
        // Unlike scatter/scout which depend on existing cells,
        // Gather depends on Scratchpad contents.
        // We iterate over all buckets because any bucket might be a target.
        // Optimization: Skip empty columns in scratchpad?
        // For now, simple iteration.
        for bucket_idx in 0..engine.scratchpad.bucket_count() {
            tasks.push(Tasks::GatherNeighbors(bucket_idx));
        }

        if tasks.is_empty() {
            false
        } else {
            engine.work_queue.enqueue_batch(tasks);
            true
        }
    }

    fn gather_neighbors(bucket_idx: usize, engine: &Arc<Self>) {
        let space = &engine.space;
        let mut incoming = engine.scratchpad.get_column(bucket_idx);
        // Sort for batch application (required by apply_batch)
        incoming.sort_unstable_by(|a, b| {
            let coord_a = (a.x, a.y);
            let coord_b = (b.x, b.y);
            crate::tree::CellTree::compare_coords(coord_a, coord_b)
        });

        let coords: Vec<(i128, i128)> = incoming.iter().map(|c| (c.x, c.y)).collect();

        let mut bucket_lock = space.storage().buckets[bucket_idx].write().unwrap();
        let guard = space.mask.read();
        let current_mask = guard.current_state_mask();

        bucket_lock.apply_batch(
            &coords,
            |x, y| Cell::new(x, y, CellState::Dead, current_mask),
            |cell| cell.increment_neighbor_count(current_mask),
        );
    }

    fn run_state_update_in_bucket(bucket_idx: usize, engine: &Arc<Self>) {
        let storage = engine.space.storage();
        if bucket_idx >= storage.buckets.len() {
            return;
        }

        let bucket_lock = storage.buckets[bucket_idx].read().unwrap();
        let masks = engine.space.mask.read();

        if let Some(ref node) = bucket_lock.root {
            Self::run_state_update_recursive(
                node,
                engine,
                masks.current_state_mask(),
                masks.next_state_mask(),
                masks.last_state_mask(),
            );
        }
    }

    fn run_state_update_recursive(
        node: &CellNode,
        engine: &Arc<Self>,
        cur: usize,
        next: usize,
        last: usize,
    ) {
        let next_state = node.cell.calculate_next_state(cur, next);
        if next_state == CellState::Alive {
            engine.living_count.fetch_add(1, Ordering::Relaxed);
        }
        if node.cell.presenter_view(next, cur, last).is_some() {
            engine.record_count.fetch_add(1, Ordering::Relaxed);
        }

        if let Some(ref left) = node.left {
            Self::run_state_update_recursive(left, engine, cur, next, last);
        }
        if let Some(ref right) = node.right {
            Self::run_state_update_recursive(right, engine, cur, next, last);
        }
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
