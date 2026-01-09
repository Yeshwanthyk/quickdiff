# Changelog

All notable changes to quickdiff are documented here.

## [Unreleased]

### Added
- `y` copies the selected file path to the system clipboard for quick sharing.
- `o` opens the highlighted file in `$QUICKDIFF_EDITOR`, falling back to `$VISUAL` and `$EDITOR`.
- `r` forces a manual reload of the working tree/base file list (or re-renders commit/range diffs).
- `?` shows a built-in help modal listing core keybindings.
- `[` / `]` toggle full-width old/new diff panes for focused reviews.

## [0.7.3] - 2025-01-09

### Changed
- **Git operations migrated to libgit2**: All git subprocess calls replaced with native `git2` crate bindings. Eliminates ~25% overhead from process spawning, provides typed errors, and improves cross-platform reliability.

### Internal
- Test fixtures now use git2 API (~4x faster test execution).
- Parsing functions retained under `#[cfg(test)]` for unit test coverage.

## [0.7.2] - 2025-12-25

### Fixed
- Tabs now render with proper width in diff panes, avoiding overflow on Go files.
- Guard highlight spans against out-of-bounds tree-sitter events.

## [0.7.1] - 2025-12-25

### Added
- **Fuzzy file filtering**: Sidebar filter (`/`) now uses fuzzy matching via nucleo-matcher. Smart case (lowercase = insensitive, mixed case = prefer exact). Results sorted by match score.
- **Sticky scope headers**: Shows enclosing function/class/struct pinned at top of diff panes when scrolled past definition. Supports Rust (fn, impl, struct, mod) and TypeScript/JavaScript (function, class, method).
- **Scroll to first hunk**: Diff view now opens at the first hunk instead of top-of-file, reducing navigation for large files with changes near the end.

### Changed
- Live filtering shows match count in status bar: `Filter: query (N/total)`

## [0.7.0] - 2025-12-25

Initial public release.

### Core
- **TextBuffer**: Arc<[u8]> storage with O(1) line access via precomputed offsets. CRLF normalization, binary detection (NUL in first 8KB), lossy UTF-8 handling.
- **Diff engine**: Line-based diff using `similar` crate. Delete+Insert paired into Replace rows for side-by-side alignment. Configurable context lines.
- **Hunk navigation**: O(log N) binary search for next/prev hunk (`{`/`}`).
- **Viewed state**: Persists to `~/.config/quickdiff/state.json`. Per-repo isolation. Restores last selected file.
- **Git integration**: Shell-out to git with porcelain v1 parsing. Handles renames/copies. Graceful directory/symlink handling.

### Syntax Highlighting
- Tree-sitter based highlighting for Rust, TypeScript, TSX, JavaScript, JSX
- Fallback to plain text for unsupported extensions

### Terminal UI
- Side-by-side diff panes with line numbers
- File sidebar with viewed/unviewed state
- Char-based horizontal scroll (UTF-8 safe)
- Control character sanitization (terminal injection prevention)
- Panic-safe terminal handling (RAII guard + panic hook)
- Word-level inline diff highlighting for Replace rows

### Multiple Diff Sources
- `quickdiff` - working tree vs HEAD (default)
- `quickdiff -c <commit>` - show specific commit
- `quickdiff <from>..<to>` - range comparison
- `quickdiff -b <branch>` - compare against merge-base
- `quickdiff -f <path>` - filter to specific files

### Themes
- 17 builtin themes: default, ayu, catppuccin, dracula, everforest, github, gruvbox, kanagawa, monokai, nightowl, nord, onedark, palenight, rosepine, solarized, tokyonight, zenburn
- Theme switcher (`T`): overlay with live preview
- Custom themes from `~/.config/quickdiff/themes/*.json`
- `--theme/-t` CLI flag

### Comments
- Hunk-level comments (`c` to add, `C` to view overlay, `r` to resolve)
- CLI: `quickdiff comments list|add|resolve`
- Comments stored in `.quickdiff/comments.json` (repo-local)
- Comment counts in sidebar and top bar, marker dots in gutter
- Restricted to worktree mode only

### Interactive Features
- Background diff worker with loading indicator
- Sidebar file filter (`/`) with live filtering
- Auto-advance to next unviewed file on Space
- Hunk position indicator ("hunk N/M" in top bar)
- Mouse support (scroll, click to select/focus)
- File watching with auto-refresh (WorkingTree and Base modes)

### Performance
- Viewport-scoped rendering (sidebar and diff)
- Span coalescing (consecutive same-style chars merged)
- Cached merge-base SHA for base comparisons
- Fast path CRLF normalization
- Binary search for hunk lookups
- Reused tree-sitter highlighter via Mutex

### Key Bindings
| Key | Action |
|-----|--------|
| `j/k` | Navigate files (sidebar) / Scroll vertical (diff) |
| `h/l` | Scroll horizontal (diff) |
| `Enter` | Open diff for selected file |
| `Space` | Toggle viewed + advance to next unviewed |
| `{/}` | Previous/next hunk |
| `Tab` | Switch focus between sidebar and diff |
| `1/2` | Focus sidebar / diff |
| `g/G` | Jump to start/end of diff |
| `/` | Filter files (sidebar) |
| `c` | Add comment on current hunk |
| `C` | View comments overlay |
| `T` | Theme switcher |
| `q` | Quit |

---

[0.7.2]: https://github.com/user/quickdiff/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/user/quickdiff/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/user/quickdiff/releases/tag/v0.7.0
