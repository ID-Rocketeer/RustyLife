use egui::{Pos2, Rect, Vec2};

/// Centralizes all camera and viewport math for the RustyLife GUI.
pub struct ViewProjection {
    /// Position of world (0,0) relative to the screen center.
    pub offset: Vec2,
    /// Size of each cell in screen pixels.
    pub cell_size: f32,
}

impl ViewProjection {
    pub fn new(cell_size: f32) -> Self {
        Self {
            offset: Vec2::ZERO,
            cell_size,
        }
    }

    /// Converts world coordinates to screen space.
    pub fn world_to_screen(&self, x: i128, y: i128, screen_rect: Rect) -> Pos2 {
        let center = screen_rect.center() + self.offset;
        Pos2::new(
            center.x + (x as f32 * self.cell_size),
            center.y + (y as f32 * self.cell_size),
        )
    }

    /// Converts screen coordinates to world space.
    pub fn screen_to_world(&self, pos: Pos2, screen_rect: Rect) -> (f32, f32) {
        let center = screen_rect.center() + self.offset;
        let world_x = (pos.x - center.x) / self.cell_size;
        let world_y = (pos.y - center.y) / self.cell_size;
        (world_x, world_y)
    }

    /// Zooms keeping the world coordinate at the screen center invariant.
    pub fn zoom_at_center(&mut self, delta: f32) {
        let old = self.cell_size;
        self.cell_size = (self.cell_size + delta).clamp(1.0, 32.0);
        self.offset *= self.cell_size / old;
    }

    /// Zooms keeping the world coordinate under the pointer invariant.
    pub fn zoom_at_pointer(&mut self, pointer_pos: Pos2, screen_rect: Rect, delta: f32) {
        let old_size = self.cell_size;
        let new_size = (self.cell_size + delta).clamp(1.0, 32.0);

        if new_size == old_size {
            return;
        }

        let center = screen_rect.center() + self.offset;
        let world_x = (pointer_pos.x - center.x) / old_size;
        let world_y = (pointer_pos.y - center.y) / old_size;

        self.offset =
            pointer_pos - screen_rect.center() - Vec2::new(world_x * new_size, world_y * new_size);
        self.cell_size = new_size;
    }

    /// Calculates the visible world bounds.
    pub fn visible_bounds(&self, screen_rect: Rect) -> ((i128, i128), (i128, i128)) {
        let center = screen_rect.center() + self.offset;
        let min_x = ((screen_rect.min.x - center.x) / self.cell_size).floor() as i128;
        let max_x = ((screen_rect.max.x - center.x) / self.cell_size).ceil() as i128;
        let min_y = ((screen_rect.min.y - center.y) / self.cell_size).floor() as i128;
        let max_y = ((screen_rect.max.y - center.y) / self.cell_size).ceil() as i128;
        ((min_x, min_y), (max_x, max_y))
    }
}
