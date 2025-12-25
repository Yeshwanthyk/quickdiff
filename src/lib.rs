//! quickdiff - A git-first terminal diff viewer.
//!
//! A TUI application for reviewing git diffs with syntax highlighting,
//! hunk navigation, and comment support.

#![deny(missing_docs)]

pub mod cli;
pub mod core;
pub mod highlight;
pub mod theme;
pub mod ui;
