# quickdiff

A fast, keyboard-driven terminal diff viewer for git and jj repositories.

![quickdiff screenshot](assets/screenshot.png)

## Install

**Homebrew:**
```bash
brew tap Yeshwanthyk/tools
brew install quickdiff
```

**macOS / Linux script:**
```bash
curl -fsSL https://raw.githubusercontent.com/Yeshwanthyk/quickdiff/main/install.sh | sh
```

**With Cargo:**
```bash
cargo install quickdiff
```

**From source:**
```bash
git clone https://github.com/Yeshwanthyk/quickdiff
cd quickdiff
cargo install --path .
```

## Quick Start

```bash
# View uncommitted changes
quickdiff

# View a specific commit
quickdiff HEAD~1
quickdiff abc123

# Compare two commits
quickdiff main..HEAD
quickdiff @-..@          # jj syntax

# Compare against a branch
quickdiff -b main

# Use as a pager
git diff | quickdiff --stdin
jj diff | quickdiff --stdin

# Generate HTML diff
quickdiff web HEAD~1 --open
```

## Usage

```
quickdiff [OPTIONS] [REV]

Arguments:
  [REV]  Revision or range (e.g., HEAD~3, abc123..def456, @-..@)

Options:
  -c, --commit <COMMIT>  Show changes from a specific commit
  -b, --base <BRANCH>    Compare against a base branch (e.g., origin/main)
  -f, --file <PATH>      Filter to specific file(s)
  -t, --theme <THEME>    Color theme
      --stdin            Read unified diff from stdin (pager mode)
      --pr [NUMBER]      Browse GitHub pull requests
      --vcs <TYPE>       Force VCS backend: git or jj (default: auto-detect)
  -h, --help             Print help
  -V, --version          Print version

Subcommands:
  web       Generate standalone HTML diff
  comments  Manage review comments (list, add, import, next, resolve)
```

## Navigation

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate files / scroll diff |
| `h` / `l` | Scroll horizontally |
| `Enter` | Open selected file |
| `Space` | Mark viewed and advance |
| `{` / `}` | Jump to prev/next hunk |
| `g` / `G` | Go to start/end |
| `z` | Toggle full file / hunks only |
| `w` / `n` | Toggle wrap / line numbers |
| `/` | Fuzzy filter files |
| `Tab` / `1` / `2` | Switch focus (sidebar / diff) |
| `s` | Toggle sidebar visibility |
| `[` / `]` | Fullscreen old/new pane |
| `r` | Manual reload |
| `y` | Copy file path |
| `o` | Open in editor |
| `T` | Theme picker |
| `c` / `C` | Add / view review comments (worktree mode) |
| `P` | Open PR picker / exit PR mode |
| `A` / `R` / `O` | Approve / request changes / open PR in browser |
| `?` | Help |
| `q` / `Ctrl+C` | Quit |

## Features

- **Opens at first change** - Jump straight to the first hunk, not the top of file
- **Hunks-only view** - See just the changed sections with context; press `z` to toggle full file
- **Split diff view** - Side-by-side old/new with synchronized scrolling
- **Word-level highlighting** - Inline highlights show exactly what changed within lines
- **Syntax highlighting** - Tree-sitter powered for Rust, TypeScript, Go, Python, and more
- **Sticky headers** - Function/class scope stays pinned while scrolling
- **17 themes** - Press `T` for live preview; set with `--theme`
- **Watch mode** - Auto-refreshes when files change
- **Viewed tracking** - Mark files done with `Space`; state persists across sessions
- **Works with git and jj** - Full support for both, including jj revsets
- **Pager mode** - Pipe any diff: `git diff | quickdiff --stdin`
- **Web export** - Generate standalone HTML: `quickdiff web HEAD~1 --open`

## Themes

Press `T` to open the theme picker, or use `--theme`:

```bash
quickdiff --theme ayu
quickdiff --theme dracula
quickdiff --theme nord
```

Available: ayu, catppuccin, dracula, everforest, github, gruvbox, kanagawa, monokai, nightowl, nord, onedark, palenight, rosepine, solarized, tokyonight, zenburn

Custom themes can be added to `~/.config/quickdiff/themes/`.

## Jujutsu (jj) Support

quickdiff auto-detects jj repositories. All jj revsets work:

```bash
quickdiff @              # Working copy
quickdiff @-             # Parent
quickdiff @-..@          # Working copy changes
quickdiff main..@        # Branch comparison
```

Force a specific backend with `--vcs git` or `--vcs jj` if auto-detection picks the wrong one.

## Pull Request Review

Browse and review GitHub PRs without leaving the terminal (requires the `gh` CLI):

```bash
quickdiff --pr            # Pick from open PRs
quickdiff --pr 123        # Open a specific PR
```

In PR mode, `A` approves, `R` requests changes, and `O` opens the PR in your browser. Press `P` again to exit PR mode.

## Review Comments

Leave inline comments on a diff and manage them from the CLI:

```bash
quickdiff comments list --all
quickdiff comments add --path src/main.rs --new-line 42 --message "nit: rename this"
quickdiff comments next            # Jump to the next unresolved comment
quickdiff comments resolve <id>
quickdiff comments import --json review.json
```

In the TUI (worktree mode), press `c` to add a comment on the current line and `C` to view existing comments.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## License

MIT
