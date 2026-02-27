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

use rustylife_core::{engine::EngineSubscriber, engine::SimulationEngine, space::SimulationSpace};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct GpsMonitor {
    last_telemetry: Mutex<Option<rustylife_core::Telemetry>>,
}

impl EngineSubscriber for GpsMonitor {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<u8>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        let mut lock = self.last_telemetry.lock().unwrap();
        *lock = Some(telemetry);
        true
    }
}

#[test]
fn test_repro_gps_doubling() {
    let space = Arc::new(SimulationSpace::new(2048));
    let engine = SimulationEngine::new(space, 1);
    let monitor = Arc::new(GpsMonitor {
        last_telemetry: Mutex::new(None),
    });
    engine.add_subscriber(monitor.clone());

    let start_time = Instant::now();
    engine.start();

    // Run for exactly 2 seconds
    std::thread::sleep(Duration::from_secs(2));

    engine.stop();
    let end_time = Instant::now();
    let actual_duration = end_time.duration_since(start_time).as_secs_f64();

    // Wait slightly for the last snapshot to arrive
    std::thread::sleep(Duration::from_millis(100));

    let telemetry = monitor
        .last_telemetry
        .lock()
        .unwrap()
        .expect("No telemetry received");
    let reported_gps = telemetry.gps;
    let generation = telemetry.generation;

    let expected_gps = generation as f64 / actual_duration;

    println!("Generation: {}", generation);
    println!("Actual duration: {}s", actual_duration);
    println!("Reported GPS: {}", reported_gps);
    println!("Expected GPS (gen/time): {}", expected_gps);

    // If the bug exists, reported_gps will be ~2x expected_gps
    // We expect it to be significantly higher than expected_gps.
    // Use a conservative threshold like 1.5x.
    assert!(
        reported_gps < expected_gps * 1.2,
        "Reported GPS ({}) is significantly higher than real GPS ({})",
        reported_gps,
        expected_gps
    );
}
