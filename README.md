# quickdiff

`quickdiff` is a panic-safe, terminal-native diff viewer focused on fast git/jj reviews. It keeps both panes fully rendered, jumps straight to the first change, and pairs tree-sitter highlighting with a rock-solid TextBuffer so scrolling through large files stays smooth.

## Installation

- Local build: `cargo build --release`
- Install to `$PATH`: `cargo install --path .`
- Update an existing shim (e.g., `~/commands/quickdiff`): `install -m 755 target/release/quickdiff ~/commands/quickdiff`

## Usage

```
quickdiff                     # Working tree vs HEAD/@- (default)
quickdiff -c <commit>         # Inspect a specific commit (git SHA or jj revset)
quickdiff <from>..<to>        # Compare two refs (git or jj)
quickdiff @-..@               # JJ working copy range example
quickdiff -b <branch>         # Diff against merge-base with a branch
quickdiff -f <path>           # Limit to the given files/directories
quickdiff comments list|add|resolve

# Pager mode (stdin)
git diff | quickdiff --stdin  # Render unified diff from pipe
jj diff | quickdiff --stdin   # Works with any diff source

# Web preview
quickdiff web HEAD~1          # Generate standalone HTML
quickdiff web --open          # Open in browser
git diff | quickdiff web --stdin --open
```

### Pager Mode (stdin)

Use `--stdin` to render any unified diff in the TUI:

```bash
# Use as lazygit pager
# ~/.config/lazygit/config.yml
# git:
#   paging:
#     pager: quickdiff --stdin

# Pipe from git/jj
git diff HEAD~3 | quickdiff --stdin
jj diff -r @-..@ | quickdiff --stdin
```

### Web Preview

Generate standalone HTML diffs with `quickdiff web`:

```bash
quickdiff web HEAD~1              # Generate HTML for commit
quickdiff web HEAD~1 --open       # Open in browser
quickdiff web -b main --open      # Diff against main branch
git diff | quickdiff web --stdin  # Generate from piped diff
```

Requires [Bun](https://bun.sh) for HTML rendering.

### Jujutsu (jj) Support

quickdiff auto-detects `.jj` repositories (prefers jj when colocated with `.git`). JJ revsets like `@`, `@-`, bookmarks, and change IDs are accepted for commit/range arguments. Open-ended ranges default to `@-` in jj repos and `HEAD` in git repos. PR mode requires a git repo.

### Watch Mode

Working tree and base comparisons keep a file-system watcher running so diffs refresh as soon as files change. Prefer the automatic updates, but press `r` any time you want to force a manual rescan (commit/range modes refresh just the open diff).

## Feature Highlights

### Navigation & Interaction
- **Fast, always-on rendering:** Diff rows are generated up front, while a background worker streams updates to keep the UI responsive even on huge repos.
- **Sticky context lines:** When you scroll past a function/class/impl boundary, the enclosing scope header pins to the top of each pane so you never lose place.
- **Start at the meat:** Files open with the viewport positioned on the first hunk, so you land where the change actually happened.
- **Hunks-only view (default):** Only hunks with context lines are shown by default—press `z` to toggle full-file view when you need surrounding code.
- **Hunk navigation:** `{` / `}` jump between hunks instantly.
- **Vim-style movement:** `j/k/h/l`, `g/G`, `1/2`, and `Tab` cover navigation plus focus switching between sidebar and diff panes.
- **Space-driven review flow:** `Space` toggles a file’s viewed state and auto-advances to the next unviewed entry.
- **Keyboard _and_ mouse:** Scroll wheels, clicks for focus, and full keyboard control co-exist.
- **Horizontal scrolling:** UTF-8 safe, character-based h-scrolling keeps long lines legible.
- **Single-pane popouts:** `[` shows the old file full-width, `]` does the same for the new file; tap again to return to split view.
- **Keybindings help:** `?` opens a modal cheat sheet without leaving the TUI.
- **Panel control:** `Tab`, `1`, and `2` swap focus instantly, while `[`, `]`, and `/` reshape the layout without leaving the diff.
- **Manual reload anytime:** `r` forces a rescan of the working tree/base list (or replays the open diff for historical views) even though watch mode keeps things live.
- **Clipboard + editor shortcuts:** `y` copies the selected path, while `o` launches `$QUICKDIFF_EDITOR` → `$VISUAL` → `$EDITOR` with the file.
- **Quick help overlay:** Tap `?` for an in-app cheat sheet covering the most important shortcuts.

### File Discovery & Organization
- **Fuzzy file filter:** `/` opens a live filter backed by nucleo-matcher with smart-case scoring and a live `(matches/total)` counter.
- **Sidebar grouping:** Repo-relative paths stay grouped by directory segments, with diff kind + viewed badges so you can scan hierarchies quickly.
- **Toggle focus vs. immersion:** Keep the sidebar visible for context or stay diff-only by focusing pane `2` or popping a single pane full-width.
- **Persistent viewed state:** quickdiff remembers the last selected file and which entries are marked viewed via `~/.config/quickdiff/state.json`.

### Git/JJ Sources & Review Flow
- **Multiple diff sources:** Run against working tree, a specific commit (`-c`), a branch merge-base (`-b`), or any `<from>..<to>` range (jj revsets supported).
- **Watch mode with manual backup:** Working tree and base comparisons auto-refresh on file changes; `r` is there for manual reloads or historical modes.
- **Mark-as-viewed workflow:** `Space` toggles the current file, auto-advances, and persists progress so directories inherit a “done” feel as you work.
- **Commenting workflow:** `c` adds comments, `C` reviews or resolves them, and `quickdiff comments ...` mirrors that from the CLI.
- **Auto-first-hunk & hunk counts:** Each file opens on the first change and shows `hunk N/M` so you always know where you are.
- **Clipboard + editor helpers:** `y` copies the repo-relative path, `o` launches `$QUICKDIFF_EDITOR` → `$VISUAL` → `$EDITOR`, then drops you back into the TUI.
- **Mouse + keyboard parity:** Scroll wheel, clicks, and pointer focus live alongside vim-style bindings for accessibility.

### Diff Quality & Review Aids
- **Word-level inline highlights** for replaced lines keep meaning intact even in dense text blocks.
- **Tree-sitter syntax highlighting** for Rust, TypeScript, TSX, JavaScript, JSX, Go, Python, JSON, YAML, and Bash, with a plain-text fallback for everything else.
- **Sticky scope headers** in both panes plus a **hunk position indicator** (`hunk N/M`) in the top bar.
- **Hunk comments anywhere:** Press `c` to add and `C` to browse/resolve review comments. Comments live in `.quickdiff/comments.json` and are also exposed through the CLI.
- **Word-level inline highlights** and control-char sanitization mean complex replacements remain readable without risking terminal injection.

### Customization
- **Theme switcher (`T`):** 17 built-in themes (ayu, catppuccin, dracula, everforest, github, gruvbox, kanagawa, monokai, nightowl, nord, onedark, palenight, rosepine, solarized, tokyonight, zenburn) with live preview.
- **User themes:** Drop JSON files in `~/.config/quickdiff/themes/` or pass `--theme/-t` to force a scheme per invocation.

### Reliability & Performance
- **TextBuffer power:** Files load into `Arc<[u8]>` storage with CRLF normalization, O(1) line lookup, binary detection, and lossy UTF-8 decoding for malformed blobs.
- **Git/jj integration:** Uses porcelain v1 with `-z`, handles renames/copies, and only loads file contents when selected. JJ uses jj-lib for native access.
- **Panic-safe terminal:** RAII guards restore the terminal even if the app crashes; control characters are sanitized before rendering.
- **Lazy, viewport-scoped rendering:** Only rows that matter for the current scroll position are drawn, minimizing work.
- **File watching:** notify-based watcher keeps the view in sync during active work.

## Everything quickdiff Can Do

- **Fast diffing:** Background workers + viewport rendering keep scrolling silky even on massive repos.
- **Sticky scope headers:** As you move, the enclosing fn/class sticks to the top to anchor context.
- **Vim-style navigation:** `j/k/h/l`, `g/G`, `{`/`}`, `/`, `Tab`, `1`, `2`, `[`, `]`, and `Space` cover seeking, filtering, focus, and fullscreen toggles.
- **Tree-sitter highlighting:** Ten languages today (Rust, TypeScript, TSX, JavaScript, JSX, Go, Python, JSON, YAML, Bash) with plain text fallback.
- **Hunks-only by default:** Only changed regions with context are shown; `z` toggles full-file view when you need more surrounding code.
- **File sidebar:** Directory-grouped list with change badges, viewed ticks, fuzzy filter, and focus toggles for diff-only or split layouts.
- **Watch mode:** Working tree/base modes auto-refresh on file changes; `r` can always force a reload.
- **Git/jj sources:** Inspect uncommitted work, a specific commit, branch comparisons via merge-base, or any `<from>..<to>` revision range.
- **Viewed tracking:** `Space` marks files viewed, persists progress, and auto-advances so directories effectively track review completion.
- **Clipboard/export:** `y` copies the current path; all diff data stays repo-relative for easy sharing.
- **External editor jump:** `o` opens the file in `$QUICKDIFF_EDITOR`/`$VISUAL`/`$EDITOR`, then restores the TUI when you quit.
- **Keyboard + mouse:** Scroll wheels, clicks, and pointer focus work alongside keybindings; even the help modal is keyboard-first (`?`).
- **Horizontal scrolling:** UTF-8 aware left/right motion keeps long lines readable.
- **Manual reload:** `r` refreshes files/diffs instantly—great when watch mode is off or in commit/range views.
- **Panel focus switching:** `Tab`, `1`, and `2` move between sidebar/diff instantly; `[ ]` give you single-pane focus for old/new.
- **Help modal:** `?` shows the full cheat sheet and mouse tips without exiting quickdiff.

## Key Bindings Cheat Sheet

| Key            | Action |
|----------------|--------|
| `j/k`          | Move selection (sidebar) / scroll vertical (diff) |
| `h/l`          | Horizontal scroll in diff |
| `Enter`        | Focus diff |
| `Space`        | Toggle viewed & auto-advance |
| `{` / `}`      | Previous / next hunk |
| `z`            | Toggle hunks-only / full-file view |
| `Tab`, `1`, `2`| Switch focus |
| `g` / `G`      | Jump to start / end |
| `/`            | Open fuzzy filter |
| `[`            | Toggle old diff pane fullscreen |
| `]`            | Toggle new diff pane fullscreen |
| `r`            | Manual reload working tree/base or re-render current diff |
| `y`            | Copy current file path to clipboard |
| `o`            | Open current file in `$EDITOR` |
| `T`            | Theme selector |
| `c` / `C`      | Add / view comments (worktree mode) |
| `?`            | Toggle help overlay |
| `q` or `Ctrl+C`| Quit |

## Development

Keep the loop green with:

```
cargo fmt --check && \
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test
```

Benchmarks live under `benches/`, and additional design docs are in `plans/` and `thoughts/`.

## Roadmap / In Progress

The next milestone is shaped directly by user feedback. If you have a workflow gap or language request, please open an issue so it can be prioritized.
