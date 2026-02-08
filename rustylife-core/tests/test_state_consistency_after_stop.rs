use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;

#[test]
fn test_seed_leaves_engine_stopped() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // 1. Seed the engine
    engine.seed("r-pentomino".to_string());

    // 2. Wait for seed to process
    let start = std::time::Instant::now();
    while engine.work_queue_in_flight() > 0 {
        if start.elapsed().as_secs() > 2 {
            panic!("Seed failed to complete");
        }
        std::thread::yield_now();
    }

    // 3. Assert current state
    // BUG: Currently this fails because it returns FALSE.
    // EXPECTED: TRUE (Stopped).
    assert!(
        engine.is_stopped(),
        "Engine should be STOPPED after Seeding, but claims to be RUNNING (Zombie State)"
    );
}

#[test]
fn test_reset_leaves_engine_stopped() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // 1. Reset the engine
    engine.reset();

    // 2. Wait for reset
    let start = std::time::Instant::now();
    while engine.work_queue_in_flight() > 0 {
        if start.elapsed().as_secs() > 2 {
            panic!("Reset failed to complete");
        }
        std::thread::yield_now();
    }

    // 3. Assert current state
    // BUG: Currently this fails because it returns FALSE.
    // EXPECTED: TRUE (Stopped).
    assert!(
        engine.is_stopped(),
        "Engine should be STOPPED after Reset, but claims to be RUNNING (Zombie State)"
    );
}
