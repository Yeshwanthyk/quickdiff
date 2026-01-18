//! Patch mode handling.

use super::App;
use crate::core::{parse_unified_diff, ChangedFile, DiffResult, TextBuffer};
use crate::highlight::{query_scopes, LanguageId};

impl App {
    /// Load a unified diff patch and enter patch mode.
    pub fn load_patch(&mut self, patch: String, label: String) {
        let patch_files = parse_unified_diff(&patch);
        self.patch.active = true;
        self.patch.label = label;
        self.patch.files = patch_files.clone();
        self.files = patch_files
            .iter()
            .map(|pf| ChangedFile {
                path: pf.path.clone(),
                kind: pf.kind,
                old_path: pf.old_path.clone(),
            })
            .collect();
        self.rebuild_path_cache();
        self.sidebar.selected_idx = 0;
        self.sidebar.scroll = 0;
        if !self.files.is_empty() {
            self.request_current_patch_diff();
        } else {
            self.diff = None;
            self.viewer.hunk_view_rows.clear();
            self.old_buffer = None;
            self.new_buffer = None;
            self.ui.status = Some("Patch has no changed files".to_string());
        }
        self.ui.dirty = true;
    }

    /// Request diff for the currently selected file in patch mode.
    pub(super) fn request_current_patch_diff(&mut self) {
        if !self.patch.active || self.patch.files.is_empty() {
            return;
        }
        let Some(patch_file) = self.patch.files.get(self.sidebar.selected_idx) else {
            return;
        };
        self.current_lang = patch_file
            .path
            .extension()
            .map(LanguageId::from_extension)
            .unwrap_or(LanguageId::Plain);
        self.old_scopes.clear();
        self.new_scopes.clear();
        self.old_highlights.clear();
        self.new_highlights.clear();

        let (old_content, new_content) = super::extract_content_from_patch(&patch_file.patch);
        let old_buffer = TextBuffer::new(old_content.as_bytes());
        let new_buffer = TextBuffer::new(new_content.as_bytes());
        let is_binary = old_buffer.is_binary() || new_buffer.is_binary();

        let diff = if is_binary {
            None
        } else {
            Some(DiffResult::compute(&old_buffer, &new_buffer))
        };

        if !is_binary {
            let lang = self.current_lang;
            let old_str = String::from_utf8_lossy(old_buffer.as_bytes());
            let new_str = String::from_utf8_lossy(new_buffer.as_bytes());
            self.old_scopes = query_scopes(lang, old_str.as_ref());
            self.new_scopes = query_scopes(lang, new_str.as_ref());
            self.old_highlights
                .compute(&self.highlighter, lang, old_str.as_ref());
            self.new_highlights
                .compute(&self.highlighter, lang, new_str.as_ref());
        } else {
            self.old_scopes.clear();
            self.new_scopes.clear();
            self.old_highlights.clear();
            self.new_highlights.clear();
        }

        self.is_binary = is_binary;
        self.old_buffer = Some(old_buffer);
        self.new_buffer = Some(new_buffer);
        self.diff = diff;
        self.rebuild_view_rows();
        self.viewer.scroll_y = 0;

        // Jump to first hunk if available
        if let Some(diff) = self.diff.as_ref() {
            if let Some(first) = diff.hunks().first() {
                if let Some(view_row) = self.diff_row_to_view_row(first.start_row) {
                    self.viewer.scroll_y = view_row;
                }
            }
        }

        self.viewer.scroll_x = 0;
        self.ui.error = None;
        self.worker.loading = false;
        self.ui.dirty = true;
    }
}
