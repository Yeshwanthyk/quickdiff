//! Common re-exports for convenient importing.
//!
//! # Example
//!
//! ```rust,ignore
//! use quickdiff::prelude::*;
//! ```

pub use crate::core::{
    ChangedFile, DiffResult, DiffSource, FileChangeKind, RelPath, RepoError, RepoRoot, TextBuffer,
};
