use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Verifies that a reset request is silently dropped when the engine is
/// actively running. Both UI clients disable the Reset button while running;
/// the engine enforces the same contract internally as defense in depth.
///
/// This test currently FAILS (pre-guard) because reset stops the engine.
/// After the guard is added it becomes a permanent regression test.
#[test]
fn test_reset_is_dropped_while_running() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space, rustylife_core::THREAD_POOL_SIZE);

    // Seed a glider (5 cells, stable population across generations)
    engine.seed("bo$2bo$3o!".to_string());

    // Wait for the seed task to complete
    let start = std::time::Instant::now();
    while engine
        .living_count
        .load(std::sync::atomic::Ordering::SeqCst)
        == 0
    {
        if start.elapsed().as_secs() > 2 {
            panic!("Timed out waiting for seed to process");
        }
        thread::yield_now();
    }

    // Start the engine and wait until it is genuinely running
    engine.start();
    let start = std::time::Instant::now();
    while engine.is_stopped() {
        if start.elapsed().as_secs() > 2 {
            panic!("Timed out waiting for engine to start");
        }
        thread::yield_now();
    }

    assert!(
        !engine.is_stopped(),
        "Engine should be running before the reset call"
    );

    // Call reset while the engine is running
    engine.reset();

    // Allow enough time for the reset task to be dequeued and processed
    // if it were not going to be dropped
    thread::sleep(Duration::from_millis(150));

    // The engine must still be running — reset was dropped
    assert!(
        !engine.is_stopped(),
        "Reset should be dropped while the engine is running, but engine stopped"
    );

    // Clean up
    engine.stop();
    let start = std::time::Instant::now();
    while !engine.is_stopped() {
        if start.elapsed().as_secs() > 2 {
            panic!("Timed out waiting for engine to stop after test");
        }
        thread::yield_now();
    }
}
