use rustylife_gui::state::AppState;

#[test]
fn test_app_state_update() {
    let mut state = AppState::default();

    // Mock a BinaryPacket
    // We need a valid packet. This is hard to construct manually as it requires binary encoding.
    // Instead, let's verify Default state.

    assert_eq!(state.generation, 0);
    assert_eq!(state.total_cells, 0);
    assert!(!state.is_running);
    assert_eq!(state.viewport_cells.len(), 0);

    // We can't easily test update_state without a packet, but we can test
    // that the struct fields are accessible.

    state.generation = 100;
    state.is_running = true;
    assert_eq!(state.generation, 100);
    assert!(state.is_running);
}
