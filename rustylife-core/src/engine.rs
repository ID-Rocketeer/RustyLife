use crate::cell::{Cell, CellState};
use crate::space::SimulationSpace;
use crate::tree::{CellNode, SendLockPtr, SendNodePtr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex, RwLock};
use std::thread;

pub enum WorkItem {
    Phase1(SendNodePtr),
    Phase2(SendLockPtr),
}

pub struct WorkQueue {
    queue: Mutex<Vec<WorkItem>>,
    condvar: Condvar,
    active_count: AtomicUsize,
}

impl WorkQueue {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(Vec::new()),
            condvar: Condvar::new(),
            active_count: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, item: WorkItem) {
        let mut q = self.queue.lock().unwrap();
        q.push(item);
        self.condvar.notify_one();
    }

    pub fn pop_or_wait(&self) -> Option<WorkItem> {
        let mut q = self.queue.lock().unwrap();
        loop {
            if let Some(item) = q.pop() {
                self.active_count.fetch_add(1, Ordering::SeqCst);
                return Some(item);
            }
            if self.active_count.load(Ordering::SeqCst) == 0 {
                return None;
            }
            q = self.condvar.wait(q).unwrap();
        }
    }

    pub fn finish_work(&self) {
        self.active_count.fetch_sub(1, Ordering::SeqCst);
        self.condvar.notify_all();
    }

    pub fn wait_for_quiescence(&self) {
        let mut q = self.queue.lock().unwrap();
        while !q.is_empty() || self.active_count.load(Ordering::SeqCst) != 0 {
            q = self.condvar.wait(q).unwrap();
        }
    }
}

pub trait EngineSubscriber: Send + Sync {
    /// Called when the simulation index flips.
    /// Returns true if the reader lock was successfully taken.
    fn notify_and_wait(&self) -> bool;
}

pub struct SimulationEngine {
    pub space: Arc<SimulationSpace>,
    _workers: Vec<thread::JoinHandle<()>>,
    work_queue: Arc<WorkQueue>,
    phase_barrier: Arc<Barrier>,
    _stop_flag: Arc<AtomicBool>,
    subscribers: RwLock<Vec<Arc<dyn EngineSubscriber>>>,
}

impl SimulationEngine {
    pub fn new(space: Arc<SimulationSpace>) -> Self {
        let work_queue = Arc::new(WorkQueue::new());
        let mut workers = Vec::new();
        let phase_barrier = Arc::new(Barrier::new(257));
        let stop_flag = Arc::new(AtomicBool::new(false));

        for _ in 0..256 {
            let q = Arc::clone(&work_queue);
            let s = Arc::clone(&space);
            let b = Arc::clone(&phase_barrier);
            let stop = Arc::clone(&stop_flag);

            workers.push(thread::spawn(move || {
                loop {
                    b.wait();
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }

                    loop {
                        let item = match q.pop_or_wait() {
                            Some(it) => it,
                            None => break,
                        };

                        match item {
                            WorkItem::Phase1(node_ptr) => {
                                let node = unsafe { &*node_ptr.0 };
                                let current_idx = s.read().current();
                                Self::process_node_phase1(node, &s, &q, current_idx);
                            }
                            WorkItem::Phase2(lock_ptr) => {
                                let lock = unsafe { &*lock_ptr.0 };
                                let current_idx = s.read().current();
                                let next_idx = s.read().next();
                                Self::process_node_phase2(lock, &s, &q, current_idx, next_idx);
                            }
                        }
                        q.finish_work();
                    }
                }
            }));
        }

        Self {
            space,
            _workers: workers,
            work_queue,
            phase_barrier,
            _stop_flag: stop_flag,
            subscribers: RwLock::new(Vec::new()),
        }
    }

    pub fn add_subscriber(&self, subscriber: Arc<dyn EngineSubscriber>) {
        self.subscribers.write().unwrap().push(subscriber);
    }

    fn process_node_phase1(
        node: &CellNode,
        space: &SimulationSpace,
        q: &WorkQueue,
        current_idx: usize,
    ) {
        if node.cell.state(current_idx) == CellState::Alive {
            let (x, y) = node.cell.coordinates();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x + dx;
                    let ny = y + dy;

                    space.storage.find_or_create_and_apply(
                        nx,
                        ny,
                        || {
                            let guard = space.read();
                            Cell::new(nx, ny, CellState::Dead, &guard)
                        },
                        |cell| {
                            cell.increment_neighbor_count(current_idx);
                        },
                    );
                }
            }
        }

        let left_opt = node.left.read().unwrap();
        let right_opt = node.right.read().unwrap();

        match (left_opt.as_ref(), right_opt.as_ref()) {
            (Some(left), Some(right)) => {
                q.push(WorkItem::Phase1(SendNodePtr(
                    right.as_ref() as *const CellNode
                )));
                Self::process_node_phase1(left, space, q, current_idx);
            }
            (Some(left), None) => {
                Self::process_node_phase1(left, space, q, current_idx);
            }
            (None, Some(right)) => {
                // Locality Optimization
                Self::process_node_phase1(right, space, q, current_idx);
            }
            (None, None) => {}
        }
    }

    fn process_node_phase2(
        lock: &RwLock<Option<Box<CellNode>>>,
        space: &SimulationSpace,
        q: &WorkQueue,
        current_idx: usize,
        next_idx: usize,
    ) {
        let mut write_lock = lock.write().unwrap();

        while let Some(mut node) = write_lock.take() {
            // 1. Calculate next state
            node.cell.calculate_next_state(current_idx, next_idx);

            // 2. Triple-Dead Pruning Check
            if node.cell.is_permanently_dead() {
                // Prune using standard BST delete. Standard tree deletion methods
                // will only affect child nodes and the parent.
                *write_lock = node.delete();
                // Continue to process the node that replaced it, if any.
                continue;
            }

            // 3. Not pruned. Process children.
            // Check existence without holding children write locks yet to decide on distribution.
            // We use get_mut here because we have ownership of the Box.
            let left_exists = node.left.get_mut().expect("Lock poisoned").is_some();
            let right_exists = node.right.get_mut().expect("Lock poisoned").is_some();

            let left_ptr = &node.left as *const RwLock<Option<Box<CellNode>>>;
            let right_ptr = &node.right as *const RwLock<Option<Box<CellNode>>>;

            // Put the node back so children can be processed
            *write_lock = Some(node);

            // Release parent write lock first to allow other workers into the branch.
            drop(write_lock);

            match (left_exists, right_exists) {
                (true, true) => {
                    // Queue right, recurse left.
                    q.push(WorkItem::Phase2(SendLockPtr(right_ptr)));
                    Self::process_node_phase2(
                        unsafe { &*left_ptr },
                        space,
                        q,
                        current_idx,
                        next_idx,
                    );
                }
                (true, false) => {
                    Self::process_node_phase2(
                        unsafe { &*left_ptr },
                        space,
                        q,
                        current_idx,
                        next_idx,
                    );
                }
                (false, true) => {
                    // Locality Optimization: Recurse right directly
                    Self::process_node_phase2(
                        unsafe { &*right_ptr },
                        space,
                        q,
                        current_idx,
                        next_idx,
                    );
                }
                (false, false) => {}
            }
            break;
        }
    }

    pub fn step(&self) {
        // --- Phase 1: Neighbors ---
        for bucket in &self.space.storage.buckets {
            let root_lock = bucket.root.read().unwrap();
            if let Some(ref root_node) = *root_lock {
                self.work_queue.push(WorkItem::Phase1(SendNodePtr(
                    root_node.as_ref() as *const CellNode
                )));
            }
        }
        self.phase_barrier.wait();
        self.work_queue.wait_for_quiescence();
        self.phase_barrier.wait();

        // --- Phase 2: Transition & Pruning ---
        for bucket in &self.space.storage.buckets {
            self.work_queue.push(WorkItem::Phase2(SendLockPtr(
                &bucket.root as *const RwLock<Option<Box<CellNode>>>,
            )));
        }

        self.phase_barrier.wait();
        self.work_queue.wait_for_quiescence();
        self.phase_barrier.wait();

        // --- Phase 3: Flip & Sync ---
        self.space.flip();
        let subs = self.subscribers.read().unwrap();
        for sub in subs.iter() {
            sub.notify_and_wait();
        }
    }
}
