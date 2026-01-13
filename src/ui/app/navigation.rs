use super::{App, Focus};
use crate::core::ViewedStore;

impl App {
    /// Move selection up in the sidebar list.
    pub fn select_prev(&mut self) {
        if self.sidebar.filtered_indices.is_empty() {
            if self.sidebar.selected_idx > 0 {
                self.sidebar.selected_idx -= 1;
                self.request_current_diff();
                self.ui.dirty = true;
            }
        } else if let Some(pos) = self
            .sidebar
            .filtered_indices
            .iter()
            .position(|&i| i == self.sidebar.selected_idx)
        {
            if pos > 0 {
                self.sidebar.selected_idx = self.sidebar.filtered_indices[pos - 1];
                self.request_current_diff();
                self.ui.dirty = true;
            }
        }
    }

    /// Move selection down in the sidebar list.
    pub fn select_next(&mut self) {
        if self.sidebar.filtered_indices.is_empty() {
            if self.sidebar.selected_idx + 1 < self.files.len() {
                self.sidebar.selected_idx += 1;
                self.request_current_diff();
                self.ui.dirty = true;
            }
        } else if let Some(pos) = self
            .sidebar
            .filtered_indices
            .iter()
            .position(|&i| i == self.sidebar.selected_idx)
        {
            if pos + 1 < self.sidebar.filtered_indices.len() {
                self.sidebar.selected_idx = self.sidebar.filtered_indices[pos + 1];
                self.request_current_diff();
                self.ui.dirty = true;
            }
        }
    }

    /// Toggle viewed state for the selected file.
    pub fn toggle_viewed(&mut self) {
        if let Some(file) = self.selected_file() {
            let path = file.path.clone();
            let now_viewed = self.viewed.toggle_viewed(path);
            if now_viewed {
                self.viewed_in_changeset += 1;
                self.advance_to_next_unviewed();
            } else {
                self.viewed_in_changeset = self.viewed_in_changeset.saturating_sub(1);
            }
            self.ui.dirty = true;
        }
    }

    fn advance_to_next_unviewed(&mut self) {
        let visible = if self.sidebar.filtered_indices.is_empty() {
            (0..self.files.len()).collect::<Vec<_>>()
        } else {
            self.sidebar.filtered_indices.clone()
        };

        let cur_pos = visible
            .iter()
            .position(|&i| i == self.sidebar.selected_idx)
            .unwrap_or(0);

        for offset in 1..=visible.len() {
            let pos = (cur_pos + offset) % visible.len();
            let idx = visible[pos];
            if !self.viewed.is_viewed(&self.files[idx].path) {
                self.sidebar.selected_idx = idx;
                self.request_current_diff();
                return;
            }
        }
    }

    /// Switch focus between sidebar and diff view.
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sidebar => Focus::Diff,
            Focus::Diff => Focus::Sidebar,
        };
        self.ui.dirty = true;
    }

    /// Explicitly set the UI focus.
    pub fn set_focus(&mut self, focus: Focus) {
        if self.focus != focus {
            self.focus = focus;
            self.ui.dirty = true;
        }
    }
}
