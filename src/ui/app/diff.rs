use std::collections::HashMap;
use std::sync::mpsc::TrySendError;

use super::super::worker::{DiffLoadRequest, DiffLoadResponse};
use super::{App, DiffPaneMode, DiffSource, DiffViewMode};
use crate::core::{
    digest_hunk_changed_rows, CommentStore, DiffResult, FileCommentStore, RenderRow, Selector,
};
use crate::highlight::{query_scopes, LanguageId};

impl App {
    /// Request diff for the currently selected file.
    ///
    /// Work is performed on a background thread. Call `poll_worker()` to apply results.
    pub fn request_current_diff(&mut self) {
        if self.patch.active {
            self.request_current_patch_diff();
            return;
        }
        if self.pr.active {
            self.request_current_pr_diff();
            return;
        }

        self.ui.error = None;
        self.ui.status = None;
        self.is_binary = false;
        self.commented_hunks.clear();

        let Some(file) = self.selected_file().cloned() else {
            self.diff = None;
            self.viewer.hunk_view_rows.clear();
            self.old_buffer = None;
            self.new_buffer = None;
            self.worker.pending_request_id = None;
            self.worker.queued_request = None;
            self.old_highlights.clear();
            self.new_highlights.clear();
            self.worker.loading = false;
            return;
        };

        self.current_lang = file
            .path
            .extension()
            .map(LanguageId::from_extension)
            .unwrap_or(LanguageId::Plain);

        self.diff = None;
        self.viewer.hunk_view_rows.clear();
        self.old_buffer = None;
        self.new_buffer = None;
        self.old_scopes.clear();
        self.new_scopes.clear();
        self.old_highlights.clear();
        self.new_highlights.clear();
        self.viewer.scroll_y = 0;
        self.viewer.scroll_x = 0;

        let id = self.worker.next_request_id;
        self.worker.next_request_id = self.worker.next_request_id.wrapping_add(1);
        self.worker.pending_request_id = Some(id);
        self.worker.loading = true;
        self.ui.dirty = true;

        let req = DiffLoadRequest {
            id,
            source: self.source.clone(),
            cached_merge_base: self.cached_merge_base.clone(),
            file: file.clone(),
        };

        if !self.enqueue_diff_request(req) {
            return;
        }

        if matches!(self.source, DiffSource::WorkingTree) {
            self.viewed
                .set_last_selected(Some(file.path.as_str().to_string()));
        }
    }

    fn enqueue_diff_request(&mut self, req: DiffLoadRequest) -> bool {
        let Some(tx) = self.worker.diff.request_tx.as_ref() else {
            self.ui.error = Some("Diff worker stopped".to_string());
            self.worker.loading = false;
            self.worker.pending_request_id = None;
            self.worker.queued_request = None;
            self.ui.dirty = true;
            return false;
        };

        match tx.try_send(req) {
            Ok(()) => {
                self.worker.queued_request = None;
                true
            }
            Err(TrySendError::Full(req)) => {
                self.worker.queued_request = Some(req);
                true
            }
            Err(TrySendError::Disconnected(_)) => {
                self.ui.error = Some("Diff worker stopped".to_string());
                self.worker.loading = false;
                self.worker.pending_request_id = None;
                self.worker.queued_request = None;
                self.ui.dirty = true;
                false
            }
        }
    }

    fn flush_queued_diff_request(&mut self) {
        let Some(req) = self.worker.queued_request.take() else {
            return;
        };

        self.enqueue_diff_request(req);
    }

    /// Apply any completed diff loads from background worker.
    pub fn poll_worker(&mut self) {
        while let Ok(msg) = self.worker.diff.response_rx.try_recv() {
            match msg {
                DiffLoadResponse::Loaded {
                    id,
                    old_buffer,
                    new_buffer,
                    diff,
                    is_binary,
                } => {
                    if self.worker.pending_request_id != Some(id) {
                        continue;
                    }

                    self.worker.pending_request_id = None;
                    self.worker.loading = false;
                    self.ui.error = None;

                    self.is_binary = is_binary;
                    self.old_buffer = Some(old_buffer.clone());
                    self.new_buffer = Some(new_buffer.clone());
                    self.diff = diff;

                    self.rebuild_view_rows();
                    self.viewer.scroll_y = 0;
                    if let Some(diff) = self.diff.as_ref() {
                        if let Some(first) = diff.hunks().first() {
                            if let Some(view_row) = self.diff_row_to_view_row(first.start_row) {
                                self.viewer.scroll_y = view_row;
                            }
                        }
                    }

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

                    self.refresh_current_file_comment_markers();
                    self.ui.dirty = true;
                }
                DiffLoadResponse::Error { id, message } => {
                    if self.worker.pending_request_id != Some(id) {
                        continue;
                    }

                    self.worker.pending_request_id = None;
                    self.worker.loading = false;
                    self.diff = None;
                    self.viewer.hunk_view_rows.clear();
                    self.old_buffer = None;
                    self.new_buffer = None;
                    self.old_highlights.clear();
                    self.new_highlights.clear();
                    self.commented_hunks.clear();
                    self.ui.error = Some(format!("Failed to load diff: {}", message));
                    self.ui.dirty = true;
                }
            }
        }

        self.flush_queued_diff_request();
    }

    pub(crate) fn refresh_current_file_comment_markers(&mut self) {
        self.commented_hunks.clear();

        let Some(diff) = &self.diff else {
            return;
        };
        let Some(file) = self.selected_file() else {
            return;
        };

        let Ok(store) = FileCommentStore::open(&self.repo) else {
            return;
        };

        let comments = store.list_for_path(&file.path, false);
        if comments.is_empty() {
            return;
        }

        let mut digest_to_hunk_idx = HashMap::new();
        for (idx, h) in diff.hunks().iter().enumerate() {
            digest_to_hunk_idx.insert(digest_hunk_changed_rows(diff, h), idx);
        }

        for c in comments {
            if !c.context.matches(&self.comment_context) {
                continue;
            }

            for sel in &c.anchor.selectors {
                match sel {
                    Selector::DiffHunkV1(h) => {
                        if let Some(idx) = digest_to_hunk_idx.get(&h.digest_hex) {
                            self.commented_hunks.insert(*idx);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn rebuild_view_rows(&mut self) {
        self.viewer.hunk_view_rows = match self.diff.as_ref() {
            Some(diff) => build_view_rows(diff, self.viewer.view_mode),
            None => Vec::new(),
        };
    }

    pub(crate) fn view_row_count(&self) -> usize {
        let Some(diff) = &self.diff else {
            return 0;
        };

        match self.viewer.view_mode {
            DiffViewMode::FullFile => diff.row_count(),
            DiffViewMode::HunksOnly => self.viewer.hunk_view_rows.len(),
        }
    }

    pub(crate) fn view_row_to_diff_row(&self, view_row: usize) -> Option<usize> {
        let diff = self.diff.as_ref()?;
        match self.viewer.view_mode {
            DiffViewMode::FullFile => {
                if view_row < diff.row_count() {
                    Some(view_row)
                } else {
                    None
                }
            }
            DiffViewMode::HunksOnly => self.viewer.hunk_view_rows.get(view_row).copied(),
        }
    }

    pub(crate) fn diff_row_to_view_row(&self, diff_row: usize) -> Option<usize> {
        match self.viewer.view_mode {
            DiffViewMode::FullFile => {
                let diff = self.diff.as_ref()?;
                if diff_row < diff.row_count() {
                    Some(diff_row)
                } else {
                    None
                }
            }
            DiffViewMode::HunksOnly => {
                map_diff_row_to_view_row(&self.viewer.hunk_view_rows, diff_row)
            }
        }
    }

    pub(crate) fn visible_diff_rows(&self, height: usize) -> Vec<(usize, &RenderRow)> {
        let Some(diff) = &self.diff else {
            return Vec::new();
        };

        let mut rows = Vec::new();
        match self.viewer.view_mode {
            DiffViewMode::FullFile => {
                for (offset, row) in diff.render_rows(self.viewer.scroll_y, height).enumerate() {
                    let row_idx = self.viewer.scroll_y + offset;
                    rows.push((row_idx, row));
                }
            }
            DiffViewMode::HunksOnly => {
                let start = self.viewer.scroll_y;
                let end = start.saturating_add(height);
                for view_idx in start..end {
                    let Some(&row_idx) = self.viewer.hunk_view_rows.get(view_idx) else {
                        break;
                    };
                    if let Some(row) = diff.rows().get(row_idx) {
                        rows.push((row_idx, row));
                    }
                }
            }
        }

        rows
    }

    /// Toggle between hunks-only and full-file diff views.
    pub fn toggle_diff_view_mode(&mut self) {
        let current_row = self.view_row_to_diff_row(self.viewer.scroll_y);

        self.viewer.view_mode = match self.viewer.view_mode {
            DiffViewMode::HunksOnly => DiffViewMode::FullFile,
            DiffViewMode::FullFile => DiffViewMode::HunksOnly,
        };

        self.rebuild_view_rows();

        self.viewer.scroll_y = current_row
            .and_then(|row| self.diff_row_to_view_row(row))
            .unwrap_or(0);
        self.ui.dirty = true;
    }

    /// Scroll diff view vertically and horizontally.
    pub fn scroll_diff(&mut self, delta_y: isize, delta_x: isize) {
        let old_y = self.viewer.scroll_y;
        let old_x = self.viewer.scroll_x;

        if delta_y < 0 {
            self.viewer.scroll_y = self.viewer.scroll_y.saturating_sub((-delta_y) as usize);
        } else {
            let max_scroll = self.view_row_count();
            if max_scroll == 0 {
                self.viewer.scroll_y = 0;
            } else {
                self.viewer.scroll_y =
                    (self.viewer.scroll_y + delta_y as usize).min(max_scroll - 1);
            }
        }

        if delta_x < 0 {
            self.viewer.scroll_x = self.viewer.scroll_x.saturating_sub((-delta_x) as usize);
        } else {
            self.viewer.scroll_x += delta_x as usize;
        }

        if self.viewer.scroll_y != old_y || self.viewer.scroll_x != old_x {
            self.ui.dirty = true;
        }
    }

    /// Jump to the next diff hunk.
    pub fn next_hunk(&mut self) {
        let Some(diff) = &self.diff else {
            return;
        };
        let Some(current_row) = self.view_row_to_diff_row(self.viewer.scroll_y) else {
            return;
        };

        if let Some(row) = diff.next_hunk_row(current_row) {
            if let Some(view_row) = self.diff_row_to_view_row(row) {
                self.viewer.scroll_y = view_row;
                self.ui.dirty = true;
            }
        }
    }

    /// Jump to the previous diff hunk.
    pub fn prev_hunk(&mut self) {
        let Some(diff) = &self.diff else {
            return;
        };
        let Some(current_row) = self.view_row_to_diff_row(self.viewer.scroll_y) else {
            return;
        };

        if let Some(row) = diff.prev_hunk_row(current_row) {
            if let Some(view_row) = self.diff_row_to_view_row(row) {
                self.viewer.scroll_y = view_row;
                self.ui.dirty = true;
            }
        }
    }

    /// Get current hunk position as (1-based index, total).
    pub fn current_hunk_info(&self) -> Option<(usize, usize)> {
        let diff = self.diff.as_ref()?;
        let hunks = diff.hunks();
        if hunks.is_empty() {
            return None;
        }
        let row = self.view_row_to_diff_row(self.viewer.scroll_y)?;
        let hunk_idx = diff.hunk_at_row(row)?;
        Some((hunk_idx + 1, hunks.len()))
    }

    /// Toggle the old pane between fullscreen and split view.
    pub fn toggle_old_fullscreen(&mut self) {
        let next = match self.viewer.pane_mode {
            DiffPaneMode::OldOnly => DiffPaneMode::Both,
            _ => DiffPaneMode::OldOnly,
        };
        self.set_diff_pane_mode(next);
    }

    /// Toggle the new pane between fullscreen and split view.
    pub fn toggle_new_fullscreen(&mut self) {
        let next = match self.viewer.pane_mode {
            DiffPaneMode::NewOnly => DiffPaneMode::Both,
            _ => DiffPaneMode::NewOnly,
        };
        self.set_diff_pane_mode(next);
    }

    fn set_diff_pane_mode(&mut self, mode: DiffPaneMode) {
        if self.viewer.pane_mode != mode {
            self.viewer.pane_mode = mode;
            self.ui.dirty = true;
        }
    }
}

pub(super) fn build_view_rows(diff: &DiffResult, mode: DiffViewMode) -> Vec<usize> {
    if mode != DiffViewMode::HunksOnly {
        return Vec::new();
    }

    let mut rows = Vec::new();
    for hunk in diff.hunks() {
        rows.extend(hunk.start_row..(hunk.start_row + hunk.row_count));
    }
    rows
}

pub(super) fn map_diff_row_to_view_row(view_rows: &[usize], diff_row: usize) -> Option<usize> {
    if view_rows.is_empty() {
        return None;
    }

    let pos = view_rows.partition_point(|&row| row < diff_row);
    if pos < view_rows.len() {
        Some(pos)
    } else {
        Some(view_rows.len() - 1)
    }
}
