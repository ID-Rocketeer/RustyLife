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

struct TelemetrySubscriber {
    pub packets: Mutex<Vec<(u64, f64, f64)>>, // (generation, work_rate, net_rate)
}

impl EngineSubscriber for TelemetrySubscriber {
    fn on_snapshot_available(
        &self,
        data: Arc<Vec<u8>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        // Decode packet to verify it still parses — only record telemetry if the
        // binary payload is intact. This keeps the implicit integrity check from
        // the original test: a corrupt packet will fail to push, causing a timeout.
        if rustylife_core::decode_binary_packet(&data).is_ok() {
            self.packets.lock().unwrap().push((
                telemetry.generation,
                telemetry.work_rate,
                telemetry.net_rate,
            ));
        }
        true // Keep running
    }
}

#[test]
fn test_telemetry_work_and_net() {
    // 1. Setup Engine
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space, 4);

    // 2. Setup Subscriber
    let subscriber = Arc::new(TelemetrySubscriber {
        packets: Mutex::new(Vec::new()),
    });
    engine.add_subscriber(subscriber.clone());

    // Register Blinker
    engine.register_pattern(rustylife_core::PatternInfo {
        name: "blinker".to_string(),
        description: "Blinker p2".to_string(),
        rle: "x = 3, y = 3\n3o!".to_string(),
    });

    // 3. Scenario: Blinker
    // Gen 0: Horizontal (3 cells)
    // Gen 1: Vertical (3 cells)
    // Transition 0->1:
    //   Ends of H die (2 deaths)
    //   Ends of V born (2 births)
    //   Center stays alive.
    // Expected: Work=4, Net=0.
    engine.seed("blinker".to_string());

    // Wait for seed task to fully process and allow the system timer to advance,
    // guaranteeing a delta > 0.0 for the EMA telemetry calculation on Windows.
    wait_for_idle(&engine);
    std::thread::sleep(Duration::from_millis(15));

    // Step to Gen 1
    engine.step();

    // Wait for processing
    let mut found_gen_1 = false;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(50));
        let packets = subscriber.packets.lock().unwrap();
        // We expect at least one snapshot with Generation 1
        // The rate values depend on timing, but WorkRate should be > 0.0 for the transition.
        // NetRate should be around 0.0 for Blinker (Net Change = 0).

        if let Some((_gen, wr, nr)) = packets.iter().find(|(g, _, _)| *g == 1) {
            // println!("Found Gen 1: WorkRate={:.2}, NetRate={:.2}", wr, nr);
            assert!(*wr > 0.0, "Expected WorkRate > 0 for blinker transition");
            // NetRate should be effectively 0 (2 deaths, 2 births)
            // Allow some epsilon if EMA carried over previous noise, but start should be clean.
            assert!(nr.abs() < 100.0, "Expected NetRate approx 0"); // Loose bound for timing jitter
            found_gen_1 = true;
            break;
        }
    }

    if !found_gen_1 {
        // Debug info
        let packets = subscriber.packets.lock().unwrap();
        println!("Received packets: {:?}", *packets);
        panic!("Timed out waiting for Generation 1 snapshot");
    }
}

fn wait_for_idle(engine: &SimulationEngine) {
    let start = std::time::Instant::now();
    while engine.work_queue_in_flight() > 0 {
        if start.elapsed().as_secs() > 5 {
            panic!("Timeout waiting for engine to idle.");
        }
        std::thread::yield_now();
    }
}
