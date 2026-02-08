//! Shared text constants for Tooltips and UI labels

pub const TOOLTIP_ZOOM: &str = "Current magnification level (pixels per cell)";
pub const TOOLTIP_EXTENT: &str = "Current viewport dimensions (width x height) in universe cells";
pub const TOOLTIP_CENTER: &str = "Universe coordinates at the center of the screen";
pub const TOOLTIP_WORK: &str =
    "Computational Flux: Total Population processed per real-time second";
pub const TOOLTIP_NET: &str = "Net Reproductive Pressure: Population change per real-time second";
pub const TOOLTIP_GPS: &str = "Generations Per Second: Simulation speed";
pub const TOOLTIP_CORES: &str = "Active worker threads";

pub const STATUS_CONNECTED: &str = "Connected";
pub const STATUS_DISCONNECTED: &str = "Disconnected - retrying...";
