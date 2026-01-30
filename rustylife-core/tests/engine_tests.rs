use rustylife_core::cell::{Cell, CellState};
use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct MockSubscriber {
    signaled: AtomicBool,
}

impl EngineSubscriber for MockSubscriber {
    fn on_snapshot_available(&self, path: std::path::PathBuf) -> bool {
        if path.exists() {
            self.signaled.store(true, Ordering::SeqCst);
        }
        true
    }
}

#[test]
fn test_dot() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(0, 0, CellState::Alive, mask));
    }

    engine.step();
    std::thread::sleep(std::time::Duration::from_millis(50));

    let guard = space.read();
    let current = guard.current_state_mask();
    space.storage().find_and_apply(0, 0, |cell| {
        assert_eq!(
            cell.state(current),
            CellState::Dead,
            "Single cell should die"
        );
    });
}

#[test]
fn test_pre_block() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // L-shape (pre-block)
    // (0,0), (1,0), (0,1)
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(0, 0, CellState::Alive, mask));
        space
            .storage()
            .insert(Cell::new(1, 0, CellState::Alive, mask));
        space
            .storage()
            .insert(Cell::new(0, 1, CellState::Alive, mask));
    }

    // Gen 1: Should become a 2x2 block
    engine.step();
    std::thread::sleep(std::time::Duration::from_millis(50));

    {
        let guard = space.read();
        let current = guard.current_state_mask();
        let pts = [(0, 0), (1, 0), (0, 1), (1, 1)];
        for (x, y) in pts {
            space.storage().find_and_apply(x, y, |c| {
                assert_eq!(
                    c.state(current),
                    CellState::Alive,
                    "Block cell at ({},{}) should be alive",
                    x,
                    y
                );
            });
        }
    }

    // Gen 2: Block should persist (stable)
    engine.step();
    std::thread::sleep(std::time::Duration::from_millis(50));

    {
        let guard = space.read();
        let current = guard.current_state_mask();
        let pts = [(0, 0), (1, 0), (0, 1), (1, 1)];
        for (x, y) in pts {
            space.storage().find_and_apply(x, y, |c| {
                assert_eq!(
                    c.state(current),
                    CellState::Alive,
                    "Stable block cell at ({},{}) should be alive",
                    x,
                    y
                );
            });
        }
    }
}

#[test]
fn test_single_glider() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // Glider at (0,0): (1,0), (2,1), (0,2), (1,2), (2,2)
    let initial_pts = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        for (px, py) in initial_pts {
            space
                .storage()
                .insert(Cell::new(px, py, CellState::Alive, mask));
        }
    }

    // Run 4 generations (one full cycle = 1 cell diagonal shift)
    for _ in 0..4 {
        engine.step();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let guard = space.read();
    let current = guard.current_state_mask();

    // Verify shift to (1,1)
    for (px, py) in initial_pts {
        let tx = px + 1;
        let ty = py + 1;
        space.storage().find_and_apply(tx, ty, |c| {
            assert_eq!(
                c.state(current),
                CellState::Alive,
                "Glider cell at ({},{}) should be alive after 4 gens",
                tx,
                ty
            );
        });
    }
}

#[test]
fn test_engine_single_step_cycle() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    let sub = Arc::new(MockSubscriber {
        signaled: AtomicBool::new(false),
    });
    engine.add_subscriber(sub.clone());

    // 1. Setup initial state: A lonely living cell (should die)
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(0, 0, CellState::Alive, mask));
    }

    // 2. Run one step
    engine.step();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 3. Verify results
    let guard = space.read();
    let current = guard.current_state_mask();

    // The cell at (0, 0) should have died due to underpopulation (0 neighbors)
    space.storage().find_and_apply(0, 0, |cell| {
        assert_eq!(cell.state(current), CellState::Dead);
    });

    // Subscriber should have been notified
    assert!(sub.signaled.load(Ordering::SeqCst));
}

#[test]
fn test_engine_blinker_reproduction() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // A block of 3 cells (Blinker part 1)
    // (0,0), (1,0), (2,0) -> Alive
    // Next generation should have (1,-1), (1,0), (1,1) -> Alive
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(0, 0, CellState::Alive, mask));
        space
            .storage()
            .insert(Cell::new(1, 0, CellState::Alive, mask));
        space
            .storage()
            .insert(Cell::new(2, 0, CellState::Alive, mask));
    }

    engine.step();
    // In decentralized mode, step() is asynchronous.
    // Wait for the workers to finish the cycle.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let guard = space.read();
    let current = guard.current_state_mask();

    // (1,0) should stay alive
    space
        .storage()
        .find_and_apply(1, 0, |c| assert_eq!(c.state(current), CellState::Alive));
    // (1,1) should be born
    space
        .storage()
        .find_and_apply(1, 1, |c| assert_eq!(c.state(current), CellState::Alive));
    // (1,-1) should be born
    space
        .storage()
        .find_and_apply(1, -1, |c| assert_eq!(c.state(current), CellState::Alive));

    // (0,0) and (2,0) should have died
    space
        .storage()
        .find_and_apply(0, 0, |c| assert_eq!(c.state(current), CellState::Dead));
    space
        .storage()
        .find_and_apply(2, 0, |c| assert_eq!(c.state(current), CellState::Dead));
}

#[test]
fn test_engine_autonomous_start_stop() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // Blinker vertical
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(1, 0, CellState::Alive, mask));
        space
            .storage()
            .insert(Cell::new(1, 1, CellState::Alive, mask));
        space
            .storage()
            .insert(Cell::new(1, 2, CellState::Alive, mask));
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
        .storage()
        .find_and_apply(1, 1, |c| assert_eq!(c.state(current), CellState::Alive));
}

#[test]
fn test_engine_start_ignore_if_running() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    engine.start();
    // Second start should be ignored as per the "Only Start if Idle" rule.
    engine.start();

    engine.stop();
}

#[test]
fn test_four_gliders_stability() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // Glider helper: (1,0), (2,1), (0,2), (1,2), (2,2)
    let add_glider = |ox: i128, oy: i128, dx: i128, dy: i128| {
        let guard = space.read();
        let mask = guard.current_state_mask();
        let pts = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
        for (px, py) in pts {
            let x = ox + px * dx;
            let y = oy + py * dy;
            space
                .storage()
                .insert(Cell::new(x, y, CellState::Alive, mask));
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
        fn on_snapshot_available(&self, path: std::path::PathBuf) -> bool {
            if path.exists() {
                let val = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if val >= 12 {
                    let _ = self.1.send(());
                }
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
            space.storage().find_and_apply(x, y, |c| {
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

#[test]
fn test_engine_immediate_stop_stress() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // 1. Add a lot of cells to create load
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        for x in 0..50 {
            for y in 0..50 {
                space
                    .storage()
                    .insert(Cell::new(x, y, CellState::Alive, mask));
            }
        }
    }

    // 2. Start and then immediately stop
    engine.start();
    // Tiny sleep to let some threads pick up work
    std::thread::sleep(std::time::Duration::from_micros(100));
    engine.stop();

    // 3. Wait for quiescence
    let mut success = false;
    for _ in 0..100 {
        if engine.get_cells_in_rect((-100, -100), (100, 100)).len() > 0 {
            if engine.work_queue_in_flight() == 0 {
                success = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(success, "Engine failed to quiesce after immediate stop");

    // 4. Verify no new work is accidentally started
    let count_before = engine.get_cells_in_rect((-100, -100), (100, 100)).len();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let count_after = engine.get_cells_in_rect((-100, -100), (100, 100)).len();

    // In stop mode, we shouldn't have flipped the space or cleared the storage
    assert_eq!(
        count_before, count_after,
        "Engine performed work after stop"
    );
}
#[test]
fn test_cell_lifecycle_4states() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // Initial state: Lonely cell at (0,0)
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(0, 0, CellState::Alive, mask));
    }

    // Capture initial state (Born) - Wait, we haven't stepped yet.
    // In our engine, snapshots are taken AFTER a step.
    // So if we insert and then step 1:
    // Step 1: Lonely cell dies.
    engine.step();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // After Step 1 (Generation 1):
    // Cell was Alive in G0, now Dead in G1.
    // presenter_view(G1, G0, G-1) should return Some(0b01) (Dying)
    {
        let guard = space.read();
        let curr = guard.current_state_mask();
        let last = guard.last_state_mask();
        let last_last = guard.next_state_mask();

        assert_eq!(guard.current_state_mask(), 0b10, "Should be Gen 1");

        space.storage().find_and_apply(0, 0, |cell| {
            let view = cell.presenter_view(curr, last, last_last);
            assert_eq!(view, Some(0b01), "Should be Dying frame");
        });

        // record_count should be 1
        assert_eq!(engine.record_count.load(Ordering::SeqCst), 1);
        // living_count should be 0
        assert_eq!(engine.living_count.load(Ordering::SeqCst), 0);
    }

    // Step 2:
    engine.step();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // After Step 2 (Generation 2):
    // Cell was Alive in G0, Dead in G1, Dead in G2.
    // presenter_view(G2, G1, G0) should return Some(0b00) (Newly Dead / Erasure)
    {
        let guard = space.read();
        let curr = guard.current_state_mask();
        let last = guard.last_state_mask();
        let last_last = guard.next_state_mask();

        assert_eq!(guard.current_state_mask(), 0b100, "Should be Gen 2");

        space.storage().find_and_apply(0, 0, |cell| {
            let view = cell.presenter_view(curr, last, last_last);
            assert_eq!(view, Some(0b00), "Should be Newly Dead (Erasure) frame");
        });

        // record_count should still be 1 (due to the ghost frame)
        assert_eq!(engine.record_count.load(Ordering::SeqCst), 1);
    }

    // Step 3:
    engine.step();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // After Step 3 (Generation 3):
    // Cell was Alive in G0 (now overwritten by G3?), Dead in G1, Dead in G2, Dead in G3.
    // Wait, G0 is same bit as G3.
    // But calculate_next_state overwrites the mask.
    // presenter_view(G3, G2, G1) should return None (Stable Dead)
    {
        let guard = space.read();
        let curr = guard.current_state_mask();
        let last = guard.last_state_mask();
        let last_last = guard.next_state_mask();

        assert_eq!(guard.current_state_mask(), 0b1, "Should be Gen 3");

        space.storage().find_and_apply(0, 0, |cell| {
            let view = cell.presenter_view(curr, last, last_last);
            assert_eq!(view, None, "Should be Stable Dead (Invisible)");
        });

        // record_count should be 0
        assert_eq!(engine.record_count.load(Ordering::SeqCst), 0);
    }
}
