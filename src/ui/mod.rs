//! Terminal UI using ratatui.

mod app;
mod input;
mod render;
mod worker;

pub use app::{App, DiffPaneMode, Focus, Mode};
pub use input::handle_input;
pub use render::render;
