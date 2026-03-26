//! Core primitives for quickdiff (no TUI dependencies).

mod comments;
mod comments_store;
mod config;
mod diff;
mod fuzzy;
mod gh;
mod pr_diff;
mod repo;
mod stdin_input;
mod text;
mod viewed;
mod watcher;

pub use comments::*;
pub use comments_store::*;
pub use config::*;
pub use diff::*;
pub use fuzzy::*;
pub use gh::*;
pub use pr_diff::*;
pub use repo::*;
pub use stdin_input::*;
pub use text::*;
pub use viewed::*;
pub use watcher::*;
