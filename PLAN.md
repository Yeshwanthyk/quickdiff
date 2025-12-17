# Quickdiff Plan

## Overview
Build a **git-first** terminal diff viewer in Rust.

- Run `quickdiff` inside *any* git repo.
- Show changed files (left) and a side-by-side diff (old/new).
- Be fast by default: render only what’s visible; compute diff/highlight lazily.
- Be easy to extend: core primitives are UI-agnostic and heavily tested.

## MVP Definition (Git-First)
**Input:** current working directory must be inside a git repo.

**Compare:** working tree content vs `HEAD` for each path.

**UI:**
- Top bar: current file, mode hints, key hints.
- Left sidebar: list of changed files.
- Main: side-by-side diff (old/new) with line numbers.

**Keys (MVP):**
- Navigation: `h/j/k/l`
- Next changed hunk: `}` (and later `{` for previous)
- Mark file as viewed: `Space`
- Quit: `q`

## Non-Goals (MVP)
- AST/structural diff (difftastic-style) — this is a text diff viewer with syntax highlighting.
- Applying patches, staging, merging.
- Full repo browser; sidebar is “changed files”, not “all files”.
- Perfect rename/copy tracking; handle as delete/add initially.

## Key Design Principles
- **Primitives first:** core types are small, explicit, and tested.
- **Lazy work:** diff/highlight computed per-selected-file (and only for visible rows when possible).
- **Performance budgets:** avoid per-frame allocations that scale with file size.
- **Degrade gracefully:** on huge files or pathological diffs, fall back rather than freezing.

## Glossary
- **Hunk:** a contiguous block of changes.
- **Viewport:** the visible terminal region (height/width) currently rendered.
- **Render row:** one displayed row in the diff view (maps to 0..1 old line + 0..1 new line).

## Development Loop (Strong)
Run these constantly; the project should stay green.

- Format: `cargo fmt --check`
- Lint: `cargo clippy --all-targets --all-features -- -D warnings`
- Test: `cargo test`

(Once benches exist) `cargo bench`

## Milestones

### 1) Define product scope and UX flows
- [x] Define product scope and UX flows

Acceptance:
- `quickdiff` behavior defined for “not in git repo”, “no changes”, and “has changes”.
- Layout/focus model decided (sidebar vs diff focus).

### 2) Choose core crates and high-level architecture
- [x] Choose core crates and high-level architecture

Initial crate choices (keep small; revisit later):
- UI: `ratatui` + `crossterm`
- Diff: `similar`
- Highlighting: `tree-sitter` + `tree-sitter-highlight` + a small set of language crates
- Persistence (later): `directories`, `serde`, `serde_json`

Git integration (MVP):
- Shell out to `git` via `std::process::Command`.
- Use `-z` porcelain outputs for robust parsing.
- Lazy-load old content (`git show HEAD:<path>`) only when selected.

### 3) Design minimal primitives and module boundaries
- [x] Design minimal primitives and module boundaries

Goal: define core types so UI is just a projection.

Proposed module layout:
- `core/` (no TUI dependencies)
  - `text.rs`: `TextBuffer`
  - `repo.rs`: repo discovery + file listing + content loading
  - `diff.rs`: diff/hunks/render-row model
  - `viewed.rs`: viewed state store trait + impls
- `highlight/` (tree-sitter; UI-agnostic output)
- `ui/` (ratatui rendering + input handling)

Core primitives (draft):
- `RepoRoot(PathBuf)` (canonicalized)
- `RelPath(String)` or `RelPath(PathBuf)` (repo-relative; never absolute)
- `FileChangeKind`: Added/Modified/Deleted/Untracked/Renamed (Renamed may be “best effort”)
- `TextBuffer { bytes: Arc<[u8]>, line_starts: Vec<usize> }`
  - O(1) line slicing by offsets
  - Handles missing trailing newline
  - CRLF normalization strategy defined (display vs internal)

Acceptance:
- Core types compiled and unit tested without ratatui.
- UI state does not “own” file contents; it references IDs and caches.

### 4) Design diff model and hunk navigation pipeline
- [x] Design diff model and hunk navigation pipeline

MVP diff algorithm:
- Use `similar` line diff (`TextDiff::from_lines` initially).
- Group changes into hunks with configurable context lines.

Render-row model:
- Represent the diff as a sequence of blocks:
  - Equal runs
  - Changed hunks (delete/insert/replace)
- A viewport iterator produces only visible rows:
  - `render_rows(start_row, height)` → iterator of `RenderRow { old: Option<LineRef>, new: Option<LineRef>, kind }`

Hunk navigation:
- Build `HunkIndex` as sorted list of changed-block start rows.
- `}` = jump to next hunk start after current scroll row (binary search).
- (Later) `{` = previous hunk.

Degrade strategy:
- Set a deadline/timeout on diff for huge inputs.
- On timeout: show simplified view (e.g., “changed-only” blocks) or disable inline alignment.

Tests:
- Unit tests for:
  - hunk grouping boundaries
  - `}` jump correctness across multiple hunks
  - row generation for insert/delete/replace

Acceptance:
- `}` navigation is deterministic and O(log N).
- Diff view can render without allocating proportional to file size.

### 5) Design Tree-sitter highlighting pipeline
- [x] Design Tree-sitter highlighting pipeline (stub implemented)

Language detection:
- Map file extension → `LanguageId`.
- Start with a small set: TS/TSX + Rust + “plain text fallback”.

Highlight engine:
- `Highlighter` trait returning UI-agnostic spans:
  - `highlight_line(range) -> Vec<StyledSpan { byte_range, style_id }>`
- `TreeSitterHighlighter` implementation:
  - Uses `tree-sitter-highlight` highlight events.
  - Converts highlight names → `style_id` (stable palette).

Performance strategy:
- Highlight lazily for visible lines only OR compute per-file once in a background job.
- Set size/time caps; if exceeded, skip highlighting (still show diff colors).

Tests:
- Unit tests around extension→language mapping.
- Golden test for a small snippet to ensure highlight spans are stable.

Acceptance:
- Highlight never blocks the UI loop.
- Fallback behavior is clear and tested.

### 6) Design rendering strategy for speed (virtualized rows, lazy spans)
- [x] Design rendering strategy for speed (virtualized rows, lazy spans)

Layout:
- Top bar + sidebar + two diff panes.
- Column widths computed from terminal size (including gutters).

Rendering approach:
- Keep per-frame allocations bounded by viewport height.
- Maintain reusable scratch buffers inside UI state (e.g., `Vec<Line>`).
- Only convert bytes→display text for lines in the viewport.

Visual rules:
- Diff background (red/green) based on change kind.
- Line number gutter.
- Optional: thin change marker column.

Tests:
- Snapshot tests of the rendered buffer for a fixed terminal size.

Acceptance:
- Rendering cost scales with viewport size, not file length.

### 7) Specify keymaps and interaction details
- [x] Specify keymaps and interaction details

Modes:
- Sidebar focus vs Diff focus.

MVP keys:
- Sidebar: `j/k` move, `Enter` open, `Space` viewed
- Diff: `j/k` scroll, `h/l` horizontal scroll, `}` next hunk, `Space` viewed
- Global: `1` sidebar, `2` diff, `Tab` toggle focus, `q` quit

Acceptance:
- Key handling is table-driven (easy to extend).
- Conflicts documented.

### 8) Plan persistence for the “viewed” state
- [x] Plan persistence for the "viewed" state (in-memory + `~/.config/quickdiff/state.json`)

MVP persistence design:
- Store under `$XDG_CONFIG_HOME/quickdiff/state.json` (default `~/.config/quickdiff/state.json`).
- Key by canonical repo root path.

Schema (draft):
- `version: 1`
- `repos: { repo_root: { viewed: [relpath...], last_selected?: relpath } }`

Implementation details:
- Atomic write: write temp file then rename.
- Keep file small; only store what’s necessary.

Tests:
- Round-trip tests for state serialization.

Acceptance:
- Viewed flags persist across runs and don’t leak across repos.

### 9) Add `AGENTS.md` documenting standard tooling/tests
- [x] Add `AGENTS.md` documenting standard tooling/tests

Contents:
- Required dev loop commands.
- Code structure rules (core/UI separation).
- Guidance for adding new languages.

Acceptance:
- New contributors/agents can run the project correctly without guesswork.

### 10) Define the tests + linting loop and add initial coverage
- [x] Define the tests + linting loop (fmt, clippy, test) and add initial coverage

Testing strategy:
- Core logic gets unit tests (diff, hunk navigation, text slicing).
- Rendering gets snapshot-like tests using ratatui’s buffer comparisons.
- Avoid slow integration tests as default; keep a small number.

CI strategy (optional early):
- GitHub Actions: fmt, clippy, test.

Acceptance:
- `cargo fmt --check && cargo clippy … && cargo test` passes.

### 11) Implement git working-tree MVP
- [x] Implement git working-tree MVP (file list, side-by-side diff, viewed toggle)

Steps:
- Create Rust crate + binary `quickdiff`.
- Repo discovery (`git rev-parse --show-toplevel`).
- Changed files listing (`git status --porcelain=v1 -z`).
- Load old content (from `HEAD`) and new content (from disk).
- Compute diff model and render.
- Implement `Space` → in-memory viewed.

Acceptance:
- Running `quickdiff` in a repo shows a list of changes and a usable diff view.

### 12) Add Tree-sitter highlighting for key languages
- [x] Add Tree-sitter highlighting for key languages (TSX, Rust, fallback)

Acceptance:
- Highlighting works for TSX/Rust examples and falls back cleanly.

### 13) Persist “viewed” flags across runs
- [x] Persist "viewed" flags across runs

Acceptance:
- Viewed state restored on startup for that repo.

### 14) Polish navigation
- [x] Polish navigation (next/prev file, hunk jump, horizontal scroll)

Acceptance:
- `}` is solid; add `{` and next/prev file keys.

### 15) Performance validation + benchmarking scripts
- [x] Performance validation + benchmarking scripts

What to measure:
- Time-to-first-frame (UI visible with file list).
- Time-to-open-file (diff computed for a medium file).
- Worst-case behavior on huge diffs (no UI lock-up).

Acceptance:
- Benchmarks exist and regressions can be caught.

## Tracking
- This file (`PLAN.md`) is the source of truth.
- As tasks are completed, checkboxes get marked `[x]`.
