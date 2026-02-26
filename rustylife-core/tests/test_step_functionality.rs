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
fn test_repro_run_stop_step_kills_cells() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // 1. Seed
    {
        let mut patterns = engine.patterns.lock().unwrap();
        patterns.push(rustylife_core::PatternInfo {
            name: "r-pentomino".to_string(),
            description: "R-pentomino test".to_string(),
            rle: "b2o$2ob$bo!".to_string(),
        });
    }
    engine.seed("r-pentomino".to_string());

    // Wait for seed to settle
    while engine.generation() == 0 && engine.work_queue_in_flight() > 0 {
        thread::sleep(Duration::from_millis(10));
    }

    let initial_pop = space.collect_all_states().len();
    assert_eq!(initial_pop, 5, "Initial R-pentomino should have 5 cells");

    // 2. Run for a few generations
    engine.start();

    // Let it run until generation > 10
    while engine.generation() < 10 {
        thread::sleep(Duration::from_millis(10));
    }

    // 3. Stop
    engine.stop();
    assert!(engine.is_stopped());

    // Allow engine to quiesce completely
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        engine.work_queue_in_flight(),
        0,
        "Engine should be idle after stop"
    );

    let pop_after_run = engine
        .living_count
        .load(std::sync::atomic::Ordering::SeqCst);
    assert!(pop_after_run > 0, "Population should be > 0 after run");
    let gen_stopped = engine.generation();

    // 4. Step (The Regression Point)
    engine.step();

    // Wait for step
    while engine.generation() <= gen_stopped {
        thread::sleep(Duration::from_millis(10));
    }
    while engine.work_queue_in_flight() > 0 {
        thread::sleep(Duration::from_millis(10));
    }

    let pop_after_step = engine
        .living_count
        .load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        pop_after_step > 0,
        "Cells should stay alive after Step following Run/Stop. Found {}",
        pop_after_step
    );

    // Double check generation advanced
    assert_eq!(engine.generation(), gen_stopped + 1);
}
