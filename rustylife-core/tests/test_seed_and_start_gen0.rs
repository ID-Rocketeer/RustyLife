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

//! Regression guard: seed_and_start must emit a Generation-0 snapshot before
//! any spread/commit cycles begin.
//!
//! Before the fix, SeedAndStart called `initiate_spread()` directly without
//! first calling `capture_state()`, so subscribers never saw Gen-0 — the first
//! snapshot they received was Gen-1.

use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct SnapshotCapture {
    generations_seen: Vec<u64>,
}

struct CapturingSubscriber(Arc<Mutex<SnapshotCapture>>);

impl EngineSubscriber for CapturingSubscriber {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        self.0
            .lock()
            .unwrap()
            .generations_seen
            .push(telemetry.generation);
        true
    }
}

#[test]
fn test_seed_and_start_first_snapshot_is_generation_zero() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    let capture = Arc::new(Mutex::new(SnapshotCapture {
        generations_seen: Vec::new(),
    }));
    engine.add_subscriber(Arc::new(CapturingSubscriber(capture.clone())));

    // seed_and_start should emit Gen-0 before the first spread runs.
    engine.seed_and_start("r-pentomino".to_string(), None);

    // Wait for at least two snapshots so we can inspect the first one.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if std::time::Instant::now() > deadline {
            panic!("Timed out waiting for snapshots from seed_and_start");
        }
        {
            let cap = capture.lock().unwrap();
            if cap.generations_seen.len() >= 2 {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    engine.stop();

    let cap = capture.lock().unwrap();
    let first = cap.generations_seen[0];
    assert_eq!(
        first, 0,
        "First snapshot after seed_and_start must be Generation 0, got Generation {}",
        first
    );
}
