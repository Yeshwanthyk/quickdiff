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

use crate::core::{ChangeKind, CommentStatus, FileChangeKind, ViewedStore};
use crate::highlight::StyleId;
use crate::theme::Theme;

use super::app::{App, Focus, Mode};

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

    // Open comments for this file (in current context)
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

    // Pad to fill width
    let content_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let padding = " ".repeat(area.width as usize - content_len.min(area.width as usize));
    spans.push(Span::styled(
        padding,
        Style::default().bg(app.theme.bg_elevated),
    ));

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

        let comment_count = app
            .open_comment_counts
            .get(&file.path)
            .copied()
            .unwrap_or(0);
        let (comment_text, comment_color) = if comment_count == 0 {
            ("  ".to_string(), app.theme.text_faint)
        } else if comment_count < 10 {
            (format!(" {}", comment_count), app.theme.accent)
        } else {
            ("9+".to_string(), app.theme.accent)
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

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(" Diff ", title_style))
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

    if diff.rows.is_empty() || !diff.has_changes() {
        let msg =
            Paragraph::new("Files are identical").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(msg, inner);
        return;
    }

    // Split into old/new panes with divider
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(1), // Divider
            Constraint::Percentage(50),
        ])
        .split(inner);

    render_diff_pane(frame, app, panes[0], true); // Old
    render_pane_divider(frame, panes[1], &app.theme); // Divider
    render_diff_pane(frame, app, panes[2], false); // New
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

fn render_diff_pane(frame: &mut Frame, app: &App, area: Rect, is_old: bool) {
    let Some(diff) = &app.diff else {
        return;
    };

    let height = area.height as usize;
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
    let mut rendered: Vec<RenderedLine> = Vec::with_capacity(height);
    let mut max_visible_len = 0usize;

    for (offset, row) in diff.render_rows(app.scroll_y, height).enumerate() {
        let row_idx = app.scroll_y + offset;
        let has_comment = diff
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

        let line_num = line_ref.map(|l| l.line_num + 1);
        let content = line_ref.map(|l| l.content.as_str()).unwrap_or("");
        let inline_spans = line_ref.and_then(|l| l.inline_spans.as_ref());

        let line_num_str = line_num
            .map(|n| format!("{:>4}", n))
            .unwrap_or_else(|| "    ".to_string());

        // Build syntax-highlighted content spans with truncation
        let mut code_spans: Vec<Span> = Vec::new();
        let mut visible_len = 0usize;

        if !content.is_empty() && max_content > 0 {
            let hl_spans = app.highlighter.highlight(app.current_lang, content);
            let scroll_x = app.scroll_x;
            let mut char_pos = 0usize;

            'outer: for hl in hl_spans {
                let span_text = content.get(hl.start..hl.end).unwrap_or("");
                let span_char_count = span_text.chars().count();
                let span_end_char = char_pos + span_char_count;

                // Skip spans entirely before scroll offset
                if span_end_char <= scroll_x {
                    char_pos = span_end_char;
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
                    // Fast path: no inline changes, emit whole span (truncated)
                    let skip = scroll_x.saturating_sub(char_pos);
                    let remaining = max_content.saturating_sub(visible_len);
                    let text: String = span_text
                        .chars()
                        .skip(skip)
                        .take(remaining)
                        .map(sanitize_char)
                        .collect();
                    if !text.is_empty() {
                        visible_len += text.chars().count();
                        code_spans.push(Span::styled(text, Style::default().fg(fg).bg(bg_color)));
                    }
                    if visible_len >= max_content {
                        break 'outer;
                    }
                } else {
                    // Slow path: split by inline regions for word-level highlighting
                    let mut byte_offset = hl.start;
                    let mut local_char = 0usize;

                    let mut pending_style: Option<Style> = None;
                    let mut pending_text = String::new();

                    for ch in span_text.chars() {
                        let global_char = char_pos + local_char;

                        // Skip chars before scroll
                        if global_char < scroll_x {
                            byte_offset += ch.len_utf8();
                            local_char += 1;
                            continue;
                        }

                        // Stop if we've hit width limit
                        if visible_len >= max_content {
                            break;
                        }

                        // Determine if this byte is in a changed region
                        let is_changed = inline_spans.is_some_and(|spans| {
                            spans
                                .iter()
                                .any(|s| s.changed && byte_offset >= s.start && byte_offset < s.end)
                        });
                        let char_bg = if is_changed { inline_bg } else { bg_color };
                        let style = Style::default().fg(fg).bg(char_bg);

                        let same_style = pending_style.as_ref().is_some_and(|ps| ps == &style);
                        if !same_style {
                            if !pending_text.is_empty() {
                                code_spans.push(Span::styled(
                                    std::mem::take(&mut pending_text),
                                    pending_style.take().unwrap_or_default(),
                                ));
                            }
                            pending_style = Some(style);
                        }

                        pending_text.push(sanitize_char(ch));
                        visible_len += 1;

                        byte_offset += ch.len_utf8();
                        local_char += 1;
                    }

                    if !pending_text.is_empty() {
                        code_spans.push(Span::styled(
                            pending_text,
                            pending_style.unwrap_or_default(),
                        ));
                    }

                    if visible_len >= max_content {
                        break 'outer;
                    }
                }

                char_pos = span_end_char;
            }
        }

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

    let mut lines: Vec<Line> = Vec::with_capacity(height);

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
        '\t' => '\t',
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

    // Key hints
    let hints: &[(&str, &str)] = match app.focus {
        Focus::Sidebar => &[
            ("j/k", "navigate"),
            ("↵", "open"),
            ("/", "filter"),
            ("␣", "viewed"),
            ("⇥", "switch"),
            ("q", "quit"),
        ],
        Focus::Diff => &[
            ("j/k", "scroll"),
            ("{/}", "hunks"),
            ("c", "comment"),
            ("C", "view"),
            ("␣", "viewed"),
            ("⇥", "switch"),
            ("q", "quit"),
        ],
    };

    let mut spans = vec![Span::styled(" ", Style::default().bg(app.theme.bg_surface))];
    for (i, (key, desc)) in hints.iter().enumerate() {
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
