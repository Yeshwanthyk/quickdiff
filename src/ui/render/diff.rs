//! Diff pane rendering.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::core::{ChangeKind, FileChangeKind, InlineSpan};
use crate::highlight::{find_enclosing_scope, ScopeInfo, StyleId, StyledSpan};
use crate::ui::app::{App, DiffPaneMode, Focus};

use super::helpers::{
    boost_muted_fg, gutter_width, line_number_width, sanitize_char, spaces, style_to_color,
    truncate_str, visible_tab_spaces, SpanBuilder, ThemeStyles,
};

/// Render the diff view.
pub fn render_diff(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Diff;
    let border_style = if is_focused {
        app.theme_styles.border_focus
    } else {
        app.theme_styles.border_dim
    };
    let title_style = if is_focused {
        app.theme_styles.accent
    } else {
        app.theme_styles.text_muted
    };

    let effective_mode = effective_pane_mode(app, area.width);
    let title = match effective_mode {
        DiffPaneMode::Both => " Diff ",
        DiffPaneMode::OldOnly => " Diff (old only) ",
        DiffPaneMode::NewOnly => " Diff (new only) ",
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(title, title_style))
        .style(app.theme_styles.bg_dark);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    render_diff_header(frame, app, chunks[0]);
    let content = chunks[1];

    // Handle empty states
    if app.diff_loading() {
        render_state_card(
            frame,
            app,
            content,
            "Loading diff…",
            "Preparing file contents",
        );
        return;
    }

    if app.is_binary {
        render_state_card(
            frame,
            app,
            content,
            "Binary file",
            "quickdiff cannot display binary contents",
        );
        return;
    }

    let Some(diff) = &app.diff else {
        render_state_card(
            frame,
            app,
            content,
            "No diff selected",
            "Choose a file from the sidebar",
        );
        return;
    };

    if diff.rows().is_empty() || !diff.has_changes() {
        render_state_card(
            frame,
            app,
            content,
            "Files are identical",
            "No changed rows to display",
        );
        return;
    }

    match effective_mode {
        DiffPaneMode::Both => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Length(1),
                    Constraint::Percentage(50),
                ])
                .split(content);
            render_diff_pane(frame, app, panes[0], true);
            render_pane_divider(frame, panes[1], &app.theme_styles);
            render_diff_pane(frame, app, panes[2], false);
        }
        DiffPaneMode::OldOnly => {
            render_diff_pane(frame, app, content, true);
        }
        DiffPaneMode::NewOnly => {
            render_diff_pane(frame, app, content, false);
        }
    }
}

fn render_diff_header(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }

    let file = app.selected_file();
    let name = file.map(|f| f.path.as_str()).unwrap_or("No file selected");
    let kind = file.map(|f| match f.kind {
        crate::core::FileChangeKind::Added => ("A", app.theme.success),
        crate::core::FileChangeKind::Modified => ("M", app.theme.warning),
        crate::core::FileChangeKind::Deleted => ("D", app.theme.error),
        crate::core::FileChangeKind::Untracked => ("?", app.theme.text_muted),
        crate::core::FileChangeKind::Renamed => ("R", app.theme.accent_dim),
    });
    let hunk_text = app
        .current_hunk_info()
        .map(|(current, total)| format!("hunk {}/{}", current, total));
    let effective_mode = effective_pane_mode(app, area.width);
    let mode_text = match (app.viewer.pane_mode, effective_mode) {
        (DiffPaneMode::Both, DiffPaneMode::OldOnly) => "auto old",
        (DiffPaneMode::Both, DiffPaneMode::NewOnly) => "auto new",
        (_, DiffPaneMode::Both) => "split",
        (_, DiffPaneMode::OldOnly) => "old",
        (_, DiffPaneMode::NewOnly) => "new",
    };
    let view_text = match app.viewer.view_mode {
        crate::ui::app::DiffViewMode::HunksOnly => "hunks",
        crate::ui::app::DiffViewMode::FullFile => "full",
    };
    let right = [
        hunk_text.as_deref(),
        Some(view_text),
        Some(mode_text),
        Some(if app.viewer.wrap_lines {
            "wrap"
        } else {
            "nowrap"
        }),
        Some(if app.viewer.show_line_numbers {
            "nums"
        } else {
            "nonums"
        }),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("  ");

    let mut spans = Vec::new();
    spans.push(Span::styled(" ", app.theme_styles.bg_elevated));
    if let Some((label, color)) = kind {
        spans.push(Span::styled(
            format!(" {} ", label),
            Style::default()
                .fg(app.theme.bg_dark)
                .bg(color)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" ", app.theme_styles.bg_elevated));
    }

    let width = area.width as usize;
    let reserved = right.chars().count()
        + spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum::<usize>()
        + 3;
    let name_width = width.saturating_sub(reserved).max(1);
    let display_name = truncate_str(name, name_width);
    spans.push(Span::styled(
        display_name,
        app.theme_styles
            .text_bright
            .bg(app.theme.bg_elevated)
            .add_modifier(Modifier::BOLD),
    ));

    let left_len = spans
        .iter()
        .map(|s| s.content.chars().count())
        .sum::<usize>();
    let right_len = right.chars().count();
    let padding = width.saturating_sub(left_len).saturating_sub(right_len + 1);
    spans.push(Span::styled(spaces(padding), app.theme_styles.bg_elevated));
    if !right.is_empty() {
        spans.push(Span::styled(
            right,
            app.theme_styles.text_muted.bg(app.theme.bg_elevated),
        ));
        spans.push(Span::styled(" ", app.theme_styles.bg_elevated));
    }

    let para = Paragraph::new(Line::from(spans)).style(app.theme_styles.bg_elevated);
    frame.render_widget(para, area);
}

fn render_state_card(frame: &mut Frame, app: &App, area: Rect, title: &str, hint: &str) {
    let text = vec![
        Line::from(Span::styled(
            title.to_string(),
            app.theme_styles.text_bright.add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(hint.to_string(), app.theme_styles.text_muted)),
    ];
    let para = Paragraph::new(text)
        .alignment(Alignment::Center)
        .style(app.theme_styles.bg_dark);
    let y = area.y + area.height.saturating_sub(2) / 2;
    let card_area = Rect::new(area.x, y, area.width, 2.min(area.height));
    frame.render_widget(para, card_area);
}

const MIN_SPLIT_DIFF_WIDTH: u16 = 72;

fn effective_pane_mode(app: &App, width: u16) -> DiffPaneMode {
    if app.viewer.pane_mode != DiffPaneMode::Both || width >= MIN_SPLIT_DIFF_WIDTH {
        return app.viewer.pane_mode;
    }

    match app.selected_file().map(|file| file.kind) {
        Some(FileChangeKind::Deleted) => DiffPaneMode::OldOnly,
        _ => DiffPaneMode::NewOnly,
    }
}

/// Render vertical divider between old/new panes.
fn render_pane_divider(frame: &mut Frame, area: Rect, styles: &ThemeStyles) {
    let lines: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("│", styles.pane_divider)))
        .collect();
    let para = Paragraph::new(lines).style(styles.bg_dark);
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

    let max_line_num = app
        .old_buffer
        .as_ref()
        .map(|buf| buf.line_count())
        .into_iter()
        .chain(app.new_buffer.as_ref().map(|buf| buf.line_count()))
        .max()
        .unwrap_or(1);
    let line_num_width = line_number_width(max_line_num);
    let gutter = gutter_width(app.viewer.show_line_numbers, max_line_num);

    // Check for sticky scope
    let scopes = if is_old {
        &app.old_scopes
    } else {
        &app.new_scopes
    };
    let first_row = app
        .view_row_to_diff_row(app.viewer.scroll_y)
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

    let sticky_height = if has_sticky { 1 } else { 0 };
    let content_height = (area.height as usize).saturating_sub(sticky_height);
    let pane_content_width = (area.width as usize).saturating_sub(gutter);

    struct RenderedLine {
        line_num_str: String,
        bg_color: Color,
        bg_style: Style,
        code_spans: Vec<Span<'static>>,
        visible_len: usize,
        has_comment: bool,
    }

    let visible_rows = app.visible_diff_rows(content_height.max(1));
    let mut rendered: Vec<RenderedLine> = Vec::with_capacity(visible_rows.len());
    let mut max_visible_len = 0usize;

    for (row_idx, row) in visible_rows {
        let has_comment = app.is_worktree_mode()
            && diff
                .hunk_at_row(row_idx)
                .is_some_and(|h| app.comment_index.has_open_comment(h));
        let (line_ref, bg_color, inline_bg, bg_style) = if is_old {
            match (&row.old, row.kind) {
                (Some(line), ChangeKind::Equal) => (
                    Some(line),
                    app.theme.bg_dark,
                    app.theme.bg_dark,
                    app.theme_styles.diff_equal,
                ),
                (Some(line), ChangeKind::Delete) | (Some(line), ChangeKind::Replace) => (
                    Some(line),
                    app.theme.diff_delete_bg,
                    app.theme.inline_delete_bg,
                    app.theme_styles.diff_delete,
                ),
                (None, ChangeKind::Insert) => (
                    None,
                    app.theme.diff_empty_bg,
                    app.theme.diff_empty_bg,
                    app.theme_styles.diff_empty,
                ),
                _ => (
                    None,
                    app.theme.bg_dark,
                    app.theme.bg_dark,
                    app.theme_styles.diff_equal,
                ),
            }
        } else {
            match (&row.new, row.kind) {
                (Some(line), ChangeKind::Equal) => (
                    Some(line),
                    app.theme.bg_dark,
                    app.theme.bg_dark,
                    app.theme_styles.diff_equal,
                ),
                (Some(line), ChangeKind::Insert) | (Some(line), ChangeKind::Replace) => (
                    Some(line),
                    app.theme.diff_insert_bg,
                    app.theme.inline_insert_bg,
                    app.theme_styles.diff_insert,
                ),
                (None, ChangeKind::Delete) => (
                    None,
                    app.theme.diff_empty_bg,
                    app.theme.diff_empty_bg,
                    app.theme_styles.diff_empty,
                ),
                _ => (
                    None,
                    app.theme.bg_dark,
                    app.theme.bg_dark,
                    app.theme_styles.diff_equal,
                ),
            }
        };

        let line_idx = line_ref.map(|l| l.line_num);
        let line_num_str = line_idx
            .map(|n| format!("{:>width$}", n + 1, width = line_num_width))
            .unwrap_or_else(|| spaces(line_num_width).to_string());
        let content = line_ref.map(|l| l.content.as_str()).unwrap_or("");
        let inline_spans = line_ref.and_then(|l| l.inline_spans.as_ref());

        let mut builder = SpanBuilder::new();
        let mut visible_len = 0usize;

        if !content.is_empty() {
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

            let scroll_x = if app.viewer.wrap_lines {
                0
            } else {
                app.viewer.scroll_x
            };
            let content_budget = if app.viewer.wrap_lines {
                content.chars().count().saturating_add(8)
            } else {
                pane_content_width
            };
            let mut col_pos = 0usize;

            for hl in hl_spans {
                let span_text = content.get(hl.start..hl.end).unwrap_or("");
                if span_text.is_empty() {
                    continue;
                }

                let fg = style_to_color(hl.style_id, &app.theme);
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
                        content_budget,
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
                        content_budget,
                        &mut col_pos,
                        &mut visible_len,
                        hl.start,
                    );
                }
            }
        }

        let code_spans = builder.finish();
        max_visible_len = max_visible_len.max(visible_len);
        rendered.push(RenderedLine {
            line_num_str,
            bg_color,
            bg_style,
            code_spans,
            visible_len,
            has_comment,
        });
    }

    let common_left_pad = if is_old && !app.viewer.wrap_lines {
        pane_content_width.saturating_sub(max_visible_len)
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();

    if let Some(scope) = sticky_scope {
        let sticky_bg = app.theme.bg_elevated;
        let sticky_bg_style = app.theme_styles.bg_elevated;
        let display_text = if scope.name.is_empty() {
            scope.kind.to_string()
        } else {
            format!("{} {}", scope.kind, scope.name)
        };
        let text_len = display_text.chars().count();
        let mut spans: Vec<Span> = Vec::new();

        if is_old {
            let left_pad = if app.viewer.wrap_lines {
                0
            } else {
                pane_content_width.saturating_sub(text_len)
            };
            if left_pad > 0 {
                spans.push(Span::styled(spaces(left_pad), sticky_bg_style));
            }
            spans.push(Span::styled(
                display_text,
                app.theme_styles
                    .text_muted
                    .bg(sticky_bg)
                    .add_modifier(Modifier::ITALIC),
            ));
            let trailing = pane_content_width
                .saturating_sub(left_pad)
                .saturating_sub(text_len);
            if trailing > 0 {
                spans.push(Span::styled(spaces(trailing), sticky_bg_style));
            }
            append_gutter(
                &mut spans,
                None,
                false,
                sticky_bg,
                sticky_bg_style,
                is_old,
                app.viewer.show_line_numbers,
                line_num_width,
                &app.theme_styles,
            );
        } else {
            append_gutter(
                &mut spans,
                None,
                false,
                sticky_bg,
                sticky_bg_style,
                is_old,
                app.viewer.show_line_numbers,
                line_num_width,
                &app.theme_styles,
            );
            spans.push(Span::styled(
                display_text,
                app.theme_styles
                    .text_muted
                    .bg(sticky_bg)
                    .add_modifier(Modifier::ITALIC),
            ));
            let trailing = pane_content_width.saturating_sub(text_len);
            if trailing > 0 {
                spans.push(Span::styled(spaces(trailing), sticky_bg_style));
            }
        }

        lines.push(Line::from(spans));
    }

    for row in rendered {
        let wrapped_segments = wrap_rendered_segments(
            &row.code_spans,
            row.visible_len,
            pane_content_width,
            app.viewer.wrap_lines,
        );

        for (segment_idx, (segment_spans, segment_len)) in wrapped_segments.into_iter().enumerate()
        {
            let mut spans: Vec<Span> = Vec::new();
            let show_marker = segment_idx == 0 && row.has_comment;
            let line_num = if segment_idx == 0 {
                Some(row.line_num_str.as_str())
            } else {
                None
            };

            if is_old {
                let left_pad = if segment_idx == 0 { common_left_pad } else { 0 };
                if left_pad > 0 {
                    spans.push(Span::styled(spaces(left_pad), row.bg_style));
                }
                spans.extend(segment_spans);
                let trailing = pane_content_width
                    .saturating_sub(left_pad)
                    .saturating_sub(segment_len);
                if trailing > 0 {
                    spans.push(Span::styled(spaces(trailing), row.bg_style));
                }
                append_gutter(
                    &mut spans,
                    line_num,
                    show_marker,
                    row.bg_color,
                    row.bg_style,
                    is_old,
                    app.viewer.show_line_numbers,
                    line_num_width,
                    &app.theme_styles,
                );
            } else {
                append_gutter(
                    &mut spans,
                    line_num,
                    show_marker,
                    row.bg_color,
                    row.bg_style,
                    is_old,
                    app.viewer.show_line_numbers,
                    line_num_width,
                    &app.theme_styles,
                );
                spans.extend(segment_spans);
                let trailing = pane_content_width.saturating_sub(segment_len);
                if trailing > 0 {
                    spans.push(Span::styled(spaces(trailing), row.bg_style));
                }
            }

            lines.push(Line::from(spans));
        }
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

#[allow(clippy::too_many_arguments)]
fn append_gutter(
    spans: &mut Vec<Span<'static>>,
    line_num: Option<&str>,
    has_marker: bool,
    bg_color: Color,
    bg_style: Style,
    is_old: bool,
    show_line_numbers: bool,
    line_num_width: usize,
    styles: &ThemeStyles,
) {
    let marker_char = if has_marker { "•" } else { " " };
    let marker_style = if has_marker {
        styles.accent.bg(bg_color)
    } else {
        bg_style
    };
    let line_num = line_num.unwrap_or_else(|| spaces(line_num_width));

    if is_old {
        spans.push(Span::styled(marker_char, marker_style));
        spans.push(Span::styled("│", styles.gutter_sep.bg(bg_color)));
        if show_line_numbers {
            spans.push(Span::styled(
                line_num.to_string(),
                styles.text_faint.bg(bg_color),
            ));
        }
    } else {
        if show_line_numbers {
            spans.push(Span::styled(
                line_num.to_string(),
                styles.text_faint.bg(bg_color),
            ));
        }
        spans.push(Span::styled("│", styles.gutter_sep.bg(bg_color)));
        spans.push(Span::styled(marker_char, marker_style));
    }
}

fn wrap_rendered_segments(
    spans: &[Span<'static>],
    visible_len: usize,
    width: usize,
    wrap_lines: bool,
) -> Vec<(Vec<Span<'static>>, usize)> {
    if !wrap_lines || width == 0 || visible_len <= width {
        return vec![(spans.to_vec(), visible_len.min(width))];
    }

    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut current_text = String::new();
    let mut current_style: Option<Style> = None;
    let mut current_len = 0usize;

    let flush_text =
        |current: &mut Vec<Span<'static>>, text: &mut String, style: &mut Option<Style>| {
            if !text.is_empty() {
                current.push(Span::styled(
                    std::mem::take(text),
                    style.unwrap_or_default(),
                ));
            }
        };

    for span in spans {
        for ch in span.content.chars() {
            if current_len == width {
                flush_text(&mut current, &mut current_text, &mut current_style);
                lines.push((std::mem::take(&mut current), current_len));
                current_len = 0;
                current_style = None;
            }

            if current_style != Some(span.style) {
                flush_text(&mut current, &mut current_text, &mut current_style);
                current_style = Some(span.style);
            }
            current_text.push(ch);
            current_len += 1;
        }
    }

    flush_text(&mut current, &mut current_text, &mut current_style);
    if !current.is_empty() || lines.is_empty() {
        lines.push((current, current_len));
    }

    lines
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
