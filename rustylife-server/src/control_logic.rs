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

pub struct ControlStates {
    pub run_enabled: bool,
    pub step_enabled: bool,
    pub stop_enabled: bool,
    pub reset_enabled: bool,
}

pub fn get_enabled_controls(is_running: bool) -> ControlStates {
    // CORRECTED IMPLEMENTATION
    ControlStates {
        run_enabled: !is_running,
        step_enabled: !is_running, // Fixed: Only enable step when NOT running
        stop_enabled: is_running,
        reset_enabled: !is_running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_disabled_when_running() {
        let controls = get_enabled_controls(true); // is_running = true
        assert_eq!(
            controls.step_enabled, false,
            "Step button should be disabled when running"
        );
    }

    #[test]
    fn test_step_enabled_when_stopped() {
        let controls = get_enabled_controls(false); // is_running = false
        assert_eq!(
            controls.step_enabled, true,
            "Step button should be enabled when stopped"
        );
    }
}
