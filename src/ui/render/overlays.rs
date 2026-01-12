//! Modal overlay rendering (comments, theme selector, help, PR picker).

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::core::CommentStatus;
use crate::theme::Theme;
use crate::ui::app::{App, PRActionType};

use super::helpers::truncate_str;

/// Render the comments overlay.
pub fn render_comments_overlay(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let max_overlay_width = area.width.saturating_sub(4).max(1);
    let min_overlay_width = 40.min(max_overlay_width);
    let max_overlay_width = 70.min(max_overlay_width);
    let width = (area.width * 3 / 4).clamp(min_overlay_width, max_overlay_width);

    let max_overlay_height = area.height.saturating_sub(4).max(1);
    let min_overlay_height = 6.min(max_overlay_height);
    let height =
        (app.comments.viewing.len() as u16 + 4).clamp(min_overlay_height, max_overlay_height);

    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let title = if app.comments.include_resolved {
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

    if app.comments.viewing.is_empty() {
        lines.push(Line::from(Span::styled(
            "No comments",
            Style::default()
                .fg(app.theme.text_muted)
                .bg(app.theme.bg_elevated),
        )));
    } else {
        let total = app.comments.viewing.len();
        let selected = app.comments.selected.min(total - 1);
        let visible = inner.height as usize;

        let scroll = (selected + 1).saturating_sub(visible);

        let end = (scroll + visible).min(total);

        for idx in scroll..end {
            let item = &app.comments.viewing[idx];
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

/// Render the theme selector overlay.
pub fn render_theme_selector(frame: &mut Frame, app: &App) {
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

/// Render the help overlay.
pub fn render_help_overlay(frame: &mut Frame, app: &App) {
    let entries = [
        ("j/k or ↑/↓", "Navigate files / scroll vertically"),
        ("h/l or ←/→", "Scroll horizontally in diff"),
        ("g / G", "Jump to start / end of file"),
        ("Tab, 1, 2", "Switch focus between sidebar/diff"),
        ("Space", "Toggle viewed & jump to next file"),
        ("{ / }", "Previous / next hunk"),
        ("z", "Toggle hunks-only / full file view"),
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
        ("O", "Open PR in browser (in PR mode)"),
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

/// Render the PR picker overlay.
pub fn render_pr_picker_overlay(frame: &mut Frame, app: &mut App) {
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
            app.pr.filter == crate::core::PRFilter::All,
            &app.theme,
        ),
        Span::raw("  "),
        styled_filter_tab(
            "Mine",
            app.pr.filter == crate::core::PRFilter::Mine,
            &app.theme,
        ),
        Span::raw("  "),
        styled_filter_tab(
            "Review Requested",
            app.pr.filter == crate::core::PRFilter::ReviewRequested,
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

    if app.pr.loading {
        let loading = Paragraph::new("Loading...").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(loading, list_area);
    } else if app.pr.list.is_empty() {
        let empty = Paragraph::new("No PRs found").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(empty, list_area);
    } else {
        // Render PR list
        let visible_height = list_area.height as usize;

        // Keep selection visible (mirror sidebar scroll logic)
        let max_scroll = app.pr.list.len().saturating_sub(visible_height);
        app.pr.picker_scroll = app.pr.picker_scroll.min(max_scroll);
        if app.pr.picker_selected < app.pr.picker_scroll {
            app.pr.picker_scroll = app.pr.picker_selected;
        } else if app.pr.picker_selected >= app.pr.picker_scroll + visible_height {
            app.pr.picker_scroll = app.pr.picker_selected + 1 - visible_height;
        }

        let start = app.pr.picker_scroll;
        let end = (start + visible_height).min(app.pr.list.len());

        for (i, pr) in app.pr.list[start..end].iter().enumerate() {
            let y = list_area.y + i as u16;
            let is_selected = start + i == app.pr.picker_selected;

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

/// Render a styled filter tab.
pub fn styled_filter_tab<'a>(label: &'a str, active: bool, theme: &Theme) -> Span<'a> {
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

/// Render the PR action overlay.
pub fn render_pr_action_overlay(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let width = 60.min(area.width.saturating_sub(4));
    let height = 10.min(area.height.saturating_sub(2));
    if width < 20 || height < 5 {
        return; // Terminal too small
    }
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let action_area = Rect::new(x, y, width, height);

    let title = match app.pr.action_type {
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
        app.pr.action_type,
        Some(PRActionType::Comment) | Some(PRActionType::RequestChanges)
    );

    if show_input {
        let label =
            Paragraph::new("Message (required):").style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(label, Rect::new(inner.x, inner.y, inner.width, 1));

        let input_text = format!("{}_", &app.pr.action_text);
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
