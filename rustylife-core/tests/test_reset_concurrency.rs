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
use std::thread;
use std::time::Duration;

#[test]
fn test_repro_run_stop_reset_hang() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // 1. Seed
    engine.seed("r-pentomino".to_string());

    // Wait for seed
    while engine.generation() == 0 && engine.work_queue_in_flight() > 0 {
        thread::sleep(Duration::from_millis(10));
    }

    // 2. Run
    engine.start();

    // Let it run for a bit
    while engine.generation() < 5 {
        thread::sleep(Duration::from_millis(10));
    }

    // 3. Stop
    engine.stop();
    assert!(engine.is_stopped());

    // Wait for quiescence
    let start_wait = std::time::Instant::now();
    while engine.work_queue_in_flight() > 0 {
        if start_wait.elapsed().as_secs() > 2 {
            panic!("Engine failed to quiesce after Stop!");
        }
        thread::sleep(Duration::from_millis(10));
    }

    // 4. Reset (This is where it hangs)
    println!("Triggering Reset...");
    engine.reset();

    // Reset should reset generation to 0
    let start_reset = std::time::Instant::now();
    while engine.generation() != 0 {
        if start_reset.elapsed().as_secs() > 2 {
            panic!(
                "Reset timed out! Generation is still {}",
                engine.generation()
            );
        }
        thread::sleep(Duration::from_millis(10));
    }

    // And ensure we can run again (Use Start to verify the stopping flag is cleared)
    engine.start();

    // Wait for at least one generation
    let start_resume = std::time::Instant::now();
    while engine.generation() < 1 {
        if start_resume.elapsed().as_secs() > 4 {
            // Total timeout
            panic!("Failed to Start after Reset! Stopping flag likely still set.");
        }
        thread::sleep(Duration::from_millis(10));
    }

    engine.stop();
}
