use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_reset_clears_stopping_flag() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // 1. Manually dirties the state (Simulate a Stop invocation)
    // Since 'stopping' is private, we use 'stop()' which sets it to true.
    engine.stop();
    assert!(
        engine.is_stopped(),
        "Precondition failed: Engine should be stopped"
    );

    // 2. Perform Reset
    engine.reset();

    // 3. Wait for Reset to complete
    let start = std::time::Instant::now();
    while engine.work_queue_in_flight() > 0 {
        if start.elapsed().as_secs() > 2 {
            panic!("Reset timed out");
        }
        thread::sleep(Duration::from_millis(10));
    }

    // 4. Assert that Reset successfully cleared the stopping flag
    // Without the fix, this will fail (it will remain true).
    // With the fix, this will pass.
    let is_stopping = engine.is_stopped();
    assert!(
        is_stopping,
        "Reset failed to stop the engine! Engine should be STOPPED after Reset."
    );
}
