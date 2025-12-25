//! Theme support for quickdiff.

use ratatui::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// A complete theme definition.
///
/// All fields are public for direct access. Field names are self-documenting
/// (e.g., `bg_dark` = dark background, `text_muted` = muted text color).
#[derive(Debug, Clone)]
#[allow(missing_docs)]
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

impl Default for Theme {
    fn default() -> Self {
        Self::builtin_default()
    }
}

/// JSON theme file format.
#[derive(Debug, Deserialize)]
#[allow(missing_docs)]
pub struct ThemeJson {
    #[serde(default)]
    pub defs: HashMap<String, String>,
    pub theme: ThemeColorsJson,
}

/// Theme color definitions from JSON.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
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
            "monokai" => Self::monokai(),
            "github" => Self::github(),
            "kanagawa" => Self::kanagawa(),
            "everforest" => Self::everforest(),
            "nightowl" => Self::nightowl(),
            "ayu" => Self::ayu(),
            "palenight" => Self::palenight(),
            "zenburn" => Self::zenburn(),
            _ => Self::builtin_default(),
        }
    }

    /// List available theme names.
    pub fn list() -> Vec<String> {
        let mut themes = vec![
            "default".to_string(),
            "ayu".to_string(),
            "catppuccin".to_string(),
            "dracula".to_string(),
            "everforest".to_string(),
            "github".to_string(),
            "gruvbox".to_string(),
            "kanagawa".to_string(),
            "monokai".to_string(),
            "nightowl".to_string(),
            "solarized".to_string(),
            "tokyonight".to_string(),
            "zenburn".to_string(),
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

    /// Monokai theme.
    pub fn monokai() -> Self {
        Self {
            bg_dark: Color::Rgb(39, 40, 34),
            bg_surface: Color::Rgb(49, 50, 44),
            bg_elevated: Color::Rgb(59, 60, 54),
            bg_selected: Color::Rgb(73, 72, 62),

            border_dim: Color::Rgb(59, 60, 54),
            border_active: Color::Rgb(166, 226, 46),
            gutter_sep: Color::Rgb(49, 50, 44),
            pane_divider: Color::Rgb(59, 60, 54),

            text_faint: Color::Rgb(70, 71, 65),
            text_muted: Color::Rgb(117, 113, 94),
            text_dim: Color::Rgb(150, 145, 130),
            text_normal: Color::Rgb(248, 248, 242),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(166, 226, 46),
            accent_dim: Color::Rgb(102, 217, 239),

            diff_delete_bg: Color::Rgb(60, 35, 40),
            diff_insert_bg: Color::Rgb(40, 60, 35),
            diff_empty_bg: Color::Rgb(49, 50, 44),
            inline_delete_bg: Color::Rgb(100, 50, 60),
            inline_insert_bg: Color::Rgb(60, 100, 50),

            success: Color::Rgb(166, 226, 46),
            error: Color::Rgb(249, 38, 114),
            warning: Color::Rgb(230, 219, 116),

            syn_keyword: Color::Rgb(249, 38, 114),
            syn_type: Color::Rgb(102, 217, 239),
            syn_function: Color::Rgb(166, 226, 46),
            syn_string: Color::Rgb(230, 219, 116),
            syn_number: Color::Rgb(174, 129, 255),
            syn_comment: Color::Rgb(117, 113, 94),
            syn_operator: Color::Rgb(249, 38, 114),
            syn_punctuation: Color::Rgb(248, 248, 242),
            syn_constant: Color::Rgb(174, 129, 255),
            syn_property: Color::Rgb(102, 217, 239),
            syn_attribute: Color::Rgb(166, 226, 46),
        }
    }

    /// GitHub Dark theme.
    pub fn github() -> Self {
        Self {
            bg_dark: Color::Rgb(13, 17, 23),
            bg_surface: Color::Rgb(22, 27, 34),
            bg_elevated: Color::Rgb(33, 38, 45),
            bg_selected: Color::Rgb(48, 54, 61),

            border_dim: Color::Rgb(48, 54, 61),
            border_active: Color::Rgb(88, 166, 255),
            gutter_sep: Color::Rgb(33, 38, 45),
            pane_divider: Color::Rgb(48, 54, 61),

            text_faint: Color::Rgb(72, 79, 88),
            text_muted: Color::Rgb(125, 133, 144),
            text_dim: Color::Rgb(160, 168, 178),
            text_normal: Color::Rgb(201, 209, 217),
            text_bright: Color::Rgb(240, 246, 252),

            accent: Color::Rgb(88, 166, 255),
            accent_dim: Color::Rgb(56, 139, 253),

            diff_delete_bg: Color::Rgb(50, 25, 30),
            diff_insert_bg: Color::Rgb(25, 50, 35),
            diff_empty_bg: Color::Rgb(22, 27, 34),
            inline_delete_bg: Color::Rgb(90, 40, 50),
            inline_insert_bg: Color::Rgb(40, 90, 55),

            success: Color::Rgb(63, 185, 80),
            error: Color::Rgb(248, 81, 73),
            warning: Color::Rgb(210, 153, 34),

            syn_keyword: Color::Rgb(255, 123, 114),
            syn_type: Color::Rgb(255, 166, 87),
            syn_function: Color::Rgb(210, 168, 255),
            syn_string: Color::Rgb(165, 214, 255),
            syn_number: Color::Rgb(121, 192, 255),
            syn_comment: Color::Rgb(125, 133, 144),
            syn_operator: Color::Rgb(255, 123, 114),
            syn_punctuation: Color::Rgb(201, 209, 217),
            syn_constant: Color::Rgb(121, 192, 255),
            syn_property: Color::Rgb(121, 192, 255),
            syn_attribute: Color::Rgb(255, 166, 87),
        }
    }

    /// Kanagawa theme.
    pub fn kanagawa() -> Self {
        Self {
            bg_dark: Color::Rgb(31, 31, 40),
            bg_surface: Color::Rgb(42, 42, 54),
            bg_elevated: Color::Rgb(54, 54, 70),
            bg_selected: Color::Rgb(73, 73, 95),

            border_dim: Color::Rgb(54, 54, 70),
            border_active: Color::Rgb(192, 163, 142),
            gutter_sep: Color::Rgb(42, 42, 54),
            pane_divider: Color::Rgb(54, 54, 70),

            text_faint: Color::Rgb(84, 84, 109),
            text_muted: Color::Rgb(114, 114, 135),
            text_dim: Color::Rgb(150, 150, 168),
            text_normal: Color::Rgb(220, 215, 186),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(192, 163, 142),
            accent_dim: Color::Rgb(126, 156, 160),

            diff_delete_bg: Color::Rgb(55, 35, 40),
            diff_insert_bg: Color::Rgb(35, 55, 45),
            diff_empty_bg: Color::Rgb(42, 42, 54),
            inline_delete_bg: Color::Rgb(95, 55, 60),
            inline_insert_bg: Color::Rgb(55, 95, 65),

            success: Color::Rgb(152, 187, 108),
            error: Color::Rgb(195, 64, 67),
            warning: Color::Rgb(255, 169, 88),

            syn_keyword: Color::Rgb(149, 127, 184),
            syn_type: Color::Rgb(122, 170, 153),
            syn_function: Color::Rgb(126, 156, 160),
            syn_string: Color::Rgb(152, 187, 108),
            syn_number: Color::Rgb(208, 126, 139),
            syn_comment: Color::Rgb(114, 114, 135),
            syn_operator: Color::Rgb(192, 163, 142),
            syn_punctuation: Color::Rgb(156, 154, 141),
            syn_constant: Color::Rgb(255, 169, 88),
            syn_property: Color::Rgb(126, 156, 160),
            syn_attribute: Color::Rgb(122, 170, 153),
        }
    }

    /// Everforest theme.
    pub fn everforest() -> Self {
        Self {
            bg_dark: Color::Rgb(47, 53, 55),
            bg_surface: Color::Rgb(52, 59, 61),
            bg_elevated: Color::Rgb(59, 66, 68),
            bg_selected: Color::Rgb(78, 86, 89),

            border_dim: Color::Rgb(59, 66, 68),
            border_active: Color::Rgb(163, 190, 140),
            gutter_sep: Color::Rgb(52, 59, 61),
            pane_divider: Color::Rgb(59, 66, 68),

            text_faint: Color::Rgb(78, 86, 89),
            text_muted: Color::Rgb(127, 132, 120),
            text_dim: Color::Rgb(157, 163, 147),
            text_normal: Color::Rgb(211, 198, 170),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(163, 190, 140),
            accent_dim: Color::Rgb(131, 165, 152),

            diff_delete_bg: Color::Rgb(60, 45, 45),
            diff_insert_bg: Color::Rgb(45, 60, 50),
            diff_empty_bg: Color::Rgb(52, 59, 61),
            inline_delete_bg: Color::Rgb(100, 65, 65),
            inline_insert_bg: Color::Rgb(65, 100, 70),

            success: Color::Rgb(163, 190, 140),
            error: Color::Rgb(230, 126, 128),
            warning: Color::Rgb(219, 188, 127),

            syn_keyword: Color::Rgb(230, 126, 128),
            syn_type: Color::Rgb(219, 188, 127),
            syn_function: Color::Rgb(163, 190, 140),
            syn_string: Color::Rgb(163, 190, 140),
            syn_number: Color::Rgb(214, 153, 182),
            syn_comment: Color::Rgb(127, 132, 120),
            syn_operator: Color::Rgb(230, 152, 117),
            syn_punctuation: Color::Rgb(157, 163, 147),
            syn_constant: Color::Rgb(214, 153, 182),
            syn_property: Color::Rgb(131, 165, 152),
            syn_attribute: Color::Rgb(219, 188, 127),
        }
    }

    /// Night Owl theme.
    pub fn nightowl() -> Self {
        Self {
            bg_dark: Color::Rgb(1, 22, 39),
            bg_surface: Color::Rgb(1, 32, 49),
            bg_elevated: Color::Rgb(1, 42, 59),
            bg_selected: Color::Rgb(14, 52, 69),

            border_dim: Color::Rgb(14, 52, 69),
            border_active: Color::Rgb(130, 170, 255),
            gutter_sep: Color::Rgb(1, 32, 49),
            pane_divider: Color::Rgb(14, 52, 69),

            text_faint: Color::Rgb(65, 100, 130),
            text_muted: Color::Rgb(100, 138, 168),
            text_dim: Color::Rgb(145, 172, 192),
            text_normal: Color::Rgb(214, 222, 235),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(130, 170, 255),
            accent_dim: Color::Rgb(127, 219, 202),

            diff_delete_bg: Color::Rgb(40, 25, 35),
            diff_insert_bg: Color::Rgb(15, 50, 45),
            diff_empty_bg: Color::Rgb(1, 32, 49),
            inline_delete_bg: Color::Rgb(80, 40, 55),
            inline_insert_bg: Color::Rgb(30, 90, 75),

            success: Color::Rgb(34, 218, 166),
            error: Color::Rgb(239, 83, 80),
            warning: Color::Rgb(255, 203, 139),

            syn_keyword: Color::Rgb(199, 146, 234),
            syn_type: Color::Rgb(255, 203, 139),
            syn_function: Color::Rgb(130, 170, 255),
            syn_string: Color::Rgb(173, 219, 103),
            syn_number: Color::Rgb(247, 140, 108),
            syn_comment: Color::Rgb(100, 138, 168),
            syn_operator: Color::Rgb(127, 219, 202),
            syn_punctuation: Color::Rgb(214, 222, 235),
            syn_constant: Color::Rgb(247, 140, 108),
            syn_property: Color::Rgb(127, 219, 202),
            syn_attribute: Color::Rgb(255, 203, 139),
        }
    }

    /// Ayu Dark theme.
    pub fn ayu() -> Self {
        Self {
            bg_dark: Color::Rgb(10, 14, 20),
            bg_surface: Color::Rgb(15, 20, 28),
            bg_elevated: Color::Rgb(25, 30, 40),
            bg_selected: Color::Rgb(35, 45, 60),

            border_dim: Color::Rgb(25, 30, 40),
            border_active: Color::Rgb(232, 176, 56),
            gutter_sep: Color::Rgb(20, 25, 35),
            pane_divider: Color::Rgb(25, 30, 40),

            text_faint: Color::Rgb(50, 60, 75),
            text_muted: Color::Rgb(90, 105, 125),
            text_dim: Color::Rgb(130, 145, 165),
            text_normal: Color::Rgb(203, 204, 198),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(232, 176, 56),
            accent_dim: Color::Rgb(57, 186, 230),

            diff_delete_bg: Color::Rgb(45, 25, 30),
            diff_insert_bg: Color::Rgb(25, 45, 35),
            diff_empty_bg: Color::Rgb(15, 20, 28),
            inline_delete_bg: Color::Rgb(85, 45, 50),
            inline_insert_bg: Color::Rgb(45, 85, 55),

            success: Color::Rgb(170, 217, 76),
            error: Color::Rgb(255, 51, 51),
            warning: Color::Rgb(232, 176, 56),

            syn_keyword: Color::Rgb(255, 143, 64),
            syn_type: Color::Rgb(89, 201, 228),
            syn_function: Color::Rgb(255, 185, 100),
            syn_string: Color::Rgb(170, 217, 76),
            syn_number: Color::Rgb(232, 176, 56),
            syn_comment: Color::Rgb(90, 105, 125),
            syn_operator: Color::Rgb(255, 143, 64),
            syn_punctuation: Color::Rgb(130, 145, 165),
            syn_constant: Color::Rgb(255, 238, 153),
            syn_property: Color::Rgb(89, 201, 228),
            syn_attribute: Color::Rgb(255, 185, 100),
        }
    }

    /// Palenight theme.
    pub fn palenight() -> Self {
        Self {
            bg_dark: Color::Rgb(41, 45, 62),
            bg_surface: Color::Rgb(48, 52, 70),
            bg_elevated: Color::Rgb(58, 63, 82),
            bg_selected: Color::Rgb(75, 81, 105),

            border_dim: Color::Rgb(58, 63, 82),
            border_active: Color::Rgb(199, 146, 234),
            gutter_sep: Color::Rgb(48, 52, 70),
            pane_divider: Color::Rgb(58, 63, 82),

            text_faint: Color::Rgb(75, 81, 105),
            text_muted: Color::Rgb(103, 110, 149),
            text_dim: Color::Rgb(140, 147, 177),
            text_normal: Color::Rgb(166, 172, 205),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(199, 146, 234),
            accent_dim: Color::Rgb(130, 170, 255),

            diff_delete_bg: Color::Rgb(55, 35, 45),
            diff_insert_bg: Color::Rgb(35, 55, 45),
            diff_empty_bg: Color::Rgb(48, 52, 70),
            inline_delete_bg: Color::Rgb(95, 55, 65),
            inline_insert_bg: Color::Rgb(55, 95, 65),

            success: Color::Rgb(195, 232, 141),
            error: Color::Rgb(255, 85, 114),
            warning: Color::Rgb(255, 203, 107),

            syn_keyword: Color::Rgb(199, 146, 234),
            syn_type: Color::Rgb(255, 203, 107),
            syn_function: Color::Rgb(130, 170, 255),
            syn_string: Color::Rgb(195, 232, 141),
            syn_number: Color::Rgb(247, 140, 108),
            syn_comment: Color::Rgb(103, 110, 149),
            syn_operator: Color::Rgb(137, 221, 255),
            syn_punctuation: Color::Rgb(140, 147, 177),
            syn_constant: Color::Rgb(247, 140, 108),
            syn_property: Color::Rgb(137, 221, 255),
            syn_attribute: Color::Rgb(255, 203, 107),
        }
    }

    /// Zenburn theme.
    pub fn zenburn() -> Self {
        Self {
            bg_dark: Color::Rgb(63, 63, 63),
            bg_surface: Color::Rgb(74, 74, 74),
            bg_elevated: Color::Rgb(85, 85, 85),
            bg_selected: Color::Rgb(96, 96, 96),

            border_dim: Color::Rgb(85, 85, 85),
            border_active: Color::Rgb(220, 163, 163),
            gutter_sep: Color::Rgb(74, 74, 74),
            pane_divider: Color::Rgb(85, 85, 85),

            text_faint: Color::Rgb(96, 96, 96),
            text_muted: Color::Rgb(124, 124, 110),
            text_dim: Color::Rgb(156, 156, 140),
            text_normal: Color::Rgb(220, 220, 204),
            text_bright: Color::Rgb(255, 255, 255),

            accent: Color::Rgb(220, 163, 163),
            accent_dim: Color::Rgb(140, 208, 211),

            diff_delete_bg: Color::Rgb(80, 55, 55),
            diff_insert_bg: Color::Rgb(55, 80, 65),
            diff_empty_bg: Color::Rgb(74, 74, 74),
            inline_delete_bg: Color::Rgb(120, 75, 75),
            inline_insert_bg: Color::Rgb(75, 120, 85),

            success: Color::Rgb(127, 159, 127),
            error: Color::Rgb(204, 147, 147),
            warning: Color::Rgb(223, 175, 143),

            syn_keyword: Color::Rgb(240, 223, 175),
            syn_type: Color::Rgb(239, 239, 175),
            syn_function: Color::Rgb(239, 239, 175),
            syn_string: Color::Rgb(204, 147, 147),
            syn_number: Color::Rgb(140, 208, 211),
            syn_comment: Color::Rgb(127, 159, 127),
            syn_operator: Color::Rgb(240, 239, 208),
            syn_punctuation: Color::Rgb(156, 156, 140),
            syn_constant: Color::Rgb(220, 163, 163),
            syn_property: Color::Rgb(140, 208, 211),
            syn_attribute: Color::Rgb(223, 175, 143),
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
