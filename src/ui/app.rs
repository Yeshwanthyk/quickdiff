//! Application state and lifecycle.

use std::{
    collections::{HashMap, HashSet},
    env,
    io::{self, Write},
    process::Command,
    sync::mpsc::TrySendError,
};

use crate::core::{
    diff_source_display, digest_hunk_changed_rows, format_anchor_summary, list_changed_files,
    list_changed_files_between, list_changed_files_from_base_with_merge_base, list_commit_files,
    resolve_revision, selector_from_hunk, Anchor, ChangedFile, CommentContext, CommentStatus,
    CommentStore, DiffResult, DiffSource, FileCommentStore, FileViewedStore, FuzzyMatcher, RelPath,
    RepoRoot, RepoWatcher, Selector, TextBuffer, ViewedStore,
};
use crate::highlight::{query_scopes, HighlighterCache, LanguageId, ScopeInfo};
use crate::theme::Theme;

use arboard::Clipboard;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use shell_words::split;

use super::worker::{
    spawn_diff_worker, spawn_pr_worker, DiffLoadRequest, DiffLoadResponse, DiffWorker, PrRequest,
    PrResponse, PrWorker,
};

/// Focus state for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Diff,
}

/// UI mode (normal vs input modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Normal navigation mode.
    #[default]
    Normal,
    /// Adding a comment.
    AddComment,
    /// Viewing comments overlay.
    ViewComments,
    /// Filtering files in sidebar.
    FilterFiles,
    /// Selecting a theme.
    SelectTheme,
    /// Viewing help overlay.
    Help,
    /// Browsing PR list.
    PRPicker,
    /// Composing PR review action.
    PRAction,
}

/// Type of PR review action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PRActionType {
    /// Approve the PR.
    Approve,
    /// Comment on the PR.
    Comment,
    /// Request changes on the PR.
    RequestChanges,
}

/// Layout mode for diff panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffPaneMode {
    /// Show both old and new panes.
    Both,
    /// Show only the old (left) pane full-width.
    OldOnly,
    /// Show only the new (right) pane full-width.
    NewOnly,
}

#[derive(Debug, Clone)]
pub struct CommentViewItem {
    pub id: u64,
    pub status: CommentStatus,
    pub message: String,
    pub anchor_summary: String,
    pub hunk_start_row: Option<usize>,
    pub stale: bool,
}

/// Application state.
pub struct App {
    /// Repository root.
    pub repo: RepoRoot,
    /// Diff source specification.
    pub source: DiffSource,
    /// Cached merge-base SHA for `DiffSource::Base`.
    pub cached_merge_base: Option<String>,
    /// Comment context for this view.
    pub comment_context: CommentContext,
    /// List of changed files.
    pub files: Vec<ChangedFile>,
    /// File path filter (only show this file if set).
    pub file_filter: Option<String>,
    /// Currently selected file index in sidebar.
    pub selected_idx: usize,
    /// Sidebar scroll offset (first visible file index).
    pub sidebar_scroll: usize,
    /// Current focus.
    pub focus: Focus,
    /// Viewed state store.
    pub viewed: FileViewedStore,
    /// Viewed files count in the current list.
    pub viewed_in_changeset: usize,
    /// Open comment counts for this context.
    pub open_comment_counts: HashMap<RelPath, usize>,
    /// Hunks (by index) that have open comments in this context for the selected file.
    pub commented_hunks: HashSet<usize>,
    /// Should the app quit?
    pub should_quit: bool,

    // Background diff loading
    worker: DiffWorker,
    next_request_id: u64,
    pending_request_id: Option<u64>,
    queued_request: Option<DiffLoadRequest>,
    /// Whether a diff is currently being loaded.
    pub loading: bool,

    // Diff state (lazy loaded)
    /// Current file's diff result.
    pub diff: Option<DiffResult>,
    /// Old text buffer for current file.
    pub old_buffer: Option<TextBuffer>,
    /// New text buffer for current file.
    pub new_buffer: Option<TextBuffer>,
    /// Whether current file is binary.
    pub is_binary: bool,

    // Viewport state
    /// Vertical scroll offset in diff view.
    pub scroll_y: usize,
    /// Horizontal scroll offset in diff view.
    pub scroll_x: usize,

    // UI state
    /// Current UI mode.
    pub mode: Mode,
    /// Draft comment text (when in AddComment mode).
    pub draft_comment: String,
    /// Comments to display in ViewComments mode.
    pub viewing_comments: Vec<CommentViewItem>,
    /// Selected index in comment list.
    pub viewing_comments_selected: usize,
    /// Scroll offset in comment list.
    pub viewing_comments_scroll: usize,
    /// Whether to include resolved comments.
    pub viewing_include_resolved: bool,
    /// Sidebar filter query (when in FilterFiles mode).
    pub sidebar_filter: String,
    /// Filtered file indices (empty = show all).
    pub filtered_indices: Vec<usize>,
    /// Fuzzy matcher for file filtering.
    fuzzy_matcher: FuzzyMatcher,
    /// Last error message to display.
    pub error_msg: Option<String>,
    /// Status message to display (non-error).
    pub status_msg: Option<String>,
    /// Whether the UI needs redrawing.
    pub dirty: bool,

    // Highlighting
    /// Syntax highlighter cache.
    pub highlighter: HighlighterCache,
    /// Current file's language.
    pub current_lang: LanguageId,

    // Sticky line scopes
    /// Scopes for old file content.
    pub old_scopes: Vec<ScopeInfo>,
    /// Scopes for new file content.
    pub new_scopes: Vec<ScopeInfo>,

    // Theme
    /// Current color theme.
    pub theme: Theme,
    /// Available theme names for selector.
    pub theme_list: Vec<String>,
    /// Selected index in theme selector.
    pub theme_selector_idx: usize,
    /// Original theme name (for cancel).
    pub theme_original: String,
    /// Current diff pane layout mode.
    pub diff_pane_mode: DiffPaneMode,

    // File watching
    /// File system watcher for live reload.
    watcher: Option<RepoWatcher>,

    // PR worker state
    pr_worker: PrWorker,
    next_pr_request_id: u64,
    pending_pr_list_id: Option<u64>,
    pending_pr_load_id: Option<u64>,
    pending_pr: Option<crate::core::PullRequest>,

    // PR mode state
    /// Whether we're in PR review mode.
    pub pr_mode: bool,
    /// Current PR if in PR mode.
    pub current_pr: Option<crate::core::PullRequest>,
    /// PR file list (parsed from diff).
    pub pr_files: Vec<crate::core::PRChangedFile>,
    /// Available PRs for picker.
    pub pr_list: Vec<crate::core::PullRequest>,
    /// Loading state for PR operations.
    pub pr_loading: bool,
    /// PR picker filter.
    pub pr_filter: crate::core::PRFilter,
    /// Selected index in PR picker.
    pub pr_picker_selected: usize,
    /// Scroll offset in PR picker.
    pub pr_picker_scroll: usize,
    /// PR action draft text.
    pub pr_action_text: String,
    /// PR action type being composed.
    pub pr_action_type: Option<PRActionType>,
}

fn comment_context_for_source(source: &DiffSource) -> CommentContext {
    match source {
        DiffSource::WorkingTree => CommentContext::Worktree,
        DiffSource::Base(base) => CommentContext::Base { base: base.clone() },
        DiffSource::Commit(commit) => CommentContext::Commit {
            commit: commit.clone(),
        },
        DiffSource::Range { from, to } => CommentContext::Range {
            from: from.clone(),
            to: to.clone(),
        },
        DiffSource::PullRequest { number, .. } => CommentContext::Commit {
            // Use PR number as pseudo-commit context
            commit: format!("pr-{}", number),
        },
    }
}

fn load_open_comment_counts(repo: &RepoRoot, context: &CommentContext) -> HashMap<RelPath, usize> {
    let Ok(store) = FileCommentStore::open(repo) else {
        return HashMap::new();
    };

    let mut counts: HashMap<RelPath, usize> = HashMap::new();
    for c in store.list(false) {
        if c.context.matches(context) {
            *counts.entry(c.path.clone()).or_insert(0) += 1;
        }
    }

    counts
}

impl App {
    /// Create a new App from a repository root with optional diff source and file filter.
    pub fn new(
        repo: RepoRoot,
        source: DiffSource,
        file_filter: Option<String>,
        theme_name: Option<&str>,
    ) -> anyhow::Result<Self> {
        let theme = Theme::load(theme_name.unwrap_or("default"));
        // Canonicalize commit/range sources so comment contexts match across invocations.
        let source = match source {
            DiffSource::Commit(commit) => {
                DiffSource::Commit(resolve_revision(&repo, &commit)?.to_string())
            }
            DiffSource::Range { from, to } => DiffSource::Range {
                from: resolve_revision(&repo, &from)?.to_string(),
                to: resolve_revision(&repo, &to)?.to_string(),
            },
            other => other,
        };

        let comment_context = comment_context_for_source(&source);

        // Load files based on diff source
        let (mut files, cached_merge_base) = match &source {
            DiffSource::WorkingTree => (list_changed_files(&repo)?, None),
            DiffSource::Commit(commit) => (list_commit_files(&repo, commit)?, None),
            DiffSource::Range { from, to } => (list_changed_files_between(&repo, from, to)?, None),
            DiffSource::Base(base) => {
                let result = list_changed_files_from_base_with_merge_base(&repo, base)?;
                (result.files, Some(result.merge_base))
            }
            DiffSource::PullRequest { .. } => {
                // PR files are loaded separately via load_pr()
                (Vec::new(), None)
            }
        };

        // Apply file filter if set
        if let Some(ref filter) = file_filter {
            files.retain(|f| f.path.as_str().contains(filter));
        }

        let viewed = FileViewedStore::new(repo.as_str())?;
        let viewed_in_changeset = files.iter().filter(|f| viewed.is_viewed(&f.path)).count();

        let open_comment_counts = load_open_comment_counts(&repo, &comment_context);

        let worker = spawn_diff_worker(repo.clone());
        let pr_worker = spawn_pr_worker(repo.clone());

        let mut app = Self {
            repo,
            source,
            cached_merge_base,
            comment_context,
            files,
            file_filter,
            selected_idx: 0,
            sidebar_scroll: 0,
            focus: Focus::Sidebar,
            viewed,
            viewed_in_changeset,
            open_comment_counts,
            commented_hunks: HashSet::new(),
            should_quit: false,
            worker,
            next_request_id: 1,
            pending_request_id: None,
            queued_request: None,
            loading: false,
            diff: None,
            old_buffer: None,
            new_buffer: None,
            is_binary: false,
            scroll_y: 0,
            scroll_x: 0,
            mode: Mode::default(),
            draft_comment: String::new(),
            viewing_comments: Vec::new(),
            viewing_comments_selected: 0,
            viewing_comments_scroll: 0,
            viewing_include_resolved: false,
            sidebar_filter: String::new(),
            filtered_indices: Vec::new(),
            fuzzy_matcher: FuzzyMatcher::new(),
            error_msg: None,
            status_msg: None,
            dirty: true,
            highlighter: HighlighterCache::new(),
            current_lang: LanguageId::Plain,
            old_scopes: Vec::new(),
            new_scopes: Vec::new(),
            theme,
            theme_list: Theme::list(),
            theme_selector_idx: 0,
            theme_original: theme_name.unwrap_or("default").to_string(),
            diff_pane_mode: DiffPaneMode::Both,
            watcher: None,
            pr_worker,
            next_pr_request_id: 1,
            pending_pr_list_id: None,
            pending_pr_load_id: None,
            pending_pr: None,
            pr_mode: false,
            current_pr: None,
            pr_files: Vec::new(),
            pr_list: Vec::new(),
            pr_loading: false,
            pr_filter: crate::core::PRFilter::default(),
            pr_picker_selected: 0,
            pr_picker_scroll: 0,
            pr_action_text: String::new(),
            pr_action_type: None,
        };

        // Initialize file watcher for live-reload modes (WorkingTree, Base)
        if matches!(app.source, DiffSource::WorkingTree | DiffSource::Base(_)) {
            match RepoWatcher::new(&app.repo) {
                Ok(w) => app.watcher = Some(w),
                Err(e) => {
                    // Non-fatal: just log and continue without watching
                    eprintln!("Warning: file watching disabled: {}", e);
                }
            }
        }

        // Restore last selected file if available (only for working tree mode)
        if matches!(app.source, DiffSource::WorkingTree) {
            if let Some(last) = app.viewed.last_selected() {
                if let Some(idx) = app.files.iter().position(|f| f.path.as_str() == last) {
                    app.selected_idx = idx;
                }
            }
        }

        // Load initial diff if there are files
        if !app.files.is_empty() {
            app.request_current_diff();
        }

        Ok(app)
    }

    /// Get display string for current diff source.
    pub fn source_display(&self) -> String {
        diff_source_display(&self.source, &self.repo)
    }

    /// Get the currently selected file.
    pub fn selected_file(&self) -> Option<&ChangedFile> {
        self.files.get(self.selected_idx)
    }

    fn refresh_current_file_comment_markers(&mut self) {
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

        let mut digest_to_hunk_idx: HashMap<String, usize> = HashMap::new();
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

    /// Request diff for the currently selected file.
    ///
    /// Work is performed on a background thread. Call `poll_worker()` to apply results.
    pub fn request_current_diff(&mut self) {
        // PR mode uses patch-based diff extraction instead of git show
        if self.pr_mode {
            self.request_current_pr_diff();
            return;
        }

        self.error_msg = None;
        self.status_msg = None;
        self.is_binary = false;
        self.commented_hunks.clear();

        let Some(file) = self.selected_file().cloned() else {
            self.diff = None;
            self.old_buffer = None;
            self.new_buffer = None;
            self.pending_request_id = None;
            self.queued_request = None;
            self.loading = false;
            return;
        };

        // Detect language for highlighting
        self.current_lang = file
            .path
            .extension()
            .map(LanguageId::from_extension)
            .unwrap_or(LanguageId::Plain);

        // Clear existing state immediately so UI reflects selection changes.
        self.diff = None;
        self.old_buffer = None;
        self.new_buffer = None;
        self.old_scopes.clear();
        self.new_scopes.clear();
        self.scroll_y = 0;
        self.scroll_x = 0;

        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        self.pending_request_id = Some(id);
        self.loading = true;
        self.dirty = true;

        let req = DiffLoadRequest {
            id,
            source: self.source.clone(),
            cached_merge_base: self.cached_merge_base.clone(),
            file: file.clone(),
        };

        if !self.enqueue_diff_request(req) {
            return;
        }

        // Update last selected (only for working tree mode)
        if matches!(self.source, DiffSource::WorkingTree) {
            self.viewed
                .set_last_selected(Some(file.path.as_str().to_string()));
        }
    }

    fn enqueue_diff_request(&mut self, req: DiffLoadRequest) -> bool {
        let Some(tx) = self.worker.request_tx.as_ref() else {
            self.error_msg = Some("Diff worker stopped".to_string());
            self.loading = false;
            self.pending_request_id = None;
            self.queued_request = None;
            self.dirty = true;
            return false;
        };

        match tx.try_send(req) {
            Ok(()) => {
                self.queued_request = None;
                true
            }
            Err(TrySendError::Full(req)) => {
                self.queued_request = Some(req);
                true
            }
            Err(TrySendError::Disconnected(_)) => {
                self.error_msg = Some("Diff worker stopped".to_string());
                self.loading = false;
                self.pending_request_id = None;
                self.queued_request = None;
                self.dirty = true;
                false
            }
        }
    }

    fn flush_queued_diff_request(&mut self) {
        let Some(req) = self.queued_request.take() else {
            return;
        };

        self.enqueue_diff_request(req);
    }

    fn send_pr_request(&mut self, req: PrRequest) -> bool {
        let Some(tx) = self.pr_worker.request_tx.as_ref() else {
            self.error_msg = Some("PR worker stopped".to_string());
            self.pr_loading = false;
            self.loading = false;
            self.pending_pr_list_id = None;
            self.pending_pr_load_id = None;
            self.pending_pr = None;
            self.dirty = true;
            return false;
        };

        if tx.send(req).is_err() {
            self.error_msg = Some("PR worker stopped".to_string());
            self.pr_loading = false;
            self.loading = false;
            self.pending_pr_list_id = None;
            self.pending_pr_load_id = None;
            self.pending_pr = None;
            self.dirty = true;
            return false;
        }

        true
    }

    /// Apply any completed diff loads.
    pub fn poll_worker(&mut self) {
        while let Ok(msg) = self.worker.response_rx.try_recv() {
            match msg {
                DiffLoadResponse::Loaded {
                    id,
                    old_buffer,
                    new_buffer,
                    diff,
                    is_binary,
                } => {
                    if self.pending_request_id != Some(id) {
                        continue;
                    }

                    self.pending_request_id = None;
                    self.loading = false;
                    self.error_msg = None;

                    self.is_binary = is_binary;
                    self.old_buffer = Some(old_buffer.clone());
                    self.new_buffer = Some(new_buffer.clone());
                    self.diff = diff;

                    if let Some(diff) = self.diff.as_ref() {
                        if let Some(first) = diff.hunks().first() {
                            self.scroll_y = first.start_row;
                        }
                    }

                    // Compute scopes for sticky line display
                    if !is_binary {
                        let lang = self.current_lang;
                        let old_str = String::from_utf8_lossy(old_buffer.as_bytes());
                        let new_str = String::from_utf8_lossy(new_buffer.as_bytes());
                        self.old_scopes = query_scopes(lang, &old_str);
                        self.new_scopes = query_scopes(lang, &new_str);
                    } else {
                        self.old_scopes.clear();
                        self.new_scopes.clear();
                    }

                    self.refresh_current_file_comment_markers();
                    self.dirty = true;
                }
                DiffLoadResponse::Error { id, message } => {
                    if self.pending_request_id != Some(id) {
                        continue;
                    }

                    self.pending_request_id = None;
                    self.loading = false;
                    self.diff = None;
                    self.old_buffer = None;
                    self.new_buffer = None;
                    self.commented_hunks.clear();
                    self.error_msg = Some(format!("Failed to load diff: {}", message));
                    self.dirty = true;
                }
            }
        }

        self.flush_queued_diff_request();
    }

    /// Apply any completed PR loads.
    pub fn poll_pr_worker(&mut self) {
        while let Ok(msg) = self.pr_worker.response_rx.try_recv() {
            match msg {
                PrResponse::List { id, prs } => {
                    if self.pending_pr_list_id != Some(id) {
                        continue;
                    }

                    self.pending_pr_list_id = None;
                    self.pr_list = prs;
                    self.pr_loading = false;
                    self.error_msg = None;

                    if self.pr_list.is_empty() {
                        self.status_msg = Some("No PRs found".to_string());
                    }

                    self.dirty = true;
                }
                PrResponse::ListError { id, message } => {
                    if self.pending_pr_list_id != Some(id) {
                        continue;
                    }

                    self.pending_pr_list_id = None;
                    self.pr_list.clear();
                    self.pr_loading = false;
                    self.error_msg = Some(format!("Failed to fetch PRs: {}", message));
                    self.dirty = true;
                }
                PrResponse::Diff { id, diff } => {
                    if self.pending_pr_load_id != Some(id) {
                        continue;
                    }

                    self.pending_pr_load_id = None;
                    self.pr_loading = false;
                    self.loading = false;
                    self.error_msg = None;

                    let pr = match self.pending_pr.take().or_else(|| self.current_pr.clone()) {
                        Some(pr) => pr,
                        None => {
                            self.error_msg = Some("No PR selected".to_string());
                            self.dirty = true;
                            continue;
                        }
                    };

                    let pr_files = crate::core::parse_unified_diff(&diff);

                    self.files = pr_files
                        .iter()
                        .map(|pf| ChangedFile {
                            path: pf.path.clone(),
                            kind: pf.kind,
                            old_path: pf.old_path.clone(),
                        })
                        .collect();

                    self.pr_files = pr_files;
                    self.pr_mode = true;
                    self.current_pr = Some(pr.clone());

                    self.source = DiffSource::PullRequest {
                        number: pr.number,
                        head: pr.head_ref_name.clone(),
                        base: pr.base_ref_name.clone(),
                    };

                    self.selected_idx = 0;
                    self.sidebar_scroll = 0;

                    if !self.files.is_empty() {
                        self.request_current_pr_diff();
                    } else {
                        self.diff = None;
                        self.old_buffer = None;
                        self.new_buffer = None;
                        self.status_msg = Some("PR has no changed files".to_string());
                    }

                    self.dirty = true;
                }
                PrResponse::DiffError { id, message } => {
                    if self.pending_pr_load_id != Some(id) {
                        continue;
                    }

                    self.pending_pr_load_id = None;
                    self.pending_pr = None;
                    self.pr_loading = false;
                    self.loading = false;
                    self.error_msg = Some(format!("Failed to load PR diff: {}", message));
                    self.dirty = true;
                }
            }
        }
    }

    /// Move selection up in sidebar.
    pub fn select_prev(&mut self) {
        if self.filtered_indices.is_empty() {
            // No filter - simple prev
            if self.selected_idx > 0 {
                self.selected_idx -= 1;
                self.request_current_diff();
                self.dirty = true;
            }
        } else {
            // Find current position in filtered list and move to prev
            if let Some(pos) = self
                .filtered_indices
                .iter()
                .position(|&i| i == self.selected_idx)
            {
                if pos > 0 {
                    self.selected_idx = self.filtered_indices[pos - 1];
                    self.request_current_diff();
                    self.dirty = true;
                }
            }
        }
    }

    /// Move selection down in sidebar.
    pub fn select_next(&mut self) {
        if self.filtered_indices.is_empty() {
            // No filter - simple next
            if self.selected_idx + 1 < self.files.len() {
                self.selected_idx += 1;
                self.request_current_diff();
                self.dirty = true;
            }
        } else {
            // Find current position in filtered list and move to next
            if let Some(pos) = self
                .filtered_indices
                .iter()
                .position(|&i| i == self.selected_idx)
            {
                if pos + 1 < self.filtered_indices.len() {
                    self.selected_idx = self.filtered_indices[pos + 1];
                    self.request_current_diff();
                    self.dirty = true;
                }
            }
        }
    }

    /// Toggle viewed state for current file.
    /// If marking as viewed, advances to next unviewed file.
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
            self.dirty = true;
        }
    }

    /// Advance to next unviewed file, respecting filter.
    fn advance_to_next_unviewed(&mut self) {
        let visible = if self.filtered_indices.is_empty() {
            (0..self.files.len()).collect::<Vec<_>>()
        } else {
            self.filtered_indices.clone()
        };

        // Find current position in visible list
        let cur_pos = visible
            .iter()
            .position(|&i| i == self.selected_idx)
            .unwrap_or(0);

        // Search forward from current position, wrapping around
        for offset in 1..=visible.len() {
            let pos = (cur_pos + offset) % visible.len();
            let idx = visible[pos];
            if !self.viewed.is_viewed(&self.files[idx].path) {
                self.selected_idx = idx;
                self.request_current_diff();
                return;
            }
        }
        // All files viewed - stay on current
    }

    /// Scroll diff view.
    pub fn scroll_diff(&mut self, delta_y: isize, delta_x: isize) {
        let old_y = self.scroll_y;
        let old_x = self.scroll_x;

        if delta_y < 0 {
            self.scroll_y = self.scroll_y.saturating_sub((-delta_y) as usize);
        } else {
            let max_scroll = self.diff.as_ref().map(|d| d.row_count()).unwrap_or(0);
            self.scroll_y = (self.scroll_y + delta_y as usize).min(max_scroll.saturating_sub(1));
        }

        if delta_x < 0 {
            self.scroll_x = self.scroll_x.saturating_sub((-delta_x) as usize);
        } else {
            self.scroll_x += delta_x as usize;
        }

        if self.scroll_y != old_y || self.scroll_x != old_x {
            self.dirty = true;
        }
    }

    /// Jump to next hunk.
    pub fn next_hunk(&mut self) {
        if let Some(diff) = &self.diff {
            if let Some(row) = diff.next_hunk_row(self.scroll_y) {
                self.scroll_y = row;
                self.dirty = true;
            }
        }
    }

    /// Jump to previous hunk.
    pub fn prev_hunk(&mut self) {
        if let Some(diff) = &self.diff {
            if let Some(row) = diff.prev_hunk_row(self.scroll_y) {
                self.scroll_y = row;
                self.dirty = true;
            }
        }
    }

    /// Get current hunk position as (1-based index, total).
    /// Returns None if no diff or not on a hunk.
    pub fn current_hunk_info(&self) -> Option<(usize, usize)> {
        let diff = self.diff.as_ref()?;
        let hunks = diff.hunks();
        if hunks.is_empty() {
            return None;
        }
        let hunk_idx = diff.hunk_at_row(self.scroll_y)?;
        Some((hunk_idx + 1, hunks.len()))
    }

    /// Switch focus.
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sidebar => Focus::Diff,
            Focus::Diff => Focus::Sidebar,
        };
        self.dirty = true;
    }

    /// Set focus directly.
    pub fn set_focus(&mut self, focus: Focus) {
        if self.focus != focus {
            self.focus = focus;
            self.dirty = true;
        }
    }

    /// Toggle old pane fullscreen mode (old-only <-> both).
    pub fn toggle_old_fullscreen(&mut self) {
        let next = match self.diff_pane_mode {
            DiffPaneMode::OldOnly => DiffPaneMode::Both,
            _ => DiffPaneMode::OldOnly,
        };
        self.set_diff_pane_mode(next);
    }

    /// Toggle new pane fullscreen mode (new-only <-> both).
    pub fn toggle_new_fullscreen(&mut self) {
        let next = match self.diff_pane_mode {
            DiffPaneMode::NewOnly => DiffPaneMode::Both,
            _ => DiffPaneMode::NewOnly,
        };
        self.set_diff_pane_mode(next);
    }

    fn set_diff_pane_mode(&mut self, mode: DiffPaneMode) {
        if self.diff_pane_mode != mode {
            self.diff_pane_mode = mode;
            self.dirty = true;
        }
    }

    /// Open the selected file in the user's configured editor.
    pub fn open_selected_in_editor(&mut self) {
        let Some(file) = self.selected_file() else {
            self.error_msg = Some("No file selected to open".to_string());
            self.dirty = true;
            return;
        };

        let path = file.path.to_absolute(&self.repo);
        let command_parts = match Self::editor_command() {
            Ok(parts) => parts,
            Err(msg) => {
                self.error_msg = Some(msg);
                self.dirty = true;
                return;
            }
        };

        let (program, args) = command_parts
            .split_first()
            .expect("editor command is non-empty");
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.arg(&path);

        if let Err(e) = Self::suspend_terminal_for_external() {
            self.error_msg = Some(format!("Failed to release terminal: {}", e));
            self.dirty = true;
            return;
        }

        let status = cmd.status();

        if let Err(e) = Self::resume_terminal_after_external() {
            self.error_msg = Some(format!("Failed to restore terminal: {}", e));
            self.dirty = true;
            return;
        }

        match status {
            Ok(status) => {
                if status.success() {
                    self.status_msg = Some(format!("Editor closed for {}", file.path.as_str()));
                    self.error_msg = None;
                } else {
                    self.error_msg = Some(format!("Editor exited with code {:?}", status.code()));
                }
            }
            Err(e) => {
                self.error_msg = Some(format!("Failed to launch editor: {}", e));
            }
        }

        self.dirty = true;
    }

    /// Determine the command used to launch the external editor.
    fn editor_command() -> Result<Vec<String>, String> {
        for key in ["QUICKDIFF_EDITOR", "VISUAL", "EDITOR"] {
            if let Ok(value) = env::var(key) {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match split(trimmed) {
                    Ok(parts) if !parts.is_empty() => return Ok(parts),
                    Ok(_) => continue,
                    Err(e) => {
                        return Err(format!("Failed to parse ${}: {}", key, e));
                    }
                }
            }
        }
        Err("Set $QUICKDIFF_EDITOR, $VISUAL, or $EDITOR to open files externally".to_string())
    }

    fn suspend_terminal_for_external() -> io::Result<()> {
        disable_raw_mode()?;
        execute!(io::stdout(), DisableMouseCapture)?;
        execute!(io::stdout(), LeaveAlternateScreen)?;
        io::stdout().flush()?;
        Ok(())
    }

    fn resume_terminal_after_external() -> io::Result<()> {
        execute!(io::stdout(), EnterAlternateScreen)?;
        enable_raw_mode()?;
        execute!(io::stdout(), EnableMouseCapture)?;
        io::stdout().flush()?;
        Ok(())
    }

    /// Copy the currently selected file path to the clipboard.
    pub fn copy_selected_path(&mut self) {
        let Some(file) = self.selected_file() else {
            self.error_msg = Some("No file selected to copy".to_string());
            self.dirty = true;
            return;
        };

        match Clipboard::new() {
            Ok(mut clipboard) => {
                if let Err(e) = clipboard.set_text(file.path.as_str().to_string()) {
                    self.error_msg = Some(format!("Clipboard error: {}", e));
                } else {
                    self.status_msg = Some(format!("Copied {} to clipboard", file.path));
                }
            }
            Err(e) => {
                self.error_msg = Some(format!("Clipboard unavailable: {}", e));
            }
        }

        self.dirty = true;
    }

    /// Reload the current diff or refresh file list manually.
    pub fn manual_reload(&mut self) {
        match self.source {
            DiffSource::WorkingTree | DiffSource::Base(_) => {
                self.refresh_file_list();
            }
            DiffSource::Commit(_) | DiffSource::Range { .. } => {
                if self.files.is_empty() {
                    self.status_msg = Some("No files to reload".to_string());
                } else {
                    self.request_current_diff();
                    self.status_msg = Some("Reloaded current diff".to_string());
                }
                self.dirty = true;
            }
            DiffSource::PullRequest { .. } => {
                // PR reload handled by refresh_pr() in PR mode
                self.request_current_diff();
                self.status_msg = Some("Reloaded PR diff".to_string());
                self.dirty = true;
            }
        }
    }

    /// Open the in-app help overlay.
    pub fn open_help(&mut self) {
        self.mode = Mode::Help;
        self.dirty = true;
    }

    /// Close the help overlay.
    pub fn close_help(&mut self) {
        if self.mode == Mode::Help {
            self.mode = Mode::Normal;
            self.dirty = true;
        }
    }

    /// Save state before exit.
    pub fn save_state(&self) -> anyhow::Result<()> {
        self.viewed.save()?;
        Ok(())
    }

    /// Check if current file is viewed.
    pub fn is_current_viewed(&self) -> bool {
        self.selected_file()
            .map(|f| self.viewed.is_viewed(&f.path))
            .unwrap_or(false)
    }

    /// Get viewed/total count string.
    /// Only counts files that are currently in the changed list.
    pub fn viewed_status(&self) -> String {
        format!("{}/{}", self.viewed_in_changeset, self.files.len())
    }

    /// Check if we're in working tree mode (uncommitted changes).
    /// Comments are only available in this mode.
    pub fn is_worktree_mode(&self) -> bool {
        matches!(self.source, DiffSource::WorkingTree)
    }

    /// Mark dirty for redraw.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Clear dirty flag after drawing.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Start adding a comment on the current hunk.
    pub fn start_add_comment(&mut self) {
        let Some(diff) = &self.diff else {
            self.error_msg = Some("No diff available".to_string());
            self.dirty = true;
            return;
        };

        if diff.hunks().is_empty() {
            self.error_msg = Some("No hunks to comment on".to_string());
            self.dirty = true;
            return;
        }

        if diff.hunk_at_row(self.scroll_y).is_none() {
            self.error_msg = Some("Not on a hunk - navigate to a hunk first".to_string());
            self.dirty = true;
            return;
        }

        self.mode = Mode::AddComment;
        self.draft_comment.clear();
        self.error_msg = None;
        self.status_msg = None;
        self.dirty = true;
    }

    /// Cancel adding a comment.
    pub fn cancel_add_comment(&mut self) {
        self.mode = Mode::Normal;
        self.draft_comment.clear();
        self.dirty = true;
    }

    /// Save the current draft comment.
    pub fn save_comment(&mut self) {
        if self.draft_comment.trim().is_empty() {
            self.error_msg = Some("Comment cannot be empty".to_string());
            self.dirty = true;
            return;
        }

        let Some(diff) = &self.diff else {
            self.error_msg = Some("No diff available".to_string());
            self.mode = Mode::Normal;
            self.dirty = true;
            return;
        };

        let Some(hunk_idx) = diff.hunk_at_row(self.scroll_y) else {
            self.error_msg = Some("No hunk at current position".to_string());
            self.mode = Mode::Normal;
            self.dirty = true;
            return;
        };

        let Some(file) = self.selected_file() else {
            self.error_msg = Some("No file selected".to_string());
            self.mode = Mode::Normal;
            self.dirty = true;
            return;
        };

        let path = file.path.clone();

        let Some(selector) = selector_from_hunk(diff, hunk_idx) else {
            self.error_msg = Some("Failed to create comment anchor".to_string());
            self.mode = Mode::Normal;
            self.dirty = true;
            return;
        };

        let anchor = Anchor {
            selectors: vec![Selector::DiffHunkV1(selector)],
        };

        let mut store = match FileCommentStore::open(&self.repo) {
            Ok(s) => s,
            Err(e) => {
                self.error_msg = Some(format!("Failed to open comment store: {}", e));
                self.mode = Mode::Normal;
                self.dirty = true;
                return;
            }
        };

        match store.add(
            path.clone(),
            self.comment_context.clone(),
            self.draft_comment.clone(),
            anchor,
        ) {
            Ok(id) => {
                self.status_msg = Some(format!("Comment {} saved", id));
                self.error_msg = None;
                *self.open_comment_counts.entry(path).or_insert(0) += 1;
                self.commented_hunks.insert(hunk_idx);
            }
            Err(e) => {
                self.error_msg = Some(format!("Failed to save comment: {}", e));
            }
        }

        self.mode = Mode::Normal;
        self.draft_comment.clear();
        self.dirty = true;
    }

    fn refresh_viewing_comments(&mut self) {
        let Some(file) = self.selected_file() else {
            self.viewing_comments.clear();
            self.viewing_comments_selected = 0;
            self.viewing_comments_scroll = 0;
            return;
        };

        let include_resolved = self.viewing_include_resolved;

        let Ok(store) = FileCommentStore::open(&self.repo) else {
            self.viewing_comments.clear();
            self.viewing_comments_selected = 0;
            self.viewing_comments_scroll = 0;
            return;
        };

        let comments = store.list_for_path(&file.path, include_resolved);

        let selected_id = self
            .viewing_comments
            .get(self.viewing_comments_selected)
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

        self.viewing_comments = items;

        self.viewing_comments_selected = match selected_id {
            Some(id) => self
                .viewing_comments
                .iter()
                .position(|c| c.id == id)
                .unwrap_or(0),
            None => 0,
        };

        if self.viewing_comments.is_empty() {
            self.viewing_comments_selected = 0;
            self.viewing_comments_scroll = 0;
        } else {
            self.viewing_comments_selected = self
                .viewing_comments_selected
                .min(self.viewing_comments.len() - 1);
            self.viewing_comments_scroll = self
                .viewing_comments_scroll
                .min(self.viewing_comments.len() - 1);
        }
    }

    /// Show comments overlay for current file.
    pub fn show_comments(&mut self) {
        self.viewing_include_resolved = false;
        self.viewing_comments_selected = 0;
        self.viewing_comments_scroll = 0;
        self.refresh_viewing_comments();

        if self.viewing_comments.is_empty() {
            self.status_msg = Some("No comments on this file".to_string());
            self.dirty = true;
            return;
        }

        self.mode = Mode::ViewComments;
        self.dirty = true;
    }

    /// Close the comments view and return to normal mode.
    pub fn close_comments(&mut self) {
        self.mode = Mode::Normal;
        self.viewing_comments.clear();
        self.viewing_comments_selected = 0;
        self.viewing_comments_scroll = 0;
        self.dirty = true;
    }

    /// Select the next comment in the list.
    pub fn comments_select_next(&mut self) {
        if self.viewing_comments.is_empty() {
            return;
        }
        self.viewing_comments_selected =
            (self.viewing_comments_selected + 1).min(self.viewing_comments.len() - 1);
        self.dirty = true;
    }

    /// Select the previous comment in the list.
    pub fn comments_select_prev(&mut self) {
        self.viewing_comments_selected = self.viewing_comments_selected.saturating_sub(1);
        self.dirty = true;
    }

    /// Toggle whether resolved comments are shown.
    pub fn comments_toggle_include_resolved(&mut self) {
        self.viewing_include_resolved = !self.viewing_include_resolved;
        self.refresh_viewing_comments();
        self.dirty = true;
    }

    /// Jump to the selected comment's location in the diff.
    pub fn comments_jump_to_selected(&mut self) {
        if self.viewing_comments.is_empty() {
            return;
        }

        let Some(item) = self
            .viewing_comments
            .get(self.viewing_comments_selected)
            .cloned()
        else {
            return;
        };

        let Some(row) = item.hunk_start_row else {
            self.status_msg = Some("Comment anchor is stale (hunk not found)".to_string());
            self.dirty = true;
            return;
        };

        self.scroll_y = row;
        self.focus = Focus::Diff;
        self.close_comments();
        self.dirty = true;
    }

    /// Resolve the currently selected comment.
    pub fn comments_resolve_selected(&mut self) {
        if self.viewing_comments.is_empty() {
            return;
        }

        let Some(item) = self
            .viewing_comments
            .get(self.viewing_comments_selected)
            .cloned()
        else {
            return;
        };

        if item.status != CommentStatus::Open {
            self.status_msg = Some("Comment already resolved".to_string());
            self.dirty = true;
            return;
        }

        let mut store = match FileCommentStore::open(&self.repo) {
            Ok(s) => s,
            Err(e) => {
                self.error_msg = Some(format!("Failed to open comment store: {}", e));
                self.dirty = true;
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
                self.status_msg = Some(format!("Resolved comment {}", item.id));
            }
            Ok(false) => {
                self.error_msg = Some(format!("Comment {} not found", item.id));
            }
            Err(e) => {
                self.error_msg = Some(format!("Failed to resolve comment: {}", e));
            }
        }

        self.dirty = true;
    }

    // ========================================================================
    // Sidebar filter
    // ========================================================================

    /// Start filtering files in sidebar.
    pub fn start_filter(&mut self) {
        self.mode = Mode::FilterFiles;
        self.sidebar_filter.clear();
        self.dirty = true;
    }

    /// Apply the current filter query using fuzzy matching.
    pub fn apply_filter(&mut self) {
        self.recompute_filter();
        self.mode = Mode::Normal;
        self.dirty = true;
    }

    /// Recompute filtered indices from current query (for live filtering).
    fn recompute_filter(&mut self) {
        let query = self.sidebar_filter.trim();
        if query.is_empty() {
            self.filtered_indices.clear();
        } else {
            self.filtered_indices = self.fuzzy_matcher.filter_sorted(
                query,
                self.files
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (i, f.path.as_str())),
            );
        }
        // Reset selection to first match if current selection is filtered out
        if !self.filtered_indices.is_empty() && !self.filtered_indices.contains(&self.selected_idx)
        {
            self.selected_idx = self.filtered_indices[0];
            self.request_current_diff();
        }
    }

    /// Update filter live as user types.
    pub fn update_filter_live(&mut self) {
        self.recompute_filter();
        self.dirty = true;
    }

    /// Cancel filter and restore full list.
    pub fn cancel_filter(&mut self) {
        self.mode = Mode::Normal;
        self.sidebar_filter.clear();
        self.filtered_indices.clear();
        self.dirty = true;
    }

    /// Clear filter while in Normal mode.
    pub fn clear_filter(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.filtered_indices.clear();
            self.sidebar_filter.clear();
            self.dirty = true;
        }
    }

    /// Get visible files (filtered or all).
    pub fn visible_files(&self) -> Vec<(usize, &ChangedFile)> {
        if self.filtered_indices.is_empty() {
            self.files.iter().enumerate().collect()
        } else {
            self.filtered_indices
                .iter()
                .filter_map(|&i| self.files.get(i).map(|f| (i, f)))
                .collect()
        }
    }

    /// Check if a file index is visible (passes filter).
    pub fn is_file_visible(&self, idx: usize) -> bool {
        self.filtered_indices.is_empty() || self.filtered_indices.contains(&idx)
    }

    // ========================================================================
    // Theme selector
    // ========================================================================

    /// Open theme selector.
    pub fn open_theme_selector(&mut self) {
        self.theme_list = Theme::list();
        // Find current theme in list
        let current = self
            .theme_list
            .iter()
            .position(|t| t == &self.theme_original)
            .unwrap_or(0);
        self.theme_selector_idx = current;
        self.mode = Mode::SelectTheme;
        self.dirty = true;
    }

    /// Close theme selector without applying.
    pub fn close_theme_selector(&mut self) {
        // Restore original theme
        self.theme = Theme::load(&self.theme_original);
        self.mode = Mode::Normal;
        self.dirty = true;
    }

    /// Move selection up in theme list.
    pub fn theme_select_prev(&mut self) {
        if self.theme_selector_idx > 0 {
            self.theme_selector_idx -= 1;
            // Live preview
            self.theme = Theme::load(&self.theme_list[self.theme_selector_idx]);
            self.dirty = true;
        }
    }

    /// Move selection down in theme list.
    pub fn theme_select_next(&mut self) {
        if self.theme_selector_idx + 1 < self.theme_list.len() {
            self.theme_selector_idx += 1;
            // Live preview
            self.theme = Theme::load(&self.theme_list[self.theme_selector_idx]);
            self.dirty = true;
        }
    }

    /// Apply selected theme and close selector.
    pub fn theme_apply(&mut self) {
        self.theme_original = self.theme_list[self.theme_selector_idx].clone();
        self.mode = Mode::Normal;
        self.dirty = true;
    }

    // ========================================================================
    // File watching
    // ========================================================================

    /// Poll the file watcher for changes.
    ///
    /// Returns `true` if files changed and refresh is needed.
    pub fn poll_watcher(&mut self) -> bool {
        let Some(ref watcher) = self.watcher else {
            return false;
        };

        if watcher.poll().is_some() {
            self.refresh_file_list();
            true
        } else {
            false
        }
    }

    /// Refresh the file list from disk.
    ///
    /// Preserves selection if the file still exists, otherwise resets.
    /// Re-requests diff for current file if it changed.
    fn refresh_file_list(&mut self) {
        // Remember current selection
        let current_path = self.selected_file().map(|f| f.path.clone());

        // Reload files based on diff source
        let new_files = match &self.source {
            DiffSource::WorkingTree => list_changed_files(&self.repo).ok(),
            DiffSource::Base(base) => {
                list_changed_files_from_base_with_merge_base(&self.repo, base)
                    .ok()
                    .map(|r| {
                        // Update cached merge base
                        self.cached_merge_base = Some(r.merge_base);
                        r.files
                    })
            }
            // Commit/Range/PR modes don't use this refresh (historical/remote data)
            DiffSource::Commit(_) | DiffSource::Range { .. } | DiffSource::PullRequest { .. } => {
                return
            }
        };

        let Some(mut files) = new_files else {
            return;
        };

        // Apply file filter if set
        if let Some(ref filter) = self.file_filter {
            files.retain(|f| f.path.as_str().contains(filter));
        }

        // Update file list
        self.files = files;

        // Restore selection if file still exists
        if let Some(ref path) = current_path {
            if let Some(idx) = self.files.iter().position(|f| &f.path == path) {
                self.selected_idx = idx;
                // Re-request diff (file content may have changed)
                self.request_current_diff();
            } else {
                // File removed - select next valid or first
                self.selected_idx = self.selected_idx.min(self.files.len().saturating_sub(1));
                if !self.files.is_empty() {
                    self.request_current_diff();
                } else {
                    self.diff = None;
                    self.old_buffer = None;
                    self.new_buffer = None;
                }
            }
        } else if !self.files.is_empty() {
            self.selected_idx = 0;
            self.request_current_diff();
        }

        // Update viewed count
        self.viewed_in_changeset = self
            .files
            .iter()
            .filter(|f| self.viewed.is_viewed(&f.path))
            .count();

        // Update open comment counts
        self.open_comment_counts = load_open_comment_counts(&self.repo, &self.comment_context);

        // Clear filter (file indices may have shifted)
        if !self.filtered_indices.is_empty() {
            self.filtered_indices.clear();
            self.sidebar_filter.clear();
        }

        self.status_msg = Some("Refreshed".to_string());
        self.dirty = true;
    }

    // ========================================================================
    // PR Picker
    // ========================================================================

    /// Open PR picker mode.
    pub fn open_pr_picker(&mut self) {
        if !crate::core::is_gh_available() {
            self.error_msg = Some("GitHub CLI not available. Run 'gh auth login'".to_string());
            self.dirty = true;
            return;
        }

        self.mode = Mode::PRPicker;
        self.pr_picker_selected = 0;
        self.pr_picker_scroll = 0;
        self.pr_loading = true;
        self.dirty = true;

        // Fetch PRs synchronously for now (could be backgrounded later)
        self.fetch_pr_list();
    }

    /// Fetch PR list from GitHub.
    pub fn fetch_pr_list(&mut self) {
        self.pr_loading = true;
        self.error_msg = None;

        let id = self.next_pr_request_id;
        self.next_pr_request_id = self.next_pr_request_id.wrapping_add(1);
        self.pending_pr_list_id = Some(id);

        if !self.send_pr_request(PrRequest::List {
            id,
            filter: self.pr_filter,
        }) {
            self.pending_pr_list_id = None;
            self.pr_loading = false;
        }

        self.dirty = true;
    }

    /// Close PR picker and return to normal mode.
    pub fn close_pr_picker(&mut self) {
        self.mode = Mode::Normal;
        self.pr_list.clear();
        self.pr_picker_selected = 0;
        self.pr_picker_scroll = 0;
        self.pr_loading = false;
        self.pending_pr_list_id = None;
        self.dirty = true;
    }

    /// Select next PR in picker.
    pub fn pr_picker_next(&mut self) {
        if !self.pr_list.is_empty() {
            self.pr_picker_selected = (self.pr_picker_selected + 1).min(self.pr_list.len() - 1);
            self.dirty = true;
        }
    }

    /// Select previous PR in picker.
    pub fn pr_picker_prev(&mut self) {
        self.pr_picker_selected = self.pr_picker_selected.saturating_sub(1);
        self.dirty = true;
    }

    /// Cycle to next PR filter.
    pub fn pr_picker_next_filter(&mut self) {
        self.pr_filter = match self.pr_filter {
            crate::core::PRFilter::All => crate::core::PRFilter::Mine,
            crate::core::PRFilter::Mine => crate::core::PRFilter::ReviewRequested,
            crate::core::PRFilter::ReviewRequested => crate::core::PRFilter::All,
        };
        self.pr_picker_selected = 0;
        self.fetch_pr_list();
    }

    /// Cycle to previous PR filter.
    pub fn pr_picker_prev_filter(&mut self) {
        self.pr_filter = match self.pr_filter {
            crate::core::PRFilter::All => crate::core::PRFilter::ReviewRequested,
            crate::core::PRFilter::Mine => crate::core::PRFilter::All,
            crate::core::PRFilter::ReviewRequested => crate::core::PRFilter::Mine,
        };
        self.pr_picker_selected = 0;
        self.fetch_pr_list();
    }

    /// Select the highlighted PR and load its diff.
    pub fn pr_picker_select(&mut self) {
        if self.pr_list.is_empty() {
            return;
        }

        let pr = self.pr_list[self.pr_picker_selected].clone();
        self.load_pr(pr);
    }

    /// Load a specific PR's diff.
    pub fn load_pr(&mut self, pr: crate::core::PullRequest) {
        self.pr_loading = true;
        self.loading = true;
        self.error_msg = None;
        self.mode = Mode::Normal;
        self.dirty = true;

        let id = self.next_pr_request_id;
        self.next_pr_request_id = self.next_pr_request_id.wrapping_add(1);
        self.pending_pr_load_id = Some(id);
        self.pending_pr = Some(pr.clone());

        self.pr_mode = true;
        self.current_pr = Some(pr.clone());
        self.pr_files.clear();
        self.files.clear();
        self.diff = None;
        self.old_buffer = None;
        self.new_buffer = None;
        self.is_binary = false;
        self.scroll_y = 0;
        self.scroll_x = 0;

        self.source = DiffSource::PullRequest {
            number: pr.number,
            head: pr.head_ref_name.clone(),
            base: pr.base_ref_name.clone(),
        };

        if !self.send_pr_request(PrRequest::LoadDiff {
            id,
            pr_number: pr.number,
        }) {
            self.pending_pr_load_id = None;
            self.pending_pr = None;
            self.pr_loading = false;
            self.loading = false;
        }
    }

    /// Request diff for currently selected file in PR mode.
    fn request_current_pr_diff(&mut self) {
        if !self.pr_mode || self.pr_files.is_empty() {
            return;
        }

        let Some(pr_file) = self.pr_files.get(self.selected_idx) else {
            return;
        };

        // Detect language for highlighting
        self.current_lang = pr_file
            .path
            .extension()
            .map(crate::highlight::LanguageId::from_extension)
            .unwrap_or(crate::highlight::LanguageId::Plain);

        // For PR mode, we use the patch directly
        // Create synthetic old/new buffers from the patch
        let (old_content, new_content) = extract_content_from_patch(&pr_file.patch);

        let old_buffer = TextBuffer::new(old_content.as_bytes());
        let new_buffer = TextBuffer::new(new_content.as_bytes());

        let is_binary = old_buffer.is_binary() || new_buffer.is_binary();
        let diff = if is_binary {
            None
        } else {
            Some(DiffResult::compute(&old_buffer, &new_buffer))
        };

        self.is_binary = is_binary;
        self.old_buffer = Some(old_buffer);
        self.new_buffer = Some(new_buffer);
        self.diff = diff;

        // Jump to first hunk
        if let Some(diff) = self.diff.as_ref() {
            if let Some(first) = diff.hunks().first() {
                self.scroll_y = first.start_row;
            }
        }

        self.scroll_x = 0;
        self.error_msg = None;
        self.loading = false;
        self.dirty = true;
    }

    /// Exit PR mode and return to working tree.
    pub fn exit_pr_mode(&mut self) {
        if !self.pr_mode {
            return;
        }

        self.pr_mode = false;
        self.current_pr = None;
        self.pr_files.clear();
        self.pending_pr_load_id = None;
        self.pending_pr = None;
        self.pr_loading = false;
        self.loading = false;
        self.source = DiffSource::WorkingTree;

        // Reload working tree files
        match list_changed_files(&self.repo) {
            Ok(files) => {
                self.files = files;
                self.selected_idx = 0;
                if !self.files.is_empty() {
                    self.request_current_diff();
                }
            }
            Err(e) => {
                self.error_msg = Some(format!("Failed to reload: {}", e));
            }
        }

        self.dirty = true;
    }

    /// Refresh current PR.
    pub fn refresh_pr(&mut self) {
        if let Some(pr) = self.current_pr.clone() {
            self.load_pr(pr);
        }
    }

    // ========================================================================
    // PR Actions
    // ========================================================================

    /// Start approve action.
    pub fn start_pr_approve(&mut self) {
        if !self.pr_mode || self.current_pr.is_none() {
            self.error_msg = Some("Not in PR mode".to_string());
            self.dirty = true;
            return;
        }

        self.pr_action_type = Some(PRActionType::Approve);
        self.pr_action_text.clear();
        self.mode = Mode::PRAction;
        self.dirty = true;
    }

    /// Start comment action.
    pub fn start_pr_comment(&mut self) {
        if !self.pr_mode || self.current_pr.is_none() {
            self.error_msg = Some("Not in PR mode".to_string());
            self.dirty = true;
            return;
        }

        self.pr_action_type = Some(PRActionType::Comment);
        self.pr_action_text.clear();
        self.mode = Mode::PRAction;
        self.dirty = true;
    }

    /// Start request-changes action.
    pub fn start_pr_request_changes(&mut self) {
        if !self.pr_mode || self.current_pr.is_none() {
            self.error_msg = Some("Not in PR mode".to_string());
            self.dirty = true;
            return;
        }

        self.pr_action_type = Some(PRActionType::RequestChanges);
        self.pr_action_text.clear();
        self.mode = Mode::PRAction;
        self.dirty = true;
    }

    /// Cancel current PR action.
    pub fn cancel_pr_action(&mut self) {
        self.mode = Mode::Normal;
        self.pr_action_type = None;
        self.pr_action_text.clear();
        self.dirty = true;
    }

    /// Submit the current PR action.
    pub fn submit_pr_action(&mut self) {
        let Some(pr) = &self.current_pr else {
            self.error_msg = Some("No PR selected".to_string());
            self.cancel_pr_action();
            return;
        };

        let pr_number = pr.number;
        let repo_path = self.repo.path().to_path_buf();

        let result = match self.pr_action_type {
            Some(PRActionType::Approve) => {
                let body = if self.pr_action_text.trim().is_empty() {
                    None
                } else {
                    Some(self.pr_action_text.as_str())
                };
                crate::core::approve_pr(&repo_path, pr_number, body)
            }
            Some(PRActionType::Comment) => {
                if self.pr_action_text.trim().is_empty() {
                    self.error_msg = Some("Comment cannot be empty".to_string());
                    self.dirty = true;
                    return;
                }
                crate::core::comment_pr(&repo_path, pr_number, &self.pr_action_text)
            }
            Some(PRActionType::RequestChanges) => {
                if self.pr_action_text.trim().is_empty() {
                    self.error_msg = Some("Message cannot be empty".to_string());
                    self.dirty = true;
                    return;
                }
                crate::core::request_changes_pr(&repo_path, pr_number, &self.pr_action_text)
            }
            None => {
                self.cancel_pr_action();
                return;
            }
        };

        match result {
            Ok(()) => {
                let action_name = match self.pr_action_type {
                    Some(PRActionType::Approve) => "approved",
                    Some(PRActionType::Comment) => "commented on",
                    Some(PRActionType::RequestChanges) => "requested changes on",
                    None => "reviewed",
                };
                self.status_msg = Some(format!("PR #{} {}", pr_number, action_name));
                self.error_msg = None;
            }
            Err(e) => {
                self.error_msg = Some(format!("Failed: {}", e));
            }
        }

        self.cancel_pr_action();
    }
}

/// Extract old and new content from a unified diff patch.
///
/// This is a simplified extraction that reconstructs file content from
/// the patch hunks. Not perfect but good enough for diff display.
fn extract_content_from_patch(patch: &str) -> (String, String) {
    let mut old_lines: Vec<&str> = Vec::new();
    let mut new_lines: Vec<&str> = Vec::new();
    let mut in_hunk = false;

    for line in patch.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }

        if !in_hunk {
            continue;
        }

        if line.starts_with('-') && !line.starts_with("---") {
            // Deleted line - only in old
            old_lines.push(&line[1..]);
        } else if line.starts_with('+') && !line.starts_with("+++") {
            // Added line - only in new
            new_lines.push(&line[1..]);
        } else if line.starts_with(' ') || line.is_empty() {
            // Context line - in both
            let content = line.strip_prefix(' ').unwrap_or(line);
            old_lines.push(content);
            new_lines.push(content);
        }
    }

    (old_lines.join("\n"), new_lines.join("\n"))
}
