use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::sync::{Condvar, Mutex};

struct TestSync {
    state: Mutex<u64>,
    cond: Condvar,
}

impl TestSync {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(0),
            cond: Condvar::new(),
        })
    }

    fn wait_for_generation(&self, target: u64) {
        let mut guard = self.state.lock().unwrap();
        while *guard < target {
            let result = self
                .cond
                .wait_timeout(guard, std::time::Duration::from_secs(5))
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
        _data: Arc<Vec<u8>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        let mut guard = self.state.lock().unwrap();
        *guard = telemetry.generation;
        self.cond.notify_all();
        true
    }
}

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

    let sync = TestSync::new();
    engine.add_subscriber(sync.clone());

    // Step
    engine.step();

    // The workers are already running (SimulationEngine::new spawns threads)
    // Wait for the snapshot (which happens AFTER the generation is complete)
    sync.wait_for_generation(1);

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
