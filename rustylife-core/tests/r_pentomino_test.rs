use rustylife_core::cell::{Cell, CellState};
use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

struct GenerationTracker {
    target: usize,
    current: AtomicUsize,
    tx: mpsc::Sender<()>,
}

impl EngineSubscriber for GenerationTracker {
    fn on_snapshot_available(
        &self,
        _generation: u64,
        data: Arc<Vec<u8>>,
        _gps: f64,
        _work_rate: f64,
        _net_rate: f64,
        _bounds: Option<((i128, i128), (i128, i128))>,
    ) -> bool {
        if !data.is_empty() {
            let prev = self.current.fetch_add(1, Ordering::SeqCst);
            if prev + 1 >= self.target {
                let _ = self.tx.send(());
            }
        }
        true
    }
}

fn run_r_pentomino_test(pool_size: usize, bucket_count: usize) {
    println!(
        "Running r-pentomino test with pool_size: {}, bucket_count: {}",
        pool_size, bucket_count
    );
    let space = Arc::new(SimulationSpace::new(bucket_count));
    let engine = SimulationEngine::new(Arc::clone(&space), pool_size);

    // r-pentomino initial pattern:
    // . X X
    // X X .
    // . X .
    // Coordinates: (1, 0), (2, 0), (0, 1), (1, 1), (1, 2)
    {
        let guard = space.read();
        let mask = guard.current_state_mask();
        let pattern = vec![(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)];
        for (x, y) in pattern {
            space
                .storage()
                .insert(Cell::new(x, y, CellState::Alive, mask));
        }
    }

    let (tx, rx) = mpsc::channel();
    let tracker = Arc::new(GenerationTracker {
        target: 1103,
        current: AtomicUsize::new(0),
        tx,
    });

    engine.add_subscriber(tracker);
    engine.start();

    // Wait for 1103 generations.
    // The r-pentomino is chaotic, but 30 seconds should be plenty for 1.1k generations.
    rx.recv_timeout(Duration::from_secs(30))
        .expect("Simulation timed out before reaching 1103 generations");

    engine.stop();
    // Allow small time for quiescence
    std::thread::sleep(Duration::from_millis(100));

    let guard = space.read();
    let current_mask = guard.current_state_mask();

    // Final state of r-pentomino after 1103 generations has exactly 116 living cells.
    // SimulationSpace::collect_all_states returns ((x, y), view)
    // view has bit 0b10 set if alive in current generation.
    let alive_cells = space.collect_all_states();
    let living_count = alive_cells
        .iter()
        .filter(|(_, view)| (*view & 0b10) != 0)
        .count();

    assert_eq!(
        living_count, 116,
        "R-pentomino should have 116 cells after 1103 generations, found {} (current_mask: {})",
        living_count, current_mask
    );
}

#[test]
fn test_r_pentomino_stabilizes_after_long_evolution() {
    let pool_size = std::env::var("RUSTYLIFE_POOL_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(rustylife_core::THREAD_POOL_SIZE);

    let bucket_count = std::env::var("RUSTYLIFE_BUCKETS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(rustylife_core::BUCKET_COUNT);

    run_r_pentomino_test(pool_size, bucket_count);
}
