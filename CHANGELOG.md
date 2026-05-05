# Changelog

All notable changes to quickdiff are documented here.

## [Unreleased]

## [0.8.1] - 2025-05-05

### Changed
- Bumped pinned Rust toolchain to 1.89 (required by `jj-lib` 0.36 and `Cargo.lock` v4); release CI on the previous 1.75 pin could no longer parse the lockfile.
- Homebrew formula update is now chained into the release workflow so tag pushes publish binaries and refresh the Homebrew tap in a single run. The standalone `update-homebrew` workflow is kept for manual recovery only.

### Fixed
- Replaced `repeat().take()` with `repeat_n()` in the diff renderer to satisfy the new clippy lint surfaced by the toolchain bump.
- Documented `--vcs`, missing keybindings (`w`, `n`, `s`, `r`, `1`/`2`, `c`/`C`, `P`, `A`/`R`/`O`), the PR review workflow, and the `comments` subcommand in the README.

## [0.8.0] - 2025-05-03

### Added
- `--vcs <TYPE>` flag to force the git or jj backend instead of relying on auto-detection.
- `s` toggles sidebar visibility for a wider diff pane.
- Review comments: `comments import --json` for bulk import, `comments next` to jump to the next unresolved comment, and hunk-digest indexing so comments survive minor diff churn.
- Improved comment editing UI in the TUI.
- Foundations for user preferences, pager mode, and standalone file comparison.
- Repository lint policy (`clippy.toml`, `deny.toml`) and slop-scan rules for ongoing code health.

### Changed
- Split diff collapses to a single pane on narrow terminals.
- General polish to the diff pane rendering.

## [0.7.5] - 2025-01-12

### Added
- **Hunks-only view (default)**: Diff pane now shows only hunks with context lines by default—no more scrolling through unchanged code. Press `z` to toggle between hunks-only and full-file view.
- Inline diff uses unicode word boundaries for better accuracy on non-ASCII identifiers.
- Similarity gate: inline highlights are suppressed when lines share <20% content (completely different lines show as plain delete/insert).

### Changed
- Muted syntax colors in inline diff highlights are boosted to default text color for better contrast against accent backgrounds.

## [0.7.4] - 2025-01-11

### Added
- Jujutsu (jj) repository support with auto-detection, jj revsets, and jj-backed diffs.
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
