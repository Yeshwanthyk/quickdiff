//! UI rendering with ratatui.
//!
//! Design: Themeable dark UI with editorial minimalism.
//! - Muted UI chrome lets syntax highlighting pop
//! - Subtle diff backgrounds don't compete with code colors
//! - Single accent color for focus states

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::core::{ChangeKind, CommentStatus, FileChangeKind, InlineSpan, ViewedStore};
use crate::highlight::{find_enclosing_scope, ScopeInfo, StyleId, StyledSpan};
use crate::theme::Theme;

use super::app::{App, DiffPaneMode, Focus, Mode, PRActionType};

// ============================================================================
// Constants
// ============================================================================

/// Max width for path display in sidebar.
const SIDEBAR_PATH_WIDTH: usize = 22;

// ============================================================================
// Syntax Highlighting
// ============================================================================

/// Map StyleId to syntax color using theme.
fn style_to_color(style: StyleId, theme: &Theme) -> Color {
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

// ============================================================================
// Main Render
// ============================================================================

/// Main render function.
pub fn render(frame: &mut Frame, app: &mut App) {
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

    render_top_bar(frame, app, chunks[0]);
    render_main(frame, app, chunks[1]);
    render_bottom_bar(frame, app, chunks[2]);

    // Overlay for comments mode
    if app.mode == Mode::ViewComments {
        render_comments_overlay(frame, app);
    }

    // Overlay for theme selector
    if app.mode == Mode::SelectTheme {
        render_theme_selector(frame, app);
    }

    if app.mode == Mode::Help {
        render_help_overlay(frame, app);
    }

    if app.mode == Mode::PRPicker {
        render_pr_picker_overlay(frame, app);
    }

    if app.mode == Mode::PRAction {
        render_pr_action_overlay(frame, app);
    }
}

// ============================================================================
// Top Bar
// ============================================================================

fn render_top_bar(frame: &mut Frame, app: &App, area: Rect) {
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

// ============================================================================
// Main Layout
// ============================================================================

fn render_main(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(32), // Sidebar
            Constraint::Min(0),     // Diff view
        ])
        .split(area);

    render_sidebar(frame, app, chunks[0]);
    render_diff(frame, app, chunks[1]);
}

// ============================================================================
// Sidebar
// ============================================================================

fn render_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::Sidebar;

    let border_color = if is_focused {
        app.theme.accent
    } else {
        app.theme.border_dim
    };
    let title_style = if is_focused {
        Style::default().fg(app.theme.accent)
    } else {
        Style::default().fg(app.theme.text_muted)
    };

    // Show filter indicator in title if active
    let title = if app.filtered_indices.is_empty() {
        format!(" Files ({}) ", app.viewed_status())
    } else {
        format!(
            " Files ({}) [filter: {}] ",
            app.filtered_indices.len(),
            app.sidebar_filter
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(title, title_style))
        .style(Style::default().bg(app.theme.bg_surface));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = inner.height as usize;
    if height == 0 {
        return;
    }

    // Get visible file indices (filtered or all)
    let visible_indices: Vec<usize> = if app.filtered_indices.is_empty() {
        (0..app.files.len()).collect()
    } else {
        app.filtered_indices.clone()
    };

    if visible_indices.is_empty() {
        let msg = if app.files.is_empty() {
            "No files"
        } else {
            "No matches"
        };
        let para = Paragraph::new(msg).style(
            Style::default()
                .fg(app.theme.text_muted)
                .bg(app.theme.bg_surface),
        );
        frame.render_widget(para, inner);
        return;
    }

    // Find position of selected_idx in visible list
    let selected_pos = visible_indices
        .iter()
        .position(|&idx| idx == app.selected_idx)
        .unwrap_or(0);

    // Keep selection visible
    let max_scroll = visible_indices.len().saturating_sub(height);
    app.sidebar_scroll = app.sidebar_scroll.min(max_scroll);

    if selected_pos < app.sidebar_scroll {
        app.sidebar_scroll = selected_pos;
    } else if selected_pos >= app.sidebar_scroll + height {
        app.sidebar_scroll = selected_pos + 1 - height;
    }

    let end = (app.sidebar_scroll + height).min(visible_indices.len());

    let mut lines: Vec<Line> = Vec::with_capacity(height);
    for &idx in visible_indices
        .iter()
        .skip(app.sidebar_scroll)
        .take(end - app.sidebar_scroll)
    {
        let file = &app.files[idx];
        let is_selected = idx == app.selected_idx;
        let is_viewed = app.viewed.is_viewed(&file.path);

        let row_bg = if is_selected {
            app.theme.bg_selected
        } else {
            app.theme.bg_surface
        };

        // Selection indicator (left edge)
        let select_indicator = if is_selected { "▌" } else { " " };
        let select_style = Style::default()
            .fg(if is_selected {
                app.theme.accent
            } else {
                row_bg
            })
            .bg(row_bg);

        // Change kind indicator with color
        let (kind_char, kind_color) = match file.kind {
            FileChangeKind::Added => ('A', app.theme.success),
            FileChangeKind::Modified => ('M', app.theme.warning),
            FileChangeKind::Deleted => ('D', app.theme.error),
            FileChangeKind::Untracked => ('?', app.theme.text_muted),
            FileChangeKind::Renamed => ('R', app.theme.accent_dim),
        };

        let viewed_char = if is_viewed { '✓' } else { '·' };
        let viewed_color = if is_viewed {
            app.theme.success
        } else {
            app.theme.text_faint
        };

        // Comment counts only shown in worktree mode
        let (comment_text, comment_color) = if app.is_worktree_mode() {
            let comment_count = app
                .open_comment_counts
                .get(&file.path)
                .copied()
                .unwrap_or(0);
            if comment_count == 0 {
                ("  ".to_string(), app.theme.text_faint)
            } else if comment_count < 10 {
                (format!(" {}", comment_count), app.theme.accent)
            } else {
                ("9+".to_string(), app.theme.accent)
            }
        } else {
            ("  ".to_string(), app.theme.text_faint)
        };

        // Ellipsize path
        let path = file.path.as_str();
        let display_path = if path.len() > SIDEBAR_PATH_WIDTH {
            format!("…{}", &path[path.len() - SIDEBAR_PATH_WIDTH + 1..])
        } else {
            path.to_string()
        };

        // Text brightness based on state
        let text_color = if is_selected {
            app.theme.text_bright
        } else if is_viewed {
            app.theme.text_dim // Viewed files are dimmer
        } else {
            app.theme.text_normal
        };

        lines.push(Line::from(vec![
            Span::styled(select_indicator, select_style),
            Span::styled(
                kind_char.to_string(),
                Style::default().fg(kind_color).bg(row_bg),
            ),
            Span::styled(" ", Style::default().bg(row_bg)),
            Span::styled(
                viewed_char.to_string(),
                Style::default().fg(viewed_color).bg(row_bg),
            ),
            Span::styled(" ", Style::default().bg(row_bg)),
            Span::styled(comment_text, Style::default().fg(comment_color).bg(row_bg)),
            Span::styled(" ", Style::default().bg(row_bg)),
            Span::styled(display_path, Style::default().fg(text_color).bg(row_bg)),
        ]));
    }

    while lines.len() < height {
        lines.push(Line::from(Span::styled(
            "",
            Style::default().bg(app.theme.bg_surface),
        )));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(app.theme.bg_surface));
    frame.render_widget(para, inner);
}

// ============================================================================
// Diff View
// ============================================================================

fn render_diff(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Diff;
    let border_color = if is_focused {
        app.theme.accent
    } else {
        app.theme.border_dim
    };
    let title_style = if is_focused {
        Style::default().fg(app.theme.accent)
    } else {
        Style::default().fg(app.theme.text_muted)
    };

    let title = match app.diff_pane_mode {
        DiffPaneMode::Both => " Diff ",
        DiffPaneMode::OldOnly => " Diff (old only) ",
        DiffPaneMode::NewOnly => " Diff (new only) ",
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(title, title_style))
        .style(Style::default().bg(app.theme.bg_dark));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Handle empty states
    if app.loading {
        let msg = Paragraph::new("Loading diff…").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(msg, inner);
        return;
    }

    if app.is_binary {
        let msg = Paragraph::new("Binary file — cannot display diff")
            .style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(msg, inner);
        return;
    }

    let Some(diff) = &app.diff else {
        let msg =
            Paragraph::new("No diff to display").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(msg, inner);
        return;
    };

    if diff.rows().is_empty() || !diff.has_changes() {
        let msg =
            Paragraph::new("Files are identical").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(msg, inner);
        return;
    }

    match app.diff_pane_mode {
        DiffPaneMode::Both => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Length(1),
                    Constraint::Percentage(50),
                ])
                .split(inner);
            render_diff_pane(frame, app, panes[0], true);
            render_pane_divider(frame, panes[1], &app.theme);
            render_diff_pane(frame, app, panes[2], false);
        }
        DiffPaneMode::OldOnly => {
            render_diff_pane(frame, app, inner, true);
        }
        DiffPaneMode::NewOnly => {
            render_diff_pane(frame, app, inner, false);
        }
    }
}

/// Render vertical divider between old/new panes.
fn render_pane_divider(frame: &mut Frame, area: Rect, theme: &Theme) {
    let lines: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(theme.pane_divider))))
        .collect();
    let para = Paragraph::new(lines).style(Style::default().bg(theme.bg_dark));
    frame.render_widget(para, area);
}

/// Gutter: 4 (line num) + 2 (separator) = 6 chars
const GUTTER_WIDTH: usize = 6;
/// Tab stop width for display alignment.
const TAB_WIDTH: usize = 8;

fn tab_width_at(col: usize) -> usize {
    let rem = col % TAB_WIDTH;
    if rem == 0 {
        TAB_WIDTH
    } else {
        TAB_WIDTH - rem
    }
}

fn visible_tab_spaces(col: usize, scroll_x: usize, remaining: usize) -> (usize, usize) {
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

struct SpanBuilder {
    spans: Vec<Span<'static>>,
    pending_style: Option<Style>,
    pending_text: String,
}

impl SpanBuilder {
    fn new() -> Self {
        Self {
            spans: Vec::new(),
            pending_style: None,
            pending_text: String::new(),
        }
    }

    fn push_char(&mut self, ch: char, style: Style) {
        if self.pending_style != Some(style) {
            self.flush();
            self.pending_style = Some(style);
        }
        self.pending_text.push(ch);
    }

    fn push_spaces(&mut self, count: usize, style: Style) {
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

    fn finish(mut self) -> Vec<Span<'static>> {
        self.flush();
        self.spans
    }
}

/// Compute the sticky scope for a pane based on the first visible source line.
fn compute_sticky_scope<'a>(
    diff: &crate::core::DiffResult,
    scopes: &'a [ScopeInfo],
    scroll_y: usize,
    is_old: bool,
) -> Option<&'a ScopeInfo> {
    // Get the first visible row to find the source line number
    let first_row = diff.render_rows(scroll_y, 1).next()?;

    // Get the source line number for this pane
    let line_num = if is_old {
        first_row.old.as_ref()?.line_num
    } else {
        first_row.new.as_ref()?.line_num
    };

    // Find enclosing scope
    let scope = find_enclosing_scope(scopes, line_num)?;

    // Only show if we've scrolled past the definition line
    if line_num > scope.start_line {
        Some(scope)
    } else {
        None
    }
}

fn render_plain_span(
    builder: &mut SpanBuilder,
    text: &str,
    style: Style,
    scroll_x: usize,
    max_content: usize,
    col_pos: &mut usize,
    visible_len: &mut usize,
) {
    if max_content == 0 {
        return;
    }

    for ch in text.chars() {
        if *visible_len >= max_content {
            break;
        }

        if ch == '\t' {
            let remaining = max_content.saturating_sub(*visible_len);
            let (emit, advance) = visible_tab_spaces(*col_pos, scroll_x, remaining);
            if emit > 0 {
                builder.push_spaces(emit, style);
                *visible_len += emit;
            }
            *col_pos += advance;
            continue;
        }

        let ch = sanitize_char(ch);
        if *col_pos >= scroll_x {
            if *visible_len + 1 > max_content {
                break;
            }
            builder.push_char(ch, style);
            *visible_len += 1;
        }
        *col_pos += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn render_inline_span(
    builder: &mut SpanBuilder,
    text: &str,
    base_style: Style,
    bg_color: Color,
    inline_bg: Color,
    inline_spans: &[InlineSpan],
    scroll_x: usize,
    max_content: usize,
    col_pos: &mut usize,
    visible_len: &mut usize,
    start_byte: usize,
) {
    if max_content == 0 {
        return;
    }

    let mut byte_offset = start_byte;

    for ch in text.chars() {
        if *visible_len >= max_content {
            break;
        }

        let is_changed = inline_spans
            .iter()
            .any(|s| s.changed && byte_offset >= s.start && byte_offset < s.end);
        let bg = if is_changed { inline_bg } else { bg_color };
        let style = base_style.bg(bg);

        if ch == '\t' {
            let remaining = max_content.saturating_sub(*visible_len);
            let (emit, advance) = visible_tab_spaces(*col_pos, scroll_x, remaining);
            if emit > 0 {
                builder.push_spaces(emit, style);
                *visible_len += emit;
            }
            *col_pos += advance;
        } else {
            let ch = sanitize_char(ch);
            if *col_pos >= scroll_x {
                if *visible_len + 1 > max_content {
                    break;
                }
                builder.push_char(ch, style);
                *visible_len += 1;
            }
            *col_pos += 1;
        }

        byte_offset += ch.len_utf8();
    }
}

fn render_diff_pane(frame: &mut Frame, app: &App, area: Rect, is_old: bool) {
    let Some(diff) = &app.diff else {
        return;
    };

    // Check for sticky scope
    let scopes = if is_old {
        &app.old_scopes
    } else {
        &app.new_scopes
    };
    let sticky_scope = compute_sticky_scope(diff, scopes, app.scroll_y, is_old);
    let has_sticky = sticky_scope.is_some();

    // Reserve 1 line for sticky header if present
    let sticky_height = if has_sticky { 1 } else { 0 };
    let content_height = (area.height as usize).saturating_sub(sticky_height);
    let max_content = (area.width as usize).saturating_sub(GUTTER_WIDTH);
    let spaces = " ".repeat(max_content);

    struct RenderedLine {
        line_num_str: String,
        bg_color: Color,
        code_spans: Vec<Span<'static>>,
        visible_len: usize,
        has_comment: bool,
    }

    // First pass: build spans and compute max visible width in this viewport.
    let mut rendered: Vec<RenderedLine> = Vec::with_capacity(content_height);
    let mut max_visible_len = 0usize;

    for (offset, row) in diff.render_rows(app.scroll_y, content_height).enumerate() {
        let row_idx = app.scroll_y + offset;
        let has_comment = app.is_worktree_mode()
            && diff
                .hunk_at_row(row_idx)
                .is_some_and(|h| app.commented_hunks.contains(&h));
        // Extract line info based on pane side
        let (line_ref, bg_color, inline_bg) = if is_old {
            match (&row.old, row.kind) {
                (Some(line), ChangeKind::Equal) => {
                    (Some(line), app.theme.bg_dark, app.theme.bg_dark)
                }
                (Some(line), ChangeKind::Delete) | (Some(line), ChangeKind::Replace) => (
                    Some(line),
                    app.theme.diff_delete_bg,
                    app.theme.inline_delete_bg,
                ),
                (None, ChangeKind::Insert) => {
                    (None, app.theme.diff_empty_bg, app.theme.diff_empty_bg)
                }
                _ => (None, app.theme.bg_dark, app.theme.bg_dark),
            }
        } else {
            match (&row.new, row.kind) {
                (Some(line), ChangeKind::Equal) => {
                    (Some(line), app.theme.bg_dark, app.theme.bg_dark)
                }
                (Some(line), ChangeKind::Insert) | (Some(line), ChangeKind::Replace) => (
                    Some(line),
                    app.theme.diff_insert_bg,
                    app.theme.inline_insert_bg,
                ),
                (None, ChangeKind::Delete) => {
                    (None, app.theme.diff_empty_bg, app.theme.diff_empty_bg)
                }
                _ => (None, app.theme.bg_dark, app.theme.bg_dark),
            }
        };

        let line_idx = line_ref.map(|l| l.line_num);
        let line_num = line_idx.map(|n| n + 1);
        let content = line_ref.map(|l| l.content.as_str()).unwrap_or("");
        let inline_spans = line_ref.and_then(|l| l.inline_spans.as_ref());

        let line_num_str = line_num
            .map(|n| format!("{:>4}", n))
            .unwrap_or_else(|| "    ".to_string());

        // Build syntax-highlighted content spans with truncation
        let mut builder = SpanBuilder::new();
        let mut visible_len = 0usize;

        if !content.is_empty() && max_content > 0 {
            let default_span = StyledSpan {
                start: 0,
                end: content.len(),
                style_id: StyleId::Default,
            };

            let cached_spans = match (is_old, line_idx) {
                (true, Some(idx)) => app.old_highlights.line_spans(idx),
                (false, Some(idx)) => app.new_highlights.line_spans(idx),
                _ => None,
            };

            let hl_spans = match cached_spans {
                Some(spans) if !spans.is_empty() => spans,
                _ => std::slice::from_ref(&default_span),
            };

            let scroll_x = app.scroll_x;
            let mut col_pos = 0usize;

            for hl in hl_spans {
                if visible_len >= max_content {
                    break;
                }

                let span_text = content.get(hl.start..hl.end).unwrap_or("");
                if span_text.is_empty() {
                    continue;
                }

                let fg = style_to_color(hl.style_id, &app.theme);

                // Check if this syntax span overlaps with any changed inline regions
                let has_inline_changes = inline_spans.is_some_and(|spans| {
                    spans
                        .iter()
                        .any(|s| s.changed && s.start < hl.end && s.end > hl.start)
                });

                if !has_inline_changes {
                    let style = Style::default().fg(fg).bg(bg_color);
                    render_plain_span(
                        &mut builder,
                        span_text,
                        style,
                        scroll_x,
                        max_content,
                        &mut col_pos,
                        &mut visible_len,
                    );
                } else if let Some(spans) = inline_spans {
                    let base_style = Style::default().fg(fg);
                    render_inline_span(
                        &mut builder,
                        span_text,
                        base_style,
                        bg_color,
                        inline_bg,
                        spans,
                        scroll_x,
                        max_content,
                        &mut col_pos,
                        &mut visible_len,
                        hl.start,
                    );
                }

                if visible_len >= max_content {
                    break;
                }
            }
        }

        let code_spans = builder.finish();

        max_visible_len = max_visible_len.max(visible_len);
        rendered.push(RenderedLine {
            line_num_str,
            bg_color,
            code_spans,
            visible_len,
            has_comment,
        });
    }

    // For the pane with the gutter on the right (old/left), shift the whole code block right
    // uniformly based on the widest visible line in this viewport. This keeps code "right
    // adjusted" without per-line right-justification that breaks indentation.
    let common_left_pad = if is_old {
        max_content.saturating_sub(max_visible_len)
    } else {
        0
    };

    let total_height = if has_sticky {
        content_height + 1
    } else {
        content_height
    };
    let mut lines: Vec<Line> = Vec::with_capacity(total_height);

    // Render sticky header if present
    if let Some(scope) = sticky_scope {
        let sticky_bg = app.theme.bg_elevated;
        let mut spans: Vec<Span> = Vec::new();

        // Build display text: "kind name" (e.g., "fn compute_diff")
        let display_text = if scope.name.is_empty() {
            scope.kind.to_string()
        } else {
            format!("{} {}", scope.kind, scope.name)
        };

        if is_old {
            // OLD pane: right-align the sticky text
            let text_len = display_text.chars().count();
            let padding = max_content.saturating_sub(text_len);
            if padding > 0 {
                spans.push(Span::styled(
                    &spaces[..padding],
                    Style::default().bg(sticky_bg),
                ));
            }
            spans.push(Span::styled(
                display_text,
                Style::default()
                    .fg(app.theme.text_muted)
                    .bg(sticky_bg)
                    .add_modifier(Modifier::ITALIC),
            ));
            // Gutter
            spans.push(Span::styled(" ", Style::default().bg(sticky_bg)));
            spans.push(Span::styled(
                "│",
                Style::default().fg(app.theme.gutter_sep).bg(sticky_bg),
            ));
            spans.push(Span::styled("    ", Style::default().bg(sticky_bg)));
        } else {
            // NEW pane: left-align with gutter
            spans.push(Span::styled("    ", Style::default().bg(sticky_bg)));
            spans.push(Span::styled(
                "│",
                Style::default().fg(app.theme.gutter_sep).bg(sticky_bg),
            ));
            spans.push(Span::styled(" ", Style::default().bg(sticky_bg)));
            spans.push(Span::styled(
                display_text.clone(),
                Style::default()
                    .fg(app.theme.text_muted)
                    .bg(sticky_bg)
                    .add_modifier(Modifier::ITALIC),
            ));
            // Pad to fill width
            let text_len = display_text.chars().count();
            let trailing = max_content.saturating_sub(text_len);
            if trailing > 0 {
                spans.push(Span::styled(
                    &spaces[..trailing],
                    Style::default().bg(sticky_bg),
                ));
            }
        }

        lines.push(Line::from(spans));
    }

    for row in rendered {
        // Build the line with pane-specific layout:
        // OLD (left):  [pad][code][pad] │[line_num] - right-adjusted block, gutter on right
        // NEW (right): [line_num]│ [code][pad]      - left-aligned, gutter on left
        let mut spans: Vec<Span> = Vec::new();

        if is_old {
            if common_left_pad > 0 {
                spans.push(Span::styled(
                    &spaces[..common_left_pad],
                    Style::default().bg(row.bg_color),
                ));
            }
            spans.extend(row.code_spans);
            let trailing = max_content
                .saturating_sub(common_left_pad)
                .saturating_sub(row.visible_len);
            if trailing > 0 {
                spans.push(Span::styled(
                    &spaces[..trailing],
                    Style::default().bg(row.bg_color),
                ));
            }
            let marker_char = if row.has_comment { "•" } else { " " };
            let marker_style = if row.has_comment {
                Style::default().fg(app.theme.accent).bg(row.bg_color)
            } else {
                Style::default().bg(row.bg_color)
            };
            spans.push(Span::styled(marker_char, marker_style));
            spans.push(Span::styled(
                "│",
                Style::default().fg(app.theme.gutter_sep).bg(row.bg_color),
            ));
            spans.push(Span::styled(
                row.line_num_str,
                Style::default().fg(app.theme.text_faint).bg(row.bg_color),
            ));
        } else {
            spans.push(Span::styled(
                row.line_num_str,
                Style::default().fg(app.theme.text_faint).bg(row.bg_color),
            ));
            spans.push(Span::styled(
                "│",
                Style::default().fg(app.theme.gutter_sep).bg(row.bg_color),
            ));
            let marker_char = if row.has_comment { "•" } else { " " };
            let marker_style = if row.has_comment {
                Style::default().fg(app.theme.accent).bg(row.bg_color)
            } else {
                Style::default().bg(row.bg_color)
            };
            spans.push(Span::styled(marker_char, marker_style));
            spans.extend(row.code_spans);
            let trailing = max_content.saturating_sub(row.visible_len);
            if trailing > 0 {
                spans.push(Span::styled(
                    &spaces[..trailing],
                    Style::default().bg(row.bg_color),
                ));
            }
        }

        lines.push(Line::from(spans));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Sanitize control characters.
fn sanitize_char(c: char) -> char {
    match c {
        '\x00'..='\x1f' | '\x7f' => '\u{FFFD}',
        _ => c,
    }
}

// ============================================================================
// Comments Overlay
// ============================================================================

fn render_comments_overlay(frame: &mut Frame, app: &App) {
    use ratatui::widgets::Clear;

    let area = frame.area();

    let max_overlay_width = area.width.saturating_sub(4).max(1);
    let min_overlay_width = 40.min(max_overlay_width);
    let max_overlay_width = 70.min(max_overlay_width);
    let width = (area.width * 3 / 4).clamp(min_overlay_width, max_overlay_width);

    let max_overlay_height = area.height.saturating_sub(4).max(1);
    let min_overlay_height = 6.min(max_overlay_height);
    let height =
        (app.viewing_comments.len() as u16 + 4).clamp(min_overlay_height, max_overlay_height);

    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let title = if app.viewing_include_resolved {
        " Comments (all) "
    } else {
        " Comments (open) "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.accent))
        .title(Span::styled(
            title,
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(app.theme.bg_elevated));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let mut lines: Vec<Line> = Vec::new();

    if inner.height == 0 {
        return;
    }

    if app.viewing_comments.is_empty() {
        lines.push(Line::from(Span::styled(
            "No comments",
            Style::default()
                .fg(app.theme.text_muted)
                .bg(app.theme.bg_elevated),
        )));
    } else {
        let total = app.viewing_comments.len();
        let selected = app.viewing_comments_selected.min(total - 1);
        let visible = inner.height as usize;

        let scroll = (selected + 1).saturating_sub(visible);

        let end = (scroll + visible).min(total);

        for idx in scroll..end {
            let item = &app.viewing_comments[idx];
            let is_selected = idx == selected;

            let row_bg = if is_selected {
                app.theme.bg_selected
            } else {
                app.theme.bg_elevated
            };
            let msg_color = if item.status == CommentStatus::Resolved {
                app.theme.text_dim
            } else {
                app.theme.text_normal
            };

            let status_char = if item.status == CommentStatus::Resolved {
                "✓"
            } else {
                " "
            };

            let mut spans: Vec<Span> = Vec::new();
            spans.push(Span::styled(
                if is_selected { "▌" } else { " " },
                Style::default().fg(app.theme.accent).bg(row_bg),
            ));
            spans.push(Span::styled(
                format!("#{} ", item.id),
                Style::default()
                    .fg(app.theme.accent)
                    .bg(row_bg)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                status_char,
                Style::default().fg(app.theme.success).bg(row_bg),
            ));
            spans.push(Span::styled(" ", Style::default().bg(row_bg)));
            spans.push(Span::styled(
                item.anchor_summary.as_str(),
                Style::default().fg(app.theme.text_muted).bg(row_bg),
            ));
            if item.stale {
                spans.push(Span::styled(
                    " [stale]",
                    Style::default().fg(app.theme.warning).bg(row_bg),
                ));
            }
            spans.push(Span::styled(
                " - ",
                Style::default().fg(app.theme.text_muted).bg(row_bg),
            ));
            spans.push(Span::styled(
                item.message.as_str(),
                Style::default().fg(msg_color).bg(row_bg),
            ));

            lines.push(Line::from(spans));
        }
    }

    while lines.len() < inner.height as usize {
        lines.push(Line::from(Span::styled(
            "",
            Style::default().bg(app.theme.bg_elevated),
        )));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(app.theme.bg_elevated));
    frame.render_widget(para, inner);
}

// ============================================================================
// Theme Selector
// ============================================================================

fn render_theme_selector(frame: &mut Frame, app: &App) {
    use ratatui::widgets::Clear;

    let area = frame.area();

    // Size the overlay
    let width = 30.min(area.width.saturating_sub(4));
    let height = (app.theme_list.len() as u16 + 2).min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.accent))
        .title(Span::styled(
            " Theme ",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(app.theme.bg_elevated));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let visible_height = inner.height as usize;
    if visible_height == 0 {
        return;
    }

    // Scroll to keep selection visible
    let scroll = app
        .theme_selector_idx
        .saturating_sub(visible_height.saturating_sub(1));

    let mut lines: Vec<Line> = Vec::new();

    for (i, theme_name) in app
        .theme_list
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
    {
        let is_selected = i == app.theme_selector_idx;
        let row_bg = if is_selected {
            app.theme.bg_selected
        } else {
            app.theme.bg_elevated
        };
        let text_color = if is_selected {
            app.theme.text_bright
        } else {
            app.theme.text_normal
        };

        let indicator = if is_selected { "▌" } else { " " };

        lines.push(Line::from(vec![
            Span::styled(indicator, Style::default().fg(app.theme.accent).bg(row_bg)),
            Span::styled(
                format!(" {}", theme_name),
                Style::default().fg(text_color).bg(row_bg),
            ),
        ]));
    }

    // Pad remaining lines
    while lines.len() < visible_height {
        lines.push(Line::from(Span::styled(
            "",
            Style::default().bg(app.theme.bg_elevated),
        )));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(app.theme.bg_elevated));
    frame.render_widget(para, inner);
}

// ============================================================================
// Help Overlay
// ============================================================================

fn render_help_overlay(frame: &mut Frame, app: &App) {
    use ratatui::widgets::Clear;

    let entries = [
        ("j/k or ↑/↓", "Navigate files / scroll vertically"),
        ("h/l or ←/→", "Scroll horizontally in diff"),
        ("g / G", "Jump to start / end of file"),
        ("Tab, 1, 2", "Switch focus between sidebar/diff"),
        ("Space", "Toggle viewed & jump to next file"),
        ("{ / }", "Previous / next hunk"),
        ("/", "Open sidebar fuzzy filter"),
        ("T", "Theme selector"),
        ("c / C", "Add or view comments"),
        ("[", "Toggle old pane fullscreen"),
        ("]", "Toggle new pane fullscreen"),
        ("r", "Manual reload of file list/diff"),
        ("y", "Copy current path to clipboard"),
        ("o", "Open file in $EDITOR"),
        ("P", "Open PR picker / exit PR mode"),
        ("A", "Approve PR (in PR mode)"),
        ("R", "Request changes (in PR mode)"),
        ("?", "Close this help overlay"),
        ("q or Ctrl+C", "Quit quickdiff"),
    ];

    let area = frame.area();
    let max_width = area.width.saturating_sub(2).max(1);
    let width = 70.min(max_width);
    let needed_height = (entries.len() as u16 + 6).max(10);
    let max_height = area.height.saturating_sub(2).max(1);
    let height = needed_height.min(max_height);

    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.accent))
        .title(Span::styled(
            " Help ",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(app.theme.bg_elevated));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    if inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "Common keybindings",
        Style::default()
            .fg(app.theme.accent)
            .bg(app.theme.bg_elevated)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "Press ? again, Esc, or q to close.",
        Style::default()
            .fg(app.theme.text_muted)
            .bg(app.theme.bg_elevated),
    )));
    lines.push(Line::from(Span::styled(
        "",
        Style::default().bg(app.theme.bg_elevated),
    )));

    for (key, desc) in entries {
        let mut spans = Vec::new();
        let shortcut = format!("{:<16}", key);
        spans.push(Span::styled(
            shortcut,
            Style::default()
                .fg(app.theme.accent)
                .bg(app.theme.bg_elevated),
        ));
        spans.push(Span::styled(
            desc,
            Style::default()
                .fg(app.theme.text_normal)
                .bg(app.theme.bg_elevated),
        ));
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(vec![
        Span::styled(
            "Mouse",
            Style::default()
                .fg(app.theme.accent)
                .bg(app.theme.bg_elevated),
        ),
        Span::styled(
            " Click to focus, scroll wheel to move.",
            Style::default()
                .fg(app.theme.text_normal)
                .bg(app.theme.bg_elevated),
        ),
    ]));

    while lines.len() < inner.height as usize {
        lines.push(Line::from(Span::styled(
            "",
            Style::default().bg(app.theme.bg_elevated),
        )));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(app.theme.bg_elevated));
    frame.render_widget(para, inner);
}

// ============================================================================
// Bottom Bar
// ============================================================================

fn render_bottom_bar(frame: &mut Frame, app: &App, area: Rect) {
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
        // Pad remaining
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
            ("C", "comment"),
            ("R", "req-chg"),
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

// ============================================================================
// PR Picker Overlay
// ============================================================================

fn render_pr_picker_overlay(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Center the picker
    let width = (area.width * 3 / 4).min(80);
    let height = (area.height * 3 / 4).min(30);
    let x = (area.width - width) / 2;
    let y = (area.height - height) / 2;
    let picker_area = Rect::new(x, y, width, height);

    // Background
    let block = Block::default()
        .title(" Pull Requests ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.accent))
        .style(Style::default().bg(app.theme.bg_dark));
    frame.render_widget(block, picker_area);

    let inner = Rect::new(
        picker_area.x + 1,
        picker_area.y + 1,
        picker_area.width.saturating_sub(2),
        picker_area.height.saturating_sub(2),
    );

    // Filter tabs
    let filter_line = Line::from(vec![
        Span::raw(" "),
        styled_filter_tab(
            "All",
            app.pr_filter == crate::core::PRFilter::All,
            &app.theme,
        ),
        Span::raw("  "),
        styled_filter_tab(
            "Mine",
            app.pr_filter == crate::core::PRFilter::Mine,
            &app.theme,
        ),
        Span::raw("  "),
        styled_filter_tab(
            "Review Requested",
            app.pr_filter == crate::core::PRFilter::ReviewRequested,
            &app.theme,
        ),
        Span::raw(" "),
    ]);
    let filter_para = Paragraph::new(filter_line);
    frame.render_widget(filter_para, Rect::new(inner.x, inner.y, inner.width, 1));

    // Loading or list
    let list_area = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(4),
    );

    if app.pr_loading {
        let loading = Paragraph::new("Loading...").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(loading, list_area);
    } else if app.pr_list.is_empty() {
        let empty = Paragraph::new("No PRs found").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(empty, list_area);
    } else {
        // Render PR list
        let visible_height = list_area.height as usize;
        let start = app.pr_picker_scroll;
        let end = (start + visible_height).min(app.pr_list.len());

        for (i, pr) in app.pr_list[start..end].iter().enumerate() {
            let y = list_area.y + i as u16;
            let is_selected = start + i == app.pr_picker_selected;

            let style = if is_selected {
                Style::default().bg(app.theme.accent).fg(app.theme.bg_dark)
            } else {
                Style::default().fg(app.theme.text_normal)
            };

            // Format: #123 Title (head → base) +10/-5
            let pr_line = format!(
                " #{:<4} {} ({} → {}) +{}/-{}",
                pr.number,
                truncate_str(&pr.title, 30),
                truncate_str(&pr.head_ref_name, 15),
                truncate_str(&pr.base_ref_name, 15),
                pr.additions,
                pr.deletions,
            );

            let line = Paragraph::new(pr_line).style(style);
            frame.render_widget(line, Rect::new(list_area.x, y, list_area.width, 1));
        }
    }

    // Help line at bottom
    let help_line = Line::from(vec![
        Span::styled("j/k", Style::default().fg(app.theme.accent)),
        Span::raw(" navigate  "),
        Span::styled("Tab", Style::default().fg(app.theme.accent)),
        Span::raw(" filter  "),
        Span::styled("Enter", Style::default().fg(app.theme.accent)),
        Span::raw(" select  "),
        Span::styled("r", Style::default().fg(app.theme.accent)),
        Span::raw(" refresh  "),
        Span::styled("Esc", Style::default().fg(app.theme.accent)),
        Span::raw(" close"),
    ]);
    let help_para = Paragraph::new(help_line).style(Style::default().fg(app.theme.text_muted));
    frame.render_widget(
        help_para,
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
}

fn styled_filter_tab<'a>(label: &'a str, active: bool, theme: &Theme) -> Span<'a> {
    if active {
        Span::styled(
            format!("[{}]", label),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!(" {} ", label),
            Style::default().fg(theme.text_muted),
        )
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}

// ============================================================================
// PR Action Overlay
// ============================================================================

fn render_pr_action_overlay(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let width = 60.min(area.width - 4);
    let height = 10;
    let x = (area.width - width) / 2;
    let y = (area.height - height) / 2;
    let action_area = Rect::new(x, y, width, height);

    let title = match app.pr_action_type {
        Some(PRActionType::Approve) => " Approve PR ",
        Some(PRActionType::Comment) => " Comment on PR ",
        Some(PRActionType::RequestChanges) => " Request Changes ",
        None => " PR Action ",
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.accent))
        .style(Style::default().bg(app.theme.bg_dark));
    frame.render_widget(block, action_area);

    let inner = Rect::new(
        action_area.x + 2,
        action_area.y + 2,
        action_area.width.saturating_sub(4),
        action_area.height.saturating_sub(4),
    );

    // Show text input for comment/request-changes
    let show_input = matches!(
        app.pr_action_type,
        Some(PRActionType::Comment) | Some(PRActionType::RequestChanges)
    );

    if show_input {
        let label =
            Paragraph::new("Message (required):").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(label, Rect::new(inner.x, inner.y, inner.width, 1));

        let input_text = format!("{}_", &app.pr_action_text);
        let input = Paragraph::new(input_text).style(Style::default().fg(app.theme.text_normal));
        frame.render_widget(input, Rect::new(inner.x, inner.y + 1, inner.width, 3));
    } else {
        let confirm = Paragraph::new("Press Enter to approve, Esc to cancel")
            .style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(confirm, Rect::new(inner.x, inner.y + 1, inner.width, 1));
    }

    // Help line
    let help = Line::from(vec![
        Span::styled("Enter", Style::default().fg(app.theme.accent)),
        Span::raw(" submit  "),
        Span::styled("Esc", Style::default().fg(app.theme.accent)),
        Span::raw(" cancel"),
    ]);
    let help_para = Paragraph::new(help).style(Style::default().fg(app.theme.text_muted));
    frame.render_widget(
        help_para,
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn tab_expands_to_next_stop() {
        let mut builder = SpanBuilder::new();
        let mut col_pos = 0usize;
        let mut visible_len = 0usize;

        render_plain_span(
            &mut builder,
            "\tfoo",
            Style::default(),
            0,
            20,
            &mut col_pos,
            &mut visible_len,
        );

        let text = spans_text(&builder.finish());
        assert_eq!(text, "        foo");
    }

    #[test]
    fn tab_respects_scroll() {
        let mut builder = SpanBuilder::new();
        let mut col_pos = 0usize;
        let mut visible_len = 0usize;

        render_plain_span(
            &mut builder,
            "\tfoo",
            Style::default(),
            4,
            20,
            &mut col_pos,
            &mut visible_len,
        );

        let text = spans_text(&builder.finish());
        assert_eq!(text, "    foo");
    }
}
