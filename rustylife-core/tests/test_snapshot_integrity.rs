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
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

struct IntegritySubscriber {
    failure_detected: Arc<AtomicBool>,
}

impl EngineSubscriber for IntegritySubscriber {
    fn on_snapshot_available(
        &self,
        data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        let packet_data =
            rustylife_core::encode_binary_packet(telemetry.generation, &data, telemetry.clone());
        if let Err(e) = rustylife_core::decode_binary_packet(&packet_data) {
            eprintln!(
                "\n!!! DECODE FAILURE at Gen {}: {} !!!\n",
                telemetry.generation, e
            );
            self.failure_detected.store(true, Ordering::SeqCst);
            return false;
        }
        true
    }
}

#[test]
fn test_runtime_snapshot_integrity() {
    // 256 buckets is sufficient for test
    let space = Arc::new(SimulationSpace::new(256));
    // Use a large pattern to ensure serialization takes non-trivial time, increasing race window.
    let rle = include_str!("../src/patterns/breeder1.rle");
    let engine = SimulationEngine::new(space, 4);
    engine.seed_sync(0, 0, rle.to_string());
    let failure_flag = Arc::new(AtomicBool::new(false));

    let subscriber = IntegritySubscriber {
        failure_detected: failure_flag.clone(),
    };

    engine.add_subscriber(Arc::new(subscriber));

    // Start simulation at full speed
    engine.start();

    let start = Instant::now();
    // Run for enough time to trigger thousands of generations and potential masking races
    while start.elapsed() < Duration::from_secs(5) {
        if failure_flag.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    engine.stop();
    engine.shutdown();

    if failure_flag.load(Ordering::SeqCst) {
        panic!(
            "Sanity Check Failed: Engine produced torn snapshots (Header/Body mismatch or Decode Error)."
        );
    }
}
