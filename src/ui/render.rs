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
    widgets::{Block, Borders, List, ListItem, Paragraph},
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

    // Base grays (dark to light)
    pub const BG_DARK: Color = Color::Rgb(22, 22, 26);       // Near black
    pub const BG_SURFACE: Color = Color::Rgb(30, 30, 35);    // Panels
    pub const BG_ELEVATED: Color = Color::Rgb(40, 40, 46);   // Headers, overlays
    pub const BORDER_DIM: Color = Color::Rgb(55, 55, 65);    // Unfocused borders
    pub const TEXT_MUTED: Color = Color::Rgb(90, 90, 100);   // Secondary text
    pub const TEXT_DIM: Color = Color::Rgb(120, 120, 130);   // Tertiary
    pub const TEXT_NORMAL: Color = Color::Rgb(180, 180, 185); // Primary text
    pub const TEXT_BRIGHT: Color = Color::Rgb(220, 220, 225); // Emphasized

    // Accent (teal/cyan family)
    pub const ACCENT: Color = Color::Rgb(80, 200, 200);      // Focus, interactive
    pub const ACCENT_DIM: Color = Color::Rgb(60, 140, 140);  // Subtle accent

    // Diff backgrounds (very subtle, won't wash out syntax)
    pub const DIFF_DELETE_BG: Color = Color::Rgb(50, 28, 32);   // Soft red tint
    pub const DIFF_INSERT_BG: Color = Color::Rgb(28, 50, 35);   // Soft green tint
    pub const DIFF_EMPTY_BG: Color = Color::Rgb(25, 25, 28);    // Missing line

    // Status colors
    pub const SUCCESS: Color = Color::Rgb(80, 180, 100);
    pub const ERROR: Color = Color::Rgb(220, 80, 80);
    pub const WARNING: Color = Color::Rgb(220, 180, 80);

    // Syntax highlighting (vibrant to pop against muted UI)
    pub const SYN_KEYWORD: Color = Color::Rgb(198, 120, 221);   // Purple
    pub const SYN_TYPE: Color = Color::Rgb(229, 192, 123);      // Gold
    pub const SYN_FUNCTION: Color = Color::Rgb(97, 175, 239);   // Blue
    pub const SYN_STRING: Color = Color::Rgb(152, 195, 121);    // Green
    pub const SYN_NUMBER: Color = Color::Rgb(209, 154, 102);    // Orange
    pub const SYN_COMMENT: Color = Color::Rgb(92, 99, 112);     // Gray
    pub const SYN_OPERATOR: Color = Color::Rgb(171, 178, 191);  // Light gray
    pub const SYN_PUNCTUATION: Color = Color::Rgb(92, 99, 112); // Dim
    pub const SYN_CONSTANT: Color = Color::Rgb(86, 182, 194);   // Cyan
    pub const SYN_PROPERTY: Color = Color::Rgb(224, 108, 117);  // Red
    pub const SYN_ATTRIBUTE: Color = Color::Rgb(229, 192, 123); // Gold
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
    let file_name = app
        .selected_file()
        .map(|f| f.path.as_str())
        .unwrap_or("No files");

    let mut spans = vec![
        Span::styled(" ", Style::default().bg(BG_ELEVATED)),
        Span::styled(
            file_name,
            Style::default().fg(TEXT_BRIGHT).bg(BG_ELEVATED),
        ),
    ];

    if app.is_current_viewed() {
        spans.push(Span::styled(
            " ✓",
            Style::default().fg(SUCCESS).bg(BG_ELEVATED),
        ));
    }

    if app.is_binary {
        spans.push(Span::styled(
            " [binary]",
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

    let items: Vec<ListItem> = app
        .files
        .iter()
        .enumerate()
        .map(|(idx, file)| {
            let is_selected = idx == app.selected_idx;

            // Change kind indicator with color
            let (kind_char, kind_color) = match file.kind {
                FileChangeKind::Added => ('A', SUCCESS),
                FileChangeKind::Modified => ('M', WARNING),
                FileChangeKind::Deleted => ('D', ERROR),
                FileChangeKind::Untracked => ('?', TEXT_MUTED),
                FileChangeKind::Renamed => ('R', ACCENT_DIM),
            };

            let viewed_char = if app.viewed.is_viewed(&file.path) {
                '✓'
            } else {
                ' '
            };

            // Ellipsize path
            let path = file.path.as_str();
            let display_path = if path.len() > SIDEBAR_PATH_WIDTH {
                format!("…{}", &path[path.len() - SIDEBAR_PATH_WIDTH + 1..])
            } else {
                path.to_string()
            };

            let text_color = if is_selected {
                TEXT_BRIGHT
            } else {
                TEXT_NORMAL
            };

            let line = Line::from(vec![
                Span::styled(format!("{}", kind_char), Style::default().fg(kind_color)),
                Span::styled(" ", Style::default()),
                Span::styled(
                    format!("{}", viewed_char),
                    Style::default().fg(if viewed_char == '✓' { SUCCESS } else { TEXT_MUTED }),
                ),
                Span::styled(" ", Style::default()),
                Span::styled(display_path, Style::default().fg(text_color)),
            ]);

            ListItem::new(line)
        })
        .collect();

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

    let highlight_style = Style::default()
        .bg(BG_ELEVATED)
        .add_modifier(Modifier::BOLD);

    let list = List::new(items)
        .block(block)
        .highlight_style(highlight_style);

    frame.render_stateful_widget(list, area, &mut app.list_state);
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
        let msg = Paragraph::new("No diff to display")
            .style(Style::default().fg(TEXT_MUTED));
        frame.render_widget(msg, inner);
        return;
    };

    if diff.rows.is_empty() {
        let msg = Paragraph::new("Files are identical")
            .style(Style::default().fg(TEXT_MUTED));
        frame.render_widget(msg, inner);
        return;
    }

    // Split into old/new panes
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    render_diff_pane(frame, app, panes[0], true);  // Old
    render_diff_pane(frame, app, panes[1], false); // New
}

fn render_diff_pane(frame: &mut Frame, app: &App, area: Rect, is_old: bool) {
    let Some(diff) = &app.diff else {
        return;
    };

    let height = area.height as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(height);

    for row in diff.render_rows(app.scroll_y, height) {
        let (line_num, content, bg_color) = if is_old {
            match (&row.old, row.kind) {
                (Some(line), ChangeKind::Equal) => {
                    (Some(line.line_num + 1), line.content.as_str(), BG_DARK)
                }
                (Some(line), ChangeKind::Delete) | (Some(line), ChangeKind::Replace) => {
                    (Some(line.line_num + 1), line.content.as_str(), DIFF_DELETE_BG)
                }
                (None, ChangeKind::Insert) => (None, "", DIFF_EMPTY_BG),
                _ => (None, "", BG_DARK),
            }
        } else {
            match (&row.new, row.kind) {
                (Some(line), ChangeKind::Equal) => {
                    (Some(line.line_num + 1), line.content.as_str(), BG_DARK)
                }
                (Some(line), ChangeKind::Insert) | (Some(line), ChangeKind::Replace) => {
                    (Some(line.line_num + 1), line.content.as_str(), DIFF_INSERT_BG)
                }
                (None, ChangeKind::Delete) => (None, "", DIFF_EMPTY_BG),
                _ => (None, "", BG_DARK),
            }
        };

        // Line number gutter
        let line_num_str = line_num
            .map(|n| format!("{:>4} ", n))
            .unwrap_or_else(|| "     ".to_string());

        let mut spans = vec![Span::styled(
            line_num_str,
            Style::default().fg(TEXT_MUTED).bg(bg_color),
        )];

        // Separator
        spans.push(Span::styled(
            "│ ",
            Style::default().fg(BORDER_DIM).bg(bg_color),
        ));

        if content.is_empty() {
            // Fill empty line with background
            spans.push(Span::styled("", Style::default().bg(bg_color)));
        } else {
            // Syntax-highlighted content
            let hl_spans = app.highlighter.highlight(app.current_lang, content);
            let mut char_pos = 0usize;
            let scroll_x = app.scroll_x;

            for hl in hl_spans {
                let span_text = content.get(hl.start..hl.end).unwrap_or("");
                let span_char_count = span_text.chars().count();
                let span_end_pos = char_pos + span_char_count;

                if span_end_pos <= scroll_x {
                    char_pos = span_end_pos;
                    continue;
                }

                let skip = scroll_x.saturating_sub(char_pos);
                let text: String = span_text.chars().skip(skip).map(sanitize_char).collect();

                if !text.is_empty() {
                    let fg = style_to_color(hl.style_id);
                    spans.push(Span::styled(text, Style::default().fg(fg).bg(bg_color)));
                }

                char_pos = span_end_pos;
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
    let width = (area.width * 3 / 4).clamp(40, 70);
    let height = (app.viewing_comments.len() as u16 + 4).clamp(5, area.height.saturating_sub(4));

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
            Span::styled(&app.draft_comment, Style::default().fg(TEXT_BRIGHT).bg(BG_ELEVATED)),
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
