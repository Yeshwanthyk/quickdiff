//! UI rendering with ratatui.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::core::{ChangeKind, FileChangeKind, ViewedStore};
use crate::highlight::StyleId;

use super::app::{App, Focus};

/// Max width for path display in sidebar.
const SIDEBAR_PATH_WIDTH: usize = 26;

/// Map StyleId to terminal Color.
fn style_to_color(style: StyleId) -> Color {
    match style {
        StyleId::Default => Color::Reset,
        StyleId::Keyword => Color::Magenta,
        StyleId::Type => Color::Yellow,
        StyleId::Function => Color::Blue,
        StyleId::String => Color::Green,
        StyleId::Number => Color::Cyan,
        StyleId::Comment => Color::DarkGray,
        StyleId::Operator => Color::White,
        StyleId::Punctuation => Color::DarkGray,
        StyleId::Variable => Color::Reset,
        StyleId::Constant => Color::Cyan,
        StyleId::Property => Color::Blue,
        StyleId::Attribute => Color::Yellow,
    }
}

/// Main render function.
pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top bar
            Constraint::Min(0),    // Main content
            Constraint::Length(1), // Bottom bar / status
        ])
        .split(frame.area());

    render_top_bar(frame, app, chunks[0]);
    render_main(frame, app, chunks[1]);
    render_bottom_bar(frame, app, chunks[2]);
}

/// Render the top bar.
fn render_top_bar(frame: &mut Frame, app: &App, area: Rect) {
    let file_name = app
        .selected_file()
        .map(|f| f.path.as_str())
        .unwrap_or("No files");

    let viewed_marker = if app.is_current_viewed() { " ✓" } else { "" };
    let binary_marker = if app.is_binary { " [binary]" } else { "" };

    let text = format!(" {}{}{}", file_name, viewed_marker, binary_marker);
    let para = Paragraph::new(text).style(Style::default().bg(Color::Blue).fg(Color::White));

    frame.render_widget(para, area);
}

/// Render main content (sidebar + diff).
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

/// Render the file sidebar.
fn render_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|file| {
            let kind_char = match file.kind {
                FileChangeKind::Added => 'A',
                FileChangeKind::Modified => 'M',
                FileChangeKind::Deleted => 'D',
                FileChangeKind::Untracked => '?',
                FileChangeKind::Renamed => 'R',
            };

            let viewed = if app.viewed.is_viewed(&file.path) {
                "✓"
            } else {
                " "
            };

            // Show ellipsized relative path
            let path = file.path.as_str();
            let display_path = if path.len() > SIDEBAR_PATH_WIDTH {
                format!("…{}", &path[path.len() - SIDEBAR_PATH_WIDTH + 1..])
            } else {
                path.to_string()
            };

            let text = format!("{} {} {}", kind_char, viewed, display_path);
            ListItem::new(text)
        })
        .collect();

    let border_style = if app.focus == Focus::Sidebar {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(format!(" Files ({}) ", app.viewed_status())),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

/// Render the diff view.
fn render_diff(frame: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.focus == Focus::Diff {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Diff ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Show binary file message
    if app.is_binary {
        let msg = Paragraph::new("Binary file - cannot display diff")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, inner);
        return;
    }

    let Some(diff) = &app.diff else {
        let msg = Paragraph::new("No diff to display");
        frame.render_widget(msg, inner);
        return;
    };

    if diff.rows.is_empty() {
        let msg = Paragraph::new("Files are identical");
        frame.render_widget(msg, inner);
        return;
    }

    // Split into old and new panes
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    render_diff_pane(frame, app, panes[0], true); // Old
    render_diff_pane(frame, app, panes[1], false); // New
}

/// Render one side of the diff.
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
                    (Some(line.line_num + 1), line.content.as_str(), Color::Reset)
                }
                (Some(line), ChangeKind::Delete) | (Some(line), ChangeKind::Replace) => (
                    Some(line.line_num + 1),
                    line.content.as_str(),
                    Color::Rgb(60, 30, 30),
                ),
                (None, ChangeKind::Insert) => (None, "", Color::Rgb(30, 30, 30)),
                _ => (None, "", Color::Reset),
            }
        } else {
            match (&row.new, row.kind) {
                (Some(line), ChangeKind::Equal) => {
                    (Some(line.line_num + 1), line.content.as_str(), Color::Reset)
                }
                (Some(line), ChangeKind::Insert) | (Some(line), ChangeKind::Replace) => (
                    Some(line.line_num + 1),
                    line.content.as_str(),
                    Color::Rgb(30, 60, 30),
                ),
                (None, ChangeKind::Delete) => (None, "", Color::Rgb(30, 30, 30)),
                _ => (None, "", Color::Reset),
            }
        };

        let line_num_str = line_num
            .map(|n| format!("{:>3} ", n))
            .unwrap_or_else(|| "    ".to_string());

        // Build spans with syntax highlighting
        let mut spans = vec![Span::styled(
            line_num_str,
            Style::default().fg(Color::DarkGray).bg(bg_color),
        )];

        if content.is_empty() {
            spans.push(Span::styled("", Style::default().bg(bg_color)));
        } else {
            // Get highlight spans for this line
            let hl_spans = app.highlighter.highlight(app.current_lang, content);

            for hl in hl_spans {
                let text: String = content
                    .get(hl.start..hl.end)
                    .unwrap_or("")
                    .chars()
                    .skip(if hl.start == 0 { app.scroll_x } else { 0 })
                    .map(sanitize_char)
                    .collect();

                if !text.is_empty() {
                    let fg = style_to_color(hl.style_id);
                    spans.push(Span::styled(text, Style::default().fg(fg).bg(bg_color)));
                }
            }
        }

        lines.push(Line::from(spans));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Sanitize a character to prevent terminal escape injection.
/// Replaces control chars (except tab) with visual representations.
fn sanitize_char(c: char) -> char {
    match c {
        '\t' => '\t',                           // Allow tabs
        '\x00'..='\x1f' | '\x7f' => '\u{FFFD}', // Control chars -> replacement
        _ => c,
    }
}

/// Render the bottom bar with key hints or error message.
fn render_bottom_bar(frame: &mut Frame, app: &App, area: Rect) {
    // Show error if present
    if let Some(ref err) = app.error_msg {
        let para = Paragraph::new(format!(" Error: {}", err))
            .style(Style::default().bg(Color::Red).fg(Color::White));
        frame.render_widget(para, area);
        return;
    }

    let hints = match app.focus {
        Focus::Sidebar => "j/k: navigate  Enter: open  Space: viewed  Tab: switch  q: quit",
        Focus::Diff => {
            "j/k: scroll  h/l: horizontal  {/}: hunks  Space: viewed  Tab: switch  q: quit"
        }
    };

    let para = Paragraph::new(format!(" {}", hints))
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(para, area);
}
