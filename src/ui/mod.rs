//! Terminal UI using ratatui.

mod app;
mod input;
pub mod render;
mod worker;

pub use app::App;
pub use input::handle_input;
pub use render::render;
