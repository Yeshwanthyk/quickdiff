# Web Preview + Stdin Pager Implementation Plan

## Plan Metadata
- Created: 2026-01-17
- Ticket: none
- Status: completed
- Owner: yesh
- Assumptions:
  - Bun is available for HTML render step (per request for Bun support).
  - Web preview is local HTML (no upload service).
  - Patch-first output is acceptable even if `jj` diffs are best-effort.

## Progress Tracking
- [x] Phase 1: Stdin Patch Mode in TUI
- [x] Phase 2: Patch Generation + Web Data Assembly
- [x] Phase 3: Web Render + CLI Web Entry

## Overview
Add a patch-first web preview flow using `@pierre/diffs` plus a `--stdin` pager mode that renders unified diffs in the existing TUI. Web output is standalone HTML with client-side `@pierre/diffs` rendering.

## Current State
- CLI supports comments subcommand only; no stdin or web mode in `src/main.rs:24`.
- PR patch diffs are already rendered from unified patches via `request_current_pr_diff` in `src/ui/app/pr.rs:458`.
- Patch parsing and per-file stats exist in `parse_unified_diff` in `src/core/pr_diff.rs:22`.
- Patch-to-content reconstruction is implemented in `extract_content_from_patch` in `src/ui/app/mod.rs:382`.
- PR-only actions are gated purely on `app.pr.active` in `src/ui/input.rs:122`.

### Key Discoveries
- Patch parsing is already safe against false `diff --git` splits: `src/core/pr_diff.rs:22`.
- The UI already reconstructs buffers from patches for PRs: `src/ui/app/pr.rs:458`.
- Diff source display is centralized in `src/core/repo.rs:1490`.

## Desired End State
- `quickdiff --stdin` reads a unified diff from stdin and renders it in the TUI (pager mode).
- `quickdiff web [diffspec]` generates a standalone HTML file using `@pierre/diffs` and opens it when `--open` is set.
- `quickdiff web --stdin` uses stdin patch for HTML generation.
- Web rendering uses patch-first unified diff and does not depend on gitgud skills.

### Verification
- `printf 'diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-1\n+2\n' | quickdiff --stdin`
- `quickdiff web HEAD~1 --open`
- `git diff | quickdiff web --stdin --open`

### Manual observable behavior
- TUI opens and shows file list + diff based on stdin patch.
- HTML opens with file list and rendered diffs via `@pierre/diffs`.

## Out of Scope
- AI review pipeline (no YAML/markdown generation).
- Remote upload service (no critique.work equivalent).
- Full parity with PR comment UI from review-export template.

## Breaking Changes
- None. New CLI flags/commands only.

## Dependency and Configuration Changes

### Additions
```bash
# No new Rust deps
# Bun required for web render step
```
**Why needed**: Bun executes the HTML render script and keeps the web pipeline simple.

### Updates
```bash
# none
```
**Breaking changes**: none

### Removals
```bash
# none
```
**Replacement**: n/a

### Configuration Changes
**File**: `README.md`

**Before**:
```md
# (no stdin/web pager info)
```

**After**:
```md
# Add lazygit pager example and web usage
```
**Impact**: Documentation only.

## Error Handling Strategy
- `--stdin` with empty input: show error and exit with code 1.
- `web` without bun available: emit `Error: bun not found; install bun to use web preview`.
- Git/jj command failures: surface stderr via `anyhow::Context`, exit with code 1.
- Patch parsing yields no files: render empty state and set status message; do not crash.

## Implementation Approach
Patch-first flow using unified diff strings is the minimal adapter to `@pierre/diffs` and matches how PR diffs are already rendered. Reuse existing patch parsing and patch-to-content reconstruction for TUI. For web, generate `review.json` (branch, commit, patch, files, stats) and render HTML from a local template that imports `@pierre/diffs` from CDN.

**Alternatives considered**:
- Content-first (old/new buffers) → rejected due to bigger payload and diff semantics diverging from quickdiff.
- SSR (`@pierre/diffs/ssr`) → deferred; client-side render is simpler and matches review-export.

## Phase Dependencies and Parallelization
- Dependencies: Phase 2 depends on Phase 1 (patch parsing + patch state reuse).
- Parallelizable: Phase 2 and Phase 3 can overlap after Phase 1 structure lands.
- Suggested @agents:
  - review-explain: verify CLI wiring and UI gating points
  - review-explain: sanity check template JS usage of `@pierre/diffs`

---

## Phase 1: Stdin Patch Mode in TUI

### Overview
Add a patch-mode state in the UI and wire `--stdin` in CLI to load and render unified diffs using existing patch-based diff rendering logic.

### Prerequisites
- [ ] Confirm `cargo test` passes on main branch
- [ ] Open Questions resolved (none)

### Change Checklist
- [x] Add `stdin` flag to `Cli`
- [x] Wire `cli.stdin` to patch-mode TUI startup
- [x] Add patch state to `App` and new patch loader
- [x] Implement `request_current_patch_diff`
- [x] Gate PR actions when patch mode is active

### Changes

#### 1. CLI: add `--stdin`
**File**: `src/main.rs`
**Location**: lines 24-52

**Before**:
```rust
struct Cli {
    /// Show changes from a specific commit
    #[arg(short = 'c', long = "commit")]
    commit: Option<String>,

    /// Compare against a base branch (e.g., origin/main)
    #[arg(short = 'b', long = "base")]
    base: Option<String>,

    /// Revision or range (e.g., HEAD~3, abc123..def456, origin/main, @-..@)
    #[arg(value_name = "REV")]
    revision: Option<String>,

    /// Filter to specific file(s)
    #[arg(short = 'f', long = "file", value_name = "PATH")]
    file: Option<String>,

    /// Color theme (default, dracula, catppuccin, nord, gruvbox, tokyonight, rosepine, onedark, solarized)
    #[arg(short = 't', long = "theme", value_name = "THEME")]
    theme: Option<String>,

    /// Browse and review GitHub pull requests (optionally specify PR number)
    #[arg(long = "pr", value_name = "NUMBER")]
    pr: Option<Option<u32>>,

    /// Comments subcommand
    #[arg(trailing_var_arg = true, hide = true)]
    rest: Vec<String>,
}
```

**After**:
```rust
struct Cli {
    /// Show changes from a specific commit
    #[arg(short = 'c', long = "commit")]
    commit: Option<String>,

    /// Compare against a base branch (e.g., origin/main)
    #[arg(short = 'b', long = "base")]
    base: Option<String>,

    /// Revision or range (e.g., HEAD~3, abc123..def456, origin/main, @-..@)
    #[arg(value_name = "REV")]
    revision: Option<String>,

    /// Filter to specific file(s)
    #[arg(short = 'f', long = "file", value_name = "PATH")]
    file: Option<String>,

    /// Color theme (default, dracula, catppuccin, nord, gruvbox, tokyonight, rosepine, onedark, solarized)
    #[arg(short = 't', long = "theme", value_name = "THEME")]
    theme: Option<String>,

    /// Browse and review GitHub pull requests (optionally specify PR number)
    #[arg(long = "pr", value_name = "NUMBER")]
    pr: Option<Option<u32>>,

    /// Read unified diff from stdin and render in TUI (pager mode)
    #[arg(long = "stdin")]
    stdin: bool,

    /// Comments subcommand
    #[arg(trailing_var_arg = true, hide = true)]
    rest: Vec<String>,
}
```

**Why**: Enables `quickdiff --stdin` for pager use.

#### 2. CLI: route stdin to patch-mode TUI
**File**: `src/main.rs`
**Location**: lines 84-108

**Before**:
```rust
// Parse CLI args
let cli = Cli::parse();

// Initialize metrics if enabled
quickdiff::metrics::init();

// Determine diff source
let source = parse_diff_source(&cli);

// Handle --pr flag
let pr_number = match cli.pr {
    Some(Some(n)) => Some(n),
    Some(None) => Some(0),
    None => None,
};

// Run TUI
match run_tui(source, cli.file, cli.theme, pr_number) {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
        eprintln!("Error: {}", e);
        ExitCode::from(1)
    }
}
```

**After**:
```rust
// Parse CLI args
let cli = Cli::parse();

// Initialize metrics if enabled
quickdiff::metrics::init();

if cli.stdin {
    return run_tui_patch(cli.theme);
}

// Determine diff source
let source = parse_diff_source(&cli);

// Handle --pr flag
let pr_number = match cli.pr {
    Some(Some(n)) => Some(n),
    Some(None) => Some(0),
    None => None,
};

// Run TUI
match run_tui(source, cli.file, cli.theme, pr_number) {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
        eprintln!("Error: {}", e);
        ExitCode::from(1)
    }
}
```

**Why**: Ensures stdin mode bypasses diff-source parsing.

#### 3. UI state: add patch mode
**File**: `src/ui/app/state.rs`
**Location**: lines 135-158 (new struct appended after `PrState`)

**Add**:
```rust
/// Patch mode state (stdin or external patch input).
#[derive(Debug, Default)]
pub struct PatchState {
    /// Whether patch mode is active.
    pub active: bool,
    /// Patch-derived files.
    pub files: Vec<PRChangedFile>,
    /// Display label (e.g., "stdin").
    pub label: String,
}
```

**Why**: Isolates patch mode without overloading PR state.

#### 4. App fields + patch loader
**File**: `src/ui/app/mod.rs`
**Location**: lines 33-116, 151-275, 277-357

**Before** (excerpt):
```rust
mod pr;
mod state;
mod theme;

pub struct App {
    // ...
    /// PR mode state.
    pub pr: PrState,
}
```

**After**:
```rust
mod patch;
mod pr;
mod state;
mod theme;

pub struct App {
    // ...
    /// PR mode state.
    pub pr: PrState,
    /// Patch mode state.
    pub patch: PatchState,
}
```

**Add methods**:
```rust
pub fn load_patch(&mut self, patch: String, label: String) {
    let patch_files = crate::core::parse_unified_diff(&patch);
    self.patch.active = true;
    self.patch.label = label;
    self.patch.files = patch_files.clone();
    self.files = patch_files
        .iter()
        .map(|pf| ChangedFile {
            path: pf.path.clone(),
            kind: pf.kind,
            old_path: pf.old_path.clone(),
        })
        .collect();
    self.rebuild_path_cache();
    self.sidebar.selected_idx = 0;
    self.sidebar.scroll = 0;
    if !self.files.is_empty() {
        self.request_current_patch_diff();
    } else {
        self.diff = None;
        self.viewer.hunk_view_rows.clear();
        self.old_buffer = None;
        self.new_buffer = None;
        self.ui.status = Some("Patch has no changed files".to_string());
    }
    self.ui.dirty = true;
}

pub fn source_display(&self) -> String {
    if self.patch.active {
        return format!("Patch ({})", self.patch.label);
    }
    diff_source_display(&self.source, &self.repo)
}

pub fn is_worktree_mode(&self) -> bool {
    matches!(self.source, DiffSource::WorkingTree) && !self.patch.active
}
```

**Why**: Patch mode reuses existing diff rendering but keeps PR behavior separate.

#### 5. Patch diff rendering
**File**: `src/ui/app/patch.rs` (new file)
**Location**: new file

**Add**:
```rust
use super::App;
use crate::core::DiffResult;
use crate::highlight::{query_scopes, LanguageId};
use crate::core::TextBuffer;

impl App {
    pub(super) fn request_current_patch_diff(&mut self) {
        if !self.patch.active || self.patch.files.is_empty() {
            return;
        }
        let Some(patch_file) = self.patch.files.get(self.sidebar.selected_idx) else {
            return;
        };
        self.current_lang = patch_file
            .path
            .extension()
            .map(LanguageId::from_extension)
            .unwrap_or(LanguageId::Plain);
        self.old_scopes.clear();
        self.new_scopes.clear();
        self.old_highlights.clear();
        self.new_highlights.clear();
        let (old_content, new_content) = super::extract_content_from_patch(&patch_file.patch);
        let old_buffer = TextBuffer::new(old_content.as_bytes());
        let new_buffer = TextBuffer::new(new_content.as_bytes());
        let is_binary = old_buffer.is_binary() || new_buffer.is_binary();
        let diff = if is_binary { None } else { Some(DiffResult::compute(&old_buffer, &new_buffer)) };
        if !is_binary {
            let lang = self.current_lang;
            let old_str = String::from_utf8_lossy(old_buffer.as_bytes());
            let new_str = String::from_utf8_lossy(new_buffer.as_bytes());
            self.old_scopes = query_scopes(lang, old_str.as_ref());
            self.new_scopes = query_scopes(lang, new_str.as_ref());
            self.old_highlights.compute(&self.highlighter, lang, old_str.as_ref());
            self.new_highlights.compute(&self.highlighter, lang, new_str.as_ref());
        } else {
            self.old_scopes.clear();
            self.new_scopes.clear();
            self.old_highlights.clear();
            self.new_highlights.clear();
        }
        self.is_binary = is_binary;
        self.old_buffer = Some(old_buffer);
        self.new_buffer = Some(new_buffer);
        self.diff = diff;
        self.rebuild_view_rows();
        self.viewer.scroll_y = 0;
        if let Some(diff) = self.diff.as_ref() {
            if let Some(first) = diff.hunks().first() {
                if let Some(view_row) = self.diff_row_to_view_row(first.start_row) {
                    self.viewer.scroll_y = view_row;
                }
            }
        }
        self.viewer.scroll_x = 0;
        self.ui.error = None;
        self.worker.loading = false;
        self.ui.dirty = true;
    }
}
```

**Why**: Mirrors PR patch flow while keeping PR actions disabled.

#### 6. Gate PR actions in patch mode
**File**: `src/ui/input.rs`
**Location**: lines 122-137

**Before**:
```rust
KeyCode::Char('A') if app.pr.active => {
    app.start_pr_approve();
    true
}
KeyCode::Char('R') if app.pr.active => {
    app.start_pr_request_changes();
    true
}
KeyCode::Char('O') if app.pr.active => {
    app.open_pr_in_browser();
    true
}
```

**After**:
```rust
KeyCode::Char('A') if app.pr.active && app.pr.current.is_some() => {
    app.start_pr_approve();
    true
}
KeyCode::Char('R') if app.pr.active && app.pr.current.is_some() => {
    app.start_pr_request_changes();
    true
}
KeyCode::Char('O') if app.pr.active && app.pr.current.is_some() => {
    app.open_pr_in_browser();
    true
}
```

**Why**: Prevent PR actions in patch mode while keeping PR behavior intact.

### Edge Cases to Handle
- [ ] Empty stdin: exit with error
- [ ] Patch with no hunks: show empty state
- [ ] Binary patch: display `[binary]` via existing buffer detection

### Success Criteria

**Automated**:
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

**Manual**:
- [ ] `git diff | quickdiff --stdin` renders file list + diff
- [ ] PR actions (`A/R/O`) are disabled in patch mode

### Rollback
```bash
git restore -- src/main.rs src/ui/app/mod.rs src/ui/app/state.rs src/ui/input.rs src/ui/app/patch.rs
```

### Notes
- Patch mode should not enable comment overlays; `is_worktree_mode` change enforces this.

---

## Phase 2: Patch Generation + Web Data Assembly

### Overview
Build unified patch strings for git/jj sources and serialize `review.json` for web template rendering.

### Prerequisites
- [ ] Phase 1 automated checks pass
- [ ] Phase 1 manual verification complete

### Change Checklist
- [x] Add `web` module with patch builders
- [x] Add JSON schema for web template
- [x] Add file stats from patch parsing

### Changes

#### 1. Web module skeleton
**File**: `src/web/mod.rs` (new file)
**Location**: new file

**Add**:
```rust
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::core::{diff_source_display, get_pr_diff, list_changed_files, parse_unified_diff, DiffSource, FileChangeKind, RepoRoot};

#[derive(Serialize)]
struct ReviewData {
    branch: String,
    commit: String,
    summary: String,
    patch: String,
    files: Vec<ReviewFile>,
    stats: ReviewStats,
}

#[derive(Serialize)]
struct ReviewFile {
    path: String,
    summary: String,
    additions: usize,
    deletions: usize,
    comments: Vec<ReviewComment>,
}

#[derive(Serialize)]
struct ReviewComment {
    startLine: usize,
    endLine: usize,
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

#[derive(Serialize)]
struct ReviewStats {
    bugs: usize,
    warnings: usize,
    suggestions: usize,
    good: usize,
}

pub struct WebInput {
    pub source: DiffSource,
    pub stdin_patch: Option<String>,
    pub file_filter: Option<String>,
    pub label: String,
}

pub fn build_review_data(repo: &RepoRoot, input: WebInput) -> Result<ReviewData> {
    let patch = if let Some(patch) = input.stdin_patch {
        patch
    } else {
        build_patch_from_source(repo, &input.source, input.file_filter.as_deref())?
    };
    let files = parse_unified_diff(&patch);
    let review_files = files
        .iter()
        .map(|f| ReviewFile {
            path: f.path.as_str().to_string(),
            summary: String::new(),
            additions: f.additions,
            deletions: f.deletions,
            comments: Vec::new(),
        })
        .collect::<Vec<_>>();
    let summary = diff_source_display(&input.source, repo);
    Ok(ReviewData {
        branch: current_git_branch(repo.path()).unwrap_or_else(|| summary.clone()),
        commit: current_git_short(repo.path()).unwrap_or_else(|| "".to_string()),
        summary,
        patch,
        files: review_files,
        stats: ReviewStats {
            bugs: 0,
            warnings: 0,
            suggestions: 0,
            good: 0,
        },
    })
}
```

**Why**: Centralizes patch and JSON assembly for web preview.

#### 2. Patch builder helpers
**File**: `src/web/mod.rs`
**Location**: new file (continued)

**Add**:
```rust
fn build_patch_from_source(repo: &RepoRoot, source: &DiffSource, file_filter: Option<&str>) -> Result<String> {
    if repo.is_jj() {
        return build_jj_patch(repo, source, file_filter);
    }
    build_git_patch(repo, source, file_filter)
}

fn build_git_patch(repo: &RepoRoot, source: &DiffSource, file_filter: Option<&str>) -> Result<String> {
    let base = repo.path();
    let mut patch = match source {
        DiffSource::WorkingTree => run_git(base, &["diff", "--no-color", "HEAD"])?,
        DiffSource::Commit(commit) => {
            let parent = crate::core::get_parent_revision(repo, commit)?;
            run_git(base, &["diff", "--no-color", &parent, commit])?
        }
        DiffSource::Range { from, to } => run_git(base, &["diff", "--no-color", from, to])?,
        DiffSource::Base(base_ref) => {
            let merge_base = crate::core::resolve_merge_base(repo, base_ref)?;
            run_git(base, &["diff", "--no-color", &merge_base])?
        }
        DiffSource::PullRequest { number, .. } => get_pr_diff(base, *number).map_err(|e| anyhow::anyhow!(e.to_string()))?,
    };

    patch = append_untracked(repo, patch)?;
    Ok(apply_file_filter(patch, file_filter))
}

fn append_untracked(repo: &RepoRoot, mut patch: String) -> Result<String> {
    let files = list_changed_files(repo)?;
    for f in files.into_iter().filter(|f| f.kind == FileChangeKind::Untracked) {
        let path = repo.path().join(f.path.as_str());
        let extra = run_git(repo.path(), &["diff", "--no-color", "--no-index", "/dev/null", path.to_str().unwrap_or("")])?;
        if !extra.trim().is_empty() {
            patch.push('\n');
            patch.push_str(&extra);
        }
    }
    Ok(patch)
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .with_context(|| format!("git {:?} failed", args))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(stderr.trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
```

**Why**: Produces unified patch strings consistent with quickdiff diff sources.

#### 3. JJ patch stub
**File**: `src/web/mod.rs`
**Location**: new file (continued)

**Add**:
```rust
fn build_jj_patch(repo: &RepoRoot, source: &DiffSource, file_filter: Option<&str>) -> Result<String> {
    let base = repo.path();
    let args = match source {
        DiffSource::WorkingTree => vec!["diff", "--git"],
        DiffSource::Commit(commit) => vec!["diff", "--git", "-r", commit],
        DiffSource::Range { from, to } => vec!["diff", "--git", "-r", &format!("{}..{}", from, to)],
        DiffSource::Base(base_ref) => vec!["diff", "--git", "-r", &format!("{}..@", base_ref)],
        DiffSource::PullRequest { .. } => return Err(anyhow::anyhow!("PR web mode requires git")),
    };
    let output = Command::new("jj")
        .args(args)
        .current_dir(base)
        .output()
        .with_context(|| "jj diff failed")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(stderr.trim().to_string()));
    }
    let patch = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(apply_file_filter(patch, file_filter))
}
```

**Why**: Enables web mode for jj without blocking git users; fallback behavior is explicit.

#### 4. File filter helper
**File**: `src/web/mod.rs`
**Location**: new file (continued)

**Add**:
```rust
fn apply_file_filter(patch: String, filter: Option<&str>) -> String {
    let Some(filter) = filter else {
        return patch;
    };
    let files = parse_unified_diff(&patch);
    let filtered = files.into_iter().filter(|f| f.path.as_str().contains(filter));
    filtered.map(|f| f.patch).collect::<Vec<_>>().join("\n")
}
```

**Why**: Reuses existing patch parser to filter by path.

#### 5. Git ref helpers
**File**: `src/web/mod.rs`
**Location**: new file (continued)

**Add**:
```rust
fn current_git_branch(repo: &Path) -> Option<String> {
    run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).ok().map(|s| s.trim().to_string())
}

fn current_git_short(repo: &Path) -> Option<String> {
    run_git(repo, &["rev-parse", "--short", "HEAD"]).ok().map(|s| s.trim().to_string())
}
```

**Why**: Matches review-export metadata fields without new dependencies.

### Edge Cases to Handle
- [ ] `git diff` empty output → still render HTML with empty list
- [ ] Untracked files → appended via `--no-index`
- [ ] JJ diff unsupported for PR → explicit error

### Success Criteria

**Automated**:
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

**Manual**:
- [ ] `quickdiff web HEAD~1` builds JSON with non-empty patch
- [ ] `quickdiff web --stdin` honors stdin patch

### Rollback
```bash
git restore -- src/web/mod.rs
```

### Notes
- JSON schema mirrors review-export for straightforward template reuse.

---

## Phase 3: Web Render + CLI Web Entry

### Overview
Add `quickdiff web` CLI entrypoint, generate HTML via Bun render script, and document usage.

### Prerequisites
- [ ] Phase 2 automated checks pass
- [ ] Phase 2 manual verification complete

### Change Checklist
- [x] Add web CLI handler in `src/main.rs`
- [x] Add `web/template.html` and `scripts/web_render.ts`
- [x] Add README examples for web and lazygit pager

### Changes

#### 1. CLI: `quickdiff web`
**File**: `src/main.rs`
**Location**: lines 77-108 (pre-clap arg check)

**Before**:
```rust
let args: Vec<String> = std::env::args().collect();
if args.get(1).map(|s| s.as_str()) == Some("comments") {
    return run_cli_comments(&args[2..]);
}

// Parse CLI args
let cli = Cli::parse();
```

**After**:
```rust
let args: Vec<String> = std::env::args().collect();
if args.get(1).map(|s| s.as_str()) == Some("comments") {
    return run_cli_comments(&args[2..]);
}
if args.get(1).map(|s| s.as_str()) == Some("web") {
    return run_cli_web(&args[2..]);
}

// Parse CLI args
let cli = Cli::parse();
```

**Why**: Adds a dedicated web entrypoint without altering existing clap parsing.

#### 2. Web CLI implementation
**File**: `src/main.rs`
**Location**: new helper near `run_cli_comments`

**Add**:
```rust
fn run_cli_web(args: &[String]) -> ExitCode {
    // parse --stdin, --open, --output, --theme, --file, and diffspec
    // discover repo
    // build review.json via quickdiff::web::build_review_data
    // write HTML via bun script
}
```

**Why**: Encapsulates web workflow and provides exit codes.

#### 3. Web template + render script
**File**: `web/template.html`
**Location**: new file

**Add** (skeleton):
```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{{BRANCH}} @ {{COMMIT}}</title>
    <style>/* minimal layout */</style>
  </head>
  <body>
    <script type="application/json" id="review-data">{{REVIEW_DATA_JSON}}</script>
    <div id="app"></div>
    <script type="module">
      import { FileDiff, parsePatchFiles } from 'https://esm.sh/@pierre/diffs@1.0.6';
      const data = JSON.parse(document.getElementById('review-data').textContent || '{}');
      const patches = parsePatchFiles(data.patch || '', 'quickdiff');
      // render file list + FileDiff per file
    </script>
  </body>
</html>
```

**Why**: Uses `@pierre/diffs` directly without external skill dependency.

**File**: `scripts/web_render.ts`
**Location**: new file

**Add**:
```ts
import { readFileSync, writeFileSync } from 'node:fs';

const templatePath = process.argv[2];
const jsonPath = process.argv[3];
const outPath = process.argv[4];

const template = readFileSync(templatePath, 'utf8');
const jsonText = readFileSync(jsonPath, 'utf8');
const data = JSON.parse(jsonText);
const safeJson = jsonText.replaceAll('</script>', '<\\/script>');

const html = template
  .replaceAll('{{REVIEW_DATA_JSON}}', safeJson)
  .replaceAll('{{BRANCH}}', String(data.branch || ''))
  .replaceAll('{{COMMIT}}', String(data.commit || ''));

writeFileSync(outPath, html, 'utf8');
```

**Why**: Keeps HTML generation simple and bun-compatible.

#### 4. README updates
**File**: `README.md`
**Location**: new sections

**Add**:
```md
### Web Preview
quickdiff web HEAD~1 --open

git diff | quickdiff web --stdin --open

### Lazygit Pager
# ~/.config/lazygit/config.yml
# git:
#   paging:
#     pager: quickdiff --stdin
```

**Why**: Documents new usage patterns.

### Edge Cases to Handle
- [ ] Missing bun → clear error message
- [ ] HTML path collisions → unique suffix when `--output` not provided

### Success Criteria

**Automated**:
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

**Manual**:
- [ ] `quickdiff web HEAD~1 --open` opens rendered HTML
- [ ] `git diff | quickdiff web --stdin --open` renders the patch

### Rollback
```bash
git restore -- src/main.rs web/template.html scripts/web_render.ts README.md
```

### Notes
- Template can be expanded later for annotations and comment UI.

---

## Testing Strategy

### Unit Tests to Add/Modify

**File**: `src/web/tests.rs` (new)

```rust
#[test]
fn build_review_data_empty_patch() {
    // ensure empty patch yields empty files and zero stats
}

#[test]
fn apply_file_filter_keeps_matching_files() {
    // filter keeps only matching path patches
}
```

### Integration Tests
- [ ] `quickdiff web --stdin` with a tiny patch produces non-empty HTML file

### Manual Testing Checklist
1. [ ] `git diff | quickdiff --stdin` renders in TUI
2. [ ] `quickdiff web HEAD~1 --open` opens HTML with diffs
3. [ ] `quickdiff web --stdin` with empty patch returns error

## Deployment Instructions

### Database Migrations (if applicable)
```bash
# none
```
**Rollback**:
```bash
# none
```

### Feature Flags (if applicable)
- none

### Environment Variables
- none

### Deployment Order
1. Publish new quickdiff binary
2. Ensure bun is installed on hosts that use `quickdiff web`

## Anti-Patterns to Avoid
- Parsing patch with regex splits that ignore `diff --git` boundaries; reuse `parse_unified_diff`.
- Enabling PR actions in patch mode; use `app.pr.current.is_some()` gating.

## Open Questions (must resolve before implementation)
- [x] None

## References
- `src/main.rs:24` CLI and main entrypoint
- `src/ui/app/pr.rs:458` PR patch rendering flow
- `src/ui/app/mod.rs:382` patch content extraction helper
- `src/core/pr_diff.rs:22` unified patch parsing
- `src/ui/input.rs:122` PR action key handling
- review-export pattern: `~/.gitgud/skills/review-export/template.html` (reference only)
