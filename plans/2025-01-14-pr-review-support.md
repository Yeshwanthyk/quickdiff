# PR Review Support Implementation Plan

## Plan Metadata
- Created: 2025-01-14
- Status: complete
- Assumptions:
  - User has `gh` CLI installed and authenticated (`gh auth login`)
  - PRs are from GitHub repositories (not GitLab, Bitbucket, etc.)
  - Initial version uses patch-only display (no full file fetch)

## Progress Tracking
- [x] Phase 1: GitHub CLI Wrapper Module
- [x] Phase 2: DiffSource Extension & PR Diff Parser
- [x] Phase 3: PR Picker UI
- [x] Phase 4: PR Diff Viewing Integration
- [x] Phase 5: PR Review Actions & Polish

## Overview

Add GitHub PR review capabilities to quickdiff, enabling users to:
1. List and select open PRs (with filters: mine, review-requested, all)
2. View PR diffs in the existing side-by-side viewer
3. Submit reviews (approve, comment, request changes)

This mirrors the PR functionality in guck/cerebro but in a terminal UI.

## Current State

quickdiff currently supports local git operations via shell-out to `git`:
- `DiffSource` enum in `src/core/repo.rs:47-68` handles WorkingTree, Commit, Range, Base
- `App` in `src/ui/app.rs` manages UI state with `Mode` enum for input modes
- Background diff loading via `src/ui/worker.rs`
- File watching for auto-refresh in WorkingTree/Base modes

### Key Discoveries

**DiffSource enum** (`src/core/repo.rs:47-68`):
```rust
pub enum DiffSource {
    WorkingTree,
    Commit(String),
    Range { from: String, to: String },
    Base(String),
}
```

**Mode enum** (`src/ui/app.rs:35-43`):
```rust
pub enum Mode {
    #[default]
    Normal,
    AddComment,
    ViewComments,
    FilterFiles,
    SelectTheme,
    Help,
}
```

**Worker pattern** (`src/ui/worker.rs`): Background thread for diff computation with request/response channels.

**Event loop** (`src/main.rs`): Polls at 50ms intervals, calls `app.poll_worker()` and `app.poll_watcher()`.

## Desired End State

1. `quickdiff --pr` launches PR picker mode
2. `quickdiff --pr 123` opens directly to PR #123's diff
3. In PR picker: filter tabs, PR list, keyboard navigation
4. PR diff view: file sidebar, side-by-side diff (patch-based initially)
5. Review actions: `A` approve, `C` comment, `R` request changes
6. Auto-refresh on terminal focus + `r` manual refresh

### Verification
```bash
# Launch PR picker
quickdiff --pr

# Open specific PR
quickdiff --pr 42

# In app: press A to approve, should see "PR #42 approved" status
```

## Out of Scope
- Line-level inline comments (GitHub's review comment threads)
- CI/check status display
- Draft PR support (excluded from list)
- Full file content fetching (patch-only for v1)
- Cross-forge support (GitLab, Bitbucket)

## Breaking Changes
None. All changes are additive.

## Dependency and Configuration Changes

### Additions
None required. Uses `gh` CLI which user must have installed.

### Configuration Changes
None.

## Error Handling Strategy

All `gh` CLI errors surface as user-visible status messages:
- Auth errors: "GitHub auth required. Run 'gh auth login'"
- Network errors: "Failed to fetch PRs: <message>"
- Not found: "PR #N not found"

Pattern: Match existing `error_msg`/`status_msg` fields in `App`.

## Implementation Approach

**Why shell out to `gh`**: Same pattern as existing git operations. No OAuth complexity, piggybacks on user's existing auth. Battle-tested in guck/cerebro.

**Why patch-only display**: `gh pr diff` returns unified patches. Full file fetch requires additional API calls per file. Start simple, add later if needed.

**Why extend DiffSource**: Keeps unified diff loading pipeline. PR becomes another source type, reusing existing TextBuffer/DiffResult infrastructure.

**Alternative rejected**: Separate `PRMode` struct - would duplicate too much App state management.

## Phase Dependencies and Parallelization
- Phase 1 → Phase 2 (gh wrapper needed for diff fetching)
- Phase 2 → Phase 3, 4 (types needed for UI)
- Phase 3, 4 can partially overlap (picker UI vs diff integration)
- Phase 5 depends on all prior phases

---

## Phase 1: GitHub CLI Wrapper Module

### Overview
Create `src/core/gh.rs` with functions to interact with GitHub via `gh` CLI.

### Prerequisites
- [ ] `gh` CLI installed locally for testing

### Change Checklist
- [x] Create `src/core/gh.rs` with PR types and CLI wrappers
- [x] Add `mod gh` to `src/core/mod.rs`
- [x] Add `pub use gh::*` export

### Changes

#### 1. Create GitHub CLI Wrapper
**File**: `src/core/gh.rs`
**Location**: new file

**Add**:
```rust
//! GitHub CLI (`gh`) wrapper for PR operations.

use std::process::Command;

use serde::Deserialize;
use thiserror::Error;

/// Errors from GitHub CLI operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GhError {
    /// `gh` CLI not found or not authenticated.
    #[error("GitHub CLI not available or not authenticated. Run 'gh auth login'")]
    NotAvailable,
    /// GitHub API/CLI error.
    #[error("GitHub error: {0}")]
    ApiError(String),
    /// Failed to parse CLI output.
    #[error("Failed to parse gh output: {0}")]
    ParseError(String),
    /// I/O error running command.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Filter for PR listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PRFilter {
    /// All open PRs.
    #[default]
    All,
    /// PRs authored by current user.
    Mine,
    /// PRs where review is requested from current user.
    ReviewRequested,
}

/// PR author info.
#[derive(Debug, Clone, Deserialize)]
pub struct PRAuthor {
    pub login: String,
}

/// A pull request summary (from list).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u32,
    pub title: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub author: PRAuthor,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    #[serde(default)]
    pub is_draft: bool,
}

/// Check if `gh` CLI is available and authenticated.
pub fn is_gh_available() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// List open (non-draft) PRs for the repository at `repo_path`.
pub fn list_prs(repo_path: &std::path::Path, filter: PRFilter) -> Result<Vec<PullRequest>, GhError> {
    let mut args = vec![
        "pr",
        "list",
        "--state",
        "open",
        "--json",
        "number,title,headRefName,baseRefName,author,additions,deletions,changedFiles,isDraft",
    ];

    match filter {
        PRFilter::All => {}
        PRFilter::Mine => {
            args.push("--author");
            args.push("@me");
        }
        PRFilter::ReviewRequested => {
            args.push("--search");
            args.push("review-requested:@me");
        }
    }

    let output = Command::new("gh")
        .args(&args)
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("auth") || stderr.contains("login") {
            return Err(GhError::NotAvailable);
        }
        return Err(GhError::ApiError(stderr.trim().to_string()));
    }

    let prs: Vec<PullRequest> = serde_json::from_slice(&output.stdout)
        .map_err(|e| GhError::ParseError(e.to_string()))?;

    // Filter out drafts
    Ok(prs.into_iter().filter(|pr| !pr.is_draft).collect())
}

/// Get the unified diff for a PR.
pub fn get_pr_diff(repo_path: &std::path::Path, pr_number: u32) -> Result<String, GhError> {
    let output = Command::new("gh")
        .args(["pr", "diff", &pr_number.to_string()])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError::ApiError(stderr.trim().to_string()));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| GhError::ParseError(format!("Invalid UTF-8: {}", e)))
}

/// Approve a PR.
pub fn approve_pr(
    repo_path: &std::path::Path,
    pr_number: u32,
    body: Option<&str>,
) -> Result<(), GhError> {
    let mut args = vec!["pr", "review", &pr_number.to_string(), "--approve"];
    
    let body_owned;
    if let Some(b) = body {
        body_owned = b.to_string();
        args.push("-b");
        args.push(&body_owned);
    }

    let output = Command::new("gh")
        .args(&args)
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError::ApiError(stderr.trim().to_string()));
    }

    Ok(())
}

/// Add a comment review to a PR.
pub fn comment_pr(
    repo_path: &std::path::Path,
    pr_number: u32,
    body: &str,
) -> Result<(), GhError> {
    let output = Command::new("gh")
        .args(["pr", "review", &pr_number.to_string(), "--comment", "-b", body])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError::ApiError(stderr.trim().to_string()));
    }

    Ok(())
}

/// Request changes on a PR.
pub fn request_changes_pr(
    repo_path: &std::path::Path,
    pr_number: u32,
    body: &str,
) -> Result<(), GhError> {
    let output = Command::new("gh")
        .args(["pr", "review", &pr_number.to_string(), "--request-changes", "-b", body])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError::ApiError(stderr.trim().to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_filter_default_is_all() {
        assert_eq!(PRFilter::default(), PRFilter::All);
    }
}
```

#### 2. Export from core module
**File**: `src/core/mod.rs`
**Location**: after line 10 (after `mod watcher;`)

**Add**:
```rust
mod gh;
```

**Location**: after line 20 (after `pub use watcher::*;`)

**Add**:
```rust
pub use gh::*;
```

### Success Criteria

**Automated**:
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

**Manual** (requires `gh` auth):
```bash
# In a repo with PRs:
cargo run -- --help  # Should still work (no breakage)
```

### Rollback
```bash
rm src/core/gh.rs
git restore -- src/core/mod.rs
```

---

## Phase 2: DiffSource Extension & PR Diff Parser

### Overview
Extend `DiffSource` with `PullRequest` variant and add unified diff parser.

### Prerequisites
- [ ] Phase 1 complete
- [ ] Phase 1 automated checks pass

### Change Checklist
- [x] Add `PullRequest` variant to `DiffSource`
- [x] Create `src/core/pr_diff.rs` with unified diff parser
- [x] Add `PRChangedFile` type for parsed PR files
- [x] Export new types from core module

### Changes

#### 1. Extend DiffSource enum
**File**: `src/core/repo.rs`
**Location**: lines 47-68 (DiffSource enum)

**Before**:
```rust
/// Source specification for diff comparison.
#[derive(Debug, Clone)]
pub enum DiffSource {
    /// Working tree changes vs HEAD (default behavior).
    WorkingTree,
    /// Single commit (show changes introduced by that commit).
    Commit(String),
    /// Range of commits (from..to).
    Range {
        /// Starting commit.
        from: String,
        /// Ending commit.
        to: String,
    },
    /// Compare against a base ref (e.g., origin/main).
    Base(String),
}
```

**After**:
```rust
/// Source specification for diff comparison.
#[derive(Debug, Clone)]
pub enum DiffSource {
    /// Working tree changes vs HEAD (default behavior).
    WorkingTree,
    /// Single commit (show changes introduced by that commit).
    Commit(String),
    /// Range of commits (from..to).
    Range {
        /// Starting commit.
        from: String,
        /// Ending commit.
        to: String,
    },
    /// Compare against a base ref (e.g., origin/main).
    Base(String),
    /// GitHub Pull Request.
    PullRequest {
        /// PR number.
        number: u32,
        /// Head branch name.
        head: String,
        /// Base branch name.
        base: String,
    },
}
```

#### 2. Update DiffSource::default impl
**File**: `src/core/repo.rs`
**Location**: lines 70-74 (Default impl)

No change needed - WorkingTree remains default.

#### 3. Update diff_source_display function
**File**: `src/core/repo.rs`
**Location**: around line 540 (diff_source_display function)

**Before** (end of match):
```rust
        DiffSource::Base(base) => format!("vs {}", base),
    }
}
```

**After**:
```rust
        DiffSource::Base(base) => format!("vs {}", base),
        DiffSource::PullRequest { number, head, base } => {
            format!("PR #{} ({} → {})", number, head, base)
        }
    }
}
```

#### 4. Create PR diff parser module
**File**: `src/core/pr_diff.rs`
**Location**: new file

**Add**:
```rust
//! Parser for unified diff output from `gh pr diff`.

use crate::core::{FileChangeKind, RelPath};

/// A file changed in a PR with its patch content.
#[derive(Debug, Clone)]
pub struct PRChangedFile {
    /// Path to the file.
    pub path: RelPath,
    /// Original path (for renames).
    pub old_path: Option<RelPath>,
    /// Type of change.
    pub kind: FileChangeKind,
    /// Number of lines added.
    pub additions: usize,
    /// Number of lines deleted.
    pub deletions: usize,
    /// Raw unified diff patch for this file.
    pub patch: String,
}

/// Parse unified diff output into a list of changed files.
///
/// Splits on `diff --git` boundaries and extracts file info + patch.
pub fn parse_unified_diff(raw_diff: &str) -> Vec<PRChangedFile> {
    let mut files = Vec::new();
    
    // Split on "diff --git " but keep the delimiter
    let chunks: Vec<&str> = raw_diff.split("diff --git ").collect();
    
    for chunk in chunks.iter().skip(1) {
        // Skip empty chunks
        if chunk.trim().is_empty() {
            continue;
        }
        
        // Reconstruct full chunk for patch
        let full_chunk = format!("diff --git {}", chunk);
        
        if let Some(file) = parse_file_chunk(&full_chunk) {
            files.push(file);
        }
    }
    
    // Sort by path for consistent ordering
    files.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    files
}

fn parse_file_chunk(chunk: &str) -> Option<PRChangedFile> {
    let lines: Vec<&str> = chunk.lines().collect();
    let first_line = lines.first()?;
    
    // Parse header: diff --git a/path b/path
    let header_match = parse_diff_header(first_line)?;
    let (old_path_str, new_path_str) = header_match;
    
    // Determine status from diff metadata
    let mut kind = FileChangeKind::Modified;
    let mut old_path = None;
    
    for line in lines.iter().take(10) {
        if line.starts_with("new file mode") {
            kind = FileChangeKind::Added;
        } else if line.starts_with("deleted file mode") {
            kind = FileChangeKind::Deleted;
        } else if line.starts_with("rename from ") {
            kind = FileChangeKind::Renamed;
            let from = line.strip_prefix("rename from ").unwrap_or("");
            old_path = Some(RelPath::new(from));
        }
    }
    
    // For renames without explicit "rename from", use the a/ path
    if kind == FileChangeKind::Renamed && old_path.is_none() && old_path_str != new_path_str {
        old_path = Some(RelPath::new(old_path_str));
    }
    
    // Count additions and deletions
    let mut additions = 0;
    let mut deletions = 0;
    let mut in_hunk = false;
    
    for line in &lines {
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }
        
        if in_hunk {
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }
    }
    
    Some(PRChangedFile {
        path: RelPath::new(new_path_str),
        old_path,
        kind,
        additions,
        deletions,
        patch: chunk.to_string(),
    })
}

/// Parse "diff --git a/path b/path" header.
/// Returns (old_path, new_path) without a/ b/ prefixes.
fn parse_diff_header(line: &str) -> Option<(&str, &str)> {
    // Format: "diff --git a/old/path b/new/path"
    let rest = line.strip_prefix("diff --git ")?;
    
    // Find the split point - look for " b/" pattern
    // Handle paths with spaces by finding last " b/" occurrence
    let b_idx = rest.rfind(" b/")?;
    
    let a_part = &rest[..b_idx];
    let b_part = &rest[b_idx + 1..];
    
    // Strip "a/" and "b/" prefixes
    let old_path = a_part.strip_prefix("a/").unwrap_or(a_part);
    let new_path = b_part.strip_prefix("b/").unwrap_or(b_part);
    
    Some((old_path, new_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_diff() {
        let diff = r#"diff --git a/src/main.rs b/src/main.rs
index abc123..def456 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!("Hello");
     println!("World");
 }
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "src/main.rs");
        assert_eq!(files[0].kind, FileChangeKind::Modified);
        assert_eq!(files[0].additions, 1);
        assert_eq!(files[0].deletions, 0);
    }

    #[test]
    fn parse_new_file() {
        let diff = r#"diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..abc123
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+line 1
+line 2
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "new.txt");
        assert_eq!(files[0].kind, FileChangeKind::Added);
        assert_eq!(files[0].additions, 2);
    }

    #[test]
    fn parse_deleted_file() {
        let diff = r#"diff --git a/old.txt b/old.txt
deleted file mode 100644
index abc123..0000000
--- a/old.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-line 1
-line 2
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "old.txt");
        assert_eq!(files[0].kind, FileChangeKind::Deleted);
        assert_eq!(files[0].deletions, 2);
    }

    #[test]
    fn parse_rename() {
        let diff = r#"diff --git a/old_name.rs b/new_name.rs
similarity index 95%
rename from old_name.rs
rename to new_name.rs
index abc123..def456 100644
--- a/old_name.rs
+++ b/new_name.rs
@@ -1,3 +1,3 @@
 fn main() {
-    old();
+    new();
 }
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "new_name.rs");
        assert_eq!(files[0].kind, FileChangeKind::Renamed);
        assert_eq!(files[0].old_path.as_ref().map(|p| p.as_str()), Some("old_name.rs"));
    }

    #[test]
    fn parse_multiple_files() {
        let diff = r#"diff --git a/a.rs b/a.rs
index 111..222 100644
--- a/a.rs
+++ b/a.rs
@@ -1 +1 @@
-old
+new
diff --git a/b.rs b/b.rs
index 333..444 100644
--- a/b.rs
+++ b/b.rs
@@ -1 +1,2 @@
 existing
+added
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 2);
        // Sorted by path
        assert_eq!(files[0].path.as_str(), "a.rs");
        assert_eq!(files[1].path.as_str(), "b.rs");
    }

    #[test]
    fn parse_diff_header_simple() {
        let header = "diff --git a/src/main.rs b/src/main.rs";
        let (old, new) = parse_diff_header(header).unwrap();
        assert_eq!(old, "src/main.rs");
        assert_eq!(new, "src/main.rs");
    }

    #[test]
    fn parse_diff_header_rename() {
        let header = "diff --git a/old/path.rs b/new/path.rs";
        let (old, new) = parse_diff_header(header).unwrap();
        assert_eq!(old, "old/path.rs");
        assert_eq!(new, "new/path.rs");
    }
}
```

#### 5. Export pr_diff module
**File**: `src/core/mod.rs`
**Location**: after `mod gh;`

**Add**:
```rust
mod pr_diff;
```

**Location**: after `pub use gh::*;`

**Add**:
```rust
pub use pr_diff::*;
```

### Success Criteria

**Automated**:
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

### Rollback
```bash
rm src/core/pr_diff.rs
git restore -- src/core/mod.rs src/core/repo.rs
```

---

## Phase 3: PR Picker UI

### Overview
Add `Mode::PRPicker` for browsing and selecting PRs.

### Prerequisites
- [ ] Phase 2 complete
- [ ] Phase 2 automated checks pass

### Change Checklist
- [x] Add `PRPicker` mode to `Mode` enum
- [x] Add PR-related state fields to `App`
- [x] Add PR picker methods to `App`
- [x] Add input handling for PR picker mode
- [ ] Add rendering for PR picker overlay (Phase 4)

### Changes

#### 1. Extend Mode enum
**File**: `src/ui/app.rs`
**Location**: lines 35-43 (Mode enum)

**Before**:
```rust
pub enum Mode {
    #[default]
    Normal,
    AddComment,
    ViewComments,
    FilterFiles,
    SelectTheme,
    Help,
}
```

**After**:
```rust
pub enum Mode {
    #[default]
    Normal,
    AddComment,
    ViewComments,
    FilterFiles,
    SelectTheme,
    Help,
    PRPicker,
    PRAction,
}
```

#### 2. Add PR state fields to App struct
**File**: `src/ui/app.rs`
**Location**: after line 140 (after `watcher: Option<RepoWatcher>,`)

**Add**:
```rust

    // PR mode state
    /// Whether we're in PR review mode.
    pub pr_mode: bool,
    /// Current PR if in PR mode.
    pub current_pr: Option<crate::core::PullRequest>,
    /// PR file list (parsed from diff).
    pub pr_files: Vec<crate::core::PRChangedFile>,
    /// Available PRs for picker.
    pub pr_list: Vec<crate::core::PullRequest>,
    /// Loading state for PR operations.
    pub pr_loading: bool,
    /// PR picker filter.
    pub pr_filter: crate::core::PRFilter,
    /// Selected index in PR picker.
    pub pr_picker_selected: usize,
    /// Scroll offset in PR picker.
    pub pr_picker_scroll: usize,
    /// PR action draft text.
    pub pr_action_text: String,
    /// PR action type being composed.
    pub pr_action_type: Option<PRActionType>,
```

#### 3. Add PRActionType enum
**File**: `src/ui/app.rs`
**Location**: after Mode enum (around line 45)

**Add**:
```rust
/// Type of PR review action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PRActionType {
    Approve,
    Comment,
    RequestChanges,
}
```

#### 4. Update App::new to initialize PR fields
**File**: `src/ui/app.rs`
**Location**: in `App::new`, after line 270 (after `watcher: None,`)

**Add**:
```rust
            pr_mode: false,
            current_pr: None,
            pr_files: Vec::new(),
            pr_list: Vec::new(),
            pr_loading: false,
            pr_filter: crate::core::PRFilter::default(),
            pr_picker_selected: 0,
            pr_picker_scroll: 0,
            pr_action_text: String::new(),
            pr_action_type: None,
```

#### 5. Add imports to app.rs
**File**: `src/ui/app.rs`
**Location**: after existing crate imports (around line 6)

**Update** the `use crate::core` block to include new types:
```rust
use crate::core::{
    diff_source_display, digest_hunk_changed_rows, format_anchor_summary, list_changed_files,
    list_changed_files_between, list_changed_files_from_base_with_merge_base, list_commit_files,
    resolve_revision, selector_from_hunk, Anchor, ChangedFile, CommentContext, CommentStatus,
    CommentStore, DiffResult, DiffSource, FileCommentStore, FileViewedStore, FuzzyMatcher,
    PRChangedFile, PRFilter, PullRequest, RelPath, RepoRoot, RepoWatcher, Selector, TextBuffer,
    ViewedStore,
};
```

#### 6. Add PR picker methods to App
**File**: `src/ui/app.rs`
**Location**: at end of `impl App` block (before the final `}`)

**Add**:
```rust
    // ========================================================================
    // PR Picker
    // ========================================================================

    /// Open PR picker mode.
    pub fn open_pr_picker(&mut self) {
        if !crate::core::is_gh_available() {
            self.error_msg = Some("GitHub CLI not available. Run 'gh auth login'".to_string());
            self.dirty = true;
            return;
        }

        self.mode = Mode::PRPicker;
        self.pr_picker_selected = 0;
        self.pr_picker_scroll = 0;
        self.pr_loading = true;
        self.dirty = true;

        // Fetch PRs synchronously for now (could be backgrounded later)
        self.fetch_pr_list();
    }

    /// Fetch PR list from GitHub.
    fn fetch_pr_list(&mut self) {
        self.pr_loading = true;
        self.error_msg = None;

        match crate::core::list_prs(self.repo.path(), self.pr_filter) {
            Ok(prs) => {
                self.pr_list = prs;
                self.pr_loading = false;
                if self.pr_list.is_empty() {
                    self.status_msg = Some("No PRs found".to_string());
                }
            }
            Err(e) => {
                self.pr_list.clear();
                self.pr_loading = false;
                self.error_msg = Some(format!("Failed to fetch PRs: {}", e));
            }
        }

        self.dirty = true;
    }

    /// Close PR picker and return to normal mode.
    pub fn close_pr_picker(&mut self) {
        self.mode = Mode::Normal;
        self.pr_list.clear();
        self.pr_picker_selected = 0;
        self.pr_picker_scroll = 0;
        self.dirty = true;
    }

    /// Select next PR in picker.
    pub fn pr_picker_next(&mut self) {
        if !self.pr_list.is_empty() {
            self.pr_picker_selected = (self.pr_picker_selected + 1).min(self.pr_list.len() - 1);
            self.dirty = true;
        }
    }

    /// Select previous PR in picker.
    pub fn pr_picker_prev(&mut self) {
        self.pr_picker_selected = self.pr_picker_selected.saturating_sub(1);
        self.dirty = true;
    }

    /// Cycle to next PR filter.
    pub fn pr_picker_next_filter(&mut self) {
        self.pr_filter = match self.pr_filter {
            PRFilter::All => PRFilter::Mine,
            PRFilter::Mine => PRFilter::ReviewRequested,
            PRFilter::ReviewRequested => PRFilter::All,
        };
        self.pr_picker_selected = 0;
        self.fetch_pr_list();
    }

    /// Cycle to previous PR filter.
    pub fn pr_picker_prev_filter(&mut self) {
        self.pr_filter = match self.pr_filter {
            PRFilter::All => PRFilter::ReviewRequested,
            PRFilter::Mine => PRFilter::All,
            PRFilter::ReviewRequested => PRFilter::Mine,
        };
        self.pr_picker_selected = 0;
        self.fetch_pr_list();
    }

    /// Select the highlighted PR and load its diff.
    pub fn pr_picker_select(&mut self) {
        if self.pr_list.is_empty() {
            return;
        }

        let pr = self.pr_list[self.pr_picker_selected].clone();
        self.load_pr(pr);
    }

    /// Load a specific PR's diff.
    pub fn load_pr(&mut self, pr: PullRequest) {
        self.pr_loading = true;
        self.error_msg = None;
        self.mode = Mode::Normal;
        self.dirty = true;

        // Fetch PR diff
        match crate::core::get_pr_diff(self.repo.path(), pr.number) {
            Ok(raw_diff) => {
                // Parse unified diff into file list
                let pr_files = crate::core::parse_unified_diff(&raw_diff);

                // Convert to ChangedFile for compatibility with existing UI
                self.files = pr_files
                    .iter()
                    .map(|pf| ChangedFile {
                        path: pf.path.clone(),
                        kind: pf.kind,
                        old_path: pf.old_path.clone(),
                    })
                    .collect();

                self.pr_files = pr_files;
                self.pr_mode = true;
                self.current_pr = Some(pr.clone());

                // Update diff source
                self.source = DiffSource::PullRequest {
                    number: pr.number,
                    head: pr.head_ref_name.clone(),
                    base: pr.base_ref_name.clone(),
                };

                // Reset selection
                self.selected_idx = 0;
                self.sidebar_scroll = 0;
                self.pr_loading = false;

                // Load first file's diff
                if !self.files.is_empty() {
                    self.request_current_pr_diff();
                } else {
                    self.diff = None;
                    self.old_buffer = None;
                    self.new_buffer = None;
                    self.status_msg = Some("PR has no changed files".to_string());
                }

                self.dirty = true;
            }
            Err(e) => {
                self.pr_loading = false;
                self.error_msg = Some(format!("Failed to load PR diff: {}", e));
                self.dirty = true;
            }
        }
    }

    /// Request diff for currently selected file in PR mode.
    fn request_current_pr_diff(&mut self) {
        if !self.pr_mode || self.pr_files.is_empty() {
            return;
        }

        let Some(pr_file) = self.pr_files.get(self.selected_idx) else {
            return;
        };

        // Detect language for highlighting
        self.current_lang = pr_file
            .path
            .extension()
            .map(crate::highlight::LanguageId::from_extension)
            .unwrap_or(crate::highlight::LanguageId::Plain);

        // For PR mode, we use the patch directly
        // Create synthetic old/new buffers from the patch
        let (old_content, new_content) = extract_content_from_patch(&pr_file.patch);

        let old_buffer = TextBuffer::new(old_content.as_bytes());
        let new_buffer = TextBuffer::new(new_content.as_bytes());

        let is_binary = old_buffer.is_binary() || new_buffer.is_binary();
        let diff = if is_binary {
            None
        } else {
            Some(DiffResult::compute(&old_buffer, &new_buffer))
        };

        self.is_binary = is_binary;
        self.old_buffer = Some(old_buffer);
        self.new_buffer = Some(new_buffer);
        self.diff = diff;

        // Jump to first hunk
        if let Some(diff) = self.diff.as_ref() {
            if let Some(first) = diff.hunks().first() {
                self.scroll_y = first.start_row;
            }
        }

        self.scroll_x = 0;
        self.error_msg = None;
        self.loading = false;
        self.dirty = true;
    }

    /// Exit PR mode and return to working tree.
    pub fn exit_pr_mode(&mut self) {
        if !self.pr_mode {
            return;
        }

        self.pr_mode = false;
        self.current_pr = None;
        self.pr_files.clear();
        self.source = DiffSource::WorkingTree;

        // Reload working tree files
        match list_changed_files(&self.repo) {
            Ok(files) => {
                self.files = files;
                self.selected_idx = 0;
                if !self.files.is_empty() {
                    self.request_current_diff();
                }
            }
            Err(e) => {
                self.error_msg = Some(format!("Failed to reload: {}", e));
            }
        }

        self.dirty = true;
    }

    /// Refresh current PR.
    pub fn refresh_pr(&mut self) {
        if let Some(pr) = self.current_pr.clone() {
            self.load_pr(pr);
        }
    }
```

#### 7. Add helper function for patch content extraction
**File**: `src/ui/app.rs`
**Location**: after `impl App` block (at module level)

**Add**:
```rust
/// Extract old and new content from a unified diff patch.
///
/// This is a simplified extraction that reconstructs file content from
/// the patch hunks. Not perfect but good enough for diff display.
fn extract_content_from_patch(patch: &str) -> (String, String) {
    let mut old_lines: Vec<&str> = Vec::new();
    let mut new_lines: Vec<&str> = Vec::new();
    let mut in_hunk = false;

    for line in patch.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }

        if !in_hunk {
            continue;
        }

        if line.starts_with('-') && !line.starts_with("---") {
            // Deleted line - only in old
            old_lines.push(&line[1..]);
        } else if line.starts_with('+') && !line.starts_with("+++") {
            // Added line - only in new
            new_lines.push(&line[1..]);
        } else if line.starts_with(' ') || line.is_empty() {
            // Context line - in both
            let content = if line.starts_with(' ') {
                &line[1..]
            } else {
                line
            };
            old_lines.push(content);
            new_lines.push(content);
        }
    }

    (old_lines.join("\n"), new_lines.join("\n"))
}
```

#### 8. Add PR picker input handling
**File**: `src/ui/input.rs`
**Location**: in `handle_key` function, after the Mode::Help case (around line 28)

**Before**:
```rust
        Mode::Help => return handle_help_key(app, key),
        Mode::Normal => {}
```

**After**:
```rust
        Mode::Help => return handle_help_key(app, key),
        Mode::PRPicker => return handle_pr_picker_key(app, key),
        Mode::PRAction => return handle_pr_action_key(app, key),
        Mode::Normal => {}
```

**Location**: at end of file (before final `}`)

**Add**:
```rust
/// Handle keys in PR picker mode.
fn handle_pr_picker_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.close_pr_picker();
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.pr_picker_next();
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.pr_picker_prev();
            true
        }
        KeyCode::Enter => {
            app.pr_picker_select();
            true
        }
        KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
            app.pr_picker_next_filter();
            true
        }
        KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
            app.pr_picker_prev_filter();
            true
        }
        KeyCode::Char('r') => {
            app.fetch_pr_list();
            true
        }
        _ => false,
    }
}

/// Handle keys in PR action mode.
fn handle_pr_action_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.cancel_pr_action();
            true
        }
        KeyCode::Enter => {
            app.submit_pr_action();
            true
        }
        KeyCode::Char(c) => {
            app.pr_action_text.push(c);
            app.dirty = true;
            true
        }
        KeyCode::Backspace => {
            app.pr_action_text.pop();
            app.dirty = true;
            true
        }
        _ => false,
    }
}
```

#### 9. Add import to input.rs
**File**: `src/ui/input.rs`
**Location**: line 5 (update existing import)

**Before**:
```rust
use super::app::{App, Focus, Mode};
```

**After**:
```rust
use super::app::{App, Focus, Mode, PRActionType};
```

#### 10. Add global key for PR picker
**File**: `src/ui/input.rs`
**Location**: in `handle_key` function, in the global keys section (around line 55)

**Add** after the `KeyCode::Char('r')` case:
```rust
        KeyCode::Char('P') => {
            if !app.pr_mode {
                app.open_pr_picker();
            } else {
                app.exit_pr_mode();
            }
            return true;
        }
```

### Success Criteria

**Automated**:
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

**Manual**:
- Press `P` in normal mode → PR picker should open (or show auth error)
- In PR picker: `j/k` navigate, `Tab` cycle filters, `Enter` select

### Rollback
```bash
git restore -- src/ui/app.rs src/ui/input.rs
```

---

## Phase 4: PR Diff Viewing & Rendering

### Overview
Add rendering for PR picker overlay and integrate PR diff viewing with existing diff panes.

### Prerequisites
- [ ] Phase 3 complete
- [ ] Phase 3 automated checks pass

### Change Checklist
- [x] Add PR picker overlay rendering
- [x] Update top bar to show PR info
- [x] Update bottom bar for PR mode hints
- [x] Handle PR file selection in sidebar (via existing sidebar + request_current_pr_diff)

### Changes

#### 1. Add PR picker overlay rendering
**File**: `src/ui/render.rs`
**Location**: in `render` function, after the Help overlay check (around line 85)

**Add**:
```rust
    if app.mode == Mode::PRPicker {
        render_pr_picker_overlay(frame, app);
    }

    if app.mode == Mode::PRAction {
        render_pr_action_overlay(frame, app);
    }
```

#### 2. Add PR picker overlay function
**File**: `src/ui/render.rs`
**Location**: at end of file (before any closing braces)

**Add**:
```rust
// ============================================================================
// PR Picker Overlay
// ============================================================================

fn render_pr_picker_overlay(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Center the picker
    let width = (area.width * 3 / 4).min(80);
    let height = (area.height * 3 / 4).min(30);
    let x = (area.width - width) / 2;
    let y = (area.height - height) / 2;
    let picker_area = Rect::new(x, y, width, height);

    // Background
    let block = Block::default()
        .title(" Pull Requests ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.accent))
        .style(Style::default().bg(app.theme.bg_dark));
    frame.render_widget(block, picker_area);

    let inner = Rect::new(
        picker_area.x + 1,
        picker_area.y + 1,
        picker_area.width.saturating_sub(2),
        picker_area.height.saturating_sub(2),
    );

    // Filter tabs
    let filter_line = Line::from(vec![
        Span::raw(" "),
        styled_filter_tab("All", app.pr_filter == crate::core::PRFilter::All, &app.theme),
        Span::raw("  "),
        styled_filter_tab("Mine", app.pr_filter == crate::core::PRFilter::Mine, &app.theme),
        Span::raw("  "),
        styled_filter_tab("Review Requested", app.pr_filter == crate::core::PRFilter::ReviewRequested, &app.theme),
        Span::raw(" "),
    ]);
    let filter_para = Paragraph::new(filter_line);
    frame.render_widget(filter_para, Rect::new(inner.x, inner.y, inner.width, 1));

    // Loading or list
    let list_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(4));

    if app.pr_loading {
        let loading = Paragraph::new("Loading...")
            .style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(loading, list_area);
    } else if app.pr_list.is_empty() {
        let empty = Paragraph::new("No PRs found")
            .style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(empty, list_area);
    } else {
        // Render PR list
        let visible_height = list_area.height as usize;
        let start = app.pr_picker_scroll;
        let end = (start + visible_height).min(app.pr_list.len());

        for (i, pr) in app.pr_list[start..end].iter().enumerate() {
            let y = list_area.y + i as u16;
            let is_selected = start + i == app.pr_picker_selected;

            let style = if is_selected {
                Style::default()
                    .bg(app.theme.accent)
                    .fg(app.theme.bg_dark)
            } else {
                Style::default().fg(app.theme.text_normal)
            };

            // Format: #123 Title (head → base) +10/-5
            let pr_line = format!(
                " #{:<4} {} ({} → {}) +{}/-{}",
                pr.number,
                truncate_str(&pr.title, 30),
                truncate_str(&pr.head_ref_name, 15),
                truncate_str(&pr.base_ref_name, 15),
                pr.additions,
                pr.deletions,
            );

            let line = Paragraph::new(pr_line).style(style);
            frame.render_widget(line, Rect::new(list_area.x, y, list_area.width, 1));
        }
    }

    // Help line at bottom
    let help_line = Line::from(vec![
        Span::styled("j/k", Style::default().fg(app.theme.accent)),
        Span::raw(" navigate  "),
        Span::styled("Tab", Style::default().fg(app.theme.accent)),
        Span::raw(" filter  "),
        Span::styled("Enter", Style::default().fg(app.theme.accent)),
        Span::raw(" select  "),
        Span::styled("r", Style::default().fg(app.theme.accent)),
        Span::raw(" refresh  "),
        Span::styled("Esc", Style::default().fg(app.theme.accent)),
        Span::raw(" close"),
    ]);
    let help_para = Paragraph::new(help_line)
        .style(Style::default().fg(app.theme.text_muted));
    frame.render_widget(
        help_para,
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
}

fn styled_filter_tab(label: &str, active: bool, theme: &Theme) -> Span<'_> {
    if active {
        Span::styled(
            format!("[{}]", label),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!(" {} ", label),
            Style::default().fg(theme.text_muted),
        )
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}

// ============================================================================
// PR Action Overlay
// ============================================================================

fn render_pr_action_overlay(frame: &mut Frame, app: &App) {
    use super::app::PRActionType;

    let area = frame.area();
    let width = 60.min(area.width - 4);
    let height = 10;
    let x = (area.width - width) / 2;
    let y = (area.height - height) / 2;
    let action_area = Rect::new(x, y, width, height);

    let title = match app.pr_action_type {
        Some(PRActionType::Approve) => " Approve PR ",
        Some(PRActionType::Comment) => " Comment on PR ",
        Some(PRActionType::RequestChanges) => " Request Changes ",
        None => " PR Action ",
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.accent))
        .style(Style::default().bg(app.theme.bg_dark));
    frame.render_widget(block, action_area);

    let inner = Rect::new(
        action_area.x + 2,
        action_area.y + 2,
        action_area.width.saturating_sub(4),
        action_area.height.saturating_sub(4),
    );

    // Show text input for comment/request-changes
    let show_input = matches!(
        app.pr_action_type,
        Some(PRActionType::Comment) | Some(PRActionType::RequestChanges)
    );

    if show_input {
        let label = Paragraph::new("Message (required):")
            .style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(label, Rect::new(inner.x, inner.y, inner.width, 1));

        let input_text = format!("{}_", &app.pr_action_text);
        let input = Paragraph::new(input_text)
            .style(Style::default().fg(app.theme.text_normal));
        frame.render_widget(input, Rect::new(inner.x, inner.y + 1, inner.width, 3));
    } else {
        let confirm = Paragraph::new("Press Enter to approve, Esc to cancel")
            .style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(confirm, Rect::new(inner.x, inner.y + 1, inner.width, 1));
    }

    // Help line
    let help = Line::from(vec![
        Span::styled("Enter", Style::default().fg(app.theme.accent)),
        Span::raw(" submit  "),
        Span::styled("Esc", Style::default().fg(app.theme.accent)),
        Span::raw(" cancel"),
    ]);
    let help_para = Paragraph::new(help)
        .style(Style::default().fg(app.theme.text_muted));
    frame.render_widget(
        help_para,
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
}
```

#### 3. Update top bar for PR mode
**File**: `src/ui/render.rs`
**Location**: in `render_top_bar` function (around line 95)

Find where the file name is displayed and update to show PR context.

**Before** (around line 97):
```rust
    let file = app.selected_file();
    let file_name = file.map(|f| f.path.as_str()).unwrap_or("No files");
```

**After**:
```rust
    let file = app.selected_file();
    let file_name = file.map(|f| f.path.as_str()).unwrap_or("No files");

    // PR badge if in PR mode
    let pr_badge = if let Some(ref pr) = app.current_pr {
        format!(" PR #{} ", pr.number)
    } else {
        String::new()
    };
```

**Location**: Find where the title spans are constructed (around line 120)

**Add** the PR badge to the title line. Look for where `file_name` is used and prepend the badge:
```rust
    // Add PR badge before file name if in PR mode
    if !pr_badge.is_empty() {
        spans.push(Span::styled(
            pr_badge,
            Style::default()
                .fg(app.theme.bg_dark)
                .bg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
    }
```

#### 4. Update bottom bar hints for PR mode
**File**: `src/ui/render.rs`
**Location**: in `render_bottom_bar` function, where hints are constructed

Find the hints section and add PR-specific hints:

**Add** after existing hints construction:
```rust
    // PR mode hints
    if app.pr_mode {
        hints.push(Span::styled("A", Style::default().fg(app.theme.accent)));
        hints.push(Span::raw("pprove "));
        hints.push(Span::styled("C", Style::default().fg(app.theme.accent)));
        hints.push(Span::raw("omment "));
        hints.push(Span::styled("R", Style::default().fg(app.theme.accent)));
        hints.push(Span::raw("eq-changes "));
        hints.push(Span::styled("P", Style::default().fg(app.theme.accent)));
        hints.push(Span::raw(" exit PR "));
    } else {
        hints.push(Span::styled("P", Style::default().fg(app.theme.accent)));
        hints.push(Span::raw("Rs "));
    }
```

#### 5. Add import for PRActionType
**File**: `src/ui/render.rs`
**Location**: at top, update import from app

**Before**:
```rust
use super::app::{App, DiffPaneMode, Focus, Mode};
```

**After**:
```rust
use super::app::{App, DiffPaneMode, Focus, Mode, PRActionType};
```

### Success Criteria

**Automated**:
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

**Manual**:
- Press `P` → PR picker overlay appears with filter tabs
- Navigate with `j/k`, cycle filters with `Tab`
- Select PR with `Enter` → diff loads, top bar shows "PR #N"
- Bottom bar shows PR action hints

### Rollback
```bash
git restore -- src/ui/render.rs
```

---

## Phase 5: PR Review Actions & Polish

### Overview
Complete PR action submission, add focus-based auto-refresh, and CLI flags.

### Prerequisites
- [ ] Phase 4 complete
- [ ] Phase 4 automated checks pass

### Change Checklist
- [x] Add PR action methods to App (start/cancel/submit) (done in Phase 3)
- [x] Add PR action keybinds in diff view (A/R for approve/request-changes)
- [ ] Add focus-based auto-refresh for PR mode (deferred - requires terminal focus events)
- [x] Add `--pr` CLI flags
- [x] Update help text

### Changes

#### 1. Add PR action methods to App
**File**: `src/ui/app.rs`
**Location**: at end of PR methods section (after `refresh_pr`)

**Add**:
```rust
    // ========================================================================
    // PR Actions
    // ========================================================================

    /// Start approve action.
    pub fn start_pr_approve(&mut self) {
        if !self.pr_mode || self.current_pr.is_none() {
            self.error_msg = Some("Not in PR mode".to_string());
            self.dirty = true;
            return;
        }

        self.pr_action_type = Some(PRActionType::Approve);
        self.pr_action_text.clear();
        self.mode = Mode::PRAction;
        self.dirty = true;
    }

    /// Start comment action.
    pub fn start_pr_comment(&mut self) {
        if !self.pr_mode || self.current_pr.is_none() {
            self.error_msg = Some("Not in PR mode".to_string());
            self.dirty = true;
            return;
        }

        self.pr_action_type = Some(PRActionType::Comment);
        self.pr_action_text.clear();
        self.mode = Mode::PRAction;
        self.dirty = true;
    }

    /// Start request-changes action.
    pub fn start_pr_request_changes(&mut self) {
        if !self.pr_mode || self.current_pr.is_none() {
            self.error_msg = Some("Not in PR mode".to_string());
            self.dirty = true;
            return;
        }

        self.pr_action_type = Some(PRActionType::RequestChanges);
        self.pr_action_text.clear();
        self.mode = Mode::PRAction;
        self.dirty = true;
    }

    /// Cancel current PR action.
    pub fn cancel_pr_action(&mut self) {
        self.mode = Mode::Normal;
        self.pr_action_type = None;
        self.pr_action_text.clear();
        self.dirty = true;
    }

    /// Submit the current PR action.
    pub fn submit_pr_action(&mut self) {
        let Some(pr) = &self.current_pr else {
            self.error_msg = Some("No PR selected".to_string());
            self.cancel_pr_action();
            return;
        };

        let pr_number = pr.number;
        let repo_path = self.repo.path().to_path_buf();

        let result = match self.pr_action_type {
            Some(PRActionType::Approve) => {
                let body = if self.pr_action_text.trim().is_empty() {
                    None
                } else {
                    Some(self.pr_action_text.as_str())
                };
                crate::core::approve_pr(&repo_path, pr_number, body)
            }
            Some(PRActionType::Comment) => {
                if self.pr_action_text.trim().is_empty() {
                    self.error_msg = Some("Comment cannot be empty".to_string());
                    self.dirty = true;
                    return;
                }
                crate::core::comment_pr(&repo_path, pr_number, &self.pr_action_text)
            }
            Some(PRActionType::RequestChanges) => {
                if self.pr_action_text.trim().is_empty() {
                    self.error_msg = Some("Message cannot be empty".to_string());
                    self.dirty = true;
                    return;
                }
                crate::core::request_changes_pr(&repo_path, pr_number, &self.pr_action_text)
            }
            None => {
                self.cancel_pr_action();
                return;
            }
        };

        match result {
            Ok(()) => {
                let action_name = match self.pr_action_type {
                    Some(PRActionType::Approve) => "approved",
                    Some(PRActionType::Comment) => "commented on",
                    Some(PRActionType::RequestChanges) => "requested changes on",
                    None => "reviewed",
                };
                self.status_msg = Some(format!("PR #{} {}", pr_number, action_name));
                self.error_msg = None;
            }
            Err(e) => {
                self.error_msg = Some(format!("Failed: {}", e));
            }
        }

        self.cancel_pr_action();
    }
```

#### 2. Add PR action keybinds in diff view
**File**: `src/ui/input.rs`
**Location**: in `handle_diff_key` function, add PR action keybinds

**Add** at the beginning of the match (after the opening brace):
```rust
        // PR actions (only in PR mode)
        KeyCode::Char('A') if app.pr_mode => {
            app.start_pr_approve();
            return true;
        }
        KeyCode::Char('C') if app.pr_mode => {
            app.start_pr_comment();
            return true;
        }
        KeyCode::Char('R') if app.pr_mode => {
            app.start_pr_request_changes();
            return true;
        }
```

#### 3. Add focus event handling for auto-refresh
**File**: `src/ui/input.rs`
**Location**: in `handle_input` function (at the top)

**Before**:
```rust
pub fn handle_input(app: &mut App, event: Event) -> bool {
    match event {
        Event::Key(key) => handle_key(app, key),
        Event::Mouse(mouse) => handle_mouse(app, mouse),
        _ => false,
    }
}
```

**After**:
```rust
pub fn handle_input(app: &mut App, event: Event) -> bool {
    match event {
        Event::Key(key) => handle_key(app, key),
        Event::Mouse(mouse) => handle_mouse(app, mouse),
        Event::FocusGained => {
            // Auto-refresh on focus in PR mode
            if app.pr_mode {
                app.refresh_pr();
            }
            true
        }
        _ => false,
    }
}
```

#### 4. Add CLI flags
**File**: `src/main.rs`
**Location**: in `Cli` struct (around line 25)

**Add** after the `theme` field:
```rust
    /// Browse and review GitHub pull requests
    #[arg(long = "pr", value_name = "NUMBER")]
    pr: Option<Option<u32>>,
```

#### 5. Update main to handle --pr flag
**File**: `src/main.rs`
**Location**: in `main` function, after CLI parsing (around line 75)

**Before**:
```rust
    // Determine diff source
    let source = parse_diff_source(&cli);

    // Run TUI
    match run_tui(source, cli.file, cli.theme) {
```

**After**:
```rust
    // Determine diff source
    let source = parse_diff_source(&cli);

    // Handle --pr flag
    let pr_number = match cli.pr {
        Some(Some(n)) => Some(n),  // --pr 123
        Some(None) => Some(0),     // --pr (picker mode, 0 = open picker)
        None => None,              // no flag
    };

    // Run TUI
    match run_tui(source, cli.file, cli.theme, pr_number) {
```

#### 6. Update run_tui signature and PR initialization
**File**: `src/main.rs`
**Location**: `run_tui` function signature and body

**Before**:
```rust
fn run_tui(source: DiffSource, file_filter: Option<String>, theme: Option<String>) -> Result<()> {
```

**After**:
```rust
fn run_tui(
    source: DiffSource,
    file_filter: Option<String>,
    theme: Option<String>,
    pr_number: Option<u32>,
) -> Result<()> {
```

**Location**: After `let mut app = App::new(...)?;` (around line 115)

**Add**:
```rust
    // Handle PR mode initialization
    if let Some(n) = pr_number {
        if n == 0 {
            // Open PR picker
            app.open_pr_picker();
        } else {
            // Load specific PR
            if !quickdiff::core::is_gh_available() {
                eprintln!("Error: GitHub CLI not available. Run 'gh auth login'");
                std::process::exit(1);
            }
            match quickdiff::core::list_prs(app.repo.path(), quickdiff::core::PRFilter::All) {
                Ok(prs) => {
                    if let Some(pr) = prs.into_iter().find(|p| p.number == n) {
                        app.load_pr(pr);
                    } else {
                        eprintln!("Error: PR #{} not found", n);
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
```

#### 7. Update manual_reload to handle PR mode
**File**: `src/ui/app.rs`
**Location**: in `manual_reload` method

**Before** (around line 520):
```rust
    pub fn manual_reload(&mut self) {
        match self.source {
            DiffSource::WorkingTree | DiffSource::Base(_) => {
                self.refresh_file_list();
            }
```

**After**:
```rust
    pub fn manual_reload(&mut self) {
        // PR mode: refresh current PR
        if self.pr_mode {
            self.refresh_pr();
            return;
        }

        match self.source {
            DiffSource::WorkingTree | DiffSource::Base(_) => {
                self.refresh_file_list();
            }
```

#### 8. Update help text
**File**: `src/ui/render.rs`
**Location**: in `render_help_overlay` function, add PR keys to the help content

Find the help overlay rendering and add to the keybindings list:
```rust
    ("P", "PR picker / exit PR mode"),
    ("A", "Approve PR (in PR mode)"),
    ("C", "Comment on PR (in PR mode)"),
    ("R", "Request changes (in PR mode)"),
```

### Success Criteria

**Automated**:
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

**Manual**:
```bash
# Open PR picker
quickdiff --pr

# Open specific PR directly
quickdiff --pr 123

# In PR mode:
# - Press A → approve dialog
# - Press C → comment input
# - Press R → request changes input
# - Press Enter to submit
# - Lose/regain focus → auto-refreshes
```

### Rollback
```bash
git restore -- src/main.rs src/ui/app.rs src/ui/input.rs src/ui/render.rs
```

---

## Testing Strategy

### Unit Tests to Add

**File**: `src/core/gh.rs` - Already included in Phase 1.

**File**: `src/core/pr_diff.rs` - Already included in Phase 2.

### Integration Tests

Manual only for now - `gh` CLI mocking is complex.

### Manual Testing Checklist

1. [ ] `quickdiff --pr` opens PR picker
2. [ ] PR picker shows PRs with correct filter
3. [ ] Tab cycles through filters (All → Mine → Review Requested)
4. [ ] Enter on PR loads diff
5. [ ] Files sidebar shows PR's changed files
6. [ ] Diff pane shows file diff
7. [ ] `A` opens approve dialog, Enter submits
8. [ ] `C` opens comment dialog, requires text, Enter submits
9. [ ] `R` opens request-changes dialog, requires text, Enter submits
10. [ ] `P` exits PR mode and returns to working tree
11. [ ] `r` refreshes PR diff
12. [ ] Focus regained triggers auto-refresh
13. [ ] `--pr 123` opens directly to PR #123
14. [ ] Auth error shown if `gh` not authenticated
15. [ ] Draft PRs excluded from list

## Deployment Instructions

```bash
cargo build --release
install -m 755 target/release/quickdiff ~/commands/quickdiff
```

## Anti-Patterns to Avoid

- **Don't fetch PRs on every keystroke** - only on explicit refresh or filter change
- **Don't block UI on network** - PR fetching currently blocks; acceptable for v1 but could background later
- **Don't store GitHub tokens** - piggyback on `gh auth` exclusively

## Open Questions

All resolved:
- [x] Refresh strategy → `r` manual + auto on focus
- [x] Draft PRs → excluded
- [x] CI status → out of scope v1
- [x] Line comments → out of scope v1

## References

- guck/cerebro PR implementation: `/Users/yesh/Documents/personal/reference/guck/src/github/index.ts`
- guck PR diff parser: `/Users/yesh/Documents/personal/reference/guck/src/server/handlers/pr-diff.ts`
- Existing DiffSource: `src/core/repo.rs:47-68`
- Existing Mode enum: `src/ui/app.rs:35-43`
- Worker pattern: `src/ui/worker.rs`
