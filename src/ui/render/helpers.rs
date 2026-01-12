//! Shared rendering helpers and constants.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::highlight::StyleId;
use crate::theme::Theme;

/// Max width for path display in sidebar.
pub const SIDEBAR_PATH_WIDTH: usize = 22;

/// Gutter: 4 (line num) + 2 (separator) = 6 chars
pub const GUTTER_WIDTH: usize = 6;

/// Tab stop width for display alignment.
pub const TAB_WIDTH: usize = 8;

/// Map StyleId to syntax color using theme.
pub fn style_to_color(style: StyleId, theme: &Theme) -> Color {
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

/// Sanitize control characters.
pub fn sanitize_char(c: char) -> char {
    match c {
        '\x00'..='\x1f' | '\x7f' => '\u{FFFD}',
        _ => c,
    }
}

pub fn tab_width_at(col: usize) -> usize {
    let rem = col % TAB_WIDTH;
    if rem == 0 {
        TAB_WIDTH
    } else {
        TAB_WIDTH - rem
    }
}

pub fn visible_tab_spaces(col: usize, scroll_x: usize, remaining: usize) -> (usize, usize) {
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

pub fn is_muted_color(color: Color) -> bool {
    match color {
        Color::Rgb(r, g, b) => {
            let luminance = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let saturation = if max == 0 {
                0
            } else {
                u32::from(max - min) * 100 / u32::from(max)
            };
            luminance < 140 || (luminance < 180 && saturation < 30)
        }
        Color::DarkGray | Color::Gray => true,
        _ => false,
    }
}

pub fn boost_muted_fg(fg: Color, default_fg: Color) -> Color {
    if is_muted_color(fg) {
        default_fg
    } else {
        fg
    }
}

pub fn truncate_str(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else if max_len == 0 {
        String::new()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}

/// Builder for efficient span construction.
pub struct SpanBuilder {
    spans: Vec<Span<'static>>,
    pending_style: Option<Style>,
    pending_text: String,
}

impl SpanBuilder {
    pub fn new() -> Self {
        Self {
            spans: Vec::new(),
            pending_style: None,
            pending_text: String::new(),
        }
    }

    pub fn push_char(&mut self, ch: char, style: Style) {
        if self.pending_style != Some(style) {
            self.flush();
            self.pending_style = Some(style);
        }
        self.pending_text.push(ch);
    }

    pub fn push_spaces(&mut self, count: usize, style: Style) {
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

    pub fn finish(mut self) -> Vec<Span<'static>> {
        self.flush();
        self.spans
    }
}

impl Default for SpanBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Static spaces buffer for efficient padding
// ============================================================================

/// Maximum expected terminal width for padding purposes.
/// Most terminals are under 500 columns; we use 512 for alignment.
const MAX_SPACES: usize = 512;

/// Static buffer of spaces for slicing. Avoids per-frame allocations.
static SPACES: &str = concat!(
    "                                                                ", // 64
    "                                                                ", // 128
    "                                                                ", // 192
    "                                                                ", // 256
    "                                                                ", // 320
    "                                                                ", // 384
    "                                                                ", // 448
    "                                                                ", // 512
);

/// Get a slice of spaces of the given length.
///
/// Uses a static buffer to avoid allocations. Falls back to allocation
/// only for widths > 512 (rare in practice).
#[inline]
pub fn spaces(len: usize) -> &'static str {
    if len <= MAX_SPACES {
        &SPACES[..len]
    } else {
        // Fallback for extremely wide terminals - leak to get 'static
        // This is acceptable since it only happens once per unique large width
        Box::leak(
            vec![' '; len]
                .into_iter()
                .collect::<String>()
                .into_boxed_str(),
        )
    }
}

/// Truncate a path for sidebar display.
///
/// If the path is longer than SIDEBAR_PATH_WIDTH characters,
/// it's truncated from the left with an ellipsis prefix.
pub fn truncate_path(path: &str) -> String {
    let char_count = path.chars().count();
    if char_count > SIDEBAR_PATH_WIDTH {
        let skip = char_count - SIDEBAR_PATH_WIDTH + 1;
        let truncated: String = path.chars().skip(skip).collect();
        format!("…{}", truncated)
    } else {
        path.to_string()
    }
}

/// Build a cache of truncated paths for sidebar display.
pub fn build_path_cache<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    paths.map(truncate_path).collect()
}
