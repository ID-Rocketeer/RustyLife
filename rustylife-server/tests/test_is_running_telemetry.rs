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

//! Regression guard: the presenter's `is_running` field must track the engine's
//! actual running state via both the subscriber snapshot path AND via the
//! PresenterSubscriber's telemetry.
//!
//! Before the fix, `ServerActionHandler::request_state` never updated `is_running`
//! in AppState, so the UI could get permanently stuck in the "running" state after
//! the engine stopped if the final snapshot was delayed or missed.

use rustylife_core::{
    BinaryPacket, SimulationPresenter, Telemetry,
    engine::{EngineSubscriber, SimulationEngine},
    space::SimulationSpace,
};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Minimal presenter that records every is_running value it receives.
struct RunningStatePresenter {
    running_states: Vec<bool>,
}

impl SimulationPresenter for RunningStatePresenter {
    fn update_state(&mut self, _packet: BinaryPacket<'_>, telemetry: Telemetry) {
        self.running_states.push(telemetry.is_running);
    }
}

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
            rustylife_core::encode_binary_packet(telemetry.generation, &data, telemetry);
        if let Ok(packet) = rustylife_core::decode_binary_packet(&packet_data) {
            let mut presenter = self.presenter.lock().unwrap();
            presenter.update_state(packet, telemetry);
        }
        true
    }
}

/// A subscriber that receives snapshots and verifies telemetry.is_running matches
/// the engine's actual stopped/running state.
#[test]
fn test_presenter_receives_is_running_false_after_engine_stops() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    let presenter = Arc::new(Mutex::new(RunningStatePresenter {
        running_states: Vec::new(),
    }));
    engine.add_subscriber(Arc::new(PresenterSubscriber {
        presenter: presenter.clone() as Arc<Mutex<dyn SimulationPresenter>>,
    }));

    // Start running, wait for a few generations.
    engine.seed_and_start("r-pentomino".to_string(), None);
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while engine.generation() < 3 {
        if std::time::Instant::now() > deadline {
            panic!("Engine did not reach generation 3");
        }
        thread::sleep(Duration::from_millis(10));
    }

    // Stop and wait for the presenter to receive the final stopping snapshot.
    // engine.is_stopped() only tracks the engine loop; we need to verify the
    // IO delivery path too.
    engine.stop();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        {
            let states = presenter.lock().unwrap();
            if states.running_states.iter().any(|&r| !r) {
                break;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("Presenter did not receive is_running=false snapshot after stop");
        }
        thread::sleep(Duration::from_millis(10));
    }

    // The last telemetry snapshot delivered to the presenter must have
    // is_running == false.
    let states = presenter.lock().unwrap();
    let last = *states.running_states.last().unwrap();
    assert!(
        !last,
        "Last telemetry snapshot delivered to presenter must have is_running=false after engine stops"
    );
}

/// Verify that the first snapshot after seed_and_start has is_running == true.
#[test]
fn test_presenter_receives_is_running_true_on_first_snapshot() {
    let space = Arc::new(SimulationSpace::new(4));
    let engine = SimulationEngine::new(space.clone(), 2);

    let presenter = Arc::new(Mutex::new(RunningStatePresenter {
        running_states: Vec::new(),
    }));
    engine.add_subscriber(Arc::new(PresenterSubscriber {
        presenter: presenter.clone() as Arc<Mutex<dyn SimulationPresenter>>,
    }));

    engine.seed_and_start("r-pentomino".to_string(), None);

    // Wait for at least one snapshot.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if std::time::Instant::now() > deadline {
            panic!("No snapshot received after seed_and_start");
        }
        {
            let states = presenter.lock().unwrap();
            if !states.running_states.is_empty() {
                break;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }

    engine.stop();

    let states = presenter.lock().unwrap();
    let first = states.running_states[0];
    assert!(
        first,
        "First snapshot after seed_and_start must have is_running=true"
    );
}
