//! UI rendering with ratatui.
//!
//! Design: Refined dark theme with editorial minimalism.
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

use crate::core::{ChangeKind, FileChangeKind, ViewedStore};
use crate::highlight::StyleId;

use super::app::{App, Focus, Mode};

// ============================================================================
// Color Palette
// ============================================================================

mod palette {
    use ratatui::style::Color;

    // Base grays (dark to light) - refined for hierarchy
    pub const BG_DARK: Color = Color::Rgb(18, 18, 22); // Deep black
    pub const BG_SURFACE: Color = Color::Rgb(26, 26, 32); // Panels
    pub const BG_ELEVATED: Color = Color::Rgb(36, 36, 44); // Headers, overlays
    pub const BG_SELECTED: Color = Color::Rgb(45, 45, 55); // Selected items

    // Borders & separators (very subtle)
    pub const BORDER_DIM: Color = Color::Rgb(50, 50, 60); // Unfocused borders
    pub const GUTTER_SEP: Color = Color::Rgb(38, 38, 46); // Nearly invisible gutter
    pub const PANE_DIVIDER: Color = Color::Rgb(55, 55, 65); // Between old/new panes

    // Text hierarchy (critical for glanceability)
    pub const TEXT_FAINT: Color = Color::Rgb(55, 55, 65); // Line numbers, minimal
    pub const TEXT_MUTED: Color = Color::Rgb(80, 80, 92); // Secondary info
    pub const TEXT_DIM: Color = Color::Rgb(110, 110, 125); // Tertiary
    pub const TEXT_NORMAL: Color = Color::Rgb(175, 175, 185); // Primary text
    pub const TEXT_BRIGHT: Color = Color::Rgb(230, 230, 235); // Emphasized

    // Accent (teal/cyan family)
    pub const ACCENT: Color = Color::Rgb(80, 200, 200); // Focus, interactive
    pub const ACCENT_DIM: Color = Color::Rgb(55, 130, 130); // Subtle accent

    // Diff backgrounds (subtle tints, preserves syntax colors)
    pub const DIFF_DELETE_BG: Color = Color::Rgb(45, 25, 30); // Soft red tint
    pub const DIFF_INSERT_BG: Color = Color::Rgb(25, 45, 32); // Soft green tint
    pub const DIFF_EMPTY_BG: Color = Color::Rgb(22, 22, 26); // Missing line placeholder

    // Inline diff highlights (brighter, for word-level changes)
    pub const INLINE_DELETE_BG: Color = Color::Rgb(90, 40, 50); // Brighter red for changed words
    pub const INLINE_INSERT_BG: Color = Color::Rgb(40, 90, 55); // Brighter green for changed words

    // Status colors (slightly muted for polish)
    pub const SUCCESS: Color = Color::Rgb(85, 185, 105);
    pub const ERROR: Color = Color::Rgb(215, 85, 85);
    pub const WARNING: Color = Color::Rgb(215, 175, 80);

    // Syntax highlighting (vibrant - these are the stars)
    pub const SYN_KEYWORD: Color = Color::Rgb(198, 120, 221); // Purple
    pub const SYN_TYPE: Color = Color::Rgb(229, 192, 123); // Gold
    pub const SYN_FUNCTION: Color = Color::Rgb(97, 175, 239); // Blue
    pub const SYN_STRING: Color = Color::Rgb(152, 195, 121); // Green
    pub const SYN_NUMBER: Color = Color::Rgb(209, 154, 102); // Orange
    pub const SYN_COMMENT: Color = Color::Rgb(92, 99, 112); // Gray (intentionally dim)
    pub const SYN_OPERATOR: Color = Color::Rgb(171, 178, 191); // Light gray
    pub const SYN_PUNCTUATION: Color = Color::Rgb(120, 120, 135); // Subtle but visible
    pub const SYN_CONSTANT: Color = Color::Rgb(86, 182, 194); // Cyan
    pub const SYN_PROPERTY: Color = Color::Rgb(224, 108, 117); // Red
    pub const SYN_ATTRIBUTE: Color = Color::Rgb(229, 192, 123); // Gold

    // Sidebar indicators
    pub const INDICATOR_SELECTED: Color = Color::Rgb(80, 200, 200); // Selection bar
}

use palette::*;

// ============================================================================
// Constants
// ============================================================================

/// Max width for path display in sidebar.
const SIDEBAR_PATH_WIDTH: usize = 26;

// ============================================================================
// Syntax Highlighting
// ============================================================================

/// Map StyleId to syntax color.
fn style_to_color(style: StyleId) -> Color {
    match style {
        StyleId::Default => TEXT_NORMAL,
        StyleId::Keyword => SYN_KEYWORD,
        StyleId::Type => SYN_TYPE,
        StyleId::Function => SYN_FUNCTION,
        StyleId::String => SYN_STRING,
        StyleId::Number => SYN_NUMBER,
        StyleId::Comment => SYN_COMMENT,
        StyleId::Operator => SYN_OPERATOR,
        StyleId::Punctuation => SYN_PUNCTUATION,
        StyleId::Variable => TEXT_NORMAL,
        StyleId::Constant => SYN_CONSTANT,
        StyleId::Property => SYN_PROPERTY,
        StyleId::Attribute => SYN_ATTRIBUTE,
    }
}

// ============================================================================
// Main Render
// ============================================================================

/// Main render function.
pub fn render(frame: &mut Frame, app: &mut App) {
    // Fill background
    let bg_block = Block::default().style(Style::default().bg(BG_DARK));
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
}

// ============================================================================
// Top Bar
// ============================================================================

fn render_top_bar(frame: &mut Frame, app: &App, area: Rect) {
    let file = app.selected_file();
    let file_name = file.map(|f| f.path.as_str()).unwrap_or("No files");

    // Get file change kind for indicator
    let kind_indicator = file.map(|f| match f.kind {
        FileChangeKind::Added => ("A", SUCCESS),
        FileChangeKind::Modified => ("M", WARNING),
        FileChangeKind::Deleted => ("D", ERROR),
        FileChangeKind::Untracked => ("?", TEXT_MUTED),
        FileChangeKind::Renamed => ("R", ACCENT_DIM),
    });

    let mut spans = vec![Span::styled("  ", Style::default().bg(BG_ELEVATED))];

    // Change kind badge
    if let Some((kind, color)) = kind_indicator {
        spans.push(Span::styled(
            kind,
            Style::default()
                .fg(color)
                .bg(BG_ELEVATED)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("  ", Style::default().bg(BG_ELEVATED)));
    }

    // Filename (prominent)
    spans.push(Span::styled(
        file_name,
        Style::default()
            .fg(TEXT_BRIGHT)
            .bg(BG_ELEVATED)
            .add_modifier(Modifier::BOLD),
    ));

    if app.is_current_viewed() {
        spans.push(Span::styled(
            "  ✓ viewed",
            Style::default().fg(SUCCESS).bg(BG_ELEVATED),
        ));
    }

    if app.is_binary {
        spans.push(Span::styled(
            "  [binary]",
            Style::default().fg(TEXT_MUTED).bg(BG_ELEVATED),
        ));
    }

    // Pad to fill width
    let content_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let padding = " ".repeat(area.width as usize - content_len.min(area.width as usize));
    spans.push(Span::styled(padding, Style::default().bg(BG_ELEVATED)));

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

    let border_color = if is_focused { ACCENT } else { BORDER_DIM };
    let title_style = if is_focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(TEXT_MUTED)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" Files ({}) ", app.viewed_status()),
            title_style,
        ))
        .style(Style::default().bg(BG_SURFACE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = inner.height as usize;
    if height == 0 {
        return;
    }

    if app.files.is_empty() {
        let msg = Paragraph::new("No files").style(Style::default().fg(TEXT_MUTED).bg(BG_SURFACE));
        frame.render_widget(msg, inner);
        return;
    }

    // Keep selection visible without rendering O(n) list items.
    let max_scroll = app.files.len().saturating_sub(height);
    app.sidebar_scroll = app.sidebar_scroll.min(max_scroll);

    if app.selected_idx < app.sidebar_scroll {
        app.sidebar_scroll = app.selected_idx;
    } else if app.selected_idx >= app.sidebar_scroll + height {
        app.sidebar_scroll = app.selected_idx + 1 - height;
    }

    let end = (app.sidebar_scroll + height).min(app.files.len());

    let mut lines: Vec<Line> = Vec::with_capacity(height);
    for idx in app.sidebar_scroll..end {
        let file = &app.files[idx];
        let is_selected = idx == app.selected_idx;
        let is_viewed = app.viewed.is_viewed(&file.path);

        let row_bg = if is_selected { BG_SELECTED } else { BG_SURFACE };

        // Selection indicator (left edge)
        let select_indicator = if is_selected { "▌" } else { " " };
        let select_style = Style::default()
            .fg(if is_selected {
                INDICATOR_SELECTED
            } else {
                row_bg
            })
            .bg(row_bg);

        // Change kind indicator with color
        let (kind_char, kind_color) = match file.kind {
            FileChangeKind::Added => ('A', SUCCESS),
            FileChangeKind::Modified => ('M', WARNING),
            FileChangeKind::Deleted => ('D', ERROR),
            FileChangeKind::Untracked => ('?', TEXT_MUTED),
            FileChangeKind::Renamed => ('R', ACCENT_DIM),
        };

        let viewed_char = if is_viewed { '✓' } else { '·' };
        let viewed_color = if is_viewed { SUCCESS } else { TEXT_FAINT };

        // Ellipsize path
        let path = file.path.as_str();
        let display_path = if path.len() > SIDEBAR_PATH_WIDTH {
            format!("…{}", &path[path.len() - SIDEBAR_PATH_WIDTH + 1..])
        } else {
            path.to_string()
        };

        // Text brightness based on state
        let text_color = if is_selected {
            TEXT_BRIGHT
        } else if is_viewed {
            TEXT_DIM // Viewed files are dimmer
        } else {
            TEXT_NORMAL
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
            Span::styled(display_path, Style::default().fg(text_color).bg(row_bg)),
        ]));
    }

    while lines.len() < height {
        lines.push(Line::from(Span::styled(
            "",
            Style::default().bg(BG_SURFACE),
        )));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(BG_SURFACE));
    frame.render_widget(para, inner);
}

// ============================================================================
// Diff View
// ============================================================================

fn render_diff(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Diff;
    let border_color = if is_focused { ACCENT } else { BORDER_DIM };
    let title_style = if is_focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(TEXT_MUTED)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(" Diff ", title_style))
        .style(Style::default().bg(BG_DARK));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Handle empty states
    if app.is_binary {
        let msg = Paragraph::new("Binary file — cannot display diff")
            .style(Style::default().fg(TEXT_MUTED));
        frame.render_widget(msg, inner);
        return;
    }

    let Some(diff) = &app.diff else {
        let msg = Paragraph::new("No diff to display").style(Style::default().fg(TEXT_MUTED));
        frame.render_widget(msg, inner);
        return;
    };

    if diff.rows.is_empty() {
        let msg = Paragraph::new("Files are identical").style(Style::default().fg(TEXT_MUTED));
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
    render_pane_divider(frame, panes[1]); // Divider
    render_diff_pane(frame, app, panes[2], false); // New
}

/// Render vertical divider between old/new panes.
fn render_pane_divider(frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(PANE_DIVIDER))))
        .collect();
    let para = Paragraph::new(lines).style(Style::default().bg(BG_DARK));
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
    let mut lines: Vec<Line> = Vec::with_capacity(height);

    for row in diff.render_rows(app.scroll_y, height) {
        // Extract line info based on pane side
        let (line_ref, bg_color, inline_bg) = if is_old {
            match (&row.old, row.kind) {
                (Some(line), ChangeKind::Equal) => (Some(line), BG_DARK, BG_DARK),
                (Some(line), ChangeKind::Delete) | (Some(line), ChangeKind::Replace) => {
                    (Some(line), DIFF_DELETE_BG, INLINE_DELETE_BG)
                }
                (None, ChangeKind::Insert) => (None, DIFF_EMPTY_BG, DIFF_EMPTY_BG),
                _ => (None, BG_DARK, BG_DARK),
            }
        } else {
            match (&row.new, row.kind) {
                (Some(line), ChangeKind::Equal) => (Some(line), BG_DARK, BG_DARK),
                (Some(line), ChangeKind::Insert) | (Some(line), ChangeKind::Replace) => {
                    (Some(line), DIFF_INSERT_BG, INLINE_INSERT_BG)
                }
                (None, ChangeKind::Delete) => (None, DIFF_EMPTY_BG, DIFF_EMPTY_BG),
                _ => (None, BG_DARK, BG_DARK),
            }
        };

        let line_num = line_ref.map(|l| l.line_num + 1);
        let content = line_ref.map(|l| l.content.as_str()).unwrap_or("");
        let inline_spans = line_ref.and_then(|l| l.inline_spans.as_ref());

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

                let fg = style_to_color(hl.style_id);

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

        // Build the line with pane-specific layout:
        // OLD (left):  [pad][code] │[line_num] - right-aligned, gutter on right
        // NEW (right): [line_num]│ [code]     - left-aligned, gutter on left
        let mut spans: Vec<Span> = Vec::new();
        let line_num_str = line_num
            .map(|n| format!("{:>4}", n))
            .unwrap_or_else(|| "    ".to_string());

        if is_old {
            // Right-aligned: padding pushes code toward center
            let pad_len = max_content.saturating_sub(visible_len);
            if pad_len > 0 {
                spans.push(Span::styled(
                    &spaces[..pad_len],
                    Style::default().bg(bg_color),
                ));
            }
            spans.extend(code_spans);
            // Separator + line number on right edge (inside)
            spans.push(Span::styled(
                " │",
                Style::default().fg(GUTTER_SEP).bg(bg_color),
            ));
            spans.push(Span::styled(
                line_num_str,
                Style::default().fg(TEXT_FAINT).bg(bg_color),
            ));
        } else {
            // Left-aligned: line number + separator on left edge (inside)
            spans.push(Span::styled(
                line_num_str,
                Style::default().fg(TEXT_FAINT).bg(bg_color),
            ));
            spans.push(Span::styled(
                "│ ",
                Style::default().fg(GUTTER_SEP).bg(bg_color),
            ));
            spans.extend(code_spans);
            // Trailing padding
            let pad_len = max_content.saturating_sub(visible_len);
            if pad_len > 0 {
                spans.push(Span::styled(
                    &spaces[..pad_len],
                    Style::default().bg(bg_color),
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
    let min_overlay_height = 5.min(max_overlay_height);
    let height =
        (app.viewing_comments.len() as u16 + 4).clamp(min_overlay_height, max_overlay_height);

    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let mut lines: Vec<Line> = Vec::new();
    for (id, msg) in &app.viewing_comments {
        lines.push(Line::from(vec![
            Span::styled(
                format!("#{} ", id),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(msg.as_str(), Style::default().fg(TEXT_NORMAL)),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No comments",
            Style::default().fg(TEXT_MUTED),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " Comments ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(BG_ELEVATED));

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, overlay_area);
}

// ============================================================================
// Bottom Bar
// ============================================================================

fn render_bottom_bar(frame: &mut Frame, app: &App, area: Rect) {
    // Input mode
    if app.mode == Mode::AddComment {
        let line = Line::from(vec![
            Span::styled(" Comment: ", Style::default().fg(ACCENT).bg(BG_ELEVATED)),
            Span::styled(
                &app.draft_comment,
                Style::default().fg(TEXT_BRIGHT).bg(BG_ELEVATED),
            ),
            Span::styled("█", Style::default().fg(ACCENT).bg(BG_ELEVATED)),
            Span::styled(
                "  Enter: save  Esc: cancel",
                Style::default().fg(TEXT_MUTED).bg(BG_ELEVATED),
            ),
        ]);
        // Pad remaining
        let para = Paragraph::new(line).style(Style::default().bg(BG_ELEVATED));
        frame.render_widget(para, area);
        return;
    }

    // Error message
    if let Some(ref err) = app.error_msg {
        let line = Line::from(vec![
            Span::styled(" ✗ ", Style::default().fg(ERROR).bg(BG_ELEVATED)),
            Span::styled(err.as_str(), Style::default().fg(ERROR).bg(BG_ELEVATED)),
        ]);
        let para = Paragraph::new(line).style(Style::default().bg(BG_ELEVATED));
        frame.render_widget(para, area);
        return;
    }

    // Status message
    if let Some(ref msg) = app.status_msg {
        let line = Line::from(vec![
            Span::styled(" ✓ ", Style::default().fg(SUCCESS).bg(BG_ELEVATED)),
            Span::styled(msg.as_str(), Style::default().fg(SUCCESS).bg(BG_ELEVATED)),
        ]);
        let para = Paragraph::new(line).style(Style::default().bg(BG_ELEVATED));
        frame.render_widget(para, area);
        return;
    }

    // Key hints
    let hints: &[(&str, &str)] = match app.focus {
        Focus::Sidebar => &[
            ("j/k", "navigate"),
            ("↵", "open"),
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

    let mut spans = vec![Span::styled(" ", Style::default().bg(BG_SURFACE))];
    for (i, (key, desc)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default().bg(BG_SURFACE)));
        }
        spans.push(Span::styled(
            *key,
            Style::default().fg(ACCENT).bg(BG_SURFACE),
        ));
        spans.push(Span::styled(
            format!(" {}", desc),
            Style::default().fg(TEXT_DIM).bg(BG_SURFACE),
        ));
    }

    let line = Line::from(spans);
    let para = Paragraph::new(line).style(Style::default().bg(BG_SURFACE));
    frame.render_widget(para, area);
}
