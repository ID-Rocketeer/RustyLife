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
        generation: u64,
        data: Arc<Vec<u8>>,
        _gps: f64,
        _work_rate: f64,
        _net_rate: f64,
        _bounds: Option<((i128, i128), (i128, i128))>,
    ) -> bool {
        match rustylife_core::decode_binary_packet(&data) {
            Ok(packet) => {
                let reported_count = packet.total_cells;
                // Only count states >= 2 (Alive/born). States 0 and 1 are dead/dying shadows.
                let actual_living_count =
                    packet.cells().filter(|(_, state)| *state >= 2).count() as u64;

                if reported_count != actual_living_count {
                    eprintln!(
                        "\n!!! INTEGRITY FAILURE at Gen {}: Header says {}, Payload has {} (Living) !!!\n",
                        generation, reported_count, actual_living_count
                    );
                    self.failure_detected.store(true, Ordering::SeqCst);
                    return false; // Stop receiving updates
                }
            }
            Err(e) => {
                eprintln!("\n!!! DECODE FAILURE at Gen {}: {} !!!\n", generation, e);
                self.failure_detected.store(true, Ordering::SeqCst);
                return false;
            }
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
    space.seed_from_rle(0, 0, rle);

    let engine = SimulationEngine::new(space, 4);
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
