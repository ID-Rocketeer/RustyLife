use egui::{Pos2, Rect, Vec2};
use rustylife_gui::projection::ViewProjection;

#[test]
fn test_zoom_at_center_invariance() {
    let mut proj = ViewProjection::new(4.0);
    let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 1000.0));

    // Initial center world coordinate
    let (center_world_x, center_world_y) = proj.screen_to_world(screen_rect.center(), screen_rect);
    assert_eq!((center_world_x as i128, center_world_y as i128), (0, 0));

    // Zoom In
    proj.zoom_at_center(2.0); // Now 6.0
    let (new_c_x, new_c_y) = proj.screen_to_world(screen_rect.center(), screen_rect);
    assert!(
        (new_c_x - center_world_x).abs() < 0.001,
        "Center world X shifted: expected {}, got {}",
        center_world_x,
        new_c_x
    );
    assert!(
        (new_c_y - center_world_y).abs() < 0.001,
        "Center world Y shifted: expected {}, got {}",
        center_world_y,
        new_c_y
    );

    // Zoom Out
    proj.zoom_at_center(-3.0); // Now 3.0
    let (new_c_x, new_c_y) = proj.screen_to_world(screen_rect.center(), screen_rect);
    assert!((new_c_x - center_world_x).abs() < 0.001);
    assert!((new_c_y - center_world_y).abs() < 0.001);
}

#[test]
fn test_zoom_at_pointer_invariance() {
    let mut proj = ViewProjection::new(4.0);
    // Pan a bit so world (0,0) is not at center
    proj.offset = Vec2::new(100.0, -50.0);
    let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 1000.0));

    let pointer_pos = Pos2::new(250.0, 750.0);

    // Initial world coordinate under pointer
    let (world_x, world_y) = proj.screen_to_world(pointer_pos, screen_rect);

    // Zoom In
    proj.zoom_at_pointer(pointer_pos, screen_rect, 5.0);
    let (new_x, new_y) = proj.screen_to_world(pointer_pos, screen_rect);
    assert!(
        (new_x - world_x).abs() < 0.01,
        "Pointer world X shifted: expected {}, got {}",
        world_x,
        new_x
    );
    assert!(
        (new_y - world_y).abs() < 0.01,
        "Pointer world Y shifted: expected {}, got {}",
        world_y,
        new_y
    );

    // Zoom Out
    proj.zoom_at_pointer(pointer_pos, screen_rect, -2.0);
    let (new_x, new_y) = proj.screen_to_world(pointer_pos, screen_rect);
    assert!((new_x - world_x).abs() < 0.01);
    assert!((new_y - world_y).abs() < 0.01);
}

#[test]
fn test_world_screen_roundtrip() {
    let proj = ViewProjection::new(8.0);
    let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 1000.0));

    let world_coords = [(0, 0), (100, -50), (-1000000, 5000000)];

    for (x, y) in world_coords {
        let screen_pos = proj.world_to_screen(x, y, screen_rect);
        let (rx, ry) = proj.screen_to_world(screen_pos, screen_rect);
        assert_eq!(rx as i128, x);
        assert_eq!(ry as i128, y);
    }
}
