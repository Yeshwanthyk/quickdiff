//! Core primitives for quickdiff (no TUI dependencies).

mod diff;
mod repo;
mod text;
mod viewed;

pub use diff::*;
pub use repo::*;
pub use text::*;
pub use viewed::*;
