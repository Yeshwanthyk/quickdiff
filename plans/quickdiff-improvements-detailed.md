# Quickdiff Improvements - Detailed Implementation Plan

**Created:** 2025-01-20
**Status:** approved
**Ticket:** N/A (internal refactor)

## Summary

Targeted refactor and instrumentation based on cross analysis. No feature changes.
- Establish baseline performance metrics
- Reduce render complexity without behavior changes  
- Modularize App state for maintainability
- Cut per-frame allocations in hot paths
- Improve test coverage for critical scenarios
- Document architecture and state flow

## Assumptions

- All changes preserve existing behavior (no user-visible differences)
- Metrics are opt-in via environment variable
- Render split maintains same public API
- App state migration is incremental (one sub-struct at a time)

## Open Questions

None - all resolved during analysis.

---

## Phase 1: Baseline Metrics Instrumentation

**Goal:** Add lightweight timing for render frame and diff compute, gated by `QUICKDIFF_METRICS=1`.

### Changes

#### 1.1 Create `src/metrics.rs` module
- [x] Create new file `src/metrics.rs`:

```rust
//! Optional performance metrics, enabled via QUICKDIFF_METRICS=1.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static METRICS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Initialize metrics from environment. Call once at startup.
pub fn init() {
    let enabled = std::env::var("QUICKDIFF_METRICS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);
    METRICS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if metrics collection is enabled.
#[inline]
pub fn enabled() -> bool {
    METRICS_ENABLED.load(Ordering::Relaxed)
}

/// RAII timer that logs duration on drop.
pub struct Timer {
    label: &'static str,
    start: Instant,
}

impl Timer {
    /// Start a timer if metrics are enabled.
    #[inline]
    pub fn start(label: &'static str) -> Option<Self> {
        if enabled() {
            Some(Self {
                label,
                start: Instant::now(),
            })
        } else {
            None
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        log_metric(self.label, elapsed);
    }
}

/// Log a metric to stderr.
fn log_metric(label: &str, duration: Duration) {
    eprintln!("[metrics] {}: {:?}", label, duration);
}
```

#### 1.2 Add metrics module to lib.rs
- [x] Edit `src/lib.rs` to add:
```rust
pub mod metrics;
```

#### 1.3 Initialize metrics in main.rs
- [x] Edit `src/main.rs` in `main()` function, after arg parsing:
**Before:**
```rust
    // Parse CLI args
    let cli = Cli::parse();
```
**After:**
```rust
    // Parse CLI args
    let cli = Cli::parse();
    
    // Initialize metrics if enabled
    quickdiff::metrics::init();
```

#### 1.4 Instrument render path
- [x] Edit `src/ui/render.rs` at start of `render()` function:
**Before:**
```rust
pub fn render(frame: &mut Frame, app: &mut App) {
    // Fill background
```
**After:**
```rust
pub fn render(frame: &mut Frame, app: &mut App) {
    let _timer = crate::metrics::Timer::start("render_frame");
    // Fill background
```

#### 1.5 Instrument diff computation
- [x] Edit `src/core/diff.rs` at start of `compute()` function:
**Before:**
```rust
    pub fn compute(old: &TextBuffer, new: &TextBuffer) -> Self {
        Self::compute_with_context(old, new, DEFAULT_CONTEXT)
    }
```
**After:**
```rust
    pub fn compute(old: &TextBuffer, new: &TextBuffer) -> Self {
        let _timer = crate::metrics::Timer::start("diff_compute");
        Self::compute_with_context(old, new, DEFAULT_CONTEXT)
    }
```

- [x] Edit `src/core/diff.rs` at start of `compute_with_context()` function:
**Before:**
```rust
    pub fn compute_with_context(old: &TextBuffer, new: &TextBuffer, context: usize) -> Self {
```
**After:**
```rust
    pub fn compute_with_context(old: &TextBuffer, new: &TextBuffer, context: usize) -> Self {
        let _timer = crate::metrics::Timer::start("diff_compute_with_context");
```

### Success Criteria

```bash
# Must pass
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# Manual verification
QUICKDIFF_METRICS=1 cargo run -- 2>&1 | grep -q "\[metrics\]"
```

---

## Phase 2: Render Module Split

**Goal:** Split `src/ui/render.rs` (1951 lines) into organized submodules.

### Target Structure
```
src/ui/render/
├── mod.rs          # Main render + re-exports
├── sidebar.rs      # render_sidebar
├── diff.rs         # render_diff, render_diff_pane
├── overlays.rs     # All overlay renderers
├── bars.rs         # top_bar, bottom_bar
└── helpers.rs      # SpanBuilder, style helpers, constants
```

### Changes

#### 2.1 Create render directory and helpers module
- [x] Create `src/ui/render/helpers.rs`:

```rust
//! Shared rendering helpers and constants.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::highlight::StyleId;
use crate::theme::Theme;

/// Max width for path display in sidebar.
pub const SIDEBAR_PATH_WIDTH: usize = 22;

/// Gutter: 4 (line num) + 2 (separator) = 6 chars
pub const GUTTER_WIDTH: usize = 6;

/// Tab stop width for display alignment.
pub const TAB_WIDTH: usize = 8;

/// Map StyleId to syntax color using theme.
pub fn style_to_color(style: StyleId, theme: &Theme) -> Color {
    match style {
        StyleId::Default => theme.text_normal,
        StyleId::Keyword => theme.syn_keyword,
        StyleId::Type => theme.syn_type,
        StyleId::Function => theme.syn_function,
        StyleId::String => theme.syn_string,
        StyleId::Number => theme.syn_number,
        StyleId::Comment => theme.syn_comment,
        StyleId::Operator => theme.syn_operator,
        StyleId::Punctuation => theme.syn_punctuation,
        StyleId::Variable => theme.text_normal,
        StyleId::Constant => theme.syn_constant,
        StyleId::Property => theme.syn_property,
        StyleId::Attribute => theme.syn_attribute,
    }
}

/// Sanitize control characters.
pub fn sanitize_char(c: char) -> char {
    match c {
        '\x00'..='\x1f' | '\x7f' => '\u{FFFD}',
        _ => c,
    }
}

pub fn tab_width_at(col: usize) -> usize {
    let rem = col % TAB_WIDTH;
    if rem == 0 {
        TAB_WIDTH
    } else {
        TAB_WIDTH - rem
    }
}

pub fn visible_tab_spaces(col: usize, scroll_x: usize, remaining: usize) -> (usize, usize) {
    let width = tab_width_at(col);
    if remaining == 0 {
        return (0, width);
    }

    let skip = scroll_x.saturating_sub(col);
    if skip >= width {
        return (0, width);
    }

    let available = width - skip;
    let take = available.min(remaining);
    (take, width)
}

pub fn is_muted_color(color: Color) -> bool {
    match color {
        Color::Rgb(r, g, b) => {
            let luminance = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let saturation = if max == 0 {
                0
            } else {
                u32::from(max - min) * 100 / u32::from(max)
            };
            luminance < 140 || (luminance < 180 && saturation < 30)
        }
        Color::DarkGray | Color::Gray => true,
        _ => false,
    }
}

pub fn boost_muted_fg(fg: Color, default_fg: Color) -> Color {
    if is_muted_color(fg) {
        default_fg
    } else {
        fg
    }
}

pub fn truncate_str(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else if max_len == 0 {
        String::new()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}

/// Builder for efficient span construction.
pub struct SpanBuilder {
    spans: Vec<Span<'static>>,
    pending_style: Option<Style>,
    pending_text: String,
}

impl SpanBuilder {
    pub fn new() -> Self {
        Self {
            spans: Vec::new(),
            pending_style: None,
            pending_text: String::new(),
        }
    }

    pub fn push_char(&mut self, ch: char, style: Style) {
        if self.pending_style != Some(style) {
            self.flush();
            self.pending_style = Some(style);
        }
        self.pending_text.push(ch);
    }

    pub fn push_spaces(&mut self, count: usize, style: Style) {
        if count == 0 {
            return;
        }
        if self.pending_style != Some(style) {
            self.flush();
            self.pending_style = Some(style);
        }
        self.pending_text.extend(std::iter::repeat(' ').take(count));
    }

    fn flush(&mut self) {
        if !self.pending_text.is_empty() {
            let style = self.pending_style.unwrap_or_default();
            self.spans
                .push(Span::styled(std::mem::take(&mut self.pending_text), style));
        }
    }

    pub fn finish(mut self) -> Vec<Span<'static>> {
        self.flush();
        self.spans
    }
}

impl Default for SpanBuilder {
    fn default() -> Self {
        Self::new()
    }
}
```

#### 2.2 Create bars module
- [x] Create `src/ui/render/bars.rs`:

```rust
//! Top and bottom bar rendering.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::core::FileChangeKind;
use crate::ui::app::{App, Focus, Mode};

/// Render the top bar showing current file info.
pub fn render_top_bar(frame: &mut Frame, app: &App, area: Rect) {
    let file = app.selected_file();
    let file_name = file.map(|f| f.path.as_str()).unwrap_or("No files");

    // Get file change kind for indicator
    let kind_indicator = file.map(|f| match f.kind {
        FileChangeKind::Added => ("A", app.theme.success),
        FileChangeKind::Modified => ("M", app.theme.warning),
        FileChangeKind::Deleted => ("D", app.theme.error),
        FileChangeKind::Untracked => ("?", app.theme.text_muted),
        FileChangeKind::Renamed => ("R", app.theme.accent_dim),
    });

    let mut spans = vec![Span::styled(
        "  ",
        Style::default().bg(app.theme.bg_elevated),
    )];

    // PR badge if in PR mode
    if let Some(ref pr) = app.current_pr {
        spans.push(Span::styled(
            format!(" PR #{} ", pr.number),
            Style::default()
                .fg(app.theme.bg_dark)
                .bg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            " ",
            Style::default().bg(app.theme.bg_elevated),
        ));
    }

    // Change kind badge
    if let Some((kind, color)) = kind_indicator {
        spans.push(Span::styled(
            kind,
            Style::default()
                .fg(color)
                .bg(app.theme.bg_elevated)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            "  ",
            Style::default().bg(app.theme.bg_elevated),
        ));
    }

    // Filename (prominent)
    spans.push(Span::styled(
        file_name,
        Style::default()
            .fg(app.theme.text_bright)
            .bg(app.theme.bg_elevated)
            .add_modifier(Modifier::BOLD),
    ));

    if app.is_current_viewed() {
        spans.push(Span::styled(
            "  ✓ viewed",
            Style::default()
                .fg(app.theme.success)
                .bg(app.theme.bg_elevated),
        ));
    }

    if app.is_binary {
        spans.push(Span::styled(
            "  [binary]",
            Style::default()
                .fg(app.theme.text_muted)
                .bg(app.theme.bg_elevated),
        ));
    }

    // Open comments for this file (only in worktree mode)
    if app.is_worktree_mode() {
        if let Some(file) = file {
            if let Some(count) = app.open_comment_counts.get(&file.path) {
                if *count > 0 {
                    spans.push(Span::styled(
                        format!("  {} comments", count),
                        Style::default()
                            .fg(app.theme.accent)
                            .bg(app.theme.bg_elevated),
                    ));
                }
            }
        }
    }

    // Build right-aligned hunk indicator
    let right_text = app
        .current_hunk_info()
        .map(|(cur, tot)| format!("hunk {}/{}  ", cur, tot))
        .unwrap_or_default();
    let right_len = right_text.chars().count();

    // Pad to fill width, leaving room for right indicator
    let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let total_width = area.width as usize;
    let padding_len = total_width
        .saturating_sub(left_len)
        .saturating_sub(right_len);
    spans.push(Span::styled(
        " ".repeat(padding_len),
        Style::default().bg(app.theme.bg_elevated),
    ));

    // Right-aligned hunk indicator
    if !right_text.is_empty() {
        spans.push(Span::styled(
            right_text,
            Style::default()
                .fg(app.theme.accent)
                .bg(app.theme.bg_elevated),
        ));
    }

    let line = Line::from(spans);
    let para = Paragraph::new(line);
    frame.render_widget(para, area);
}

/// Render the bottom bar with mode-specific hints.
pub fn render_bottom_bar(frame: &mut Frame, app: &App, area: Rect) {
    // Input mode
    if app.mode == Mode::AddComment {
        let line = Line::from(vec![
            Span::styled(
                " Comment: ",
                Style::default()
                    .fg(app.theme.accent)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                &app.draft_comment,
                Style::default()
                    .fg(app.theme.text_bright)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                "█",
                Style::default()
                    .fg(app.theme.accent)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                "  Enter: save  Esc: cancel",
                Style::default()
                    .fg(app.theme.text_muted)
                    .bg(app.theme.bg_elevated),
            ),
        ]);
        let para = Paragraph::new(line).style(Style::default().bg(app.theme.bg_elevated));
        frame.render_widget(para, area);
        return;
    }

    // Filter mode
    if app.mode == Mode::FilterFiles {
        let match_count = if app.sidebar_filter.is_empty() {
            app.files.len()
        } else {
            app.filtered_indices.len()
        };
        let match_info = format!(" ({}/{})", match_count, app.files.len());
        let line = Line::from(vec![
            Span::styled(
                " Filter: ",
                Style::default()
                    .fg(app.theme.accent)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                &app.sidebar_filter,
                Style::default()
                    .fg(app.theme.text_bright)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                "█",
                Style::default()
                    .fg(app.theme.accent)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                match_info,
                Style::default()
                    .fg(app.theme.text_muted)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                "  Enter: apply  Esc: cancel",
                Style::default()
                    .fg(app.theme.text_muted)
                    .bg(app.theme.bg_elevated),
            ),
        ]);
        let para = Paragraph::new(line).style(Style::default().bg(app.theme.bg_elevated));
        frame.render_widget(para, area);
        return;
    }

    // Theme selector mode
    if app.mode == Mode::SelectTheme {
        let line = Line::from(vec![
            Span::styled(
                " Theme ",
                Style::default()
                    .fg(app.theme.accent)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                " j/k: move  Enter: apply  Esc: cancel",
                Style::default()
                    .fg(app.theme.text_muted)
                    .bg(app.theme.bg_elevated),
            ),
        ]);
        let para = Paragraph::new(line).style(Style::default().bg(app.theme.bg_elevated));
        frame.render_widget(para, area);
        return;
    }

    // Comments overlay mode
    if app.mode == Mode::ViewComments {
        let scope = if app.viewing_include_resolved {
            "all"
        } else {
            "open"
        };
        let line = Line::from(vec![
            Span::styled(
                format!(" Comments ({}) ", scope),
                Style::default()
                    .fg(app.theme.accent)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                " j/k: move  Enter: jump  r: resolve  a: all/open  Esc: close",
                Style::default()
                    .fg(app.theme.text_muted)
                    .bg(app.theme.bg_elevated),
            ),
        ]);
        let para = Paragraph::new(line).style(Style::default().bg(app.theme.bg_elevated));
        frame.render_widget(para, area);
        return;
    }

    // Error message
    if let Some(ref err) = app.error_msg {
        let line = Line::from(vec![
            Span::styled(
                " ✗ ",
                Style::default()
                    .fg(app.theme.error)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                err.as_str(),
                Style::default()
                    .fg(app.theme.error)
                    .bg(app.theme.bg_elevated),
            ),
        ]);
        let para = Paragraph::new(line).style(Style::default().bg(app.theme.bg_elevated));
        frame.render_widget(para, area);
        return;
    }

    // Status message
    if let Some(ref msg) = app.status_msg {
        let line = Line::from(vec![
            Span::styled(
                " ✓ ",
                Style::default()
                    .fg(app.theme.success)
                    .bg(app.theme.bg_elevated),
            ),
            Span::styled(
                msg.as_str(),
                Style::default()
                    .fg(app.theme.success)
                    .bg(app.theme.bg_elevated),
            ),
        ]);
        let para = Paragraph::new(line).style(Style::default().bg(app.theme.bg_elevated));
        frame.render_widget(para, area);
        return;
    }

    // Key hints - build dynamically based on mode
    let base_hints: &[(&str, &str)] = match app.focus {
        Focus::Sidebar => &[
            ("j/k", "navigate"),
            ("↵", "open"),
            ("/", "filter"),
            ("␣", "view+next"),
            ("⇥", "switch"),
        ],
        Focus::Diff => &[
            ("j/k", "scroll"),
            ("{/}", "hunks"),
            ("z", "fold"),
            ("c", "comment"),
            ("C", "view"),
            ("␣", "view+next"),
            ("⇥", "switch"),
        ],
    };

    // PR mode hints
    let pr_hints: &[(&str, &str)] = if app.pr_mode {
        &[
            ("A", "approve"),
            ("R", "req-chg"),
            ("O", "open"),
            ("P", "exit PR"),
        ]
    } else {
        &[("P", "PRs")]
    };

    let mut spans = vec![Span::styled(" ", Style::default().bg(app.theme.bg_surface))];

    // Combine base hints + pr hints + quit
    let all_hints = base_hints
        .iter()
        .chain(pr_hints.iter())
        .chain(std::iter::once(&("q", "quit")));

    for (i, (key, desc)) in all_hints.enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                "  ",
                Style::default().bg(app.theme.bg_surface),
            ));
        }
        spans.push(Span::styled(
            *key,
            Style::default()
                .fg(app.theme.accent)
                .bg(app.theme.bg_surface),
        ));
        spans.push(Span::styled(
            format!(" {}", desc),
            Style::default()
                .fg(app.theme.text_dim)
                .bg(app.theme.bg_surface),
        ));
    }

    let line = Line::from(spans);
    let para = Paragraph::new(line).style(Style::default().bg(app.theme.bg_surface));
    frame.render_widget(para, area);
}
```

#### 2.3 Create sidebar module
- [x] Create `src/ui/render/sidebar.rs` (extract `render_sidebar` function - ~120 lines)

#### 2.4 Create diff module
- [x] Create `src/ui/render/diff.rs` (extract `render_diff`, `render_diff_pane`, `render_pane_divider`, and span rendering helpers - ~400 lines)

#### 2.5 Create overlays module
- [x] Create `src/ui/render/overlays.rs` (extract all overlay functions - ~450 lines):
  - `render_comments_overlay`
  - `render_theme_selector`
  - `render_help_overlay`
  - `render_pr_picker_overlay`
  - `render_pr_action_overlay`
  - `styled_filter_tab`

#### 2.6 Create mod.rs that re-exports and contains main render
- [x] Create `src/ui/render/mod.rs`:

```rust
//! UI rendering with ratatui.
//!
//! Design: Themeable dark UI with editorial minimalism.
//! - Muted UI chrome lets syntax highlighting pop
//! - Subtle diff backgrounds don't compete with code colors
//! - Single accent color for focus states

mod bars;
mod diff;
mod helpers;
mod overlays;
mod sidebar;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::Block,
    Frame,
};

use super::app::{App, Mode};

pub use helpers::{SpanBuilder, GUTTER_WIDTH, SIDEBAR_PATH_WIDTH, TAB_WIDTH};

/// Main render function.
pub fn render(frame: &mut Frame, app: &mut App) {
    let _timer = crate::metrics::Timer::start("render_frame");
    
    // Fill background
    let bg_block = Block::default().style(Style::default().bg(app.theme.bg_dark));
    frame.render_widget(bg_block, frame.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top bar
            Constraint::Min(0),    // Main content
            Constraint::Length(1), // Bottom bar
        ])
        .split(frame.area());

    bars::render_top_bar(frame, app, chunks[0]);
    render_main(frame, app, chunks[1]);
    bars::render_bottom_bar(frame, app, chunks[2]);

    // Overlays
    match app.mode {
        Mode::ViewComments => overlays::render_comments_overlay(frame, app),
        Mode::SelectTheme => overlays::render_theme_selector(frame, app),
        Mode::Help => overlays::render_help_overlay(frame, app),
        Mode::PRPicker => overlays::render_pr_picker_overlay(frame, app),
        Mode::PRAction => overlays::render_pr_action_overlay(frame, app),
        _ => {}
    }
}

fn render_main(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(32), // Sidebar
            Constraint::Min(0),     // Diff view
        ])
        .split(area);

    sidebar::render_sidebar(frame, app, chunks[0]);
    diff::render_diff(frame, app, chunks[1]);
}
```

#### 2.7 Update src/ui/mod.rs
- [x] Edit `src/ui/mod.rs`:
**Before:**
```rust
pub mod render;
```
**After:**
```rust
mod render;
pub use render::render;
```

#### 2.8 Delete old render.rs
- [x] Delete `src/ui/render.rs` after all modules are created and verified

### Success Criteria

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# Verify render still works
cargo run -- 2>&1 | head -1  # Should launch TUI
```

---

## Phase 3: App State Modularization

**Goal:** Extract coherent state groups into sub-structs within `App`.

### Target Sub-structs

1. `SidebarState` - sidebar selection, scroll, filter
2. `ViewerState` - diff viewport, scroll positions, view mode
3. `CommentsState` - comment viewing/editing state  
4. `PrState` - PR mode state
5. `UiState` - overlays, mode, messages
6. `WorkerState` - background worker handles and pending requests

### Changes

#### 3.1 Create SidebarState
- [x] Edit `src/ui/app.rs` to add struct before `App`:

```rust
/// Sidebar navigation and filter state.
#[derive(Debug, Default)]
pub struct SidebarState {
    /// Currently selected file index.
    pub selected_idx: usize,
    /// Scroll offset (first visible file).
    pub scroll: usize,
    /// Filter query string.
    pub filter: String,
    /// Filtered file indices (empty = show all).
    pub filtered_indices: Vec<usize>,
}
```

- [x] Migrate fields from `App`:
  - `selected_idx` → `sidebar.selected_idx`
  - `sidebar_scroll` → `sidebar.scroll`
  - `sidebar_filter` → `sidebar.filter`
  - `filtered_indices` → `sidebar.filtered_indices`

- [x] Update all usages (search/replace with verification)

#### 3.2 Create ViewerState  
- [x] Add struct:

```rust
/// Diff viewer viewport state.
#[derive(Debug, Default)]
pub struct ViewerState {
    /// Vertical scroll offset.
    pub scroll_y: usize,
    /// Horizontal scroll offset.
    pub scroll_x: usize,
    /// View mode (hunks-only vs full file).
    pub view_mode: DiffViewMode,
    /// Pane layout mode.
    pub pane_mode: DiffPaneMode,
    /// Precomputed hunk view rows.
    pub hunk_view_rows: Vec<usize>,
}
```

- [x] Migrate: `scroll_y`, `scroll_x`, `diff_view_mode`, `diff_pane_mode`, `hunk_view_rows`

#### 3.3 Create CommentsState
- [x] Add struct:

```rust
/// Comment viewing/editing state.
#[derive(Debug, Default)]
pub struct CommentsState {
    /// Draft comment text.
    pub draft: String,
    /// Comments being viewed.
    pub viewing: Vec<CommentViewItem>,
    /// Selected index in view.
    pub selected: usize,
    /// Scroll offset in view.
    pub scroll: usize,
    /// Include resolved comments.
    pub include_resolved: bool,
}
```

- [x] Migrate relevant fields

#### 3.4 Create PrState
- [x] Add struct:

```rust
/// Pull request mode state.
#[derive(Debug, Default)]
pub struct PrState {
    /// Whether in PR mode.
    pub active: bool,
    /// Current PR.
    pub current: Option<crate::core::PullRequest>,
    /// PR file list.
    pub files: Vec<crate::core::PRChangedFile>,
    /// Available PRs.
    pub list: Vec<crate::core::PullRequest>,
    /// Loading state.
    pub loading: bool,
    /// Filter for PR list.
    pub filter: crate::core::PRFilter,
    /// Picker selection.
    pub picker_selected: usize,
    /// Picker scroll.
    pub picker_scroll: usize,
    /// Action text.
    pub action_text: String,
    /// Action type.
    pub action_type: Option<PRActionType>,
}
```

- [x] Migrate PR-related fields

#### 3.5 Create UiState
- [x] Add struct:

```rust
/// UI mode and message state.
#[derive(Debug, Default)]
pub struct UiState {
    /// Current mode.
    pub mode: Mode,
    /// Error message.
    pub error: Option<String>,
    /// Status message.
    pub status: Option<String>,
    /// Dirty flag for redraw.
    pub dirty: bool,
}
```

- [x] Migrate: `mode`, `error_msg`, `status_msg`, `dirty`

### Success Criteria

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# Line count check (app.rs should be smaller)
wc -l src/ui/app.rs  # Target: <2000 lines
```

---

## Phase 4: Performance Pass

**Goal:** Reduce per-frame allocations in hot paths.

### Changes

#### 4.1 Reuse spaces buffer in render_diff_pane
- [ ] Edit `src/ui/render/diff.rs` - hoist spaces allocation:

**Before (repeated per call):**
```rust
let spaces = " ".repeat(max_content);
```

**After (use thread-local or pass through):**
```rust
// At module level
thread_local! {
    static SPACES_BUF: RefCell<String> = RefCell::new(String::new());
}

fn get_spaces(len: usize) -> String {
    SPACES_BUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        if buf.len() < len {
            buf.clear();
            buf.extend(std::iter::repeat(' ').take(len));
        }
        buf[..len].to_string()
    })
}
```

#### 4.2 Cache truncated filenames
- [ ] Add cache to `SidebarState`:

```rust
/// Cached truncated paths (invalidated on file list change).
pub path_cache: Vec<String>,
```

- [ ] Populate on file list load, reuse in render

#### 4.3 Precompute style lookups
- [ ] Create a `ThemeStyles` struct that caches common style combinations:

```rust
pub struct ThemeStyles {
    pub diff_delete: Style,
    pub diff_insert: Style,
    pub diff_equal: Style,
    pub gutter: Style,
    // ... etc
}

impl ThemeStyles {
    pub fn from_theme(theme: &Theme) -> Self { ... }
}
```

- [ ] Compute once per theme change, use in render

### Success Criteria

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# Performance check with metrics
QUICKDIFF_METRICS=1 cargo run -- 2>&1 | grep render_frame
# Target: render <= 16ms for typical files
```

---

## Phase 5: Tests and Architecture Documentation

**Goal:** Add integration tests and create ARCHITECTURE.md.

### Changes

#### 5.1 Add large file handling test
- [ ] Create `tests/large_file.rs`:

```rust
//! Integration tests for large file handling.

use quickdiff::core::{DiffResult, TextBuffer};

#[test]
fn diff_large_file_completes_in_reasonable_time() {
    use std::time::Instant;
    
    // Generate 10k line file
    let old_content: String = (0..10000)
        .map(|i| format!("line {}\n", i))
        .collect();
    let new_content: String = (0..10000)
        .map(|i| {
            if i == 5000 {
                "modified line\n".to_string()
            } else {
                format!("line {}\n", i)
            }
        })
        .collect();
    
    let old = TextBuffer::new(old_content.as_bytes());
    let new = TextBuffer::new(new_content.as_bytes());
    
    let start = Instant::now();
    let diff = DiffResult::compute(&old, &new);
    let elapsed = start.elapsed();
    
    assert!(elapsed.as_millis() < 500, "Diff took too long: {:?}", elapsed);
    assert!(diff.has_changes());
}
```

#### 5.2 Add binary detection test
- [ ] Add to `src/core/text.rs` tests:

```rust
#[test]
fn binary_detection_nul_in_first_8kb() {
    // NUL in first 8KB = binary
    let mut content = vec![b'a'; 4000];
    content.push(0);
    content.extend(vec![b'b'; 4000]);
    let buf = TextBuffer::new(&content);
    assert!(buf.is_binary());
    
    // NUL after 8KB = not binary (text with trailing garbage)
    let mut content = vec![b'a'; 9000];
    content.push(0);
    let buf = TextBuffer::new(&content);
    assert!(!buf.is_binary());
}
```

#### 5.3 Create ARCHITECTURE.md
- [ ] Create `ARCHITECTURE.md`:

```markdown
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
  - `SidebarState`: file list navigation
  - `ViewerState`: diff viewport
  - `CommentsState`: comment editing
  - `PrState`: PR review mode
  - `UiState`: modes and messages

## Adding Features

### New Language Support
1. Add grammar to `Cargo.toml`
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
```

### Success Criteria

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# Verify new tests run
cargo test large_file
cargo test binary_detection

# Check ARCHITECTURE.md exists and is readable
test -f ARCHITECTURE.md && head -20 ARCHITECTURE.md
```

---

## Rollback Instructions

### Phase 1 (Metrics)
```bash
git checkout HEAD -- src/metrics.rs src/lib.rs src/main.rs src/ui/render.rs src/core/diff.rs
rm -f src/metrics.rs
```

### Phase 2 (Render Split)
```bash
git checkout HEAD -- src/ui/render.rs src/ui/mod.rs
rm -rf src/ui/render/
```

### Phase 3-5
```bash
git checkout HEAD -- src/ui/app.rs
```

---

## Validation Checklist

After all phases:
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] `QUICKDIFF_METRICS=1 cargo run` shows timing output
- [ ] Manual smoke test: sidebar navigation, diff scrolling, hunk jumping
- [ ] `wc -l src/ui/render.rs` shows 0 (file deleted)
- [ ] `wc -l src/ui/app.rs` shows <2000 lines
- [ ] `ARCHITECTURE.md` exists and documents module layout
