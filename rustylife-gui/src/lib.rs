// Force rebuild
pub mod app;
pub mod state;
pub mod style;
pub mod text;
pub mod utils;
pub use app::RustyLifeApp;
pub use state::AppState;
pub use utils::{fmt_num, format_si};

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
