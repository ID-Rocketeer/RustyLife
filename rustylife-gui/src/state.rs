use rustylife_core::{BinaryPacket, SimulationPresenter};

#[derive(Clone)]
pub struct AppState {
    pub viewport_cells: Vec<((i128, i128), u8)>,
    pub generation: u64,
    pub total_cells: u64,
    pub is_running: bool,
    pub target_viewport: Option<((i128, i128), (i128, i128))>,
    pub gps: f64,
    pub work_rate: f64,
    pub net_rate: f64,
    pub cores: usize,
    pub is_connected: bool,
    pub patterns: Vec<rustylife_core::PatternInfo>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            viewport_cells: Vec::new(),
            generation: 0,
            total_cells: 0,
            is_running: false,
            target_viewport: None,
            gps: 0.0,
            work_rate: 0.0,
            net_rate: 0.0,
            cores: 0,
            is_connected: false, // Default to false, Native/Server execution should set to true
            patterns: Vec::new(),
        }
    }
}

impl SimulationPresenter for AppState {
    fn update_state(&mut self, packet: BinaryPacket<'_>) {
        if let Some(((min_x, min_y), (max_x, max_y))) = self.target_viewport {
            self.viewport_cells = packet
                .cells()
                .filter(|((x, y), _)| *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y)
                .collect();
        } else {
            self.viewport_cells = packet.cells().collect();
        }
        self.generation = packet.generation;
        self.total_cells = packet.total_cells;
        self.is_running = packet.is_running;
        self.gps = packet.gps;
        self.work_rate = packet.work_rate;
        self.net_rate = packet.net_rate;
    }

    fn get_viewport(&self) -> Option<((i128, i128), (i128, i128))> {
        self.target_viewport
    }
}
