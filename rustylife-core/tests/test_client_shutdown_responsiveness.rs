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

use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::{Arc, atomic::AtomicBool};
use std::thread;
use std::time::Duration;

struct MockClient {
    _running: AtomicBool,
}

impl EngineSubscriber for MockClient {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<((i128, i128), u8)>>,
        _telemetry: rustylife_core::Telemetry,
    ) -> bool {
        // In real app, client processes snapshot.
        // If engine says "running", client updates UI.
        true
    }
}

#[test]
fn test_simulation_resumes_after_stop_and_reset() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);
    let client = Arc::new(MockClient {
        _running: AtomicBool::new(false),
    });
    engine.add_subscriber(client.clone());

    // 0. Register Pattern
    engine.register_pattern(rustylife_core::PatternInfo {
        name: "r-pentomino".to_string(),
        description: "Test pattern".to_string(),
        rle: "x = 3, y = 3\n2o$2o$2o!".to_string(),
    });

    // 1. Run (Atomic)
    engine.seed_and_start("r-pentomino".to_string(), None);

    // Wait for running
    let start = std::time::Instant::now();
    while engine.generation() < 5 {
        if start.elapsed().as_secs() > 5 {
            panic!("Failed to run initial gen");
        }
        thread::sleep(Duration::from_millis(10));
    }

    // 2. Stop
    engine.stop();
    // Wait for stop
    let stop_start = std::time::Instant::now();
    while engine.work_queue_in_flight() > 0 {
        if stop_start.elapsed().as_secs() > 2 {
            panic!("Failed to quiesce");
        }
        thread::sleep(Duration::from_millis(10));
    }

    // 3. Reset
    // This is the critical moment.
    // If Reset doesn't clear stopping flag...
    engine.reset();

    // Wait for reset to process
    thread::sleep(Duration::from_millis(50));

    // 4. Run Again
    // If existing stopping flag (true) prevents start...
    engine.start(); // This sets stopping = false.

    // So why does it fail for user?
    // Maybe `engine.start()` sets it to false, but the WORKER picks up the OLD stopping=true value?
    // No, atomic ordering SeqCst handles that.

    // What if `in_flight_count` is wrong?
    let resume_start = std::time::Instant::now();
    while engine.generation() < 1 {
        if resume_start.elapsed().as_secs() > 4 {
            // DUMP STATE
            println!("DEADLOCK DETECTED!");
            println!("In Flight: {}", engine.work_queue_in_flight());
            println!("Is Stopped: {}", engine.is_stopped());
            println!("Generation: {}", engine.generation());
            panic!("Simulation failed to resume after reset!");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
