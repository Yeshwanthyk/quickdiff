# AGENTS.md - quickdiff

## Development Loop

Run these commands constantly; the project should stay green.

```bash
cargo fmt --check      # Format check
cargo clippy --all-targets --all-features -- -D warnings  # Lint
cargo test             # Unit tests
```

Or all at once:
```bash
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
```

## Project Structure

```
src/
├── lib.rs              # Library root
├── main.rs             # Binary entry point (panic-safe terminal handling)
├── core/               # Core primitives (no TUI dependencies)
│   ├── mod.rs
│   ├── text.rs         # TextBuffer - O(1) line slicing, binary detection
│   ├── repo.rs         # Git repo discovery + file ops
│   ├── diff.rs         # Diff model + hunk navigation
│   └── viewed.rs       # Viewed state storage
├── highlight/          # Syntax highlighting (stub for now)
│   └── mod.rs
└── ui/                 # Terminal UI (ratatui)
    ├── mod.rs
    ├── app.rs          # Application state + error handling
    ├── input.rs        # Key handling
    └── render.rs       # Rendering + sanitization
```

## Architecture Rules

1. **Core is UI-agnostic**: `src/core/` must not depend on ratatui/crossterm
2. **Lazy loading**: Diff/highlight computed per-selected-file only
3. **Viewport-scoped rendering**: Only render visible rows
4. **Dirty flag redraw**: Only redraw when state changes
5. **Graceful degradation**: Binary files, directories, errors handled without panic

## Key Abstractions

### TextBuffer (`core/text.rs`)
- Stores bytes as `Arc<[u8]>` for cheap cloning
- Precomputes line offsets for O(1) line access
- Normalizes CRLF to LF
- Detects binary content (NUL in first 8KB)
- Lossy UTF-8 handling for invalid sequences

### DiffResult (`core/diff.rs`)
- `rows: Vec<RenderRow>` - all diff rows
- `hunks: Vec<Hunk>` - index for navigation
- `next_hunk_row()`/`prev_hunk_row()` - O(log N) binary search

### ViewedStore (`core/viewed.rs`)
- Trait-based for testability
- `MemoryViewedStore` - ephemeral
- `FileViewedStore` - persists to `~/.config/quickdiff/state.json`

## Robustness Features

- **Panic-safe terminal**: RAII guard + panic hook restore terminal state
- **Horizontal scroll**: Char-based, not byte-based (safe for UTF-8)
- **Control char sanitization**: Terminal injection prevented
- **Directory handling**: Gracefully shows empty for non-files
- **Error display**: Errors shown in status bar, not panics

## Syntax Highlighting

Tree-sitter based highlighting is enabled for:
- **Rust** (`.rs`)
- **TypeScript/JavaScript** (`.ts`, `.js`, `.mjs`, `.cjs`)
- **TSX/JSX** (`.tsx`, `.jsx`)
- **Go** (`.go`)
- **Python** (`.py`, `.pyi`)
- **JSON** (`.json`)
- **YAML** (`.yaml`, `.yml`)
- **Bash** (`.sh`, `.bash`, `.zsh`)

Falls back to plain text for unsupported extensions.

### Adding New Languages

1. Add grammar crate to `Cargo.toml` (e.g., `tree-sitter-python`)
2. Add variant to `LanguageId` enum in `highlight/mod.rs`
3. Update `from_extension()` mapping
4. Add language initialization in `TreeSitterHighlighter::new()`

## Testing Strategy

- **Unit tests**: Core logic (diff, hunk navigation, text slicing, rename parsing)
- **Integration tests**: Minimal; avoid slow tests
- Each module has `#[cfg(test)] mod tests` at bottom

## Key Bindings Reference

| Key | Sidebar | Diff |
|-----|---------|------|
| `j/k` | Navigate files | Scroll vertical |
| `h/l` | - | Scroll horizontal |
| `Enter` | Open diff | - |
| `Space` | Toggle viewed | Toggle viewed |
| `{/}` | - | Prev/next hunk |
| `Tab` | Switch focus | Switch focus |
| `1/2` | Focus sidebar | Focus diff |
| `g/G` | - | Start/end |
| `q` | Quit | Quit |

## Git Integration

- Uses shell-out to `git` via `std::process::Command`
- Porcelain v1 format with `-z` for robust parsing
- Handles renames and copies (both treated as rename)
- Lazy-loads file content from HEAD only when selected
- Gracefully handles directories/symlinks (empty content)

## Local Install (`~/commands`)

If `quickdiff` is invoked from `~/commands`, update the binary after changes:

```bash
cargo build --release
install -m 755 target/release/quickdiff ~/commands/quickdiff
```

Sanity check:

```bash
which quickdiff
quickdiff --version
```
