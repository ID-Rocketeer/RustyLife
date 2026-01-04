use rustylife_core::cell::{Cell, CellState};
use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct MockSubscriber {
    signaled: AtomicBool,
}

impl EngineSubscriber for MockSubscriber {
    fn notify_and_wait(&self) -> bool {
        self.signaled.store(true, Ordering::SeqCst);
        true
    }
}

#[test]
fn test_engine_single_step_cycle() {
    let space = Arc::new(SimulationSpace::new());
    let engine = SimulationEngine::new(Arc::clone(&space));

    let sub = Arc::new(MockSubscriber {
        signaled: AtomicBool::new(false),
    });
    engine.add_subscriber(sub.clone());

    // 1. Setup initial state: A lonely living cell (should die)
    {
        let guard = space.read();
        space
            .storage
            .insert(Cell::new(0, 0, CellState::Alive, &guard));
    }

    // 2. Run one step
    engine.step();

    // 3. Verify results
    let guard = space.read();
    let current = guard.current();

    // The cell at (0, 0) should have died due to underpopulation (0 neighbors)
    space.storage.find_and_apply(0, 0, |cell| {
        assert_eq!(cell.state(current), CellState::Dead);
    });

    // Phase 3 subscriber should have been notified
    assert!(sub.signaled.load(Ordering::SeqCst));
}

#[test]
fn test_engine_blinker_reproduction() {
    let space = Arc::new(SimulationSpace::new());
    let engine = SimulationEngine::new(Arc::clone(&space));

    // A block of 3 cells (Blinker part 1)
    // (0,0), (1,0), (2,0) -> Alive
    // Next generation should have (1,-1), (1,0), (1,1) -> Alive
    {
        let guard = space.read();
        space
            .storage
            .insert(Cell::new(0, 0, CellState::Alive, &guard));
        space
            .storage
            .insert(Cell::new(1, 0, CellState::Alive, &guard));
        space
            .storage
            .insert(Cell::new(2, 0, CellState::Alive, &guard));
    }

    engine.step();

    let guard = space.read();
    let current = guard.current();

    // (1,0) should stay alive
    space
        .storage
        .find_and_apply(1, 0, |c| assert_eq!(c.state(current), CellState::Alive));
    // (1,1) should be born
    space
        .storage
        .find_and_apply(1, 1, |c| assert_eq!(c.state(current), CellState::Alive));
    // (1,-1) should be born
    space
        .storage
        .find_and_apply(1, -1, |c| assert_eq!(c.state(current), CellState::Alive));

    // (0,0) and (2,0) should have died
    space
        .storage
        .find_and_apply(0, 0, |c| assert_eq!(c.state(current), CellState::Dead));
    space
        .storage
        .find_and_apply(2, 0, |c| assert_eq!(c.state(current), CellState::Dead));
}
