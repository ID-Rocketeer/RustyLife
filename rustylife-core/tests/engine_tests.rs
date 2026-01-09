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
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 3. Verify results
    let guard = space.read();
    let current = guard.current_state_mask();

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
    // In decentralized mode, step() is asynchronous.
    // Wait for the workers to finish the cycle.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let guard = space.read();
    let current = guard.current_state_mask();

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

#[test]
fn test_engine_autonomous_start_stop() {
    let space = Arc::new(SimulationSpace::new());
    let engine = SimulationEngine::new(Arc::clone(&space));

    // Blinker vertical
    {
        let guard = space.read();
        space
            .storage
            .insert(Cell::new(1, 0, CellState::Alive, &guard));
        space
            .storage
            .insert(Cell::new(1, 1, CellState::Alive, &guard));
        space
            .storage
            .insert(Cell::new(1, 2, CellState::Alive, &guard));
    }

    // 1. Start the engine
    engine.start();

    // 2. Wait for a few generations
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 3. Stop the engine
    engine.stop();

    // 4. Wait for it to quiesce
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 5. Verify it stopped and the state is valid
    let guard = space.read();
    let current = guard.current_state_mask();

    // Middle cell (1,1) is always stable in a blinker
    space
        .storage
        .find_and_apply(1, 1, |c| assert_eq!(c.state(current), CellState::Alive));
}

#[test]
fn test_engine_start_ignore_if_running() {
    let space = Arc::new(SimulationSpace::new());
    let engine = SimulationEngine::new(Arc::clone(&space));

    engine.start();
    // Second start should be ignored as per the "Only Start if Idle" rule.
    engine.start();

    engine.stop();
}

#[test]
fn test_four_gliders_stability() {
    let space = Arc::new(SimulationSpace::new());
    let engine = SimulationEngine::new(Arc::clone(&space));

    // Glider helper: (1,0), (2,1), (0,2), (1,2), (2,2)
    let add_glider = |ox: i128, oy: i128, dx: i128, dy: i128| {
        let guard = space.read();
        let pts = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
        for (px, py) in pts {
            let x = ox + px * dx;
            let y = oy + py * dy;
            space
                .storage
                .insert(Cell::new(x, y, CellState::Alive, &guard));
        }
    };

    // 1. Setup 4 gliders moving away from each other
    add_glider(10, 10, 1, 1); // SE: (+, +)
    add_glider(-10, 10, -1, 1); // SW: (-, +)
    add_glider(-10, -10, -1, -1); // NW: (-, -)
    add_glider(10, -10, 1, -1); // NE: (+, -)

    // 2. Setup generation counter subscriber
    struct Counter(std::sync::atomic::AtomicUsize, std::sync::mpsc::Sender<()>);
    impl EngineSubscriber for Counter {
        fn notify_and_wait(&self) -> bool {
            let val = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if val >= 12 {
                let _ = self.1.send(());
            }
            true
        }
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let counter = Arc::new(Counter(std::sync::atomic::AtomicUsize::new(0), tx));
    engine.add_subscriber(counter);

    // 3. Start simulation
    engine.start();

    // 4. Wait for 12 generations
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .expect("Simulation timed out or failed");

    // 5. Stop and verify
    engine.stop();
    // Small wait for quiescence
    std::thread::sleep(std::time::Duration::from_millis(50));

    let guard = space.read();
    let current = guard.current_state_mask();

    // Verify each glider has moved exactly (3, 3) relative to its direction
    let check_glider = |ox: i128, oy: i128, dx: i128, dy: i128| {
        let pts = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
        for (px, py) in pts {
            let x = (ox + px * dx) + (3 * dx);
            let y = (oy + py * dy) + (3 * dy);
            space.storage.find_and_apply(x, y, |c| {
                assert_eq!(
                    c.state(current),
                    CellState::Alive,
                    "Glider part at ({}, {}) failed",
                    x,
                    y
                );
            });
        }
    };

    check_glider(10, 10, 1, 1);
    check_glider(-10, 10, -1, 1);
    check_glider(-10, -10, -1, -1);
    check_glider(10, -10, 1, -1);
}
