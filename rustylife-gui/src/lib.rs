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

// Force rebuild
pub mod app;
pub mod projection;
pub mod state;
pub mod style;
pub mod text;
pub mod utils;
pub use app::RustyLifeApp;
pub use state::AppState;
pub use utils::{fmt_num, format_si, should_repaint};

/// Trait for handling user actions from the GUI.
/// This abstracts away whether the action is performed directly on the engine (Server)
/// or sent over IPC (Client).
pub trait UserActionHandler: Send + Sync {
    fn start(&mut self);
    fn stop(&mut self);
    fn step(&mut self);
    fn reset(&mut self);
    fn seed(&mut self, pattern: String);
    fn request_state(&mut self, generation: u64, viewport: Option<((i128, i128), (i128, i128))>);
    fn shutdown(&mut self);
}
