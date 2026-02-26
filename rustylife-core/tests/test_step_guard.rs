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

use rustylife_core::cell::{Cell, CellState};
use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[test]
fn test_step_command_ignored_while_running() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space.clone(), rustylife_core::THREAD_POOL_SIZE);

    // Seed a simple pattern (blinker)
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        space
            .storage()
            .insert(Cell::new(0, 0, CellState::Alive, mask));
        space
            .storage()
            .insert(Cell::new(1, 0, CellState::Alive, mask));
        space
            .storage()
            .insert(Cell::new(2, 0, CellState::Alive, mask));
    }

    // Start the engine
    engine.start();

    // Wait a moment for engine to start processing
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Verify engine is running
    assert!(
        !engine.stopping.load(Ordering::SeqCst),
        "Engine should be running"
    );

    // Try to send a Step command while running - this should be IGNORED
    engine.step();

    // Wait a moment
    std::thread::sleep(std::time::Duration::from_millis(100));

    // The engine should still be running (Step should have been ignored)
    assert!(
        !engine.stopping.load(Ordering::SeqCst),
        "Engine should still be running after Step command"
    );

    // Stop the engine
    engine.stop();

    // Wait for stop to complete
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Now Step should work
    let gen_before = engine.generation();
    engine.step();

    // Wait for step to complete
    std::thread::sleep(std::time::Duration::from_millis(500));

    let gen_after = engine.generation();
    assert_eq!(
        gen_after,
        gen_before + 1,
        "Step should work when engine is stopped"
    );
}
