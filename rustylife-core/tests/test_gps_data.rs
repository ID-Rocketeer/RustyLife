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

use rustylife_core::{engine::SimulationEngine, space::SimulationSpace};
use std::sync::Arc;
use std::time::Duration;
struct GpsTracker(std::sync::Mutex<Option<rustylife_core::Telemetry>>);
impl rustylife_core::engine::EngineSubscriber for GpsTracker {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        *self.0.lock().unwrap() = Some(telemetry);
        true
    }
}

#[test]
fn test_gps_data_integrity() {
    let space = Arc::new(SimulationSpace::new(2048));
    // Use smaller pool for test
    let engine = SimulationEngine::new(space, 2);

    let tracker = Arc::new(GpsTracker(std::sync::Mutex::new(None)));
    engine.add_subscriber(tracker.clone() as Arc<dyn rustylife_core::engine::EngineSubscriber>);

    // 1. Start Engine
    engine.start();

    // 2. Wait for a few generations to ensure >0 GPS
    std::thread::sleep(Duration::from_millis(500));
    engine.stop();

    let telemetry = tracker
        .0
        .lock()
        .unwrap()
        .clone()
        .expect("Should have received telemetry");

    // Convert to binary packet to test serialization
    let payload = rustylife_core::encode_binary_packet(telemetry.generation, &[], telemetry);

    let len_bytes: [u8; 4] = payload[0..4].try_into().unwrap();
    let json_len = u32::from_le_bytes(len_bytes) as usize;
    let json_slice = &payload[4..4 + json_len];
    let json_str = std::str::from_utf8(json_slice).unwrap();

    println!("JSON Header: {}", json_str);

    assert!(
        json_str.contains("\"gps\":"),
        "JSON MUST contain gps field in BinaryStateHeader after telemetry embedding"
    );
}
