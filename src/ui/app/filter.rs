use super::{App, Mode};
use crate::core::ChangedFile;

impl App {
    /// Start filtering files in sidebar.
    pub fn start_filter(&mut self) {
        self.ui.mode = Mode::FilterFiles;
        self.sidebar.filter.clear();
        self.ui.dirty = true;
    }

    /// Apply the current filter query using fuzzy matching.
    pub fn apply_filter(&mut self) {
        self.recompute_filter();
        self.ui.mode = Mode::Normal;
        self.ui.dirty = true;
    }

    fn recompute_filter(&mut self) {
        let query = self.sidebar.filter.trim();
        if query.is_empty() {
            self.sidebar.filtered_indices.clear();
        } else {
            self.sidebar.filtered_indices = self.fuzzy_matcher.filter_sorted(
                query,
                self.files
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (i, f.path.as_str())),
            );
        }

        if !self.sidebar.filtered_indices.is_empty()
            && !self
                .sidebar
                .filtered_indices
                .contains(&self.sidebar.selected_idx)
        {
            self.sidebar.selected_idx = self.sidebar.filtered_indices[0];
            self.request_current_diff();
        }
    }

    /// Update filter live as user types.
    pub fn update_filter_live(&mut self) {
        self.recompute_filter();
        self.ui.dirty = true;
    }

    /// Cancel filter and restore full list.
    pub fn cancel_filter(&mut self) {
        self.ui.mode = Mode::Normal;
        self.sidebar.filter.clear();
        self.sidebar.filtered_indices.clear();
        self.ui.dirty = true;
    }

    /// Clear filter while in Normal mode.
    pub fn clear_filter(&mut self) {
        if !self.sidebar.filtered_indices.is_empty() {
            self.sidebar.filtered_indices.clear();
            self.sidebar.filter.clear();
            self.ui.dirty = true;
        }
    }

    /// Get visible files (filtered or all).
    pub fn visible_files(&self) -> Vec<(usize, &ChangedFile)> {
        if self.sidebar.filtered_indices.is_empty() {
            self.files.iter().enumerate().collect()
        } else {
            self.sidebar
                .filtered_indices
                .iter()
                .filter_map(|&i| self.files.get(i).map(|f| (i, f)))
                .collect()
        }
    }

    /// Check if a file index is visible (passes filter).
    pub fn is_file_visible(&self, idx: usize) -> bool {
        self.sidebar.filtered_indices.is_empty() || self.sidebar.filtered_indices.contains(&idx)
    }
}
