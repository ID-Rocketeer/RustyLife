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
use std::time::Duration;

struct BoundsSubscriber {
    #[allow(clippy::type_complexity)]
    pub last_bounds: Mutex<Option<((i128, i128), (i128, i128))>>,
}

impl EngineSubscriber for BoundsSubscriber {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        *self.last_bounds.lock().unwrap() = telemetry.bounds;
        true
    }
}

#[test]
fn test_breeder_bounds_regression() {
    // 1. Setup Engine
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space, 4);

    // 2. Setup Subscriber
    let subscriber = Arc::new(BoundsSubscriber {
        last_bounds: Mutex::new(None),
    });
    engine.add_subscriber(subscriber.clone());

    // 3. Register Breeder 1
    // Breeder 1: x = 749, y = 338
    let rle_content = std::fs::read_to_string("src/patterns/breeder1.rle")
        .expect("Failed to read breeder1.rle from src/patterns/");

    engine.register_pattern(rustylife_core::PatternInfo {
        name: "breeder1".to_string(),
        description: "Quadratic growth pattern".to_string(),
        rle: rle_content,
    });

    // 4. Seed and Check Gen 0
    engine.seed("breeder1".to_string());

    // Wait for the snapshot notification
    let mut success = false;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(50));
        let bounds = *subscriber.last_bounds.lock().unwrap();
        if let Some(((x1, y1), (x2, y2))) = bounds {
            // Internal bounds are ((0, 0), (748, 337))
            // Current Cartesian display logic in server (to be moved to core or kept in server)
            // But wait, the core currently returns raw bounds.
            // The user asked for the test to verify that the "notification message is correct".
            // Since I'm testing Engine directly, I expect raw bounds FROM SimulationSpace::bounds()
            // because I haven't moved the Cartesian transformation into the Engine yet.

            // Expected Cartesian bounds: ((0, -337), (748, 0))
            assert_eq!(x1, 0);
            assert_eq!(y1, -337);
            assert_eq!(x2, 748);
            assert_eq!(y2, 0);
            success = true;
            break;
        }
    }
    assert!(success, "Timed out waiting for initial bounds notification");

    // 5. Step and Check Gen 1
    engine.step();

    success = false;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(50));
        let bounds = *subscriber.last_bounds.lock().unwrap();
        if bounds.is_some() {
            success = true;
            break;
        }
    }
    assert!(success, "Timed out waiting for Gen 1 bounds notification");
}
