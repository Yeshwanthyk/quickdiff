//! Terminal UI using ratatui.

mod app;
mod input;
pub mod render;

pub use app::App;
pub use input::handle_input;
pub use render::render;
