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

//! Regression guard: seed() and reset() must complete synchronously when the
//! engine is quiescent (stopped, no workers processing tasks).
//!
//! Before the fix, commands were enqueued into a channel that only running worker
//! threads would drain. A quiescent engine had no active workers, so commands
//! posted while stopped were silently ignored — the command queue built up but
//! nothing ever processed it.

use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::time::Duration;

/// seed() on a fresh (stopped) engine must be fully complete the moment it
/// returns — no spinning wait required.
#[test]
fn test_seed_completes_synchronously_on_idle_engine() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // Engine starts in the stopped state; no workers are processing commands.
    assert!(
        engine.is_stopped(),
        "precondition: engine must start stopped"
    );

    engine.seed("r-pentomino".to_string());

    // The engine must be fully settled immediately — is_stopped() must be true
    // without any waiting loop. Any spin here would mean the command was NOT
    // processed synchronously.
    assert!(
        engine.is_stopped(),
        "Engine must remain stopped and fully settled after seed() on an idle engine"
    );

    // The seeded population must be visible immediately.
    let pop = engine
        .living_count
        .load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        pop > 0,
        "Population must be non-zero immediately after seed() on idle engine (got {})",
        pop
    );
}

/// reset() on a stopped engine must complete synchronously and leave the engine
/// in a fully settled stopped state, restoring the last active pattern.
#[test]
fn test_reset_completes_synchronously_on_idle_engine() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // Seed using the R-pentomino RLE directly (5 cells) — this registers the
    // RLE as the active pattern so reset() knows what to restore.
    // R-pentomino: b2o$2ob$bo!  — 5 alive cells
    const R_PENTOMINO_RLE: &str = "b2o$2ob$bo!";
    const R_PENTOMINO_CELLS: u64 = 5;

    engine.seed(R_PENTOMINO_RLE.to_string());
    assert!(
        engine.is_stopped(),
        "precondition: engine must be stopped after seed"
    );
    assert_eq!(
        engine
            .living_count
            .load(std::sync::atomic::Ordering::SeqCst),
        R_PENTOMINO_CELLS,
        "precondition: living_count must be {} after seed (got {})",
        R_PENTOMINO_CELLS,
        engine
            .living_count
            .load(std::sync::atomic::Ordering::SeqCst)
    );

    engine.reset();

    // Must be fully settled without any waiting.
    assert!(
        engine.is_stopped(),
        "Engine must remain stopped and fully settled after reset() on an idle engine"
    );

    // Generation must be back to 0 immediately.
    assert_eq!(
        engine.generation(),
        0,
        "Generation must be 0 immediately after reset() on idle engine"
    );

    // reset() re-seeds from the active RLE — R-pentomino has exactly 5 cells.
    let pop = engine
        .living_count
        .load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        pop, R_PENTOMINO_CELLS,
        "Population must be exactly {} (R-pentomino) immediately after reset() (got {})",
        R_PENTOMINO_CELLS, pop
    );
}

/// Multiple sequential seed() calls on an idle engine must each complete
/// synchronously, with the final state reflecting the last seed.
#[test]
fn test_multiple_seeds_on_idle_engine_are_each_synchronous() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    // Two seeds back-to-back; neither should require a wait loop.
    engine.seed("r-pentomino".to_string());
    assert!(engine.is_stopped(), "still stopped after first seed");

    engine.reset();
    assert!(engine.is_stopped(), "still stopped after reset");
    assert_eq!(engine.generation(), 0, "generation reset to 0 after reset");

    engine.seed("r-pentomino".to_string());
    assert!(engine.is_stopped(), "still stopped after second seed");

    let pop = engine
        .living_count
        .load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        pop > 0,
        "population non-zero after second seed (got {})",
        pop
    );
}

/// seed_and_start on a stopped engine must leave the engine running within a
/// short deadline — proving the command is processed without needing a prior
/// worker to be active.
#[test]
fn test_seed_and_start_transitions_idle_engine_to_running() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    assert!(
        engine.is_stopped(),
        "precondition: engine must start stopped"
    );

    engine.seed_and_start("r-pentomino".to_string(), None);

    // Engine should reach at least generation 1 quickly.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while engine.generation() == 0 {
        if std::time::Instant::now() > deadline {
            panic!("seed_and_start failed to advance the engine beyond generation 0");
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    engine.stop();
}
