// Copyright (C) 2026 Steven P. Collins. All rights reserved.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::time::Duration;

struct EngineGuard(Arc<SimulationEngine>);

impl std::ops::Deref for EngineGuard {
    type Target = SimulationEngine;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for EngineGuard {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

#[test]
fn test_concurrent_population_safety_no_flicker() {
    let space = Arc::new(SimulationSpace::new(4));
    // Run with multiple worker threads to maximize parallel/concurrent operations
    let engine = EngineGuard(SimulationEngine::new(space.clone(), 4));

    // Seed a blinker pattern (exactly 3 cells, oscillates but remains exactly 3 cells in all generations)
    engine.place_cell(0, 0);
    engine.place_cell(1, 0);
    engine.place_cell(2, 0);

    assert_eq!(
        engine
            .living_count
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );

    // Start the simulation engine running continuously in the background
    engine.start();

    // Query telemetry and state continuously in a tight loop from a spawned thread to detect any transient flicker
    let engine_clone = engine.0.clone();
    let start = std::time::Instant::now();
    let reader_handle = std::thread::spawn(move || {
        let mut query_count = 0;
        // Run for 500ms to allow thousands of generations and queries
        while start.elapsed() < Duration::from_millis(500) {
            let (_, telemetry) = engine_clone.capture_current_state();
            let metrics = engine_clone.capture_metrics_only();

            assert_eq!(
                telemetry.population, 3,
                "Flicker detected in capture_current_state: expected 3, got {} at Gen {}",
                telemetry.population, telemetry.generation
            );
            assert_eq!(
                metrics.population, 3,
                "Flicker detected in capture_metrics_only: expected 3, got {} at Gen {}",
                metrics.population, metrics.generation
            );
            query_count += 1;
        }
        query_count
    });

    // Wait for the reader thread and propagate any assertion failure message
    let total_queries = match reader_handle.join() {
        Ok(count) => count,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                *s
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.as_str()
            } else {
                "Unknown panic"
            };
            panic!("Reader thread panicked: {}", msg);
        }
    };
    assert!(
        total_queries > 100,
        "Should have performed significant number of queries"
    );

    // Stop the engine cleanly
    engine.stop();
    assert!(
        engine.wait_for_quiescence(Duration::from_secs(2)),
        "Engine failed to stop cleanly"
    );
}
