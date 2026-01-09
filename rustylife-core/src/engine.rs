use crate::cell::{Cell, CellState};
use crate::space::SimulationSpace;
use crate::tree::{CellNode, SendLockUnitPtr, SendUnitPtr};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;

pub enum UnitOfWork {
    Start,
    Stop,
    Step,
    Done(u8),
    Quit,
    NeighborSpread(SendUnitPtr),
    StateUpdate(SendLockUnitPtr),
}

pub struct WorkQueue {
    queue: Mutex<VecDeque<UnitOfWork>>,
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

    pub fn enqueue(&self, item: UnitOfWork) {
        let mut q = self.queue.lock().unwrap();
        self.in_flight_count.fetch_add(1, Ordering::SeqCst);
        q.push_back(item);
        self.condvar.notify_one();
    }

    pub fn purge(&self) {
        let mut q = self.queue.lock().unwrap();
        let cleared = q.len();
        q.clear();
        self.in_flight_count.fetch_sub(cleared, Ordering::SeqCst);
    }

    pub fn in_flight_count(&self) -> usize {
        self.in_flight_count.load(Ordering::SeqCst)
    }

    pub fn dequeue_or_idle(&self) -> UnitOfWork {
        let mut q = self.queue.lock().unwrap();
        loop {
            if let Some(item) = q.pop_front() {
                return item;
            }
            q = self.condvar.wait(q).unwrap();
        }
    }

    pub fn finish_work(&self) -> usize {
        let count = self.in_flight_count.fetch_sub(1, Ordering::SeqCst) - 1;
        if count == 0 {
            self.condvar.notify_all();
        }
        count
    }
}

pub trait EngineSubscriber: Send + Sync {
    fn notify_and_wait(&self) -> bool;
}

pub struct SimulationEngine {
    pub space: Arc<SimulationSpace>,
    _worker_handles: Mutex<Vec<thread::JoinHandle<()>>>,
    work_queue: Arc<WorkQueue>,
    subscribers: Arc<RwLock<Vec<Arc<dyn EngineSubscriber>>>>,
}

impl SimulationEngine {
    pub fn new(space: Arc<SimulationSpace>) -> Arc<Self> {
        let work_queue = Arc::new(WorkQueue::new());
        let subscribers = Arc::new(RwLock::new(Vec::new()));

        let engine = Arc::new(Self {
            space,
            _worker_handles: Mutex::new(Vec::new()),
            work_queue,
            subscribers,
        });

        let mut handles = engine._worker_handles.lock().unwrap();
        for _ in 0..256 {
            let e = Arc::clone(&engine);
            handles.push(thread::spawn(move || {
                loop {
                    let item = e.work_queue.dequeue_or_idle();
                    match item {
                        UnitOfWork::Quit => break,
                        UnitOfWork::Stop => {
                            e.work_queue.purge();
                            // Only re-enqueue Stop if there are other items in flight to be purged.
                            if e.work_queue.in_flight_count() > 1 {
                                e.work_queue.enqueue(UnitOfWork::Stop);
                            }
                        }
                        UnitOfWork::Start => {
                            // Only start if truly idle (we are the only item).
                            if e.work_queue.in_flight_count() == 1 {
                                Self::start_phase_neighbors(&e.space, &e.work_queue);
                            }
                        }
                        UnitOfWork::Step => {
                            if e.work_queue.in_flight_count() == 1 {
                                Self::start_phase_neighbors(&e.space, &e.work_queue);
                                e.work_queue.enqueue(UnitOfWork::Done(1));
                            }
                        }
                        UnitOfWork::Done(phase) => {
                            if e.work_queue.in_flight_count() > 1 {
                                // Others busy, requeue sentinel at the end
                                e.work_queue.enqueue(UnitOfWork::Done(phase));
                            } else {
                                // I am the last. Coordinate Step transition.
                                if phase == 1 {
                                    Self::start_phase_state_update(&e.space, &e.work_queue);
                                    e.work_queue.enqueue(UnitOfWork::Done(2));
                                } else {
                                    e.space.flip();
                                    {
                                        let subs = e.subscribers.read().unwrap();
                                        for sub in subs.iter() {
                                            sub.notify_and_wait();
                                        }
                                    }
                                }
                            }
                        }
                        UnitOfWork::NeighborSpread(ptr) => {
                            let node = unsafe { &*ptr.0 };
                            let current_mask = e.space.index.read().current_state_mask();
                            Self::run_neighbor_spread(node, &e, current_mask);
                        }
                        UnitOfWork::StateUpdate(ptr) => {
                            let lock = unsafe { &*ptr.0 };
                            let masks = e.space.index.read();
                            Self::run_state_update(
                                lock,
                                &e,
                                masks.current_state_mask(),
                                masks.next_state_mask(),
                            );
                        }
                    }

                    let remaining = e.work_queue.finish_work();
                    if remaining == 0 {
                        // Quiescence reached!
                        Self::handle_emergent_transition(&e, &item);
                    }
                }
            }));
        }
        drop(handles);

        engine
    }

    fn handle_emergent_transition(engine: &Arc<Self>, last_token: &UnitOfWork) {
        // Quiescence detected. The queue is empty and active_count is 0.
        match last_token {
            UnitOfWork::NeighborSpread(_) => {
                // Neighbors done. Start Phase 2.
                Self::start_phase_state_update(&engine.space, &engine.work_queue);
            }
            UnitOfWork::StateUpdate(_) => {
                // Generation done. Flip and restart IF NOT in stop/step mode.
                engine.space.flip();
                {
                    let subs = engine.subscribers.read().unwrap();
                    for sub in subs.iter() {
                        sub.notify_and_wait();
                    }
                }
                // Free-run detection: if queue is still empty, we continue.
                // If a Done or Stop was processed, we remain Idle.
                Self::start_phase_neighbors(&engine.space, &engine.work_queue);
            }
            UnitOfWork::Done(_) => {
                // Done already handled its own transitions in the worker loop.
            }
            _ => {} // Start/Stop/Quit don't trigger transitions themselves.
        }
    }

    fn start_phase_neighbors(space: &SimulationSpace, q: &WorkQueue) {
        for bucket in &space.storage.buckets {
            let root_lock = bucket.root.read().unwrap();
            if let Some(ref root_node) = *root_lock {
                q.enqueue(UnitOfWork::NeighborSpread(SendUnitPtr(
                    root_node.as_ref() as *const CellNode
                )));
            }
        }
    }

    fn start_phase_state_update(space: &SimulationSpace, q: &WorkQueue) {
        for bucket in &space.storage.buckets {
            q.enqueue(UnitOfWork::StateUpdate(SendLockUnitPtr(
                &bucket.root as *const RwLock<Option<Box<CellNode>>>,
            )));
        }
    }

    pub fn start(&self) {
        if self.work_queue.in_flight_count() == 0 {
            self.work_queue.enqueue(UnitOfWork::Start);
        }
    }

    pub fn stop(&self) {
        self.work_queue.purge();
        self.work_queue.enqueue(UnitOfWork::Stop);
    }

    pub fn step(&self) {
        if self.work_queue.in_flight_count() == 0 {
            self.work_queue.enqueue(UnitOfWork::Step);
        }
    }

    pub fn add_subscriber(&self, subscriber: Arc<dyn EngineSubscriber>) {
        self.subscribers.write().unwrap().push(subscriber);
    }

    fn run_neighbor_spread(
        mut unit: &CellNode,
        engine: &Arc<SimulationEngine>,
        current_mask: usize,
    ) {
        loop {
            if unit.cell.state(current_mask) == CellState::Alive {
                let (x, y) = unit.cell.coordinates();
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x + dx;
                        let ny = y + dy;

                        engine.space.storage.find_or_create_and_apply(
                            nx,
                            ny,
                            || {
                                let guard = engine.space.read();
                                Cell::new(nx, ny, CellState::Dead, &guard)
                            },
                            |cell| {
                                cell.increment_neighbor_count(current_mask);
                            },
                        );
                    }
                }
            }

            let left_opt = unit.left.read().unwrap();
            let right_opt = unit.right.read().unwrap();

            match (left_opt.as_ref(), right_opt.as_ref()) {
                (Some(left), Some(right)) => {
                    engine
                        .work_queue
                        .enqueue(UnitOfWork::NeighborSpread(SendUnitPtr(
                            right.as_ref() as *const CellNode
                        )));
                    unit = left;
                }
                (Some(left), None) => {
                    unit = left;
                }
                (None, Some(right)) => {
                    unit = right;
                }
                (None, None) => break,
            }
        }
    }

    fn run_state_update(
        lock_unit: &RwLock<Option<Box<CellNode>>>,
        engine: &Arc<SimulationEngine>,
        current_mask: usize,
        next_mask: usize,
    ) {
        let mut target_lock = lock_unit;
        loop {
            let mut write_lock = target_lock.write().unwrap();
            if let Some(mut node) = write_lock.take() {
                node.cell.calculate_next_state(current_mask, next_mask);
                if node.cell.is_permanently_dead() {
                    *write_lock = node.delete();
                    drop(write_lock);
                    // Tree structure changed, but we can't easily "loop" here without complex logic.
                    // Actually, if we delete a node, we just return.
                    break;
                }

                let left_exists = node.left.get_mut().expect("Lock poisoned").is_some();
                let right_exists = node.right.get_mut().expect("Lock poisoned").is_some();
                let left_ptr = &node.left as *const RwLock<Option<Box<CellNode>>>;
                let right_ptr = &node.right as *const RwLock<Option<Box<CellNode>>>;

                *write_lock = Some(node);
                drop(write_lock);

                match (left_exists, right_exists) {
                    (true, true) => {
                        engine
                            .work_queue
                            .enqueue(UnitOfWork::StateUpdate(SendLockUnitPtr(right_ptr)));
                        target_lock = unsafe { &*left_ptr };
                    }
                    (true, false) => {
                        target_lock = unsafe { &*left_ptr };
                    }
                    (false, true) => {
                        target_lock = unsafe { &*right_ptr };
                    }
                    (false, false) => break,
                }
            } else {
                drop(write_lock);
                break;
            }
        }
    }

    pub fn get_cells_in_rect(
        &self,
        min: (i128, i128),
        max: (i128, i128),
    ) -> Vec<((i128, i128), u8)> {
        let mut out = Vec::new();
        let guard = self.space.index.read();
        let current_mask = guard.current_state_mask();
        let last_mask = guard.last_state_mask();
        self.space
            .storage
            .collect_in_rect(min, max, current_mask, last_mask, &mut out);
        out
    }
}

impl Drop for SimulationEngine {
    fn drop(&mut self) {
        self.work_queue.purge();
        for _ in 0..256 {
            self.work_queue.enqueue(UnitOfWork::Quit);
        }
        let mut handles = self._worker_handles.lock().unwrap();
        for handle in handles.drain(..) {
            let _ = handle.join();
        }
    }
}
