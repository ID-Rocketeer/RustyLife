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

/// Verifies that `place_cell` correctly increments `living_count` and that
/// the count remains accurate through a simulation step (i.e., the old
/// living_count overflow guard in `capture_state` is no longer needed).
#[test]
fn test_place_cell_maintains_living_count() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space, rustylife_core::THREAD_POOL_SIZE);

    assert_eq!(
        engine.living_count.load(Ordering::SeqCst),
        0,
        "living_count should be 0 before placing any cells"
    );

    // Place an L-shape (3 cells) — this will become a 2x2 block after one step
    engine.place_cell(0, 0);
    engine.place_cell(1, 0);
    engine.place_cell(0, 1);

    assert_eq!(
        engine.living_count.load(Ordering::SeqCst),
        3,
        "living_count should be 3 after place_cell x3"
    );

    // Step the simulation and wait for the snapshot (which updates living_count
    // from the commit phase, not from the old clamped-zero overflow path)
    use rustylife_core::Telemetry;
    use rustylife_core::engine::EngineSubscriber;
    use std::sync::{Condvar, Mutex};

    struct Latch(Mutex<Option<u64>>, Condvar);
    impl EngineSubscriber for Latch {
        fn on_snapshot_available(&self, _data: Arc<Vec<u8>>, telemetry: Telemetry) -> bool {
            *self.0.lock().unwrap() = Some(telemetry.population);
            self.1.notify_all();
            true
        }
    }

    let latch = Arc::new(Latch(Mutex::new(None), Condvar::new()));
    engine.add_subscriber(latch.clone() as Arc<dyn EngineSubscriber>);

    engine.step();

    // Wait for the snapshot
    let mut guard = latch.0.lock().unwrap();
    let result = latch
        .1
        .wait_timeout_while(guard, std::time::Duration::from_secs(5), |v| v.is_none())
        .unwrap();
    guard = result.0;

    let reported_pop = guard.expect("Snapshot should have arrived");

    // L-shape → 2x2 block (4 cells alive after step 1). living_count must
    // reflect the actual simulation result, not a clamped 0.
    assert_eq!(
        engine.living_count.load(Ordering::SeqCst),
        4,
        "living_count should be 4 after L-shape steps to block (was: {})",
        engine.living_count.load(Ordering::SeqCst)
    );
    assert_eq!(
        reported_pop, 4,
        "Telemetry population should match living_count"
    );
}

#[test]
fn test_seed_sync_maintains_living_count() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space, rustylife_core::THREAD_POOL_SIZE);

    assert_eq!(
        engine.living_count.load(Ordering::SeqCst),
        0,
        "living_count should be 0 initially"
    );

    // Seed a glider (5 cells) synchronously
    let glider_rle = "bob$2bo$3o!".to_string();
    engine.seed_sync(0, 0, glider_rle);

    assert_eq!(
        engine.living_count.load(Ordering::SeqCst),
        5,
        "living_count should be 5 after seed_sync() for as many cells as a glider"
    );

    // Verify it stays 5 after a generation where the glider simply moves
    // (Glider moves every 4 steps, but population stays constant)
    engine.step();
    engine.step();
    engine.step();
    engine.step();

    assert_eq!(
        engine.living_count.load(Ordering::SeqCst),
        5,
        "living_count should remain 5 as glider moves"
    );
}
