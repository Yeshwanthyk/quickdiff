//! Sidebar file list rendering.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::core::{FileChangeKind, ViewedStore};
use crate::ui::app::{App, Focus};

use super::helpers::SIDEBAR_PATH_WIDTH;

/// Render the file list sidebar.
pub fn render_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
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
    let title = if app.sidebar.filtered_indices.is_empty() {
        format!(" Files ({}) ", app.viewed_status())
    } else {
        format!(
            " Files ({}) [filter: {}] ",
            app.sidebar.filtered_indices.len(),
            app.sidebar.filter
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
    let visible_indices: Vec<usize> = if app.sidebar.filtered_indices.is_empty() {
        (0..app.files.len()).collect()
    } else {
        app.sidebar.filtered_indices.clone()
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
        .position(|&idx| idx == app.sidebar.selected_idx)
        .unwrap_or(0);

    // Keep selection visible
    let max_scroll = visible_indices.len().saturating_sub(height);
    app.sidebar.scroll = app.sidebar.scroll.min(max_scroll);

    if selected_pos < app.sidebar.scroll {
        app.sidebar.scroll = selected_pos;
    } else if selected_pos >= app.sidebar.scroll + height {
        app.sidebar.scroll = selected_pos + 1 - height;
    }

    let end = (app.sidebar.scroll + height).min(visible_indices.len());

    let mut lines: Vec<Line> = Vec::with_capacity(height);
    for &idx in visible_indices
        .iter()
        .skip(app.sidebar.scroll)
        .take(end - app.sidebar.scroll)
    {
        let file = &app.files[idx];
        let is_selected = idx == app.sidebar.selected_idx;
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

        // Ellipsize path (char-safe for UTF-8)
        let path = file.path.as_str();
        let char_count = path.chars().count();
        let display_path = if char_count > SIDEBAR_PATH_WIDTH {
            let skip = char_count - SIDEBAR_PATH_WIDTH + 1;
            let truncated: String = path.chars().skip(skip).collect();
            format!("…{}", truncated)
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
