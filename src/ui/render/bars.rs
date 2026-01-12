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
