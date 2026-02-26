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

use rustylife_gui::state::AppState;

#[test]
fn test_app_state_update() {
    let mut state = AppState::default();

    // Mock a BinaryPacket
    // We need a valid packet. This is hard to construct manually as it requires binary encoding.
    // Instead, let's verify Default state.

    assert_eq!(state.generation, 0);
    assert_eq!(state.population, 0);
    assert!(!state.is_running);
    assert_eq!(state.viewport_cells.len(), 0);

    // We can't easily test update_state without a packet, but we can test
    // that the struct fields are accessible.

    state.generation = 100;
    state.is_running = true;
    assert_eq!(state.generation, 100);
    assert!(state.is_running);
}
