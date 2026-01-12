use super::super::worker::{PrRequest, PrResponse};
use super::{App, Mode, PRActionType};
use crate::core::DiffResult;
use crate::core::{
    approve_pr, comment_pr, list_changed_files, open_pr_in_browser, parse_unified_diff,
    request_changes_pr, ChangedFile, DiffSource, PullRequest, TextBuffer,
};
use crate::highlight::{query_scopes, LanguageId};

impl App {
    pub(crate) fn send_pr_request(&mut self, req: PrRequest) -> bool {
        let Some(tx) = self.worker.pr_worker.request_tx.as_ref() else {
            self.ui.error = Some("PR worker stopped".to_string());
            self.pr.loading = false;
            self.worker.loading = false;
            self.worker.pending_pr_list_id = None;
            self.worker.pending_pr_load_id = None;
            self.worker.pending_pr = None;
            self.ui.dirty = true;
            return false;
        };

        if tx.send(req).is_err() {
            self.ui.error = Some("PR worker stopped".to_string());
            self.pr.loading = false;
            self.worker.loading = false;
            self.worker.pending_pr_list_id = None;
            self.worker.pending_pr_load_id = None;
            self.worker.pending_pr = None;
            self.ui.dirty = true;
            return false;
        }

        true
    }

    /// Apply any completed PR loads from the worker thread.
    pub fn poll_pr_worker(&mut self) {
        while let Ok(msg) = self.worker.pr_worker.response_rx.try_recv() {
            match msg {
                PrResponse::List { id, prs } => {
                    if self.worker.pending_pr_list_id != Some(id) {
                        continue;
                    }

                    self.worker.pending_pr_list_id = None;
                    self.pr.list = prs;
                    self.pr.loading = false;
                    self.ui.error = None;

                    if self.pr.list.is_empty() {
                        self.ui.status = Some("No PRs found".to_string());
                    }

                    self.ui.dirty = true;
                }
                PrResponse::ListError { id, message } => {
                    if self.worker.pending_pr_list_id != Some(id) {
                        continue;
                    }

                    self.worker.pending_pr_list_id = None;
                    self.pr.list.clear();
                    self.pr.loading = false;
                    self.ui.error = Some(format!("Failed to fetch PRs: {}", message));
                    self.ui.dirty = true;
                }
                PrResponse::Diff { id, diff } => {
                    if self.worker.pending_pr_load_id != Some(id) {
                        continue;
                    }

                    self.worker.pending_pr_load_id = None;
                    self.pr.loading = false;
                    self.worker.loading = false;
                    self.ui.error = None;

                    let pr = match self
                        .worker
                        .pending_pr
                        .take()
                        .or_else(|| self.pr.current.clone())
                    {
                        Some(pr) => pr,
                        None => {
                            self.ui.error = Some("No PR selected".to_string());
                            self.ui.dirty = true;
                            continue;
                        }
                    };

                    let pr_files = parse_unified_diff(&diff);

                    self.files = pr_files
                        .iter()
                        .map(|pf| ChangedFile {
                            path: pf.path.clone(),
                            kind: pf.kind,
                            old_path: pf.old_path.clone(),
                        })
                        .collect();

                    self.rebuild_path_cache();
                    self.pr.files = pr_files;
                    self.pr.active = true;
                    self.pr.current = Some(pr.clone());

                    self.source = DiffSource::PullRequest {
                        number: pr.number,
                        head: pr.head_ref_name.clone(),
                        base: pr.base_ref_name.clone(),
                    };

                    self.sidebar.selected_idx = 0;
                    self.sidebar.scroll = 0;

                    if !self.files.is_empty() {
                        self.request_current_pr_diff();
                    } else {
                        self.diff = None;
                        self.viewer.hunk_view_rows.clear();
                        self.old_buffer = None;
                        self.new_buffer = None;
                        self.ui.status = Some("PR has no changed files".to_string());
                    }

                    self.ui.dirty = true;
                }
                PrResponse::DiffError { id, message } => {
                    if self.worker.pending_pr_load_id != Some(id) {
                        continue;
                    }

                    self.worker.pending_pr_load_id = None;
                    self.worker.pending_pr = None;
                    self.pr.loading = false;
                    self.worker.loading = false;
                    self.ui.error = Some(format!("Failed to load PR diff: {}", message));
                    self.ui.dirty = true;
                }
            }
        }
    }

    /// Open PR picker mode and begin loading PRs from GitHub.
    pub fn open_pr_picker(&mut self) {
        if !crate::core::is_gh_available() {
            self.ui.error = Some("GitHub CLI not available. Run 'gh auth login'".to_string());
            self.ui.dirty = true;
            return;
        }

        self.ui.mode = Mode::PRPicker;
        self.pr.picker_selected = 0;
        self.pr.picker_scroll = 0;
        self.pr.loading = true;
        self.ui.dirty = true;
        self.fetch_pr_list();
    }

    /// Fetch the list of PRs for the current repository.
    pub fn fetch_pr_list(&mut self) {
        self.pr.loading = true;
        self.ui.error = None;

        let id = self.worker.next_pr_request_id;
        self.worker.next_pr_request_id = self.worker.next_pr_request_id.wrapping_add(1);
        self.worker.pending_pr_list_id = Some(id);

        if !self.send_pr_request(PrRequest::List {
            id,
            filter: self.pr.filter,
        }) {
            self.worker.pending_pr_list_id = None;
            self.pr.loading = false;
        }

        self.ui.dirty = true;
    }

    /// Close PR picker and reset its state to normal mode.
    pub fn close_pr_picker(&mut self) {
        self.ui.mode = Mode::Normal;
        self.pr.list.clear();
        self.pr.picker_selected = 0;
        self.pr.picker_scroll = 0;
        self.pr.loading = false;
        self.worker.pending_pr_list_id = None;
        self.ui.dirty = true;
    }

    /// Move selection to the next PR in the picker.
    pub fn pr_picker_next(&mut self) {
        if !self.pr.list.is_empty() {
            self.pr.picker_selected = (self.pr.picker_selected + 1).min(self.pr.list.len() - 1);
            self.ui.dirty = true;
        }
    }

    /// Move selection to the previous PR in the picker.
    pub fn pr_picker_prev(&mut self) {
        self.pr.picker_selected = self.pr.picker_selected.saturating_sub(1);
        self.ui.dirty = true;
    }

    /// Cycle the picker to the next filter (all → mine → review requested).
    pub fn pr_picker_next_filter(&mut self) {
        self.pr.filter = match self.pr.filter {
            crate::core::PRFilter::All => crate::core::PRFilter::Mine,
            crate::core::PRFilter::Mine => crate::core::PRFilter::ReviewRequested,
            crate::core::PRFilter::ReviewRequested => crate::core::PRFilter::All,
        };
        self.pr.picker_selected = 0;
        self.fetch_pr_list();
    }

    /// Cycle the picker to the previous filter (reverse order).
    pub fn pr_picker_prev_filter(&mut self) {
        self.pr.filter = match self.pr.filter {
            crate::core::PRFilter::All => crate::core::PRFilter::ReviewRequested,
            crate::core::PRFilter::Mine => crate::core::PRFilter::All,
            crate::core::PRFilter::ReviewRequested => crate::core::PRFilter::Mine,
        };
        self.pr.picker_selected = 0;
        self.fetch_pr_list();
    }

    /// Load the currently highlighted PR from the picker.
    pub fn pr_picker_select(&mut self) {
        if self.pr.list.is_empty() {
            return;
        }

        let pr = self.pr.list[self.pr.picker_selected].clone();
        self.load_pr(pr);
    }

    /// Load a specific PR's diff and switch into PR mode.
    pub fn load_pr(&mut self, pr: PullRequest) {
        self.pr.loading = true;
        self.worker.loading = true;
        self.ui.error = None;
        self.ui.mode = Mode::Normal;
        self.ui.dirty = true;

        let id = self.worker.next_pr_request_id;
        self.worker.next_pr_request_id = self.worker.next_pr_request_id.wrapping_add(1);
        self.worker.pending_pr_load_id = Some(id);
        self.worker.pending_pr = Some(pr.clone());

        self.pr.active = true;
        self.pr.current = Some(pr.clone());
        self.pr.files.clear();
        self.files.clear();
        self.diff = None;
        self.viewer.hunk_view_rows.clear();
        self.old_buffer = None;
        self.new_buffer = None;
        self.old_scopes.clear();
        self.new_scopes.clear();
        self.old_highlights.clear();
        self.new_highlights.clear();
        self.is_binary = false;
        self.viewer.scroll_y = 0;
        self.viewer.scroll_x = 0;

        self.source = DiffSource::PullRequest {
            number: pr.number,
            head: pr.head_ref_name.clone(),
            base: pr.base_ref_name.clone(),
        };

        if !self.send_pr_request(PrRequest::LoadDiff {
            id,
            pr_number: pr.number,
        }) {
            self.worker.pending_pr_load_id = None;
            self.worker.pending_pr = None;
            self.pr.loading = false;
            self.worker.loading = false;
        }
    }

    /// Exit PR mode and return to the working tree diff state.
    pub fn exit_pr_mode(&mut self) {
        if !self.pr.active {
            return;
        }

        self.pr.active = false;
        self.pr.current = None;
        self.pr.files.clear();
        self.worker.pending_pr_load_id = None;
        self.worker.pending_pr = None;
        self.pr.loading = false;
        self.worker.loading = false;
        self.source = DiffSource::WorkingTree;

        match list_changed_files(&self.repo) {
            Ok(files) => {
                self.files = files;
                self.rebuild_path_cache();
                self.sidebar.selected_idx = 0;
                if !self.files.is_empty() {
                    self.request_current_diff();
                }
            }
            Err(e) => {
                self.ui.error = Some(format!("Failed to reload: {}", e));
            }
        }

        self.ui.dirty = true;
    }

    /// Reload the currently active PR from GitHub.
    pub fn refresh_pr(&mut self) {
        if let Some(pr) = self.pr.current.clone() {
            self.load_pr(pr);
        }
    }

    /// Open the active PR in the user's web browser via `gh`.
    pub fn open_pr_in_browser(&mut self) {
        let Some(pr) = &self.pr.current else {
            self.ui.error = Some("No PR selected".to_string());
            self.ui.dirty = true;
            return;
        };

        let pr_number = pr.number;
        let repo_path = self.repo.path().to_path_buf();

        match open_pr_in_browser(&repo_path, pr_number) {
            Ok(()) => {
                self.ui.status = Some(format!("Opened PR #{} in browser", pr_number));
                self.ui.error = None;
            }
            Err(e) => {
                self.ui.error = Some(format!("Failed to open PR: {}", e));
            }
        }

        self.ui.dirty = true;
    }

    /// Begin an approve action in the PR review UI.
    pub fn start_pr_approve(&mut self) {
        if !self.pr.active || self.pr.current.is_none() {
            self.ui.error = Some("Not in PR mode".to_string());
            self.ui.dirty = true;
            return;
        }

        self.pr.action_type = Some(PRActionType::Approve);
        self.pr.action_text.clear();
        self.ui.mode = Mode::PRAction;
        self.ui.dirty = true;
    }

    /// Begin a comment action in the PR review UI.
    pub fn start_pr_comment(&mut self) {
        if !self.pr.active || self.pr.current.is_none() {
            self.ui.error = Some("Not in PR mode".to_string());
            self.ui.dirty = true;
            return;
        }

        self.pr.action_type = Some(PRActionType::Comment);
        self.pr.action_text.clear();
        self.ui.mode = Mode::PRAction;
        self.ui.dirty = true;
    }

    /// Begin a request-changes action in the PR review UI.
    pub fn start_pr_request_changes(&mut self) {
        if !self.pr.active || self.pr.current.is_none() {
            self.ui.error = Some("Not in PR mode".to_string());
            self.ui.dirty = true;
            return;
        }

        self.pr.action_type = Some(PRActionType::RequestChanges);
        self.pr.action_text.clear();
        self.ui.mode = Mode::PRAction;
        self.ui.dirty = true;
    }

    /// Cancel any in-progress PR review action.
    pub fn cancel_pr_action(&mut self) {
        self.ui.mode = Mode::Normal;
        self.pr.action_type = None;
        self.pr.action_text.clear();
        self.ui.dirty = true;
    }

    /// Submit the currently composed PR review action via `gh`.
    pub fn submit_pr_action(&mut self) {
        let Some(pr) = &self.pr.current else {
            self.ui.error = Some("No PR selected".to_string());
            self.cancel_pr_action();
            return;
        };

        let pr_number = pr.number;
        let repo_path = self.repo.path().to_path_buf();

        let result = match self.pr.action_type {
            Some(PRActionType::Approve) => {
                let body = if self.pr.action_text.trim().is_empty() {
                    None
                } else {
                    Some(self.pr.action_text.as_str())
                };
                approve_pr(&repo_path, pr_number, body)
            }
            Some(PRActionType::Comment) => {
                if self.pr.action_text.trim().is_empty() {
                    self.ui.error = Some("Comment cannot be empty".to_string());
                    self.ui.dirty = true;
                    return;
                }
                comment_pr(&repo_path, pr_number, &self.pr.action_text)
            }
            Some(PRActionType::RequestChanges) => {
                if self.pr.action_text.trim().is_empty() {
                    self.ui.error = Some("Message cannot be empty".to_string());
                    self.ui.dirty = true;
                    return;
                }
                request_changes_pr(&repo_path, pr_number, &self.pr.action_text)
            }
            None => {
                self.cancel_pr_action();
                return;
            }
        };

        match result {
            Ok(()) => {
                let action_name = match self.pr.action_type {
                    Some(PRActionType::Approve) => "approved",
                    Some(PRActionType::Comment) => "commented on",
                    Some(PRActionType::RequestChanges) => "requested changes on",
                    None => "reviewed",
                };
                self.ui.status = Some(format!("PR #{} {}", pr_number, action_name));
                self.ui.error = None;
            }
            Err(e) => {
                self.ui.error = Some(format!("Failed: {}", e));
            }
        }

        self.cancel_pr_action();
    }

    /// Render the diff for the currently selected PR file from its patch.
    pub(super) fn request_current_pr_diff(&mut self) {
        if !self.pr.active || self.pr.files.is_empty() {
            return;
        }

        let Some(pr_file) = self.pr.files.get(self.sidebar.selected_idx) else {
            return;
        };

        self.current_lang = pr_file
            .path
            .extension()
            .map(LanguageId::from_extension)
            .unwrap_or(LanguageId::Plain);

        self.old_scopes.clear();
        self.new_scopes.clear();
        self.old_highlights.clear();
        self.new_highlights.clear();

        let (old_content, new_content) = super::extract_content_from_patch(&pr_file.patch);

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
