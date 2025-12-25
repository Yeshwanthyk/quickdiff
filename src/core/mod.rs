//! Core primitives for quickdiff (no TUI dependencies).

mod comments;
mod comments_store;
mod diff;
mod repo;
mod text;
mod viewed;
mod watcher;

pub use comments::*;
pub use comments_store::*;
pub use diff::*;
pub use repo::*;
pub use text::*;
pub use viewed::*;
pub use watcher::*;
