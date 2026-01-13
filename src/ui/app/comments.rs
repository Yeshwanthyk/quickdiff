use std::collections::HashMap;

use super::{App, CommentViewItem, Focus, Mode};
use crate::core::{
    digest_hunk_changed_rows, format_anchor_summary, selector_from_hunk, Anchor, CommentStatus,
    CommentStore, FileCommentStore, Selector,
};

impl App {
    /// Start adding a comment on the current hunk.
    pub fn start_add_comment(&mut self) {
        let Some(diff) = &self.diff else {
            self.ui.error = Some("No diff available".to_string());
            self.ui.dirty = true;
            return;
        };

        if diff.hunks().is_empty() {
            self.ui.error = Some("No hunks to comment on".to_string());
            self.ui.dirty = true;
            return;
        }

        let Some(row) = self.view_row_to_diff_row(self.viewer.scroll_y) else {
            self.ui.error = Some("Not on a hunk - navigate to a hunk first".to_string());
            self.ui.dirty = true;
            return;
        };

        if diff.hunk_at_row(row).is_none() {
            self.ui.error = Some("Not on a hunk - navigate to a hunk first".to_string());
            self.ui.dirty = true;
            return;
        }

        self.ui.mode = Mode::AddComment;
        self.comments.draft.clear();
        self.ui.error = None;
        self.ui.status = None;
        self.ui.dirty = true;
    }

    /// Cancel adding a comment.
    pub fn cancel_add_comment(&mut self) {
        self.ui.mode = Mode::Normal;
        self.comments.draft.clear();
        self.ui.dirty = true;
    }

    /// Save the current draft comment.
    pub fn save_comment(&mut self) {
        if self.comments.draft.trim().is_empty() {
            self.ui.error = Some("Comment cannot be empty".to_string());
            self.ui.dirty = true;
            return;
        }

        let Some(diff) = &self.diff else {
            self.ui.error = Some("No diff available".to_string());
            self.ui.mode = Mode::Normal;
            self.ui.dirty = true;
            return;
        };

        let Some(row) = self.view_row_to_diff_row(self.viewer.scroll_y) else {
            self.ui.error = Some("No hunk at current position".to_string());
            self.ui.mode = Mode::Normal;
            self.ui.dirty = true;
            return;
        };

        let Some(hunk_idx) = diff.hunk_at_row(row) else {
            self.ui.error = Some("No hunk at current position".to_string());
            self.ui.mode = Mode::Normal;
            self.ui.dirty = true;
            return;
        };

        let Some(file) = self.selected_file() else {
            self.ui.error = Some("No file selected".to_string());
            self.ui.mode = Mode::Normal;
            self.ui.dirty = true;
            return;
        };

        let path = file.path.clone();

        let Some(selector) = selector_from_hunk(diff, hunk_idx) else {
            self.ui.error = Some("Failed to create comment anchor".to_string());
            self.ui.mode = Mode::Normal;
            self.ui.dirty = true;
            return;
        };

        let anchor = Anchor {
            selectors: vec![Selector::DiffHunkV1(selector)],
        };

        let mut store = match FileCommentStore::open(&self.repo) {
            Ok(s) => s,
            Err(e) => {
                self.ui.error = Some(format!("Failed to open comment store: {}", e));
                self.ui.mode = Mode::Normal;
                self.ui.dirty = true;
                return;
            }
        };

        match store.add(
            path.clone(),
            self.comment_context.clone(),
            self.comments.draft.clone(),
            anchor,
        ) {
            Ok(id) => {
                self.ui.status = Some(format!("Comment {} saved", id));
                self.ui.error = None;
                *self.open_comment_counts.entry(path).or_insert(0) += 1;
                self.commented_hunks.insert(hunk_idx);
            }
            Err(e) => {
                self.ui.error = Some(format!("Failed to save comment: {}", e));
            }
        }

        self.ui.mode = Mode::Normal;
        self.comments.draft.clear();
        self.ui.dirty = true;
    }

    fn refresh_viewing_comments(&mut self) {
        let Some(file) = self.selected_file() else {
            self.comments.viewing.clear();
            self.comments.selected = 0;
            self.comments.scroll = 0;
            return;
        };

        let include_resolved = self.comments.include_resolved;

        let Ok(store) = FileCommentStore::open(&self.repo) else {
            self.comments.viewing.clear();
            self.comments.selected = 0;
            self.comments.scroll = 0;
            return;
        };

        let comments = store.list_for_path(&file.path, include_resolved);

        let selected_id = self
            .comments
            .viewing
            .get(self.comments.selected)
            .map(|c| c.id);

        let mut digest_to_start_row: HashMap<String, usize> = HashMap::new();
        if let Some(diff) = &self.diff {
            for h in diff.hunks() {
                digest_to_start_row.insert(digest_hunk_changed_rows(diff, h), h.start_row);
            }
        }

        let mut items: Vec<CommentViewItem> = Vec::new();
        for c in comments {
            if !c.context.matches(&self.comment_context) {
                continue;
            }

            let mut hunk_start_row = None;
            let mut stale = false;

            if let Some(Selector::DiffHunkV1(h)) = c.anchor.selectors.first() {
                if let Some(row) = digest_to_start_row.get(&h.digest_hex) {
                    hunk_start_row = Some(*row);
                } else if !digest_to_start_row.is_empty() {
                    stale = true;
                }
            }

            items.push(CommentViewItem {
                id: c.id,
                status: c.status,
                message: c.message.clone(),
                anchor_summary: format_anchor_summary(&c.anchor),
                hunk_start_row,
                stale,
            });
        }

        items.sort_by_key(|c| (c.status != CommentStatus::Open, c.id));

        self.comments.viewing = items;

        self.comments.selected = match selected_id {
            Some(id) => self
                .comments
                .viewing
                .iter()
                .position(|c| c.id == id)
                .unwrap_or(0),
            None => 0,
        };

        if self.comments.viewing.is_empty() {
            self.comments.selected = 0;
            self.comments.scroll = 0;
        } else {
            self.comments.selected = self.comments.selected.min(self.comments.viewing.len() - 1);
            self.comments.scroll = self.comments.scroll.min(self.comments.viewing.len() - 1);
        }
    }

    /// Show comments overlay for current file.
    pub fn show_comments(&mut self) {
        self.comments.include_resolved = false;
        self.comments.selected = 0;
        self.comments.scroll = 0;
        self.refresh_viewing_comments();

        if self.comments.viewing.is_empty() {
            self.ui.status = Some("No comments on this file".to_string());
            self.ui.dirty = true;
            return;
        }

        self.ui.mode = Mode::ViewComments;
        self.ui.dirty = true;
    }

    /// Close the comments view and return to normal mode.
    pub fn close_comments(&mut self) {
        self.ui.mode = Mode::Normal;
        self.comments.viewing.clear();
        self.comments.selected = 0;
        self.comments.scroll = 0;
        self.ui.dirty = true;
    }

    /// Select the next comment in the list.
    pub fn comments_select_next(&mut self) {
        if self.comments.viewing.is_empty() {
            return;
        }
        self.comments.selected = (self.comments.selected + 1).min(self.comments.viewing.len() - 1);
        self.ui.dirty = true;
    }

    /// Select the previous comment in the list.
    pub fn comments_select_prev(&mut self) {
        self.comments.selected = self.comments.selected.saturating_sub(1);
        self.ui.dirty = true;
    }

    /// Toggle whether resolved comments are shown.
    pub fn comments_toggle_include_resolved(&mut self) {
        self.comments.include_resolved = !self.comments.include_resolved;
        self.refresh_viewing_comments();
        self.ui.dirty = true;
    }

    /// Jump to the selected comment's location in the diff.
    pub fn comments_jump_to_selected(&mut self) {
        if self.comments.viewing.is_empty() {
            return;
        }

        let Some(item) = self.comments.viewing.get(self.comments.selected).cloned() else {
            return;
        };

        let Some(row) = item.hunk_start_row else {
            self.ui.status = Some("Comment anchor is stale (hunk not found)".to_string());
            self.ui.dirty = true;
            return;
        };

        self.viewer.scroll_y = self.diff_row_to_view_row(row).unwrap_or(0);
        self.focus = Focus::Diff;
        self.close_comments();
        self.ui.dirty = true;
    }

    /// Resolve the currently selected comment.
    pub fn comments_resolve_selected(&mut self) {
        if self.comments.viewing.is_empty() {
            return;
        }

        let Some(item) = self.comments.viewing.get(self.comments.selected).cloned() else {
            return;
        };

        if item.status != CommentStatus::Open {
            self.ui.status = Some("Comment already resolved".to_string());
            self.ui.dirty = true;
            return;
        }

        let mut store = match FileCommentStore::open(&self.repo) {
            Ok(s) => s,
            Err(e) => {
                self.ui.error = Some(format!("Failed to open comment store: {}", e));
                self.ui.dirty = true;
                return;
            }
        };

        match store.resolve(item.id) {
            Ok(true) => {
                if let Some(path) = self.selected_file().map(|f| f.path.clone()) {
                    let should_remove = match self.open_comment_counts.get_mut(&path) {
                        Some(count) => {
                            *count = count.saturating_sub(1);
                            *count == 0
                        }
                        None => false,
                    };
                    if should_remove {
                        self.open_comment_counts.remove(&path);
                    }
                }
                self.refresh_current_file_comment_markers();
                self.refresh_viewing_comments();
                self.ui.status = Some(format!("Resolved comment {}", item.id));
            }
            Ok(false) => {
                self.ui.error = Some(format!("Comment {} not found", item.id));
            }
            Err(e) => {
                self.ui.error = Some(format!("Failed to resolve comment: {}", e));
            }
        }

        self.ui.dirty = true;
    }
}
