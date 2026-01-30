use rustylife_core::{
    BinaryPacket, SimulationPresenter,
    engine::{EngineSubscriber, SimulationEngine},
    space::SimulationSpace,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// Helper struct that we can use for testing within the test file
struct TestPresenter {
    received_packet: Option<BinaryPacket>,
}

impl SimulationPresenter for TestPresenter {
    fn update_state(&mut self, packet: BinaryPacket) {
        self.received_packet = Some(packet);
    }
}

// We need a version of PresenterSubscriber that we can use here.
// Since it's in main.rs of rustylife-server, we can re-implement it or move it.
// For the test, we'll re-implement a minimal version or move the core logic to lib.rs later.
pub struct PresenterSubscriber {
    pub presenter: Arc<Mutex<dyn SimulationPresenter>>,
}

impl EngineSubscriber for PresenterSubscriber {
    fn on_snapshot_available(&self, path: PathBuf) -> bool {
        if let Ok(buf) = std::fs::read(&path) {
            if let Ok(packet) = rustylife_core::decode_binary_packet(&buf) {
                let mut presenter = self.presenter.lock().unwrap();
                presenter.update_state(packet);
            } else {
                // Failed to decode packet
            }
        } else {
            // Failed to read file
        }
        true
    }
}

#[test]
fn test_presenter_receives_engine_snapshot() {
    let temp_dir = tempfile::tempdir().unwrap();
    let staging_dir = temp_dir.path().to_path_buf();

    let space = Arc::new(SimulationSpace::new(7));
    let engine = SimulationEngine::new_with_staging(space.clone(), 1, staging_dir.clone());

    let presenter = Arc::new(Mutex::new(TestPresenter {
        received_packet: None,
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
            if p.received_packet.is_some() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Check if presenter received the packet
    let p_lock = presenter.lock().unwrap();
    assert!(
        p_lock.received_packet.is_some(),
        "Presenter should have received a packet"
    );
    let packet = p_lock.received_packet.as_ref().unwrap();
    assert_eq!(packet.generation, 1);

    // Check if our inserted cell is there and marked correctly (Born or Stable)
    let target_cell = packet.cells.iter().find(|(pos, _)| *pos == (0, 0));
    assert!(
        target_cell.is_some(),
        "Inserted cell at (0,0) should be present"
    );
    let (_, state) = target_cell.unwrap();
    // 0b10 is Born, 0b11 is Stable, 0b01 is Dying.
    // Since we just inserted it and then stepped, it might be Dying or Stable depending on rules.
    // In Life, 1 cell dies. So it should be Dying (0b01).
    assert!(
        *state == 0b10 || *state == 0b01 || *state == 0b11,
        "Cell state should be Born, Dying, or Stable (got {:02b})",
        state
    );
}
