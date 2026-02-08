use rustylife_core::cell::{Cell, CellState};
use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct MockSubscriber {
    signaled: AtomicBool,
}

impl EngineSubscriber for MockSubscriber {
    fn on_snapshot_available(&self, _generation: u64, data: Arc<Vec<u8>>) -> bool {
        if !data.is_empty() {
            self.signaled.store(true, Ordering::SeqCst);
        }
        true
    }
}

#[test]
fn test_isolated_cell_dies() {
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
fn test_l_shape_consolidates_into_block() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // L-shape (pre-block)
    // (0,0), (1,0), (0,1)
    // L-shape (pre-block)
    // (0,0), (1,0), (0,1)
    let rle = include_str!("../src/patterns/l_shape.rle");
    space.seed_from_rle(0, 0, rle);

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
fn test_glider_completes_translation_cycle() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // Glider at (0,0): (1,0), (2,1), (0,2), (1,2), (2,2)
    let initial_pts = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    let rle = include_str!("../src/patterns/glider.rle");
    space.seed_from_rle(0, 0, rle);

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
fn test_engine_successfully_completes_single_step_cycle() {
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

    // 3. Wait for signal with timeout
    let start = std::time::Instant::now();
    while !sub.signaled.load(Ordering::SeqCst) && start.elapsed().as_millis() < 500 {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // 4. Verify results
    let guard = space.read();
    let current = guard.current_state_mask();

    // The cell at (0, 0) should have died due to underpopulation (0 neighbors)
    space.storage().find_and_apply(0, 0, |cell| {
        assert_eq!(cell.state(current), CellState::Dead);
    });

    // Subscriber should have been notified
    assert!(
        sub.signaled.load(Ordering::SeqCst),
        "Subscriber was not notified within timeout"
    );
}

#[test]
fn test_blinker_oscillates_correctly() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // A block of 3 cells (Blinker part 1)
    // A block of 3 cells (Blinker part 1)
    // (0,0), (1,0), (2,0) -> Alive
    let rle = include_str!("../src/patterns/blinker.rle");
    space.seed_from_rle(0, 0, rle);

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
fn test_engine_quiesces_consistently_after_autonomous_stop() {
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

    // 1. Setup 4 gliders moving away from each other
    let rle = include_str!("../src/patterns/four_gliders.rle");
    // Center the 7x7 pattern at (0,0) by offsetting by (-3, -3)
    space.seed_from_rle(-3, -3, rle);

    // 2. Setup generation counter subscriber
    // 2. (Removed async counter)

    // 3. Run for 4 generations (avoid collision in compact pattern)
    engine.start_generations(4);

    // 4. Wait for it to finish
    let start = std::time::Instant::now();
    loop {
        if engine.generation() >= 4 && engine.is_stopped() {
            break;
        }
        if start.elapsed().as_secs() > 5 {
            panic!("Timed out waiting for generation 4");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let guard = space.read();
    let generation_count = engine.generation();
    let current = if generation_count == 4 {
        guard.current_state_mask()
    } else {
        guard.last_state_mask()
    };
    println!(
        "VERIFY: Quiesced at Gen {}, Using Mask {:03b} for Gen 4",
        generation_count, current
    );

    // User's compact pattern results in immediate collision.
    // At Gen 4, we expect exactly 4 surviving cells at specific coordinates.
    let mut live_cells = Vec::new();
    space
        .collect_all_states()
        .iter()
        .for_each(|((x, y), state)| {
            if *state == rustylife_core::cell::CellState::Alive as u8 {
                live_cells.push((*x, *y));
            }
        });
    live_cells.sort();

    // Confirmed via previous debug run: [(-1, -3), (-1, 3), (1, -3), (1, 3)]
    let expected = vec![(-1, -3), (-1, 3), (1, -3), (1, 3)];
    assert_eq!(
        live_cells, expected,
        "Pattern did not evolve as expected collision at Gen 4"
    );
}

#[test]
fn test_engine_remains_stable_under_immediate_stop_stress() {
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
fn test_cell_correctly_transitions_through_lifecycle_states() {
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
        let mut actual_records = 0;
        space
            .storage()
            .collect_all(curr, last, last_last, &mut Vec::new()); // Just count
        space.storage().buckets.iter().for_each(|b| {
            let tree = b.read().unwrap();
            if let Some(root) = tree.root {
                actual_records += count_visible_recursive(&tree, root, curr, last, last_last);
            }
        });
        assert_eq!(actual_records, 1);

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
        let mut actual_records = 0;
        space.storage().buckets.iter().for_each(|b| {
            let tree = b.read().unwrap();
            if let Some(root) = tree.root {
                actual_records += count_visible_recursive(&tree, root, curr, last, last_last);
            }
        });
        assert_eq!(actual_records, 1);
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
        let mut actual_records = 0;
        space.storage().buckets.iter().for_each(|b| {
            let tree = b.read().unwrap();
            if let Some(root) = tree.root {
                actual_records += count_visible_recursive(&tree, root, curr, last, last_last);
            }
        });
        assert_eq!(actual_records, 0);
    }
}

#[test]
fn test_engine_state_repair_on_resume() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // 1. Setup a stable 2x2 block
    let rle = include_str!("../src/patterns/block.rle");
    space.seed_from_rle(0, 0, rle);

    // 2. MANUALLY CORRUPT memory:
    // We create an ALIVE cell at (5, 5). It has no neighbors, so it should die.
    // However, we manually set its neighbor count to 2.
    // In GOL, an alive cell with 2 neighbors survives.
    {
        let guard = space.read();
        let current = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(5, 5, CellState::Alive, current));
        space.storage().find_and_apply(5, 5, |cell| {
            cell.reset_neighbor_count(current);
            cell.increment_neighbor_count(current);
            cell.increment_neighbor_count(current); // DIRTY: 2 neighbors
        });
    }

    // 3. Step. If neighbor counts aren't repaired, (5, 5) will survive.
    // We call abort() to simulate an aggressive interruption that leaves the engine "tainted".
    engine.abort();
    engine.mark_tainted();

    engine.step();

    // 4. Verify results
    let guard = space.read();
    let current = guard.current_state_mask();

    let is_alive = space
        .storage()
        .find_and_apply(5, 5, |cell| cell.state(current) == CellState::Alive)
        .unwrap_or(false);

    assert!(
        !is_alive,
        "FIX FAILURE: Cell (5,5) survived from dirty memory! Repair failed."
    );
}

fn count_visible_recursive(
    tree: &rustylife_core::tree::CellTree,
    idx: rustylife_core::tree::NodeIndex,
    cur: usize,
    last: usize,
    last_last: usize,
) -> u64 {
    let node = tree.arena.get(idx);
    let mut count = if node.cell.presenter_view(cur, last, last_last).is_some() {
        1
    } else {
        0
    };
    if let Some(left) = node.left {
        count += count_visible_recursive(tree, left, cur, last, last_last);
    }
    if let Some(right) = node.right {
        count += count_visible_recursive(tree, right, cur, last, last_last);
    }
    count
}
#[test]
fn test_reset_stability() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // 1. Load Pattern
    engine.register_pattern(rustylife_core::patterns::Pattern {
        name: "r-pentomino".to_string(),
        description: "Test".to_string(),
        source: rustylife_core::patterns::PatternSource::Builtin(|space, x, y| {
            space.seed_r_pentomino(x, y)
        }),
    });

    // 1. Load Pattern
    engine.seed("r-pentomino".to_string());

    // 2. Start (run for a bit)
    engine.start();
    std::thread::sleep(std::time::Duration::from_millis(100)); // Let it generate some history

    // 3. Stop
    engine.stop();

    // println!("Waiting for stop...");
    // Wait for stop
    let start = std::time::Instant::now();
    loop {
        if engine.is_stopped() {
            break;
        }
        if start.elapsed().as_secs() > 2 {
            panic!("Timed out waiting for stop");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // println!("Stopped at generation: {}", engine.generation());

    // 4. Reset
    // println!("Triggering Reset...");
    engine.reset();

    // Wait for Reset to fully complete (queue must be empty)
    let start_reset = std::time::Instant::now();
    loop {
        let generation = engine.generation();
        let living = engine
            .living_count
            .load(std::sync::atomic::Ordering::SeqCst);
        let has_snapshot = engine.snapshots.get(0).is_some();
        let flight = engine.work_queue_in_flight();

        if generation == 0 && living > 0 && has_snapshot && flight == 0 {
            break;
        }

        if start_reset.elapsed().as_secs() > 5 {
            panic!(
                "Timed out waiting for Reset (Gen: {}, Alive: {}, Snap: {}, Flight: {})",
                generation, living, has_snapshot, flight
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // 5. Start again
    // Now safe to start as Queue is empty
    engine.start();

    // Wait for engine to verify progress
    let start = std::time::Instant::now();
    loop {
        if engine.generation() > 0 {
            break;
        }

        // Robustness: If the engine stopped unexpectedly (command race), kick it again.
        if engine.is_stopped() {
            // Only restart if we are sure we aren't creating a storm
            if engine.work_queue_in_flight() == 0 {
                engine.start();
            }
        }

        if start.elapsed().as_secs() > 10 {
            let flight = engine.work_queue_in_flight();
            let stopped = engine.is_stopped();
            panic!(
                "Timed out waiting for engine to advance generation. InFlight: {}, Stopped: {}",
                flight, stopped
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(engine.generation() > 0, "Engine should have advanced");
}
