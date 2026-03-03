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
        _data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        self.packets.lock().unwrap().push((
            telemetry.generation,
            telemetry.work_rate,
            telemetry.net_rate,
        ));
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

#[test]
fn test_telemetry_gps_anomaly_during_prune() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space, 4);

    let subscriber = Arc::new(TelemetrySubscriber {
        packets: Mutex::new(Vec::new()),
    });
    engine.add_subscriber(subscriber.clone());

    // Plant 1005 simple isolated cells so they all die in generation 1, triggering a prune.
    for i in 0..1005 {
        engine.place_cell(i * 8, 0);
    }

    // Step and wait
    engine.step();
    wait_for_idle(&engine);

    // We expect the first snapshot (Generation 1) to be delivered.
    // The engine has finished computation, but the bg_presenter thread needs a moment to deliver the snapshot.
    let mut found_rate = None;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let packets = subscriber.packets.lock().unwrap();
        if let Some((_, wr, _)) = packets.iter().find(|(g, _, _)| *g == 1) {
            found_rate = Some(*wr);
            break;
        }
    }

    assert!(found_rate.is_some(), "Generation 1 snapshot not received");
    let work_rate = found_rate.unwrap();

    // Reasonable work rate for 1005 cells on Gen 1 should be a few thousand or million / second.
    // If it's over 10 Billion (10,000,000,000.0), then the time delta was incorrectly zeroed.
    assert!(
        work_rate < 10_000_000_000.0,
        "Work Rate spiked anomalously high ({:.2} / s), indicating a timer reset bug during a prune!",
        work_rate
    );
}

struct GpsSubscriber {
    pub packets: Mutex<Vec<(u64, f64, f64)>>, // (generation, gps, work_rate)
}

impl EngineSubscriber for GpsSubscriber {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        self.packets.lock().unwrap().push((
            telemetry.generation,
            telemetry.gps,
            telemetry.work_rate,
        ));
        true
    }
}

#[test]
fn test_telemetry_resumes_after_pattern_load() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space, 4);

    let subscriber = Arc::new(GpsSubscriber {
        packets: Mutex::new(Vec::new()),
    });
    engine.add_subscriber(subscriber.clone());

    engine.register_pattern(rustylife_core::PatternInfo {
        name: "blinker".to_string(),
        description: "Blinker".to_string(),
        rle: "x = 3, y = 3\n3o!".to_string(),
    });

    // 1. Run engine past Gen 0
    engine.seed("blinker".to_string());
    wait_for_idle(&engine);
    std::thread::sleep(Duration::from_millis(15));

    // Step a few times to advance generation and record data
    for _ in 0..5 {
        engine.step();
        wait_for_idle(&engine);
        std::thread::sleep(Duration::from_millis(5));
    }

    // Capture telemetry from run 1
    let gen1_last;
    {
        let packets = subscriber.packets.lock().unwrap();
        gen1_last = packets.last().unwrap().0;
    }
    assert!(gen1_last >= 5, "Engine did not reach expected generation");

    // 2. Load new pattern (resets generation to 0)
    engine.seed("blinker".to_string());
    wait_for_idle(&engine);

    // Clear packets so we only see the new run
    subscriber.packets.lock().unwrap().clear();

    // 3. Step to Gen 1
    engine.step();
    wait_for_idle(&engine);

    // 4. Step again a few times to compute moving averages and ensure EMA records
    for _ in 0..3 {
        engine.step();
        wait_for_idle(&engine);
        std::thread::sleep(Duration::from_millis(5));
    }

    // Give subscriber time to process IO
    std::thread::sleep(Duration::from_millis(50));

    let packets = subscriber.packets.lock().unwrap();
    // Prove that the EMA is unlocked and > 0.0 for the second run!
    let has_gps = packets
        .iter()
        .any(|(g, gps, work_rate)| *g > 0 && *gps > 0.0 && *work_rate > 0.0);
    assert!(
        has_gps,
        "GPS/Work metrics did not update after loading a new pattern because last_generation wasn't reset!"
    );
}
