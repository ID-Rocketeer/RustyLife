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
use std::sync::atomic::Ordering;

#[test]
fn test_gui_workflow_state_management() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // 1. Verify Initial State
    // "stopping" should be true (Paused) so "Start" button is enabled
    assert!(
        engine.is_stopped(),
        "Engine should start in STOPPED (Paused) state"
    );
    assert_eq!(engine.generation(), 0, "Initial generation should be 0");
    assert_eq!(
        engine.living_count.load(Ordering::SeqCst),
        0,
        "Initial population should be 0"
    );

    // 2. Load Pattern 1 (Blinker)
    // 3 cells
    let blinker_rle = "bo$bo$bo!".to_string();
    engine.seed(blinker_rle);

    // Wait for seed to process (since we don't have a sync subscriber here, we poll or use a helper?)
    // Engine::seed pushes to queue. Worker picks it up.
    // We can wait for living_count > 0.
    let start = std::time::Instant::now();
    while engine.living_count.load(Ordering::SeqCst) == 0 {
        if start.elapsed().as_secs() > 2 {
            panic!("Timed out waiting for Blinker seed");
        }
        std::thread::yield_now();
    }

    // Verify State After Seed
    assert!(
        engine.is_stopped(),
        "Engine should remain STOPPED after Seeding"
    );
    assert_eq!(
        engine.living_count.load(Ordering::SeqCst),
        3,
        "Blinker should have 3 cells"
    );

    // 3. Load Pattern 2 (Glider)
    // 5 cells
    // THIS IS THE CRITICAL CHECK: Did it clear the Blinker?
    // If it didn't clear, we'd have 3 + 5 = 8 cells (assuming no collision logic needed yet)
    let glider_rle = "bo$2bo$3o!".to_string();
    engine.seed(glider_rle);

    // Wait for population to change to 5.
    // Use a loop to detect change.
    let start_2 = std::time::Instant::now();
    loop {
        let pop = engine.living_count.load(Ordering::SeqCst);
        if pop == 5 {
            break; // Success
        }
        if pop > 5 {
            panic!("Population is {} > 5! Pattern Overlap Detected!", pop);
        }
        if start_2.elapsed().as_secs() > 2 {
            // It might still be 3 if it hasn't picked up the task?
            // Or if it added them, it might be 8.
            let pop = engine.living_count.load(Ordering::SeqCst);
            if pop == 3 {
                // Still waiting?
                std::thread::yield_now();
                continue;
            }
            panic!("Timed out waiting for Glider seed. Current Pop: {}", pop);
        }
        std::thread::yield_now();
    }

    assert!(
        engine.is_stopped(),
        "Engine should remain STOPPED after Seeding 2nd pattern"
    );

    // 4. Start Engine
    engine.start();
    // Wait for running state logic (stopping = false)
    // process_task sets stopping = false.
    // Since we don't have a subscriber to wait for generation, we loop on is_stopped().
    let start_3 = std::time::Instant::now();
    while engine.is_stopped() {
        if start_3.elapsed().as_secs() > 2 {
            panic!("Timed out waiting for Engine Start");
        }
        std::thread::yield_now();
    }
    assert!(!engine.is_stopped(), "Engine should be RUNNING");

    // 5. Stop before Reset — Reset is dropped while running (mirrors UI behaviour;
    // both clients disable the Reset button while running)
    engine.stop();
    let start_stop = std::time::Instant::now();
    while !engine.is_stopped() {
        if start_stop.elapsed().as_secs() > 2 {
            panic!("Timed out waiting for engine to stop before reset");
        }
        std::thread::yield_now();
    }

    // 6. Reset
    engine.reset();
    let start_4 = std::time::Instant::now();
    loop {
        let stopped = engine.is_stopped();
        let curr_gen = engine.generation();
        let pop = engine.living_count.load(Ordering::SeqCst);
        // curr_gen==0 is the only discriminating condition: after stop() alone,
        // generation is still > 0. Only a completed reset sets it back to 0.
        if stopped && curr_gen == 0 && pop == 5 {
            break;
        }
        if start_4.elapsed().as_secs() > 2 {
            panic!(
                "Timed out waiting for Engine Reset (stopped={}, gen={}, pop={})",
                stopped, curr_gen, pop
            );
        }
        std::thread::yield_now();
    }

    // Check clean slate
    assert_eq!(engine.generation(), 0, "Reset should set generation to 0");
    assert_eq!(
        engine.living_count.load(Ordering::SeqCst),
        5,
        "Reset should restore population (Glider = 5)"
    );
    assert!(
        engine.is_stopped(),
        "Reset should leave engine in STOPPED state"
    );
}
