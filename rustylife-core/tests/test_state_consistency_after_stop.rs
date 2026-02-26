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

#[test]
fn test_seed_leaves_engine_stopped() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // 1. Seed the engine
    engine.seed("r-pentomino".to_string());

    // 2. Wait for seed to process
    let start = std::time::Instant::now();
    while engine.work_queue_in_flight() > 0 {
        if start.elapsed().as_secs() > 2 {
            panic!("Seed failed to complete");
        }
        std::thread::yield_now();
    }

    // 3. Assert current state
    // BUG: Currently this fails because it returns FALSE.
    // EXPECTED: TRUE (Stopped).
    assert!(
        engine.is_stopped(),
        "Engine should be STOPPED after Seeding, but claims to be RUNNING (Zombie State)"
    );
}

#[test]
fn test_reset_leaves_engine_stopped() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // 1. Reset the engine
    engine.reset();

    // 2. Wait for reset
    let start = std::time::Instant::now();
    while engine.work_queue_in_flight() > 0 {
        if start.elapsed().as_secs() > 2 {
            panic!("Reset failed to complete");
        }
        std::thread::yield_now();
    }

    // 3. Assert current state
    // BUG: Currently this fails because it returns FALSE.
    // EXPECTED: TRUE (Stopped).
    assert!(
        engine.is_stopped(),
        "Engine should be STOPPED after Reset, but claims to be RUNNING (Zombie State)"
    );
}
