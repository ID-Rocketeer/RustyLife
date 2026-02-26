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
fn test_full_user_cycle_stability() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 4);

    // 1. Load Pattern (Seed)
    println!("1. Seeding...");
    engine.seed("r-pentomino".to_string());
    wait_for_idle(&engine);
    assert!(
        engine.is_stopped(),
        "Engine should be stopped after seeding"
    );
    assert_eq!(engine.generation(), 0, "Generation should be 0");

    // 2. Run
    println!("2. Running...");
    engine.start();

    // Allow it to run for some ticks (simulating 3 seconds, but faster)
    thread::sleep(Duration::from_millis(100));
    assert!(!engine.is_stopped(), "Engine should be running");
    let gen_during_run = engine.generation();
    assert!(gen_during_run > 0, "Generation should advance");

    // 3. Stop
    println!("3. Stopping...");
    engine.stop(); // Stop is now async, enqueues Tasks::Stop
    wait_for_idle(&engine);
    assert!(engine.is_stopped(), "Engine should be stopped");
    assert!(engine.work_queue_in_flight() == 0, "Queue should be empty");
    let gen_at_stop = engine.generation();
    println!("   Stopped at generation {}", gen_at_stop);

    // 4. Reset
    println!("4. Resetting...");
    engine.reset();
    wait_for_idle(&engine); // Reset is async, need to wait for it to process

    assert!(engine.is_stopped(), "Engine should be stopped after Reset");
    assert_eq!(engine.generation(), 0, "Generation should reset to 0");
    assert!(engine.work_queue_in_flight() == 0, "Queue should be empty");

    // 5. Run Again (Verification of deadlock fix)
    println!("5. Running Again...");
    engine.start();
    thread::sleep(Duration::from_millis(100));

    assert!(!engine.is_stopped(), "Engine should be running 2nd time");
    assert!(
        engine.generation() > 0,
        "Generation should increment 2nd time"
    );

    println!("Cycle Complete. Success.");
}

fn wait_for_idle(engine: &SimulationEngine) {
    // Simple busy wait with timeout
    let start = std::time::Instant::now();
    // We wait for queue to be empty.
    // Note: If a Task is currently *executing* but queue is empty, in_flight > 0.
    while engine.work_queue_in_flight() > 0 {
        if start.elapsed().as_secs() > 5 {
            panic!(
                "Timeout waiting for engine to idle. in_flight: {}",
                engine.work_queue_in_flight()
            );
        }
        thread::yield_now();
        thread::sleep(Duration::from_millis(5));
    }
}
