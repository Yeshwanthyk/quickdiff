use super::{App, Mode};
use crate::theme::Theme;
use crate::ui::render::ThemeStyles;

impl App {
    /// Open the theme selector overlay.
    pub fn open_theme_selector(&mut self) {
        self.theme_list = Theme::list();
        let current = self
            .theme_list
            .iter()
            .position(|t| t == &self.theme_original)
            .unwrap_or(0);
        self.theme_selector_idx = current;
        self.ui.mode = Mode::SelectTheme;
        self.ui.dirty = true;
    }

    /// Close the theme selector without applying changes.
    pub fn close_theme_selector(&mut self) {
        self.theme = Theme::load(&self.theme_original);
        self.rebuild_theme_styles();
        self.ui.mode = Mode::Normal;
        self.ui.dirty = true;
    }

    /// Move selection to the previous theme preview.
    pub fn theme_select_prev(&mut self) {
        if self.theme_selector_idx > 0 {
            self.theme_selector_idx -= 1;
            self.theme = Theme::load(&self.theme_list[self.theme_selector_idx]);
            self.rebuild_theme_styles();
            self.ui.dirty = true;
        }
    }

    /// Move selection to the next theme preview.
    pub fn theme_select_next(&mut self) {
        if self.theme_selector_idx + 1 < self.theme_list.len() {
            self.theme_selector_idx += 1;
            self.theme = Theme::load(&self.theme_list[self.theme_selector_idx]);
            self.rebuild_theme_styles();
            self.ui.dirty = true;
        }
    }

    /// Apply the currently highlighted theme.
    pub fn theme_apply(&mut self) {
        self.theme_original = self.theme_list[self.theme_selector_idx].clone();
        self.ui.mode = Mode::Normal;
        self.ui.dirty = true;
    }

    pub(crate) fn rebuild_theme_styles(&mut self) {
        self.theme_styles = ThemeStyles::from_theme(&self.theme);
    }
}
