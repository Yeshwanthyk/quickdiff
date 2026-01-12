//! Diff pane rendering.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::core::{ChangeKind, InlineSpan};
use crate::highlight::{find_enclosing_scope, ScopeInfo, StyleId, StyledSpan};
use crate::theme::Theme;
use crate::ui::app::{App, DiffPaneMode, Focus};

use super::helpers::{
    boost_muted_fg, sanitize_char, style_to_color, visible_tab_spaces, SpanBuilder, GUTTER_WIDTH,
};

/// Render the diff view.
pub fn render_diff(frame: &mut Frame, app: &App, area: Rect) {
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

/// Compute the sticky scope for a pane based on the first visible source line.
fn compute_sticky_scope(line_num: usize, scopes: &[ScopeInfo]) -> Option<&ScopeInfo> {
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
    fg: Color,
    default_fg: Color,
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
        let active_fg = if is_changed {
            boost_muted_fg(fg, default_fg)
        } else {
            fg
        };
        let style = Style::default().fg(active_fg).bg(bg);

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
    let first_row = app
        .view_row_to_diff_row(app.scroll_y)
        .and_then(|row_idx| diff.rows().get(row_idx));
    let first_line_num = first_row.and_then(|row| {
        if is_old {
            row.old.as_ref().map(|line| line.line_num)
        } else {
            row.new.as_ref().map(|line| line.line_num)
        }
    });
    let sticky_scope = first_line_num.and_then(|line_num| compute_sticky_scope(line_num, scopes));
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
    let visible_rows = app.visible_diff_rows(content_height);
    let mut rendered: Vec<RenderedLine> = Vec::with_capacity(visible_rows.len());
    let mut max_visible_len = 0usize;

    for (row_idx, row) in visible_rows {
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
                    render_inline_span(
                        &mut builder,
                        span_text,
                        fg,
                        app.theme.text_normal,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

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
