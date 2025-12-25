//! Theme support for quickdiff.

use ratatui::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// A complete theme definition.
#[derive(Debug, Clone)]
pub struct Theme {
    // Base colors
    pub bg_dark: Color,
    pub bg_surface: Color,
    pub bg_elevated: Color,
    pub bg_selected: Color,

    // Borders
    pub border_dim: Color,
    pub border_active: Color,
    pub gutter_sep: Color,
    pub pane_divider: Color,

    // Text
    pub text_faint: Color,
    pub text_muted: Color,
    pub text_dim: Color,
    pub text_normal: Color,
    pub text_bright: Color,

    // Accent
    pub accent: Color,
    pub accent_dim: Color,

    // Diff backgrounds
    pub diff_delete_bg: Color,
    pub diff_insert_bg: Color,
    pub diff_empty_bg: Color,

    // Inline diff highlights
    pub inline_delete_bg: Color,
    pub inline_insert_bg: Color,

    // Status
    pub success: Color,
    pub error: Color,
    pub warning: Color,

    // Syntax highlighting
    pub syn_keyword: Color,
    pub syn_type: Color,
    pub syn_function: Color,
    pub syn_string: Color,
    pub syn_number: Color,
    pub syn_comment: Color,
    pub syn_operator: Color,
    pub syn_punctuation: Color,
    pub syn_constant: Color,
    pub syn_property: Color,
    pub syn_attribute: Color,
}

/// JSON theme file format.
#[derive(Debug, Deserialize)]
pub struct ThemeJson {
    #[serde(default)]
    pub defs: HashMap<String, String>,
    pub theme: ThemeColorsJson,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeColorsJson {
    // Base
    pub bg_dark: Option<String>,
    pub bg_surface: Option<String>,
    pub bg_elevated: Option<String>,
    pub bg_selected: Option<String>,

    // Borders
    pub border_dim: Option<String>,
    pub border_active: Option<String>,
    pub gutter_sep: Option<String>,
    pub pane_divider: Option<String>,

    // Text
    pub text_faint: Option<String>,
    pub text_muted: Option<String>,
    pub text_dim: Option<String>,
    pub text_normal: Option<String>,
    pub text_bright: Option<String>,

    // Accent
    pub accent: Option<String>,
    pub accent_dim: Option<String>,

    // Diff
    pub diff_delete_bg: Option<String>,
    pub diff_insert_bg: Option<String>,
    pub diff_empty_bg: Option<String>,
    pub inline_delete_bg: Option<String>,
    pub inline_insert_bg: Option<String>,

    // Status
    pub success: Option<String>,
    pub error: Option<String>,
    pub warning: Option<String>,

    // Syntax
    pub syn_keyword: Option<String>,
    pub syn_type: Option<String>,
    pub syn_function: Option<String>,
    pub syn_string: Option<String>,
    pub syn_number: Option<String>,
    pub syn_comment: Option<String>,
    pub syn_operator: Option<String>,
    pub syn_punctuation: Option<String>,
    pub syn_constant: Option<String>,
    pub syn_property: Option<String>,
    pub syn_attribute: Option<String>,
}

impl Theme {
    /// Load a theme by name. Checks user themes first, then builtin.
    pub fn load(name: &str) -> Self {
        // Try user themes first
        if let Some(theme) = load_user_theme(name) {
            return theme;
        }

        // Fall back to builtin themes
        match name {
            "dracula" => Self::dracula(),
            "catppuccin" => Self::catppuccin(),
            "nord" => Self::nord(),
            "gruvbox" => Self::gruvbox(),
            "tokyonight" => Self::tokyonight(),
            "rosepine" => Self::rosepine(),
            "onedark" | "one-dark" => Self::onedark(),
            "solarized" => Self::solarized(),
            _ => Self::builtin_default(),
        }
    }

    /// List available theme names.
    pub fn list() -> Vec<String> {
        let mut themes = vec![
            "default".to_string(),
            "dracula".to_string(),
            "catppuccin".to_string(),
            "nord".to_string(),
            "gruvbox".to_string(),
            "tokyonight".to_string(),
            "rosepine".to_string(),
            "onedark".to_string(),
            "solarized".to_string(),
        ];

        // Add user themes
        if let Some(dir) = user_themes_dir() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.path().file_stem() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            let name = name.to_string_lossy().to_string();
                            if !themes.contains(&name) {
                                themes.push(name);
                            }
                        }
                    }
                }
            }
        }

        themes.sort();
        themes
    }

    /// Default dark theme (original quickdiff colors).
    pub fn builtin_default() -> Self {
        Self {
            bg_dark: Color::Rgb(18, 18, 22),
            bg_surface: Color::Rgb(26, 26, 32),
            bg_elevated: Color::Rgb(36, 36, 44),
            bg_selected: Color::Rgb(45, 45, 55),

            border_dim: Color::Rgb(50, 50, 60),
            border_active: Color::Rgb(80, 200, 200),
            gutter_sep: Color::Rgb(38, 38, 46),
            pane_divider: Color::Rgb(55, 55, 65),

            text_faint: Color::Rgb(55, 55, 65),
            text_muted: Color::Rgb(80, 80, 92),
            text_dim: Color::Rgb(110, 110, 125),
            text_normal: Color::Rgb(175, 175, 185),
            text_bright: Color::Rgb(230, 230, 235),

            accent: Color::Rgb(80, 200, 200),
            accent_dim: Color::Rgb(55, 130, 130),

            diff_delete_bg: Color::Rgb(45, 25, 30),
            diff_insert_bg: Color::Rgb(25, 45, 32),
            diff_empty_bg: Color::Rgb(22, 22, 26),
            inline_delete_bg: Color::Rgb(90, 40, 50),
            inline_insert_bg: Color::Rgb(40, 90, 55),

            success: Color::Rgb(85, 185, 105),
            error: Color::Rgb(215, 85, 85),
            warning: Color::Rgb(215, 175, 80),

            syn_keyword: Color::Rgb(198, 120, 221),
            syn_type: Color::Rgb(229, 192, 123),
            syn_function: Color::Rgb(97, 175, 239),
            syn_string: Color::Rgb(152, 195, 121),
            syn_number: Color::Rgb(209, 154, 102),
            syn_comment: Color::Rgb(92, 99, 112),
            syn_operator: Color::Rgb(171, 178, 191),
            syn_punctuation: Color::Rgb(120, 120, 135),
            syn_constant: Color::Rgb(86, 182, 194),
            syn_property: Color::Rgb(224, 108, 117),
            syn_attribute: Color::Rgb(229, 192, 123),
        }
    }

    /// Dracula theme.
    pub fn dracula() -> Self {
        Self {
            bg_dark: Color::Rgb(40, 42, 54),
            bg_surface: Color::Rgb(33, 34, 44),
            bg_elevated: Color::Rgb(68, 71, 90),
            bg_selected: Color::Rgb(68, 71, 90),

            border_dim: Color::Rgb(68, 71, 90),
            border_active: Color::Rgb(189, 147, 249),
            gutter_sep: Color::Rgb(50, 52, 62),
            pane_divider: Color::Rgb(68, 71, 90),

            text_faint: Color::Rgb(68, 71, 90),
            text_muted: Color::Rgb(98, 114, 164),
            text_dim: Color::Rgb(128, 134, 174),
            text_normal: Color::Rgb(248, 248, 242),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(139, 233, 253),
            accent_dim: Color::Rgb(80, 150, 170),

            diff_delete_bg: Color::Rgb(58, 26, 26),
            diff_insert_bg: Color::Rgb(26, 58, 26),
            diff_empty_bg: Color::Rgb(33, 34, 44),
            inline_delete_bg: Color::Rgb(100, 40, 40),
            inline_insert_bg: Color::Rgb(40, 100, 50),

            success: Color::Rgb(80, 250, 123),
            error: Color::Rgb(255, 85, 85),
            warning: Color::Rgb(241, 250, 140),

            syn_keyword: Color::Rgb(255, 121, 198),
            syn_type: Color::Rgb(139, 233, 253),
            syn_function: Color::Rgb(80, 250, 123),
            syn_string: Color::Rgb(241, 250, 140),
            syn_number: Color::Rgb(189, 147, 249),
            syn_comment: Color::Rgb(98, 114, 164),
            syn_operator: Color::Rgb(255, 121, 198),
            syn_punctuation: Color::Rgb(248, 248, 242),
            syn_constant: Color::Rgb(189, 147, 249),
            syn_property: Color::Rgb(139, 233, 253),
            syn_attribute: Color::Rgb(80, 250, 123),
        }
    }

    /// Catppuccin Mocha theme.
    pub fn catppuccin() -> Self {
        Self {
            bg_dark: Color::Rgb(30, 30, 46),
            bg_surface: Color::Rgb(36, 39, 58),
            bg_elevated: Color::Rgb(49, 50, 68),
            bg_selected: Color::Rgb(69, 71, 90),

            border_dim: Color::Rgb(69, 71, 90),
            border_active: Color::Rgb(137, 180, 250),
            gutter_sep: Color::Rgb(49, 50, 68),
            pane_divider: Color::Rgb(69, 71, 90),

            text_faint: Color::Rgb(88, 91, 112),
            text_muted: Color::Rgb(127, 132, 156),
            text_dim: Color::Rgb(166, 173, 200),
            text_normal: Color::Rgb(205, 214, 244),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(137, 180, 250),
            accent_dim: Color::Rgb(116, 199, 236),

            diff_delete_bg: Color::Rgb(50, 30, 40),
            diff_insert_bg: Color::Rgb(30, 50, 40),
            diff_empty_bg: Color::Rgb(36, 39, 58),
            inline_delete_bg: Color::Rgb(90, 50, 60),
            inline_insert_bg: Color::Rgb(50, 90, 60),

            success: Color::Rgb(166, 227, 161),
            error: Color::Rgb(243, 139, 168),
            warning: Color::Rgb(249, 226, 175),

            syn_keyword: Color::Rgb(203, 166, 247),
            syn_type: Color::Rgb(249, 226, 175),
            syn_function: Color::Rgb(137, 180, 250),
            syn_string: Color::Rgb(166, 227, 161),
            syn_number: Color::Rgb(250, 179, 135),
            syn_comment: Color::Rgb(127, 132, 156),
            syn_operator: Color::Rgb(148, 226, 213),
            syn_punctuation: Color::Rgb(147, 153, 178),
            syn_constant: Color::Rgb(250, 179, 135),
            syn_property: Color::Rgb(243, 139, 168),
            syn_attribute: Color::Rgb(249, 226, 175),
        }
    }

    /// Nord theme.
    pub fn nord() -> Self {
        Self {
            bg_dark: Color::Rgb(46, 52, 64),
            bg_surface: Color::Rgb(59, 66, 82),
            bg_elevated: Color::Rgb(67, 76, 94),
            bg_selected: Color::Rgb(76, 86, 106),

            border_dim: Color::Rgb(67, 76, 94),
            border_active: Color::Rgb(136, 192, 208),
            gutter_sep: Color::Rgb(59, 66, 82),
            pane_divider: Color::Rgb(67, 76, 94),

            text_faint: Color::Rgb(76, 86, 106),
            text_muted: Color::Rgb(96, 106, 126),
            text_dim: Color::Rgb(150, 160, 180),
            text_normal: Color::Rgb(216, 222, 233),
            text_bright: Color::Rgb(236, 239, 244),

            accent: Color::Rgb(136, 192, 208),
            accent_dim: Color::Rgb(129, 161, 193),

            diff_delete_bg: Color::Rgb(60, 45, 50),
            diff_insert_bg: Color::Rgb(45, 60, 55),
            diff_empty_bg: Color::Rgb(59, 66, 82),
            inline_delete_bg: Color::Rgb(90, 60, 65),
            inline_insert_bg: Color::Rgb(60, 90, 75),

            success: Color::Rgb(163, 190, 140),
            error: Color::Rgb(191, 97, 106),
            warning: Color::Rgb(235, 203, 139),

            syn_keyword: Color::Rgb(180, 142, 173),
            syn_type: Color::Rgb(143, 188, 187),
            syn_function: Color::Rgb(136, 192, 208),
            syn_string: Color::Rgb(163, 190, 140),
            syn_number: Color::Rgb(180, 142, 173),
            syn_comment: Color::Rgb(96, 106, 126),
            syn_operator: Color::Rgb(129, 161, 193),
            syn_punctuation: Color::Rgb(216, 222, 233),
            syn_constant: Color::Rgb(180, 142, 173),
            syn_property: Color::Rgb(136, 192, 208),
            syn_attribute: Color::Rgb(235, 203, 139),
        }
    }

    /// Gruvbox Dark theme.
    pub fn gruvbox() -> Self {
        Self {
            bg_dark: Color::Rgb(40, 40, 40),
            bg_surface: Color::Rgb(50, 48, 47),
            bg_elevated: Color::Rgb(60, 56, 54),
            bg_selected: Color::Rgb(80, 73, 69),

            border_dim: Color::Rgb(60, 56, 54),
            border_active: Color::Rgb(215, 153, 33),
            gutter_sep: Color::Rgb(50, 48, 47),
            pane_divider: Color::Rgb(60, 56, 54),

            text_faint: Color::Rgb(80, 73, 69),
            text_muted: Color::Rgb(146, 131, 116),
            text_dim: Color::Rgb(168, 153, 132),
            text_normal: Color::Rgb(235, 219, 178),
            text_bright: Color::Rgb(251, 241, 199),

            accent: Color::Rgb(215, 153, 33),
            accent_dim: Color::Rgb(152, 151, 26),

            diff_delete_bg: Color::Rgb(60, 35, 35),
            diff_insert_bg: Color::Rgb(35, 55, 35),
            diff_empty_bg: Color::Rgb(50, 48, 47),
            inline_delete_bg: Color::Rgb(100, 50, 50),
            inline_insert_bg: Color::Rgb(50, 90, 50),

            success: Color::Rgb(152, 151, 26),
            error: Color::Rgb(204, 36, 29),
            warning: Color::Rgb(250, 189, 47),

            syn_keyword: Color::Rgb(204, 36, 29),
            syn_type: Color::Rgb(250, 189, 47),
            syn_function: Color::Rgb(184, 187, 38),
            syn_string: Color::Rgb(184, 187, 38),
            syn_number: Color::Rgb(211, 134, 155),
            syn_comment: Color::Rgb(146, 131, 116),
            syn_operator: Color::Rgb(254, 128, 25),
            syn_punctuation: Color::Rgb(168, 153, 132),
            syn_constant: Color::Rgb(211, 134, 155),
            syn_property: Color::Rgb(131, 165, 152),
            syn_attribute: Color::Rgb(250, 189, 47),
        }
    }

    /// Tokyo Night theme.
    pub fn tokyonight() -> Self {
        Self {
            bg_dark: Color::Rgb(26, 27, 38),
            bg_surface: Color::Rgb(36, 40, 59),
            bg_elevated: Color::Rgb(41, 46, 66),
            bg_selected: Color::Rgb(51, 59, 81),

            border_dim: Color::Rgb(41, 46, 66),
            border_active: Color::Rgb(125, 207, 255),
            gutter_sep: Color::Rgb(36, 40, 59),
            pane_divider: Color::Rgb(41, 46, 66),

            text_faint: Color::Rgb(61, 66, 86),
            text_muted: Color::Rgb(86, 95, 137),
            text_dim: Color::Rgb(145, 152, 179),
            text_normal: Color::Rgb(192, 202, 245),
            text_bright: Color::Rgb(220, 230, 255),

            accent: Color::Rgb(125, 207, 255),
            accent_dim: Color::Rgb(65, 166, 181),

            diff_delete_bg: Color::Rgb(50, 30, 40),
            diff_insert_bg: Color::Rgb(30, 50, 40),
            diff_empty_bg: Color::Rgb(36, 40, 59),
            inline_delete_bg: Color::Rgb(90, 50, 60),
            inline_insert_bg: Color::Rgb(50, 90, 60),

            success: Color::Rgb(158, 206, 106),
            error: Color::Rgb(247, 118, 142),
            warning: Color::Rgb(224, 175, 104),

            syn_keyword: Color::Rgb(187, 154, 247),
            syn_type: Color::Rgb(42, 195, 222),
            syn_function: Color::Rgb(125, 207, 255),
            syn_string: Color::Rgb(158, 206, 106),
            syn_number: Color::Rgb(255, 158, 100),
            syn_comment: Color::Rgb(86, 95, 137),
            syn_operator: Color::Rgb(137, 221, 255),
            syn_punctuation: Color::Rgb(145, 152, 179),
            syn_constant: Color::Rgb(255, 158, 100),
            syn_property: Color::Rgb(115, 218, 202),
            syn_attribute: Color::Rgb(224, 175, 104),
        }
    }

    /// Rosé Pine theme.
    pub fn rosepine() -> Self {
        Self {
            bg_dark: Color::Rgb(25, 23, 36),
            bg_surface: Color::Rgb(31, 29, 46),
            bg_elevated: Color::Rgb(38, 35, 58),
            bg_selected: Color::Rgb(57, 53, 82),

            border_dim: Color::Rgb(38, 35, 58),
            border_active: Color::Rgb(196, 167, 231),
            gutter_sep: Color::Rgb(31, 29, 46),
            pane_divider: Color::Rgb(38, 35, 58),

            text_faint: Color::Rgb(57, 53, 82),
            text_muted: Color::Rgb(110, 106, 134),
            text_dim: Color::Rgb(144, 140, 170),
            text_normal: Color::Rgb(224, 222, 244),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(196, 167, 231),
            accent_dim: Color::Rgb(156, 207, 216),

            diff_delete_bg: Color::Rgb(50, 30, 35),
            diff_insert_bg: Color::Rgb(30, 50, 40),
            diff_empty_bg: Color::Rgb(31, 29, 46),
            inline_delete_bg: Color::Rgb(90, 50, 55),
            inline_insert_bg: Color::Rgb(50, 90, 60),

            success: Color::Rgb(156, 207, 216),
            error: Color::Rgb(235, 111, 146),
            warning: Color::Rgb(246, 193, 119),

            syn_keyword: Color::Rgb(49, 116, 143),
            syn_type: Color::Rgb(156, 207, 216),
            syn_function: Color::Rgb(235, 188, 186),
            syn_string: Color::Rgb(246, 193, 119),
            syn_number: Color::Rgb(234, 154, 151),
            syn_comment: Color::Rgb(110, 106, 134),
            syn_operator: Color::Rgb(144, 140, 170),
            syn_punctuation: Color::Rgb(144, 140, 170),
            syn_constant: Color::Rgb(234, 154, 151),
            syn_property: Color::Rgb(196, 167, 231),
            syn_attribute: Color::Rgb(156, 207, 216),
        }
    }

    /// One Dark theme.
    pub fn onedark() -> Self {
        Self {
            bg_dark: Color::Rgb(40, 44, 52),
            bg_surface: Color::Rgb(33, 37, 43),
            bg_elevated: Color::Rgb(50, 56, 66),
            bg_selected: Color::Rgb(60, 66, 76),

            border_dim: Color::Rgb(50, 56, 66),
            border_active: Color::Rgb(97, 175, 239),
            gutter_sep: Color::Rgb(40, 44, 52),
            pane_divider: Color::Rgb(50, 56, 66),

            text_faint: Color::Rgb(60, 66, 76),
            text_muted: Color::Rgb(92, 99, 112),
            text_dim: Color::Rgb(127, 132, 142),
            text_normal: Color::Rgb(171, 178, 191),
            text_bright: Color::Rgb(220, 223, 228),

            accent: Color::Rgb(97, 175, 239),
            accent_dim: Color::Rgb(86, 182, 194),

            diff_delete_bg: Color::Rgb(55, 35, 40),
            diff_insert_bg: Color::Rgb(35, 55, 40),
            diff_empty_bg: Color::Rgb(33, 37, 43),
            inline_delete_bg: Color::Rgb(95, 55, 60),
            inline_insert_bg: Color::Rgb(55, 95, 60),

            success: Color::Rgb(152, 195, 121),
            error: Color::Rgb(224, 108, 117),
            warning: Color::Rgb(229, 192, 123),

            syn_keyword: Color::Rgb(198, 120, 221),
            syn_type: Color::Rgb(229, 192, 123),
            syn_function: Color::Rgb(97, 175, 239),
            syn_string: Color::Rgb(152, 195, 121),
            syn_number: Color::Rgb(209, 154, 102),
            syn_comment: Color::Rgb(92, 99, 112),
            syn_operator: Color::Rgb(171, 178, 191),
            syn_punctuation: Color::Rgb(127, 132, 142),
            syn_constant: Color::Rgb(86, 182, 194),
            syn_property: Color::Rgb(224, 108, 117),
            syn_attribute: Color::Rgb(229, 192, 123),
        }
    }

    /// Solarized Dark theme.
    pub fn solarized() -> Self {
        Self {
            bg_dark: Color::Rgb(0, 43, 54),
            bg_surface: Color::Rgb(7, 54, 66),
            bg_elevated: Color::Rgb(19, 66, 78),
            bg_selected: Color::Rgb(31, 78, 90),

            border_dim: Color::Rgb(19, 66, 78),
            border_active: Color::Rgb(38, 139, 210),
            gutter_sep: Color::Rgb(7, 54, 66),
            pane_divider: Color::Rgb(19, 66, 78),

            text_faint: Color::Rgb(88, 110, 117),
            text_muted: Color::Rgb(101, 123, 131),
            text_dim: Color::Rgb(131, 148, 150),
            text_normal: Color::Rgb(147, 161, 161),
            text_bright: Color::Rgb(253, 246, 227),

            accent: Color::Rgb(38, 139, 210),
            accent_dim: Color::Rgb(42, 161, 152),

            diff_delete_bg: Color::Rgb(40, 40, 50),
            diff_insert_bg: Color::Rgb(20, 60, 50),
            diff_empty_bg: Color::Rgb(7, 54, 66),
            inline_delete_bg: Color::Rgb(80, 50, 60),
            inline_insert_bg: Color::Rgb(40, 100, 80),

            success: Color::Rgb(133, 153, 0),
            error: Color::Rgb(220, 50, 47),
            warning: Color::Rgb(181, 137, 0),

            syn_keyword: Color::Rgb(203, 75, 22),
            syn_type: Color::Rgb(181, 137, 0),
            syn_function: Color::Rgb(38, 139, 210),
            syn_string: Color::Rgb(42, 161, 152),
            syn_number: Color::Rgb(108, 113, 196),
            syn_comment: Color::Rgb(88, 110, 117),
            syn_operator: Color::Rgb(133, 153, 0),
            syn_punctuation: Color::Rgb(101, 123, 131),
            syn_constant: Color::Rgb(108, 113, 196),
            syn_property: Color::Rgb(38, 139, 210),
            syn_attribute: Color::Rgb(181, 137, 0),
        }
    }
}



/// Get user themes directory (~/.config/quickdiff/themes/).
fn user_themes_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("quickdiff").join("themes"))
}

/// Load a theme from user themes directory.
fn load_user_theme(name: &str) -> Option<Theme> {
    let dir = user_themes_dir()?;
    let path = dir.join(format!("{}.json", name));

    if !path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&path).ok()?;
    let json: ThemeJson = serde_json::from_str(&content).ok()?;

    Some(resolve_theme(&json))
}

/// Parse a hex color string to Color.
fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }

    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;

    Some(Color::Rgb(r, g, b))
}

/// Resolve a color value (hex or reference).
fn resolve_color(value: &str, defs: &HashMap<String, String>, fallback: Color) -> Color {
    if value.starts_with('#') {
        parse_hex(value).unwrap_or(fallback)
    } else if let Some(def) = defs.get(value) {
        parse_hex(def).unwrap_or(fallback)
    } else {
        fallback
    }
}

/// Resolve a theme JSON to a Theme struct.
fn resolve_theme(json: &ThemeJson) -> Theme {
    let default = Theme::builtin_default();
    let defs = &json.defs;
    let t = &json.theme;

    Theme {
        bg_dark: t
            .bg_dark
            .as_ref()
            .map_or(default.bg_dark, |v| resolve_color(v, defs, default.bg_dark)),
        bg_surface: t.bg_surface.as_ref().map_or(default.bg_surface, |v| {
            resolve_color(v, defs, default.bg_surface)
        }),
        bg_elevated: t.bg_elevated.as_ref().map_or(default.bg_elevated, |v| {
            resolve_color(v, defs, default.bg_elevated)
        }),
        bg_selected: t.bg_selected.as_ref().map_or(default.bg_selected, |v| {
            resolve_color(v, defs, default.bg_selected)
        }),
        border_dim: t.border_dim.as_ref().map_or(default.border_dim, |v| {
            resolve_color(v, defs, default.border_dim)
        }),
        border_active: t.border_active.as_ref().map_or(default.border_active, |v| {
            resolve_color(v, defs, default.border_active)
        }),
        gutter_sep: t.gutter_sep.as_ref().map_or(default.gutter_sep, |v| {
            resolve_color(v, defs, default.gutter_sep)
        }),
        pane_divider: t.pane_divider.as_ref().map_or(default.pane_divider, |v| {
            resolve_color(v, defs, default.pane_divider)
        }),
        text_faint: t.text_faint.as_ref().map_or(default.text_faint, |v| {
            resolve_color(v, defs, default.text_faint)
        }),
        text_muted: t.text_muted.as_ref().map_or(default.text_muted, |v| {
            resolve_color(v, defs, default.text_muted)
        }),
        text_dim: t.text_dim.as_ref().map_or(default.text_dim, |v| {
            resolve_color(v, defs, default.text_dim)
        }),
        text_normal: t.text_normal.as_ref().map_or(default.text_normal, |v| {
            resolve_color(v, defs, default.text_normal)
        }),
        text_bright: t.text_bright.as_ref().map_or(default.text_bright, |v| {
            resolve_color(v, defs, default.text_bright)
        }),
        accent: t
            .accent
            .as_ref()
            .map_or(default.accent, |v| resolve_color(v, defs, default.accent)),
        accent_dim: t.accent_dim.as_ref().map_or(default.accent_dim, |v| {
            resolve_color(v, defs, default.accent_dim)
        }),
        diff_delete_bg: t
            .diff_delete_bg
            .as_ref()
            .map_or(default.diff_delete_bg, |v| {
                resolve_color(v, defs, default.diff_delete_bg)
            }),
        diff_insert_bg: t
            .diff_insert_bg
            .as_ref()
            .map_or(default.diff_insert_bg, |v| {
                resolve_color(v, defs, default.diff_insert_bg)
            }),
        diff_empty_bg: t.diff_empty_bg.as_ref().map_or(default.diff_empty_bg, |v| {
            resolve_color(v, defs, default.diff_empty_bg)
        }),
        inline_delete_bg: t
            .inline_delete_bg
            .as_ref()
            .map_or(default.inline_delete_bg, |v| {
                resolve_color(v, defs, default.inline_delete_bg)
            }),
        inline_insert_bg: t
            .inline_insert_bg
            .as_ref()
            .map_or(default.inline_insert_bg, |v| {
                resolve_color(v, defs, default.inline_insert_bg)
            }),
        success: t
            .success
            .as_ref()
            .map_or(default.success, |v| resolve_color(v, defs, default.success)),
        error: t
            .error
            .as_ref()
            .map_or(default.error, |v| resolve_color(v, defs, default.error)),
        warning: t
            .warning
            .as_ref()
            .map_or(default.warning, |v| resolve_color(v, defs, default.warning)),
        syn_keyword: t.syn_keyword.as_ref().map_or(default.syn_keyword, |v| {
            resolve_color(v, defs, default.syn_keyword)
        }),
        syn_type: t.syn_type.as_ref().map_or(default.syn_type, |v| {
            resolve_color(v, defs, default.syn_type)
        }),
        syn_function: t.syn_function.as_ref().map_or(default.syn_function, |v| {
            resolve_color(v, defs, default.syn_function)
        }),
        syn_string: t.syn_string.as_ref().map_or(default.syn_string, |v| {
            resolve_color(v, defs, default.syn_string)
        }),
        syn_number: t.syn_number.as_ref().map_or(default.syn_number, |v| {
            resolve_color(v, defs, default.syn_number)
        }),
        syn_comment: t.syn_comment.as_ref().map_or(default.syn_comment, |v| {
            resolve_color(v, defs, default.syn_comment)
        }),
        syn_operator: t.syn_operator.as_ref().map_or(default.syn_operator, |v| {
            resolve_color(v, defs, default.syn_operator)
        }),
        syn_punctuation: t
            .syn_punctuation
            .as_ref()
            .map_or(default.syn_punctuation, |v| {
                resolve_color(v, defs, default.syn_punctuation)
            }),
        syn_constant: t.syn_constant.as_ref().map_or(default.syn_constant, |v| {
            resolve_color(v, defs, default.syn_constant)
        }),
        syn_property: t.syn_property.as_ref().map_or(default.syn_property, |v| {
            resolve_color(v, defs, default.syn_property)
        }),
        syn_attribute: t.syn_attribute.as_ref().map_or(default.syn_attribute, |v| {
            resolve_color(v, defs, default.syn_attribute)
        }),
    }
}
