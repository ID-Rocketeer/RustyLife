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
