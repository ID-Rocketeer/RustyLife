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

use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::{Arc, Mutex};

struct GpsMonitor {
    gen0_gps: Arc<Mutex<Option<f64>>>,
}

impl EngineSubscriber for GpsMonitor {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        if telemetry.generation == 0 {
            let mut lock = self.gen0_gps.lock().unwrap();
            if lock.is_none() {
                *lock = Some(telemetry.gps);
            }
        }
        true
    }
}

#[test]
fn test_gen0_gps_is_zero() {
    let space = Arc::new(SimulationSpace::new(185));
    let engine = SimulationEngine::new(space, 4);

    let gen0_gps = Arc::new(Mutex::new(None));
    engine.add_subscriber(Arc::new(GpsMonitor {
        gen0_gps: gen0_gps.clone(),
    }));

    // Seed the engine (which triggers Gen 0 snapshot)
    engine.seed("glider".to_string());

    // Wait a bit for the snapshot
    std::thread::sleep(std::time::Duration::from_millis(100));

    let gps = gen0_gps
        .lock()
        .unwrap()
        .expect("Did not receive Gen 0 snapshot");
    println!("Gen 0 GPS: {}", gps);

    // It should be exactly 0.0
    assert_eq!(gps, 0.0, "Gen 0 GPS must be 0.0");
}
