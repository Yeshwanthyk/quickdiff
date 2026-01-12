# Quickdiff Architecture

## Overview

Quickdiff is a terminal-based diff viewer for git and jj repositories.

## Module Layout

```
src/
├── lib.rs              # Library root, re-exports
├── main.rs             # CLI entry point
├── metrics.rs          # Optional performance metrics
├── cli.rs              # CLI subcommands
├── theme.rs            # Color themes
├── prelude.rs          # Common imports
├── core/               # Core logic (UI-agnostic)
│   ├── mod.rs          # Re-exports
│   ├── text.rs         # TextBuffer: O(1) line access
│   ├── diff.rs         # DiffResult: Myers diff + rendering
│   ├── repo.rs         # Git/jj repository abstraction
│   ├── viewed.rs       # Viewed state persistence
│   ├── comments.rs     # Comment anchoring
│   ├── comments_store.rs # Comment storage
│   ├── fuzzy.rs        # Fuzzy file matching
│   ├── gh.rs           # GitHub CLI integration
│   ├── pr_diff.rs      # PR diff parsing
│   └── watcher.rs      # File system watching
├── highlight/          # Syntax highlighting
│   └── mod.rs          # Tree-sitter integration
└── ui/                 # Terminal UI (ratatui)
    ├── mod.rs          # Re-exports
    ├── app.rs          # Application state
    ├── input.rs        # Key/mouse handling
    ├── worker.rs       # Background diff loading
    └── render/         # Rendering
        ├── mod.rs      # Main render orchestration
        ├── bars.rs     # Top/bottom bars
        ├── sidebar.rs  # File list
        ├── diff.rs     # Diff panes
        ├── overlays.rs # Modal overlays
        └── helpers.rs  # Shared utilities
```

## Data Flow

```
User Input → input.rs → App state mutation
                            ↓
                      worker thread (diff computation)
                            ↓
App state → render.rs → Terminal
```

## Key Abstractions

### TextBuffer (`core/text.rs`)
- Immutable text storage with precomputed line offsets
- O(1) line access, CRLF normalization, binary detection
- Cheap cloning via `Arc<[u8]>`

### DiffResult (`core/diff.rs`)
- Myers diff algorithm with patience improvements
- Hunk-based navigation with O(log N) lookup
- Inline change highlighting at character level

### App (`ui/app.rs`)
- Central state container
- Sub-structs for logical grouping:
  - `SidebarState`: file list navigation, scroll, filter, path cache
  - `ViewerState`: diff viewport, scroll positions, view/pane modes
  - `CommentsState`: comment viewing/editing
  - `PrState`: PR review mode
  - `UiState`: modes and messages

## Performance Optimizations

### Render Path
- Static spaces buffer (512 chars) for padding without allocation
- Cached truncated paths in SidebarState
- Dirty flag redraw - only render when state changes
- Viewport-scoped rendering - only render visible rows

### Diff Computation
- Background worker thread for non-blocking UI
- Request coalescing - only latest file request is processed
- Lazy loading - diff computed per-selected-file only

### Metrics
- Optional timing instrumentation via `QUICKDIFF_METRICS=1`
- RAII Timer for render frame and diff compute

## Adding Features

### New Language Support
1. Add grammar crate to `Cargo.toml`
2. Add variant to `LanguageId` in `highlight/mod.rs`
3. Update `from_extension()` mapping
4. Initialize in `TreeSitterHighlighter::new()`

### New Overlay
1. Add mode variant to `Mode` enum
2. Create render function in `ui/render/overlays.rs`
3. Add case to `render()` in `ui/render/mod.rs`
4. Handle input in `ui/input.rs`

### New Diff Source
1. Add variant to `DiffSource` in `core/mod.rs`
2. Handle in `list_changed_files_*` functions
3. Update `App::new()` and `request_current_diff()`

## Testing Strategy

- **Unit tests**: Core logic (diff, hunk navigation, text slicing, binary detection)
- **Integration tests**: Large file handling, git operations
- Each module has `#[cfg(test)] mod tests` at bottom

## Git Integration

- Uses shell-out to `git` via `std::process::Command`
- Porcelain v1 format with `-z` for robust parsing
- Handles renames and copies (both treated as rename)
- Lazy-loads file content from HEAD only when selected
- Gracefully handles directories/symlinks (empty content)
