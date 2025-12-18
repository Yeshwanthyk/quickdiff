# Diff Hunk Comments (Repo-Local) Implementation Plan

## Overview

Add repo-local, gitignored **hunk-level comments** to quickdiff:
- Create comments from the **TUI** at the “current hunk”.
- Manage comments via a **CLI**: `comments list`, `comments add`, `comments resolve`.

Primary goal: enable a workflow where you annotate diffs in the TUI, then later (including from coding agents) pull comments and address them.

## Current State Analysis

quickdiff today:
- Has a core diff model with hunks: `DiffResult { rows, hunks }` and `Hunk { start_row, row_count, old_range, new_range }` in `src/core/diff.rs:41`.
- Tracks a “current position” in the diff view via scroll offset: `App.scroll_y` in `src/ui/app.rs:47`.
- Has a persistence pattern (versioned JSON + atomic-ish rename) for “viewed” state in `src/core/viewed.rs:157`, but that state is global under `~/.config`.
- No CLI subcommands exist; `src/main.rs:36` always launches the TUI.

Key constraints:
- Core must remain UI-agnostic: `src/core/` must not depend on ratatui/crossterm (`AGENTS.md:41`).

## Desired End State

After implementation:
- A repo-local file `.quickdiff/comments.json` stores comments.
- `.quickdiff/` is ignored by git (added to `.gitignore`).
- In the TUI (diff focus), the user can:
  - Press a key (proposed `c`) to enter single-line comment input.
  - Press `Enter` to save a comment anchored to the hunk containing the current scroll position.
- In the CLI, the user can:
  - `quickdiff comments list` to view comments (default: open only).
  - `quickdiff comments add` to add a comment anchored to a specific hunk.
  - `quickdiff comments resolve <id>` to soft-resolve a comment.

Verification (high level):
- Add comment in TUI → exit → `quickdiff comments list` shows it.
- `quickdiff comments resolve <id>` marks it resolved.

### Key Discoveries
- Hunks already provide line ranges for anchoring: `src/core/diff.rs:41`.
- The TUI has enough state to pick a “current hunk” without a cursor: `src/ui/app.rs:47`.
- There is an established JSON persistence pattern we can reuse: `src/core/viewed.rs:157`.

## What We’re NOT Doing

Explicitly out of scope for v1:
- Multi-line comment input (single-line only).
- Listing/resolving comments inside the TUI (TUI is add-only).
- Sophisticated re-anchoring across heavy edits/renames beyond the minimal selector described below.
- Network sync, PR review integration, or commenting on specific tokens/columns.

## Implementation Approach

### Minimal Anchoring (Extensible)

We’ll store a location as an **anchor** that can evolve over time.

For v1 we implement a single selector type:

- **DiffHunkSelectorV1** (minimal, stable for common edits):
  - `old_range` and `new_range` from `Hunk` (line-based, used for display + fallback)
  - `digest`: a stable hash of the hunk’s **changed rows** (derived from `DiffResult.rows` for that hunk)

Rationale:
- Line ranges alone drift when lines are inserted above.
- A digest based on the changed lines stays stable when the *change* stays the same but the file shifts.
- The schema will allow adding future selectors (text quote, tree-sitter symbol path) without breaking old comments.

**Important:** v1 does not need to fully re-anchor comments at render-time. It’s enough to store digest now so we can add re-anchoring later. CLI/TUI output can display the stored header + snippet.

### Repo-Local Storage

Store comments under repo root:
- `.quickdiff/comments.json`

This will be gitignored and treated as local working state.

### CLI Integration

Add a small CLI dispatcher in `src/main.rs`:
- If invoked as `quickdiff comments ...` → run CLI mode and exit.
- Otherwise → start the existing TUI.

For v1 we can use minimal manual parsing (no new dependencies) since there are only 3 commands.

## Phase 1: Core Model + Anchor Digest

### Overview
Define comment types and the minimal anchor selector in `src/core/`.

### Changes Required

#### 1) New core module
**File**: `src/core/comments.rs`
**Changes**:
- Define `CommentId = u64`.
- Define:
  - `CommentStatus` (`Open`, `Resolved`)
  - `Comment` { `id`, `path: RelPath`, `message`, `status`, `anchor`, optional `created_at_ms`, optional `resolved_at_ms` }
  - `Anchor { selectors: Vec<Selector> }`
  - `Selector::DiffHunkV1(DiffHunkSelectorV1)`
  - `DiffHunkSelectorV1 { old_range: (usize, usize), new_range: (usize, usize), digest_hex: String }`
- Add helper functions:
  - `fn selector_from_hunk(diff: &DiffResult, hunk_idx: usize) -> DiffHunkSelectorV1`
  - `fn digest_hunk_changed_rows(diff: &DiffResult, hunk: &Hunk) -> String`

Digest algorithm (minimal, stable, dependency-free):
- Iterate rows in the hunk’s row slice.
- For each `RenderRow`:
  - If old side is present and row is `Delete` or `Replace`, feed bytes of `b"-" + old_line + b"\n"` into the hasher.
  - If new side is present and row is `Insert` or `Replace`, feed bytes of `b"+" + new_line + b"\n"`.
- Use a stable in-code hash (e.g., FNV-1a 64-bit) and store as lowercase hex.

#### 2) Export module
**File**: `src/core/mod.rs:3`
**Changes**: add `mod comments;` and `pub use comments::*;`.

### Success Criteria

#### Automated Verification
- [ ] Format passes: `cargo fmt --check`
- [ ] Lint passes: `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Unit tests pass: `cargo test`

#### Manual Verification
- [ ] None (core-only).

---

## Phase 2: Repo-Local Comment Store

### Overview
Add `.quickdiff/comments.json` persistence with a versioned schema.

### Changes Required

#### 1) CommentStore trait + file implementation
**File**: `src/core/comments_store.rs` (or `src/core/comments.rs` if you prefer one file initially)
**Changes**:
- Define `CommentStore` trait (list/add/resolve) to keep UI/CLI decoupled.
- Implement `FileCommentStore`:
  - `fn open(repo_root: &RepoRoot) -> io::Result<Self>` (uses `repo_root.path().join(".quickdiff/comments.json")`)
  - `fn load() -> Result<State, Error>`
  - `fn save(state) -> Result<(), Error>` using temp file + rename (pattern like `src/core/viewed.rs:157`).

Schema (v1):
```json
{ "version": 1, "next_id": 1, "comments": [ ... ] }
```

Error handling:
- If the JSON is unreadable/invalid, return an error (do not silently discard comments).

#### 2) Ensure `.quickdiff/` is gitignored
**File**: `.gitignore:1`
**Changes**: add `.quickdiff/`.

### Success Criteria

#### Automated Verification
- [ ] Format passes: `cargo fmt --check`
- [ ] Lint passes: `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Unit tests pass (store roundtrip using `tempfile`): `cargo test`

#### Manual Verification
- [ ] Create `.quickdiff/comments.json` automatically on first comment add.

---

## Phase 3: CLI Commands (`quickdiff comments …`)

### Overview
Add CLI subcommands for listing, adding, and resolving comments.

### Changes Required

#### 1) CLI dispatch
**File**: `src/main.rs:36`
**Changes**:
- Parse `std::env::args()`.
- If args start with `comments`, run CLI handler and return.
- Otherwise execute existing TUI flow.

#### 2) CLI behavior
**File**: `src/cli/comments.rs` (or `src/cli/mod.rs`)
**Commands**:
- `quickdiff comments list [--all] [--json] [--path <relpath>]`
  - default output: open comments only
  - `--all` includes resolved
  - `--json` outputs machine-readable entries (including id, path, status, message, stored hunk header)
- `quickdiff comments add --path <relpath> --hunk <n> --message <text>`
  - `--hunk` is 1-based index into `DiffResult.hunks` for that file
  - loads HEAD + working content using existing core functions (`src/core/repo.rs:205`) then computes diff (`src/core/diff.rs:64`)
  - builds selector via `selector_from_hunk`
- `quickdiff comments resolve <id>`
  - toggles comment status to `Resolved`

Exit codes:
- Non-zero if not in a git repo (same behavior as TUI path in `src/main.rs:51`).
- Non-zero if comment id not found.

### Success Criteria

#### Automated Verification
- [ ] Format passes: `cargo fmt --check`
- [ ] Lint passes: `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Unit tests pass: `cargo test`

#### Manual Verification
- [ ] `quickdiff comments add --path … --hunk 1 --message "…"` creates a comment.
- [ ] `quickdiff comments list` shows it.
- [ ] `quickdiff comments resolve <id>` marks it resolved.

---

## Phase 4: TUI “Add Comment” (Single-Line)

### Overview
Add a lightweight input mode to create a comment on the current hunk.

### Changes Required

#### 1) App state for comment entry
**File**: `src/ui/app.rs:18`
**Changes**:
- Add a UI mode enum (e.g., `Mode::Normal | Mode::AddComment`).
- Add `draft_comment: String`.
- Add `status_msg: Option<String>` (separate from `error_msg`).

#### 2) Input handling
**File**: `src/ui/input.rs:73`
**Changes**:
- In diff focus, bind `c` to enter AddComment mode.
- While in AddComment mode:
  - `Char(x)` appends
  - `Backspace` deletes
  - `Enter` saves and exits mode
  - `Esc` cancels and exits mode

#### 3) Determining “current hunk”
**File**: `src/core/diff.rs` (or new helper in comments module)
**Changes**:
- Add helper `fn hunk_at_row(&self, row: usize) -> Option<usize>` returning the index of the hunk whose range includes the row.

Save behavior:
- If no diff or no hunks → set `error_msg` or `status_msg` and do nothing.
- Otherwise create a comment with selector built from that hunk and persist to store.

#### 4) Rendering prompt
**File**: `src/ui/render.rs:282`
**Changes**:
- When in AddComment mode, bottom bar shows: `Comment: <draft>` and key hints (`Enter` save, `Esc` cancel).
- Otherwise show existing hints.

### Success Criteria

#### Automated Verification
- [ ] Format passes: `cargo fmt --check`
- [ ] Lint passes: `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Unit tests pass: `cargo test`

#### Manual Verification
- [ ] In diff view, press `c`, type a short comment, press `Enter`.
- [ ] Exit quickdiff and run `quickdiff comments list` → comment appears.

---

## Testing Strategy

### Unit Tests
- Selector digest determinism (same diff → same digest).
- Store roundtrip (add + resolve) using `tempfile`.
- `hunk_at_row` correctness for rows inside/outside hunks.

### Manual Testing Steps
1. In a git repo with changes, run `quickdiff`.
2. Navigate to a file with a hunk, scroll into the hunk.
3. Press `c`, enter a comment, press `Enter`.
4. Run `quickdiff comments list` to verify output.
5. Resolve via `quickdiff comments resolve <id>` and confirm `--all` shows resolved.

## Performance Considerations

- Comments should be loaded once per app start and cached, or loaded on demand when adding.
- Digest computation is bounded by hunk size; should be negligible compared to existing diff computation.

## Migration Notes

- On first write, create `.quickdiff/` directory if missing.
- If `.quickdiff/comments.json` exists but is invalid JSON, return an error and do not overwrite.

## References

- Diff/hunk model: `src/core/diff.rs:41`
- TUI scroll position: `src/ui/app.rs:47`
- Existing persistence pattern: `src/core/viewed.rs:157`
