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

use rustylife_core::{BinaryPacket, SimulationPresenter};

#[derive(Clone)]
pub struct AppState {
    pub viewport_cells: Vec<((i128, i128), u8)>,
    pub generation: u64,
    pub population: u64,
    pub is_running: bool,
    pub target_viewport: Option<((i128, i128), (i128, i128))>,
    pub gps: f64,
    pub work_rate: f64,
    pub net_rate: f64,
    pub cores: usize,
    pub is_connected: bool,
    pub patterns: Vec<rustylife_core::PatternInfo>,
    pub bounds: Option<((i128, i128), (i128, i128))>,
    pub repaint_ctx: Option<egui::Context>,
    pub palette: rustylife_core::ColorPalette,
    pub states: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            viewport_cells: Vec::new(),
            generation: 0,
            population: 0,
            is_running: false,
            target_viewport: None,
            gps: 0.0,
            work_rate: 0.0,
            net_rate: 0.0,
            cores: 0,
            is_connected: false,
            patterns: Vec::new(),
            bounds: None,
            repaint_ctx: None,
            palette: rustylife_core::ColorPalette::default(),
            states: 3,
        }
    }
}

impl AppState {
    pub fn expanse(&self) -> (u64, u64) {
        if let Some(((min_x, min_y), (max_x, max_y))) = self.bounds {
            let width = (max_x - min_x).unsigned_abs() as u64 + 1;
            let height = (max_y - min_y).unsigned_abs() as u64 + 1;
            (width, height)
        } else {
            (0, 0)
        }
    }
}

impl SimulationPresenter for AppState {
    fn update_state(&mut self, packet: BinaryPacket<'_>, telemetry: rustylife_core::Telemetry) {
        if let Some(((min_x, min_y), (max_x, max_y))) = self.target_viewport {
            self.viewport_cells = packet
                .cells()
                .filter(|((x, y), _)| *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y)
                .collect();
        } else {
            self.viewport_cells = packet.cells().collect();
        }
        self.generation = packet.generation;
        self.population = telemetry.population;
        self.is_running = telemetry.is_running;
        self.gps = telemetry.gps;
        self.work_rate = telemetry.work_rate;
        self.net_rate = telemetry.net_rate;
        self.bounds = telemetry.bounds;

        if let Some(ctx) = &self.repaint_ctx {
            if crate::utils::should_repaint(ctx) {
                ctx.request_repaint();
            }
        }
    }

    fn update_bounds(&mut self, bounds: Option<((i128, i128), (i128, i128))>) {
        self.bounds = bounds;
    }

    fn get_viewport(&self) -> Option<((i128, i128), (i128, i128))> {
        self.target_viewport
    }
}
