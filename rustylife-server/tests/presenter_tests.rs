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
        data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        let packet_data =
            rustylife_core::encode_binary_packet(telemetry.generation, &data, telemetry.clone());
        if let Ok(packet) = rustylife_core::decode_binary_packet(&packet_data) {
            let mut presenter = self.presenter.lock().unwrap();
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

    engine.place_cell(0, 0);

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
