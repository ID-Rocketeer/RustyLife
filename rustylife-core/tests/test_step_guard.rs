use rustylife_core::cell::{Cell, CellState};
use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[test]
fn test_step_command_ignored_while_running() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space.clone(), rustylife_core::THREAD_POOL_SIZE);

    // Seed a simple pattern (blinker)
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

    // Start the engine
    engine.start();

    // Wait a moment for engine to start processing
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Verify engine is running
    assert!(
        !engine.stopping.load(Ordering::SeqCst),
        "Engine should be running"
    );

    // Try to send a Step command while running - this should be IGNORED
    engine.step();

    // Wait a moment
    std::thread::sleep(std::time::Duration::from_millis(100));

    // The engine should still be running (Step should have been ignored)
    assert!(
        !engine.stopping.load(Ordering::SeqCst),
        "Engine should still be running after Step command"
    );

    // Stop the engine
    engine.stop();

    // Wait for stop to complete
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Now Step should work
    let gen_before = engine.generation();
    engine.step();

    // Wait for step to complete
    std::thread::sleep(std::time::Duration::from_millis(500));

    let gen_after = engine.generation();
    assert_eq!(
        gen_after,
        gen_before + 1,
        "Step should work when engine is stopped"
    );
}
