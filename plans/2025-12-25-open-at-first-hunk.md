# Open Diff View at First Hunk Implementation Plan

## Overview
When a diff finishes loading for a selected file, initialize the diff viewport (`scroll_y`) to the first hunk’s start row rather than row 0.

## Current State
- File selection calls `App::request_current_diff()` which clears diff state and resets scroll to the top (`scroll_y = 0`, `scroll_x = 0`) while the async diff load runs.
  - `src/ui/app.rs:349`
- When the worker responds, `App::poll_worker()` installs `self.diff` but does not change `scroll_y`, so the viewport stays at the top-of-file.
  - `src/ui/app.rs:413`
- Diffs are computed in the background thread; binary files return `diff: None`.
  - `src/ui/worker.rs:89`
- “First hunk” is defined by `Hunk::start_row`, which includes leading context (e.g. 3 lines by default).
  - `src/core/diff.rs:470`

### Key Discoveries
- Best hook point: `DiffLoadResponse::Loaded { .. }` handler in `App::poll_worker()` after `self.diff = diff;`.
  - `src/ui/app.rs:416`
- Avoid using `DiffResult::next_hunk_row(0)` here; it can skip a hunk that starts at row 0 due to its `<=` partition logic.
  - `src/core/diff.rs:131`

## Desired End State
- When a file’s diff finishes loading (i.e. `DiffLoadResponse::Loaded` applied):
  - If `diff` is `Some` and `diff.hunks().first()` exists → set `scroll_y` to that hunk’s `start_row`.
  - Otherwise (binary/no hunks/no diff) → keep `scroll_y` at 0.
- Verification:
  - `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`
  - Manual: select a changed file with hunks; confirm the first visible region is the first hunk (context + first changes) rather than the top of the file.

## Out of Scope
- Changing hunk computation, context size, or diff model semantics.
- Adding a new navigation command or altering keybindings.
- Persisting per-file scroll positions.

## Error Handling Strategy
- No new fallible operations.
- Maintain existing behavior for worker errors (`DiffLoadResponse::Error`) and binary detection (`diff: None`).

## Implementation Approach
- Minimal UI-only change: adjust `scroll_y` after diff load is applied.
- Keep core/UI boundaries intact (no changes in `src/core/`).

Alternative considered (not chosen): add `DiffResult::first_hunk_row()` helper in `src/core/diff.rs` with tests, and call it from UI. Rejected to avoid introducing/expanding public core API for a one-liner UI behavior.

---

## Phase 1: Jump to First Hunk on Diff Load

### Overview
Initialize `scroll_y` to the first hunk start row when the async diff load completes.

### Prerequisites
- [x] Repo builds locally.
- [x] Baseline checks run once (optional but recommended):
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`

### Changes

#### 1. Update diff-load apply path
**File**: `src/ui/app.rs`
**Lines**: `src/ui/app.rs:416`

**Before** (current `DiffLoadResponse::Loaded` handler core):
```rust
self.is_binary = is_binary;
self.old_buffer = Some(old_buffer);
self.new_buffer = Some(new_buffer);
self.diff = diff;
self.refresh_current_file_comment_markers();
self.dirty = true;
```

**After**:
```rust
self.is_binary = is_binary;
self.old_buffer = Some(old_buffer);
self.new_buffer = Some(new_buffer);
self.diff = diff;

if let Some(diff) = self.diff.as_ref() {
    if let Some(first) = diff.hunks().first() {
        self.scroll_y = first.start_row;
    }
}

self.refresh_current_file_comment_markers();
self.dirty = true;
```

**Why**
- `request_current_diff()` resets scroll while loading; this restores initial view to the first hunk once hunks exist.
- Guarding on `self.diff.as_ref()` naturally skips binary cases (`diff: None`).

### Edge Cases to Handle
- [x] Binary file (`diff: None`) → no change, stays at top.
- [x] Empty/no-change diff (no hunks) → stays at top.
- [x] First hunk starts at row 0 → sets `scroll_y = 0` (no change, still correct).

### Success Criteria

**Automated**:
```bash
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
```

**Manual**:
- [ ] Run `quickdiff` in a repo with multiple changed files.
- [ ] Select a file whose first change is far from line 1.
- [ ] Confirm the diff panes start at the first hunk (you should see the change within the first ~context lines).
- [ ] Confirm `{`/`}` hunk navigation still behaves as before.

### Rollback
```bash
git checkout -- src/ui/app.rs
```

### Notes
If you decide you want to jump to the first *changed* row (not hunk start w/ context), implement this instead:
- Find the first row where `row.kind != ChangeKind::Equal` and set `scroll_y` to that row index (clamp to 0 if none). This is a different UX than “first hunk”.

---

## Testing Strategy
- No new tests planned; change is a small UI wiring adjustment with low risk and no new core logic.
- If you want regression coverage, the lowest-friction option is to refactor the “choose initial scroll row” logic into a small pure helper (UI module) and unit test it, but that’s optional.

## Anti-Patterns to Avoid
- Using `diff.next_hunk_row(0)` for initial positioning (can skip a hunk at row 0).
- Trying to jump in `request_current_diff()` (diff/hunks not available yet).

## References
- `src/ui/app.rs:349` (`request_current_diff()` scroll reset)
- `src/ui/app.rs:413` (`poll_worker()` load apply path)
- `src/ui/worker.rs:89` (async diff computation)
- `src/core/diff.rs:131` (`next_hunk_row` partition nuance)
- `src/core/diff.rs:470` (hunk start includes context)
