use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;

#[test]
fn test_cross_boundary_block() {
    // 2x2 block at (7,7), (8,7), (7,8), (8,8)
    // This spans 4 blocks:
    // (0,0) -> cell (7,7)
    // (1,0) -> cell (0,7) relative (8,7 world)
    // (0,1) -> cell (7,0) relative (7,8 world)
    // (1,1) -> cell (0,0) relative (8,8 world)

    // Each cell has 3 neighbors (the other 3).
    // (7,7) needs:
    // - (8,7) [East]
    // - (7,8) [South]
    // - (8,8) [South-East] -> CORNER Propagation Check!

    // (8,8) needs:
    // - (7,8) [West]
    // - (8,7) [North]
    // - (7,7) [North-West] -> CORNER Propagation Check!

    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space.clone(), 1);

    let cells = vec![(7, 7), (8, 7), (7, 8), (8, 8)];

    for (x, y) in &cells {
        engine.place_cell(*x, *y);
    }

    // Step
    engine.step();

    // Wait slightly (though step puts task in queue, we need to wait for worker)
    // Actually engine.step() just enqueues. We need to run the engine or manually process?
    // In unit tests, we usually run the worker manually or use a helper.
    // `engine.run_worker` blocks.
    // Let's use `process_task` logic or a short sleep if we spawn a thread.
    // Better: Spawn a thread for the engine worker.

    let engine_clone = engine.clone();
    std::thread::spawn(move || {
        SimulationEngine::run_worker(engine_clone, 0);
    });

    // Wait for generation to advance
    let start = std::time::Instant::now();
    while engine.generation() < 1 {
        if start.elapsed().as_secs() > 1 {
            panic!("Timeout waiting for step");
        }
        std::thread::yield_now();
    }

    // Check results
    // All should survive.
    let survivors = space.collect_all_states();

    assert_eq!(
        survivors.len(),
        4,
        "Expected 4 survivors, found {}",
        survivors.len()
    );

    for (x, y) in cells {
        let found = survivors.iter().any(|((sx, sy), _)| *sx == x && *sy == y);
        assert!(found, "Cell at ({}, {}) died!", x, y);
    }
}
