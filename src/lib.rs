//! quickdiff - A git/jj-first terminal diff viewer.
//!
//! A TUI application for reviewing git/jj diffs with syntax highlighting,
//! hunk navigation, and comment support.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use quickdiff::prelude::*;
//!
//! let repo = RepoRoot::discover(std::path::Path::new("."))?;
//! let files = quickdiff::core::list_changed_files(&repo)?;
//! ```

#![deny(missing_docs)]

pub mod cli;
pub mod core;
pub mod highlight;
pub mod metrics;
pub mod prelude;
pub mod theme;
pub mod ui;
