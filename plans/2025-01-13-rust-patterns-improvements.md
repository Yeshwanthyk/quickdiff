# Rust Patterns Improvements Implementation Plan

## Overview

Apply production-grade Rust patterns (from ripgrep/crossbeam/parking_lot analysis) to quickdiff. 
Improves API stability, documentation, safety, and performance.

## Current State

- `RepoError` enum at `src/core/repo.rs:10` lacks `#[non_exhaustive]`
- No `#![deny(missing_docs)]` in `src/lib.rs`
- `RelPath::new()` uses `debug_assert!` only — panics not enforced in release
- `DiffWorker` spawns detached thread with no graceful shutdown
- `DiffResult` clones full `Vec<RenderRow>` on clone
- Config dir computed on every call to `dirs_fallback()`
- Tree-sitter languages always compiled, no feature flags
- No integration tests with real git repos

## Desired End State

1. All error enums marked `#[non_exhaustive]`
2. `#![deny(missing_docs)]` enforced crate-wide
3. `RelPath::new()` returns `Result`, unsafe variant available
4. Worker thread joins on drop
5. `DiffResult` uses `Arc<[T]>` for cheap clones
6. Config dir cached in `OnceLock`
7. Languages gated behind feature flags
8. Integration tests exercise git operations

**Verification:**
```bash
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
```

## Out of Scope

- Builder pattern for `App` (deferred — significant refactor)
- Strategy enums for binary handling (deferred — low impact)
- Full integration test suite (this plan adds foundation only)

---

## Phase 1: Error Enum Hardening

### Overview
Add `#[non_exhaustive]` to all public error enums for semver stability.

### Prerequisites
- [ ] Clean `cargo check`

### Changes

#### 1. RepoError
**File**: `src/core/repo.rs`
**Lines**: 10-23

**Before**:
```rust
/// Errors from repository operations.
#[derive(Debug, Error)]
pub enum RepoError {
    #[error("not inside a git repository")]
    NotARepo,
```

**After**:
```rust
/// Errors from repository operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RepoError {
    #[error("not inside a git repository")]
    NotARepo,
```

**Why**: Allows adding variants without breaking downstream `match` statements.

### Success Criteria

**Automated**:
```bash
cargo check 2>&1 | grep -i error && exit 1 || echo "OK"
cargo test
```

### Rollback
```bash
git checkout HEAD -- src/core/repo.rs
```

---

## Phase 2: OnceLock for Config Directory

### Overview
Cache config directory path to avoid repeated computation and env var lookups.

### Prerequisites
- [ ] Phase 1 complete

### Changes

#### 1. Add OnceLock import and static
**File**: `src/core/viewed.rs`
**Lines**: 1-10 (add to imports)

**Add after existing imports**:
```rust
use std::sync::OnceLock;
```

#### 2. Add static cache
**File**: `src/core/viewed.rs`
**Lines**: After imports, before `ViewedStore` trait (around line 10)

**Add**:
```rust
/// Cached config directory path.
static CONFIG_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Get the quickdiff config directory (cached).
fn config_dir() -> &'static std::path::Path {
    CONFIG_DIR.get_or_init(|| {
        directories::ProjectDirs::from("", "", "quickdiff")
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(dirs_fallback)
    })
}
```

#### 3. Update default_state_path
**File**: `src/core/viewed.rs`
**Lines**: 132-139

**Before**:
```rust
    /// Get the default state file path.
    fn default_state_path() -> std::io::Result<std::path::PathBuf> {
        let config_dir = directories::ProjectDirs::from("", "", "quickdiff")
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(dirs_fallback);

        Ok(config_dir.join("state.json"))
    }
```

**After**:
```rust
    /// Get the default state file path.
    fn default_state_path() -> std::io::Result<std::path::PathBuf> {
        Ok(config_dir().join("state.json"))
    }
```

### Success Criteria

**Automated**:
```bash
cargo test -- viewed
```

### Rollback
```bash
git checkout HEAD -- src/core/viewed.rs
```

---

## Phase 3: RelPath Validation

### Overview
Make `RelPath::new()` fallible in release builds. Provide unchecked variant for trusted internal use.

### Prerequisites
- [ ] Phase 2 complete

### Changes

#### 1. Add error type
**File**: `src/core/repo.rs`
**Lines**: After `RepoError` enum (around line 24)

**Add**:
```rust
/// Error when constructing a RelPath with an absolute path.
#[derive(Debug, Clone, thiserror::Error)]
#[error("path must be relative, got: {0}")]
pub struct InvalidRelPath(pub String);
```

#### 2. Update RelPath impl
**File**: `src/core/repo.rs`
**Lines**: 88-100

**Before**:
```rust
impl RelPath {
    /// Create a new RelPath from a string.
    /// Panics if the path is absolute.
    pub fn new(path: impl Into<String>) -> Self {
        let path = path.into();
        debug_assert!(
            !path.starts_with('/'),
            "RelPath must not be absolute: {}",
            path
        );
        Self(path)
    }
```

**After**:
```rust
impl RelPath {
    /// Create a new RelPath from a string.
    ///
    /// Returns an error if the path is absolute (starts with `/`).
    pub fn try_new(path: impl Into<String>) -> Result<Self, InvalidRelPath> {
        let path = path.into();
        if path.starts_with('/') {
            return Err(InvalidRelPath(path));
        }
        Ok(Self(path))
    }

    /// Create a new RelPath without validation.
    ///
    /// # Safety (logical)
    /// Caller must ensure `path` is relative (does not start with `/`).
    /// Used for trusted input from git commands.
    pub fn new_unchecked(path: impl Into<String>) -> Self {
        let path = path.into();
        debug_assert!(
            !path.starts_with('/'),
            "RelPath must not be absolute: {}",
            path
        );
        Self(path)
    }

    /// Convenience alias for `new_unchecked` — use when path is from git output.
    #[inline]
    pub fn new(path: impl Into<String>) -> Self {
        Self::new_unchecked(path)
    }
```

**Why**: 
- `try_new()` is the safe public API
- `new()` remains for internal use (git output is trusted)
- `new_unchecked()` explicitly documents the trust assumption

#### 3. Export InvalidRelPath
**File**: `src/core/mod.rs`
**Lines**: After existing exports

**Current**:
```rust
pub use repo::*;
```

No change needed — wildcard already exports it.

#### 4. Update test
**File**: `src/core/repo.rs`
**Lines**: In test module (around line 588)

**Add test**:
```rust
    #[test]
    fn relpath_try_new_rejects_absolute() {
        assert!(RelPath::try_new("/absolute/path").is_err());
        assert!(RelPath::try_new("relative/path").is_ok());
    }
```

### Success Criteria

**Automated**:
```bash
cargo test -- relpath
```

### Rollback
```bash
git checkout HEAD -- src/core/repo.rs src/core/mod.rs
```

---

## Phase 4: Worker Thread Graceful Shutdown

### Overview
Store thread handle and join on drop to prevent resource leaks.

### Prerequisites
- [ ] Phase 3 complete

### Changes

#### 1. Update imports
**File**: `src/ui/worker.rs`
**Lines**: 3-4

**Before**:
```rust
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
```

**After**:
```rust
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
```

#### 2. Update DiffWorker struct
**File**: `src/ui/worker.rs`
**Lines**: 31-35

**Before**:
```rust
#[derive(Debug)]
pub(crate) struct DiffWorker {
    pub request_tx: Sender<DiffLoadRequest>,
    pub response_rx: Receiver<DiffLoadResponse>,
}
```

**After**:
```rust
pub(crate) struct DiffWorker {
    pub request_tx: Sender<DiffLoadRequest>,
    pub response_rx: Receiver<DiffLoadResponse>,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for DiffWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiffWorker")
            .field("request_tx", &self.request_tx)
            .field("response_rx", &self.response_rx)
            .field("handle", &self.handle.as_ref().map(|_| "..."))
            .finish()
    }
}
```

#### 3. Update spawn_diff_worker
**File**: `src/ui/worker.rs`
**Lines**: 37-47

**Before**:
```rust
pub(crate) fn spawn_diff_worker(repo: RepoRoot) -> DiffWorker {
    let (request_tx, request_rx) = mpsc::channel::<DiffLoadRequest>();
    let (response_tx, response_rx) = mpsc::channel::<DiffLoadResponse>();

    thread::spawn(move || worker_loop(repo, request_rx, response_tx));

    DiffWorker {
        request_tx,
        response_rx,
    }
}
```

**After**:
```rust
pub(crate) fn spawn_diff_worker(repo: RepoRoot) -> DiffWorker {
    let (request_tx, request_rx) = mpsc::channel::<DiffLoadRequest>();
    let (response_tx, response_rx) = mpsc::channel::<DiffLoadResponse>();

    let handle = thread::spawn(move || worker_loop(repo, request_rx, response_tx));

    DiffWorker {
        request_tx,
        response_rx,
        handle: Some(handle),
    }
}
```

#### 4. Add Drop impl
**File**: `src/ui/worker.rs`
**Lines**: After DiffWorker struct (around line 47)

**Add**:
```rust
impl Drop for DiffWorker {
    fn drop(&mut self) {
        // Dropping request_tx closes the channel, causing worker_loop to exit.
        // We must drop it first (it's already being dropped), then join.
        // The Sender is dropped automatically, so just join the thread.
        if let Some(handle) = self.handle.take() {
            // Don't panic if thread panicked — just ignore
            let _ = handle.join();
        }
    }
}
```

### Success Criteria

**Automated**:
```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

**Manual**:
- [ ] Run `quickdiff`, quit with `q`, verify clean exit (no hung process)

### Rollback
```bash
git checkout HEAD -- src/ui/worker.rs
```

---

## Phase 5: Arc for DiffResult

### Overview
Use `Arc<[T]>` for rows and hunks to make `DiffResult` cheap to clone.

### Prerequisites
- [ ] Phase 4 complete

### Changes

#### 1. Update imports
**File**: `src/core/diff.rs`
**Lines**: 1-5

**Add import**:
```rust
use std::sync::Arc;
```

#### 2. Update DiffResult struct
**File**: `src/core/diff.rs`
**Lines**: 69-74

**Before**:
```rust
/// Complete diff result between two text buffers.
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// All render rows.
    pub rows: Vec<RenderRow>,
    /// Hunk index for navigation (sorted by start_row).
    pub hunks: Vec<Hunk>,
}
```

**After**:
```rust
/// Complete diff result between two text buffers.
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// All render rows (Arc for cheap cloning).
    rows: Arc<[RenderRow]>,
    /// Hunk index for navigation (sorted by start_row).
    hunks: Arc<[Hunk]>,
}
```

#### 3. Add accessor methods
**File**: `src/core/diff.rs`
**Lines**: After DiffResult struct, in impl block

**Add to impl DiffResult** (around line 77):
```rust
    /// Get all render rows.
    pub fn rows(&self) -> &[RenderRow] {
        &self.rows
    }

    /// Get all hunks.
    pub fn hunks(&self) -> &[Hunk] {
        &self.hunks
    }
```

#### 4. Update compute_diff return
**File**: `src/core/diff.rs`
**Lines**: In `compute_diff` function (around line 170)

**Before**:
```rust
    DiffResult { rows, hunks }
```

**After**:
```rust
    DiffResult {
        rows: rows.into(),
        hunks: hunks.into(),
    }
```

#### 5. Update methods using rows/hunks directly
**File**: `src/core/diff.rs`
**Lines**: Various — update field access to use accessor

In `row_count()`:
```rust
    pub fn row_count(&self) -> usize {
        self.rows.len()  // Still works — Arc<[T]> derefs to [T]
    }
```

In `render_rows()`:
```rust
    pub fn render_rows(&self, start_row: usize, height: usize) -> impl Iterator<Item = &RenderRow> {
        self.rows.iter().skip(start_row).take(height)
    }
```

In `next_hunk_row()`:
```rust
    pub fn next_hunk_row(&self, current_row: usize) -> Option<usize> {
        let idx = self.hunks.partition_point(|h| h.start_row <= current_row);
        self.hunks.get(idx).map(|h| h.start_row)
    }
```

These should all work without changes due to `Deref` — verify with `cargo check`.

#### 6. Update external access
**File**: `src/ui/app.rs`
**Lines**: Various — search for `diff.rows` and `diff.hunks`

**Search and update pattern**:
- `diff.rows` → `diff.rows()` (if accessing as slice)
- `diff.hunks` → `diff.hunks()` (if accessing as slice)
- `diff.rows.iter()` → `diff.rows().iter()` 
- `diff.hunks.len()` → `diff.hunks().len()`

Run `cargo check` to find all locations that need updating.

#### 7. Update cli/comments.rs
**File**: `src/cli/comments.rs`
**Lines**: Search for `diff.hunks`

Update field access to use accessor methods.

### Success Criteria

**Automated**:
```bash
cargo check
cargo test
```

### Rollback
```bash
git checkout HEAD -- src/core/diff.rs src/ui/app.rs src/cli/comments.rs
```

---

## Phase 6: Feature Flags for Languages

### Overview
Gate tree-sitter language grammars behind feature flags to reduce binary size.

### Prerequisites
- [ ] Phase 5 complete

### Changes

#### 1. Update Cargo.toml
**File**: `Cargo.toml`
**Lines**: After `[dev-dependencies]`

**Add features section**:
```toml
[features]
default = ["lang-rust", "lang-typescript"]
lang-rust = ["dep:tree-sitter-rust"]
lang-typescript = ["dep:tree-sitter-typescript"]
```

**Update dependencies**:
```toml
# Highlighting
tree-sitter = "0.24"
tree-sitter-highlight = "0.24"
tree-sitter-rust = { version = "0.23", optional = true }
tree-sitter-typescript = { version = "0.23", optional = true }
```

#### 2. Update highlight/mod.rs
**File**: `src/highlight/mod.rs`
**Lines**: At top, add cfg gates

**Add after imports**:
```rust
#[cfg(feature = "lang-rust")]
use tree_sitter_rust;

#[cfg(feature = "lang-typescript")]
use tree_sitter_typescript;
```

**Update LanguageId::from_extension**:
```rust
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            #[cfg(feature = "lang-rust")]
            "rs" => Self::Rust,
            #[cfg(feature = "lang-typescript")]
            "ts" => Self::TypeScript,
            #[cfg(feature = "lang-typescript")]
            "tsx" => Self::TypeScriptReact,
            #[cfg(feature = "lang-typescript")]
            "js" | "mjs" | "cjs" => Self::JavaScript,
            #[cfg(feature = "lang-typescript")]
            "jsx" => Self::JavaScriptReact,
            _ => Self::Plain,
        }
    }
```

**Update TreeSitterHighlighter::new**:
```rust
    pub fn new(lang: LanguageId) -> Option<Self> {
        let (language, highlights_query) = match lang {
            #[cfg(feature = "lang-rust")]
            LanguageId::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::HIGHLIGHTS_QUERY,
            ),
            #[cfg(feature = "lang-typescript")]
            LanguageId::TypeScript | LanguageId::JavaScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ),
            #[cfg(feature = "lang-typescript")]
            LanguageId::TypeScriptReact | LanguageId::JavaScriptReact => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ),
            LanguageId::Plain => return None,
            #[allow(unreachable_patterns)]
            _ => return None,  // Disabled languages
        };
        // ... rest unchanged
    }
```

#### 3. Update LanguageId enum
**File**: `src/highlight/mod.rs`
**Lines**: LanguageId enum definition

**Update**:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageId {
    #[cfg(feature = "lang-rust")]
    Rust,
    #[cfg(feature = "lang-typescript")]
    TypeScript,
    #[cfg(feature = "lang-typescript")]
    TypeScriptReact,
    #[cfg(feature = "lang-typescript")]
    JavaScript,
    #[cfg(feature = "lang-typescript")]
    JavaScriptReact,
    Plain,
}
```

### Success Criteria

**Automated**:
```bash
# With all features (default)
cargo build --all-features
cargo test --all-features

# Without language features
cargo build --no-default-features
cargo test --no-default-features

# Just rust
cargo build --no-default-features --features lang-rust
```

### Rollback
```bash
git checkout HEAD -- Cargo.toml src/highlight/mod.rs
```

---

## Phase 7: Documentation Enforcement

### Overview
Add `#![deny(missing_docs)]` and document all public items.

### Prerequisites
- [ ] Phase 6 complete

### Changes

#### 1. Update lib.rs
**File**: `src/lib.rs`
**Lines**: 1-7

**Before**:
```rust
//! quickdiff - A git-first terminal diff viewer.

pub mod cli;
pub mod core;
pub mod highlight;
pub mod theme;
pub mod ui;
```

**After**:
```rust
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
```

#### 2. Fix missing docs
Run `cargo doc` and fix each warning. Common fixes needed:

**File**: `src/core/mod.rs`
```rust
//! Core primitives for quickdiff (no TUI dependencies).
```

**File**: `src/ui/mod.rs`
```rust
//! Terminal UI components.
```

**File**: `src/cli/mod.rs`
```rust
//! CLI subcommands.
```

**File**: `src/theme/mod.rs` (already has docs)

**File**: `src/highlight/mod.rs` (already has docs)

Continue fixing each undocumented public item reported by `cargo doc`.

### Success Criteria

**Automated**:
```bash
cargo doc 2>&1 | grep -i "missing documentation" && exit 1 || echo "OK"
cargo clippy --all-targets -- -D warnings
```

### Rollback
```bash
git checkout HEAD -- src/
```

---

## Phase 8: Integration Test Foundation

### Overview
Add test infrastructure for integration tests with real git repos.

### Prerequisites
- [ ] Phase 7 complete

### Changes

#### 1. Create tests directory
**Path**: `tests/`

#### 2. Add integration test file
**File**: `tests/git_integration.rs`

**Content**:
```rust
//! Integration tests with real git repositories.

use std::process::Command;
use tempfile::TempDir;

/// Create a temporary git repo with some commits.
fn create_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .unwrap();

    // Configure git for commits
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .unwrap();

    // Create initial commit
    std::fs::write(path.join("file.txt"), "initial content\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(path)
        .output()
        .unwrap();

    dir
}

#[test]
fn test_repo_discovery() {
    let dir = create_test_repo();
    let repo = quickdiff::core::RepoRoot::discover(dir.path()).unwrap();
    assert!(repo.path().exists());
}

#[test]
fn test_list_changed_files_empty() {
    let dir = create_test_repo();
    let repo = quickdiff::core::RepoRoot::discover(dir.path()).unwrap();
    let files = quickdiff::core::list_changed_files(&repo).unwrap();
    assert!(files.is_empty(), "Clean repo should have no changes");
}

#[test]
fn test_list_changed_files_modified() {
    let dir = create_test_repo();
    let path = dir.path();
    
    // Modify file
    std::fs::write(path.join("file.txt"), "modified content\n").unwrap();
    
    let repo = quickdiff::core::RepoRoot::discover(path).unwrap();
    let files = quickdiff::core::list_changed_files(&repo).unwrap();
    
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path.as_str(), "file.txt");
    assert_eq!(files[0].kind, quickdiff::core::FileChangeKind::Modified);
}

#[test]
fn test_list_changed_files_untracked() {
    let dir = create_test_repo();
    let path = dir.path();
    
    // Add new untracked file
    std::fs::write(path.join("new.txt"), "new file\n").unwrap();
    
    let repo = quickdiff::core::RepoRoot::discover(path).unwrap();
    let files = quickdiff::core::list_changed_files(&repo).unwrap();
    
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path.as_str(), "new.txt");
    assert_eq!(files[0].kind, quickdiff::core::FileChangeKind::Untracked);
}
```

### Success Criteria

**Automated**:
```bash
cargo test --test git_integration
```

### Rollback
```bash
rm -rf tests/
```

---

## Testing Strategy

### Unit Tests (existing + additions)
- Phase 3 adds `relpath_try_new_rejects_absolute`

### Integration Tests (new)
- Phase 8 adds `tests/git_integration.rs`

### Manual Testing Checklist
1. [ ] Run `quickdiff` in a dirty repo — sidebar shows files
2. [ ] Navigate with j/k — smooth scrolling
3. [ ] Press `q` — clean exit, no hung process (Phase 4)
4. [ ] Build with `--no-default-features` — compiles without languages (Phase 6)

## Anti-Patterns to Avoid

- **Don't use `unwrap()` in library code** — propagate errors
- **Don't add `pub` to internal helpers** — keep API surface small
- **Don't break existing tests** — verify after each phase

## Open Questions

- [x] Should `RelPath::new()` panic or return Result? → Keep `new()` as unchecked for compat, add `try_new()`
- [x] Should features be additive or exclusive? → Additive, `default` includes all

## References

- Analysis document: Previous conversation
- Pattern source: `/Users/yesh/Documents/personal/dump/rust-patterns.md`
- Ripgrep error patterns: `crates/ignore/src/lib.rs`
