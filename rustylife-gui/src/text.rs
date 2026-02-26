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
