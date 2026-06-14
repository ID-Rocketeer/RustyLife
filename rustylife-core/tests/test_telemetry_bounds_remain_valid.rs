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
use std::time::{Duration, Instant};

#[test]
fn test_telemetry_bounds_remain_valid() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space, 4);

    engine.register_pattern(rustylife_core::PatternInfo {
        name: "blinker".to_string(),
        description: "Blinker".to_string(),
        rle: "x = 3, y = 3\n3o!".to_string(),
    });

    engine.seed_and_start("blinker".to_string(), Some(100));

    let engine_clone = engine.clone();
    let thread_handle = std::thread::spawn(move || {
        let mut none_bounds_count = 0;
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            let tel = engine_clone.capture_metrics_only();
            if tel.generation > 0 && tel.bounds.is_none() {
                none_bounds_count += 1;
            }
            std::thread::yield_now();
        }
        none_bounds_count
    });

    // Let the simulation run
    std::thread::sleep(Duration::from_millis(200));
    engine.stop();

    let none_bounds_count = thread_handle.join().unwrap();
    assert_eq!(
        none_bounds_count, 0,
        "Telemetry bounds dropped to None {} times during running simulation!",
        none_bounds_count
    );
}
