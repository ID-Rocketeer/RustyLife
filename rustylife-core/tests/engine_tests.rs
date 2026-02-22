use rustylife_core::cell::{Cell, CellState};
use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use std::sync::{Condvar, Mutex};

struct TestSync {
    state: Mutex<(u64, bool)>, // (generation, is_stopped)
    cond: Condvar,
}

impl TestSync {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((0, false)),
            cond: Condvar::new(),
        })
    }

    fn wait_for_generation(&self, target: u64) {
        let mut guard = self.state.lock().unwrap();

        while guard.0 < target {
            let result = self
                .cond
                .wait_timeout(guard, std::time::Duration::from_secs(15))
                .unwrap();
            guard = result.0;
            if result.1.timed_out() {
                panic!("Timed out waiting for generation {}", target);
            }
        }
    }
}

impl EngineSubscriber for TestSync {
    fn on_snapshot_available(
        &self,
        generation: u64,
        _data: Arc<Vec<u8>>,
        _is_running: bool,
        _gps: f64,
        _work_rate: f64,
        _net_rate: f64,
        _bounds: Option<((i128, i128), (i128, i128))>,
    ) -> bool {
        let mut guard = self.state.lock().unwrap();
        guard.0 = generation;
        self.cond.notify_all();
        true
    }
}

// run_engine_in_background removed (Engine::new spawns threads)

struct TestContext {
    engine: Arc<SimulationEngine>,
}

impl TestContext {
    fn new(space: Arc<SimulationSpace>, pool_size: usize) -> Self {
        Self {
            engine: SimulationEngine::new(space, pool_size),
        }
    }
}

impl std::ops::Deref for TestContext {
    type Target = SimulationEngine;
    fn deref(&self) -> &Self::Target {
        &self.engine
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        self.engine.shutdown();
    }
}

#[test]

fn test_isolated_cell_dies() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);
    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(0, 0, CellState::Alive, mask));
    }

    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

    engine.step();
    sync.wait_for_generation(1);

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
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);
    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

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

    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

    // 1. Run 1 step
    engine.step();
    // Tiny yield to allow task processing
    std::thread::yield_now();

    // Gen 1: Should become a 2x2 block
    sync.wait_for_generation(1);

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
    sync.wait_for_generation(2);

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
    engine.stop();
}

#[test]
fn test_glider_completes_translation_cycle() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // Glider at (0,0): (1,0), (2,1), (0,2), (1,2), (2,2)
    let initial_pts = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    space.seed_glider(0, 0);

    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

    // Run exactly 4 generations
    engine.start_generations(4);

    // Run 4 generations (one full cycle = 1 cell diagonal shift)
    sync.wait_for_generation(4);

    // Engine should stop automatically after 4 gens
    while !engine.is_stopped() {
        std::thread::yield_now();
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
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

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

    // 3. Wait for generation 1
    sync.wait_for_generation(1);

    // 4. Verify results
    let guard = space.read();
    let current = guard.current_state_mask();

    // The cell at (0, 0) should have died due to underpopulation (0 neighbors)
    space.storage().find_and_apply(0, 0, |cell| {
        assert_eq!(cell.state(current), CellState::Dead);
    });
}

#[test]
fn test_blinker_oscillates_correctly() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // A block of 3 cells (Blinker part 1)
    // (0,0), (1,0), (2,0) -> Alive
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

    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

    engine.step();

    // Step implicitly completes generation. Wait for notification.
    sync.wait_for_generation(1);

    // No need to stop/wait for quiescence if we just stepped once and verify strictly.

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

    engine.stop(); // Stop the engine after the test
}

#[test]
fn test_engine_quiesces_consistently_after_autonomous_stop() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);
    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

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

    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

    // 1. Start the engine
    engine.start();

    // 2. Wait for a few generations
    sync.wait_for_generation(5);

    // 3. Stop the engine
    engine.stop();

    // 4. Wait for it to quiesce (wait for work queue to empty)
    while !engine.is_stopped() {
        std::thread::yield_now();
    }

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
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    engine.start();
    // Second start should be ignored as per the "Only Start if Idle" rule.
    engine.start();

    engine.stop();
}

#[test]
fn test_four_gliders_stability() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // 1. Setup 4 gliders moving away from each other
    let rle = include_str!("../src/patterns/four_gliders.rle");
    // Center the 7x7 pattern at (0,0) by offsetting by (-3, -3)
    space.seed_from_rle(-3, -3, rle);

    // 2. Setup generation counter subscriber
    // 2. (Removed async counter)

    // 3. Run for 4 generations (avoid collision in compact pattern)
    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

    // 3. Run for 4 generations (avoid collision in compact pattern)
    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

    engine.start_generations(4);

    // 4. Wait for it to finish
    sync.wait_for_generation(4);

    // Wait for stop
    while !engine.is_stopped() {
        std::thread::yield_now();
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

    let mut all_cells = Vec::new();
    let guard = space.read();
    space.storage().collect_all(
        guard.current_state_mask(),
        guard.last_state_mask(),
        guard.next_state_mask(),
        &mut all_cells,
    );

    let mut live_cells = Vec::new();
    for ((x, y), state) in all_cells {
        if state >= 2 {
            // 2 = Born, 3 = Stable
            live_cells.push((x, y));
        }
    }
    live_cells.sort();

    // 4 gliders of 5 cells each = 20 cells.
    // They move outwards at 1/4 speed. In 4 gens, they move 1 cell (from -3 offset to -4).
    assert_eq!(
        live_cells.len(),
        20,
        "Should have 20 living cells across 4 gliders"
    );

    // Check corner offsets to ensure outward movement
    assert!(
        live_cells.contains(&(-4, -4)),
        "Top-left glider should have moved to (-4, -4)"
    );
    assert!(
        live_cells.contains(&(4, 4)),
        "Bottom-right glider should have moved to (4, 4)"
    );
    assert!(
        live_cells.contains(&(-4, 4)),
        "Bottom-left glider should have moved to (-4, 4)"
    );
    assert!(
        live_cells.contains(&(4, -4)),
        "Top-right glider should have moved to (4, -4)"
    );
}

#[test]
fn test_engine_remains_stable_under_immediate_stop_stress() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

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
    // Tiny yield to let some threads pick up work
    std::thread::yield_now();
    engine.stop();

    // 3. Wait for quiescence (all tasks complete)
    // Note: We can't rely on cell count because if the engine advanced a generation
    // before stopping, cells will be in the old generation (count=0 in current).
    // We just need to ensure all in-flight tasks complete.
    let mut success = false;
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 5 {
        let flight = engine.work_queue_in_flight();
        if flight == 0 {
            success = true;
            break;
        }
        std::thread::yield_now();
    }

    if !success {
        let count = engine.get_cells_in_rect((-100, -100), (100, 100)).len();
        let flight = engine.work_queue_in_flight();
        println!(
            "Stress Test Failed. Final State - Count: {}, InFlight: {}",
            count, flight
        );
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
    // Initial state: Lonely cell at (0,0)
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(0, 0, CellState::Alive, mask));
    }

    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

    // Capture initial state (Born) - Wait, we haven't stepped yet.
    // In our engine, snapshots are taken AFTER a step.
    // So if we insert and then step 1:
    // Step 1: Lonely cell dies.
    engine.step();
    sync.wait_for_generation(1);

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

        // living_count should be 0
        assert_eq!(engine.living_count.load(Ordering::SeqCst), 0);
    }

    // Step 2:
    engine.step();
    sync.wait_for_generation(2);

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
    }

    // Step 3:
    engine.step();
    sync.wait_for_generation(3);

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
        // actual_records check removed
    }
}

// count_visible_recursive removed (incompatible with BlockTree)
#[test]
fn test_reset_stability() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);
    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

    // 1. Load Pattern
    engine.register_pattern(rustylife_core::PatternInfo {
        name: "r-pentomino".to_string(),
        description: "Test".to_string(),
        rle: "b2o$2ob$bo!".to_string(),
    });

    // Seed and Start
    engine.seed_and_start("r-pentomino".to_string(), None);
    sync.wait_for_generation(10);

    // 3. Stop
    engine.stop();

    // Wait for stop
    let start = std::time::Instant::now();
    loop {
        if engine.is_stopped() {
            break;
        }
        if start.elapsed().as_secs() > 2 {
            panic!("Timed out waiting for stop");
        }
        std::thread::yield_now();
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

        // After reset: generation=0, has_snapshot=true (initial), flight=0
        // living_count might be > 0 if a pattern was reloaded
        if generation == 0 && has_snapshot && flight == 0 {
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
    sync.wait_for_generation(1);

    assert!(engine.generation() > 0, "Engine should have advanced");
}

#[test]
fn test_engine_seed_processing() {
    let subscriber = TestSync::new();
    let space = std::sync::Arc::new(rustylife_core::space::SimulationSpace::new(
        rustylife_core::BUCKET_COUNT,
    ));
    let engine = rustylife_core::engine::SimulationEngine::new(std::sync::Arc::clone(&space), 4);
    engine.add_subscriber(std::sync::Arc::clone(&subscriber)
        as std::sync::Arc<dyn rustylife_core::engine::EngineSubscriber>);

    // Glider RLE
    let rle = "bo$2bo$3o!".to_string();
    engine.seed(rle);

    // Spawned workers automatically by Engine::new

    // Wait for Seed to be processed
    let start = std::time::Instant::now();
    let mut seeded = false;
    while start.elapsed() < std::time::Duration::from_secs(2) {
        let guard = space.read();
        let _mask = guard.current_state_mask();
        // Check for Glider start (1,0)
        let mut found_cell = false;
        // Check Mask 1, 2, 4
        for m in [1, 2, 4] {
            if space
                .storage()
                .find_and_apply(1, 0, |c| {
                    if c.state(m) == rustylife_core::cell::CellState::Alive {
                        found_cell = true;
                    }
                })
                .is_some()
                && found_cell
            {
                break;
            }
        }
        if found_cell {
            seeded = true;
            break;
        }
        drop(guard);
        std::thread::yield_now();
    }
    assert!(seeded, "Engine failed to process Seed task within timeout");

    // Step engine to process EXACTLY 1 seed evolution generation
    engine.step();

    // Wait for at least 1 generation to ensure processing
    subscriber.wait_for_generation(1);

    // Wait for quiescence
    let wait_start = std::time::Instant::now();
    while engine.work_queue_in_flight() > 0 {
        if wait_start.elapsed().as_secs() > 5 {
            panic!("Timeout waiting for engine to stop");
        }
        std::thread::yield_now();
    }

    let _guard = space.read();

    let mut found = false;
    // Glider at (1,0) should have died or moved.
    // Check for NEW cells (0,1) or moved glider parts.
    // Checking (0,1) which should be BORN.
    for m in [1, 2, 4] {
        if !found {
            space.storage().find_and_apply(0, 1, |c| {
                if c.state(m) == rustylife_core::cell::CellState::Alive {
                    found = true;
                }
            });
        }
    }

    assert!(found, "Engine failed to evolve seeded pattern!");
}

#[test]
fn test_glider_gun_behavior() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = TestContext::new(Arc::clone(&space), rustylife_core::THREAD_POOL_SIZE);

    // Seed Glider Gun
    let rle = "24bo$22bobo$12b2o6b2o12b2o$11bo3bo4b2o12b2o$2o8bo5bo3b2o$2o8bo3bob2o4bobo$10bo5bo7bo$11bo3bo$12b2o!";
    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

    // Seed and Start for 100 generations
    engine.seed_and_start(rle.to_string(), Some(100));
    sync.wait_for_generation(100);

    // Check population
    // Initial pop = 36.
    // Each glider = 5 cells.
    // 100 gens / 30 period ~= 3 gliders produced.
    // Total pop should be > 36 + 10 (at least 2 gliders clear)

    let pop = engine.living_count.load(Ordering::SeqCst);
    println!("Glider Gun Pop at Gen 100: {}", pop);
    assert!(
        pop > 40,
        "Glider Gun failed to produce gliders (Pop: {})",
        pop
    );
}

#[test]
fn test_engine_seed_by_name() {
    let subscriber = TestSync::new();
    let space = std::sync::Arc::new(rustylife_core::space::SimulationSpace::new(
        rustylife_core::BUCKET_COUNT,
    ));
    let engine = rustylife_core::engine::SimulationEngine::new(space.clone(), 4);

    // Register a pattern
    engine.register_pattern(rustylife_core::PatternInfo {
        name: "TestGlider".to_string(),
        description: "A test glider".to_string(),
        rle: "bo$2bo$3o!".to_string(),
    });

    engine.add_subscriber(subscriber.clone());

    // Seed by NAME
    engine.seed("TestGlider".to_string());

    // Spawn workers matching pool size
    for i in 0..4 {
        let engine_clone = std::sync::Arc::clone(&engine);
        std::thread::spawn(move || {
            rustylife_core::engine::Engine::run_worker(engine_clone, i);
        });
    }

    // Wait for Seed to be processed
    let start = std::time::Instant::now();
    let mut seeded = false;
    while start.elapsed() < std::time::Duration::from_secs(2) {
        let guard = space.read();
        let mut found_cell = false;
        for m in [1, 2, 4] {
            if space
                .storage()
                .find_and_apply(1, 0, |c| {
                    if c.state(m) == rustylife_core::cell::CellState::Alive {
                        found_cell = true;
                    }
                })
                .is_some()
                && found_cell
            {
                break;
            }
        }
        if found_cell {
            seeded = true;
            break;
        }
        drop(guard);
        std::thread::yield_now();
    }
    assert!(
        seeded,
        "Engine failed to process named Seed task within timeout"
    );
}

#[test]
fn test_engine_seed_with_header() {
    let subscriber = TestSync::new();
    let space = std::sync::Arc::new(rustylife_core::space::SimulationSpace::new(
        rustylife_core::BUCKET_COUNT,
    ));
    let engine = TestContext::new(space.clone(), 4);

    // Register a pattern with a HEADER
    engine.register_pattern(rustylife_core::PatternInfo {
        name: "HeaderGlider".to_string(),
        description: "A test glider with header".to_string(),
        // Header line x=3, y=3 should be ignored/merged but NOT break parsing if logic is robust
        rle: "#N HeaderGlider\nx = 3, y = 3\nbo$2bo$3o!".to_string(),
    });

    engine.add_subscriber(subscriber.clone());

    // Seed by NAME
    engine.seed("HeaderGlider".to_string());

    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

    // Wait for Seed
    let start = std::time::Instant::now();
    let mut seeded = false;
    while start.elapsed() < std::time::Duration::from_secs(2) {
        let guard = space.read();
        // Just check if ANY cell is alive.
        // Just check if ANY cell is alive.
        let mut found_cell = false;
        // Scan a reasonable area where the glider should be.
        for x in 0..10 {
            for y in 0..10 {
                space.storage().find_and_apply(x, y, |c| {
                    if c.state(guard.current_state_mask()) == rustylife_core::cell::CellState::Alive
                    {
                        found_cell = true;
                    }
                });
                if found_cell {
                    break;
                }
            }
            if found_cell {
                break;
            }
        }

        if found_cell {
            seeded = true;
            break;
        }
        drop(guard);
        std::thread::yield_now();
    }
    assert!(seeded, "Engine failed to process Seed task with RLE header");
}

#[test]
fn test_metrics_calculation() {
    let subscriber = TestSync::new();
    let space = std::sync::Arc::new(rustylife_core::space::SimulationSpace::new(4));

    // Seed a blinker: 3 cells.
    space.seed_blinker(0, 0);

    let engine = TestContext::new(space.clone(), 2);
    engine.add_subscriber(subscriber.clone());

    // run_engine_in_background(Arc::clone(&engine.engine)); // Removed

    // Run 1 step.
    engine.step();
    subscriber.wait_for_generation(1);

    // Check Metrics
    // Net (Pop) should be 3.
    let pop = engine.living_count.load(Ordering::SeqCst);

    assert_eq!(pop, 3, "Population should be 3 for Blinker");

    // Work should be > 0.
    // Cells touched: 3 (old) + 2 (new born) + 2 (dying).
    // Center (1,0) stays alive.
    // Use Telemetry to check metrics of the completed generation
    let tel = engine.telemetry.lock().unwrap();
    let work = tel.last_work_count;
    let net = tel.last_net_count;

    assert!(
        work > 0,
        "Work should reflect touched cells (expected > 0, got {})",
        work
    );
    assert_eq!(net, 0, "Net change should be 0 for stable oscillator");
}
