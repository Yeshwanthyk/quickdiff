use super::App;
use crate::core::ViewedStore;
use crate::core::{list_changed_files, list_changed_files_from_base_with_merge_base, DiffSource};

impl App {
    /// Poll the file watcher for changes and refresh the file list when needed.
    pub fn poll_watcher(&mut self) -> bool {
        let Some(ref watcher) = self.worker.watcher else {
            return false;
        };

        if watcher.poll().is_some() {
            self.refresh_file_list();
            true
        } else {
            false
        }
    }

    pub(crate) fn refresh_file_list(&mut self) {
        let current_path = self.selected_file().map(|f| f.path.clone());

        let new_files = match &self.source {
            DiffSource::WorkingTree => list_changed_files(&self.repo).ok(),
            DiffSource::Base(base) => {
                list_changed_files_from_base_with_merge_base(&self.repo, base)
                    .ok()
                    .map(|r| {
                        self.cached_merge_base = Some(r.merge_base);
                        r.files
                    })
            }
            DiffSource::Commit(_) | DiffSource::Range { .. } | DiffSource::PullRequest { .. } => {
                return
            }
        };

        let Some(mut files) = new_files else {
            return;
        };

        if let Some(ref filter) = self.file_filter {
            files.retain(|f| f.path.as_str().contains(filter));
        }

        self.files = files;
        self.rebuild_path_cache();

        if let Some(ref path) = current_path {
            if let Some(idx) = self.files.iter().position(|f| &f.path == path) {
                self.sidebar.selected_idx = idx;
                self.request_current_diff();
            } else {
                self.sidebar.selected_idx = self
                    .sidebar
                    .selected_idx
                    .min(self.files.len().saturating_sub(1));
                if !self.files.is_empty() {
                    self.request_current_diff();
                } else {
                    self.diff = None;
                    self.viewer.hunk_view_rows.clear();
                    self.old_buffer = None;
                    self.new_buffer = None;
                }
            }
        } else if !self.files.is_empty() {
            self.sidebar.selected_idx = 0;
            self.request_current_diff();
        }

        self.viewed_in_changeset = self
            .files
            .iter()
            .filter(|f| self.viewed.is_viewed(&f.path))
            .count();

        self.open_comment_counts =
            super::load_open_comment_counts(&self.repo, &self.comment_context);

        if !self.sidebar.filtered_indices.is_empty() {
            self.sidebar.filtered_indices.clear();
            self.sidebar.filter.clear();
        }

        self.ui.status = Some("Refreshed".to_string());
        self.ui.dirty = true;
    }
}
