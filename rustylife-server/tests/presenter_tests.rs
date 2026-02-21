use rustylife_core::{
    BinaryPacket, SimulationPresenter, Telemetry,
    engine::{EngineSubscriber, SimulationEngine},
    space::SimulationSpace,
};
use std::sync::{Arc, Mutex};

// Helper struct that we can use for testing within the test file
// Helper struct that we can use for testing within the test file
struct TestPresenter {
    received_generation: Option<u64>,
    received_cells: Vec<((i128, i128), u8)>,
}

impl SimulationPresenter for TestPresenter {
    fn update_state(&mut self, packet: BinaryPacket<'_>, _telemetry: Telemetry) {
        self.received_generation = Some(packet.generation);
        self.received_cells = packet.cells().collect();
    }
}

// We need a version of PresenterSubscriber that we can use here.
// Since it's in main.rs of rustylife-server, we can re-implement it or move it.
// For the test, we'll re-implement a minimal version or move the core logic to lib.rs later.
pub struct PresenterSubscriber {
    pub presenter: Arc<Mutex<dyn SimulationPresenter>>,
}

impl EngineSubscriber for PresenterSubscriber {
    fn on_snapshot_available(
        &self,
        _generation: u64,
        data: Arc<Vec<u8>>,
        is_running: bool,
        gps: f64,
        work_rate: f64,
        net_rate: f64,
        bounds: Option<((i128, i128), (i128, i128))>,
    ) -> bool {
        if let Ok(packet) = rustylife_core::decode_binary_packet(&data) {
            let mut presenter = self.presenter.lock().unwrap();
            let telemetry = Telemetry {
                total_cells: 0, // Mock for test
                is_running,
                gps,
                work_rate,
                net_rate,
                bounds,
            };
            presenter.update_state(packet, telemetry);
        }
        true
    }
}

#[test]
fn test_presenter_receives_engine_snapshot() {
    let space = Arc::new(SimulationSpace::new(7));
    let engine = SimulationEngine::new(space.clone(), 1);

    let presenter = Arc::new(Mutex::new(TestPresenter {
        received_generation: None,
        received_cells: Vec::new(),
    }));
    let subscriber = Arc::new(PresenterSubscriber {
        presenter: presenter.clone(),
    });

    engine.add_subscriber(subscriber);

    // Initial state setup
    {
        let guard = space.read();
        space.storage().insert(rustylife_core::cell::Cell::new(
            0,
            0,
            rustylife_core::cell::CellState::Alive,
            guard.current_state_mask(),
        ));
    }

    // Step engine - this should trigger a snapshot and notification
    engine.step();

    // Wait for presenter to receive the packet (async via disk and subscriber)
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 5 {
        {
            let p = presenter.lock().unwrap();
            if p.received_generation.is_some() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Check if presenter received the packet
    let p_lock = presenter.lock().unwrap();
    assert!(
        p_lock.received_generation.is_some(),
        "Presenter should have received a packet"
    );
    assert_eq!(p_lock.received_generation.unwrap(), 1);

    // Check if our inserted cell is there and marked correctly (Born or Stable)
    let target_cell = p_lock.received_cells.iter().find(|(pos, _)| *pos == (0, 0));
    assert!(
        target_cell.is_some(),
        "Inserted cell at (0,0) should be present"
    );
    let (_, state) = target_cell.unwrap();
    // 0b10 is Born, 0b11 is Stable, 0b01 is Dying.
    assert!(
        *state == 0b10 || *state == 0b01 || *state == 0b11,
        "Cell state should be Born, Dying, or Stable (got {:02b})",
        state
    );
}
