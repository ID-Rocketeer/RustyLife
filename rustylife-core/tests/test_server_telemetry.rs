use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct TelemetrySubscriber {
    pub packets: Mutex<Vec<(u64, f64, f64)>>, // (generation, work_rate, net_rate)
}

impl EngineSubscriber for TelemetrySubscriber {
    fn on_snapshot_available(&self, _generation: u64, data: Arc<Vec<u8>>) -> bool {
        // Decode packet to extract telemetry
        if let Ok(packet) = rustylife_core::decode_binary_packet(&data) {
            // println!(
            //     "Subscriber received Gen {}: WorkRate={:.2}, NetRate={:.2}",
            //     packet.generation, packet.work_rate, packet.net_rate
            // );
            self.packets.lock().unwrap().push((
                packet.generation,
                packet.work_rate,
                packet.net_rate,
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
    engine.register_pattern(rustylife_core::patterns::Pattern {
        name: "blinker".to_string(),
        description: "Blinker p2".to_string(),
        source: rustylife_core::patterns::PatternSource::Rle("x = 3, y = 3\n3o!".to_string()),
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
