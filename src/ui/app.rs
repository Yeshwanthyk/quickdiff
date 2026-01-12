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
    RenderRow, RepoRoot, RepoWatcher, Selector, TextBuffer, ViewedStore,
};
use crate::highlight::{query_scopes, FileHighlightCache, HighlighterCache, LanguageId, ScopeInfo};
use crate::theme::Theme;

use arboard::Clipboard;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use shell_words::split;

use super::render::build_path_cache;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffPaneMode {
    /// Show both old and new panes.
    #[default]
    Both,
    /// Show only the old (left) pane full-width.
    OldOnly,
    /// Show only the new (right) pane full-width.
    NewOnly,
}

/// View mode for diffs (hunks-only vs full file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffViewMode {
    /// Only render hunks with context lines.
    #[default]
    HunksOnly,
    /// Render the full file.
    FullFile,
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

/// Sidebar navigation and filter state.
#[derive(Debug, Default)]
pub struct SidebarState {
    /// Currently selected file index.
    pub selected_idx: usize,
    /// Scroll offset (first visible file).
    pub scroll: usize,
    /// Filter query string.
    pub filter: String,
    /// Filtered file indices (empty = show all).
    pub filtered_indices: Vec<usize>,
    /// Cached truncated paths for sidebar display.
    pub path_cache: Vec<String>,
}

/// Diff viewer viewport state.
#[derive(Debug, Default)]
pub struct ViewerState {
    /// Vertical scroll offset.
    pub scroll_y: usize,
    /// Horizontal scroll offset.
    pub scroll_x: usize,
    /// View mode (hunks-only vs full file).
    pub view_mode: DiffViewMode,
    /// Pane layout mode.
    pub pane_mode: DiffPaneMode,
    /// Precomputed hunk view rows.
    pub hunk_view_rows: Vec<usize>,
}

/// Comment viewing/editing state.
#[derive(Debug, Default)]
pub struct CommentsState {
    /// Draft comment text.
    pub draft: String,
    /// Comments being viewed.
    pub viewing: Vec<CommentViewItem>,
    /// Selected index in view.
    pub selected: usize,
    /// Scroll offset in view.
    pub scroll: usize,
    /// Include resolved comments.
    pub include_resolved: bool,
}

/// UI mode and message state.
#[derive(Debug, Default)]
pub struct UiState {
    /// Current mode.
    pub mode: Mode,
    /// Error message.
    pub error: Option<String>,
    /// Status message.
    pub status: Option<String>,
    /// Dirty flag for redraw.
    pub dirty: bool,
}

/// Pull request mode state.
#[derive(Debug, Default)]
pub struct PrState {
    /// Whether in PR mode.
    pub active: bool,
    /// Current PR.
    pub current: Option<crate::core::PullRequest>,
    /// PR file list.
    pub files: Vec<crate::core::PRChangedFile>,
    /// Available PRs.
    pub list: Vec<crate::core::PullRequest>,
    /// Loading state.
    pub loading: bool,
    /// Filter for PR list.
    pub filter: crate::core::PRFilter,
    /// Picker selection.
    pub picker_selected: usize,
    /// Picker scroll.
    pub picker_scroll: usize,
    /// Action text.
    pub action_text: String,
    /// Action type.
    pub action_type: Option<PRActionType>,
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
    /// Sidebar state.
    pub sidebar: SidebarState,
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

    /// Viewer state (scroll, view mode).
    pub viewer: ViewerState,

    /// UI state (mode, messages).
    pub ui: UiState,
    /// Comments state.
    pub comments: CommentsState,

    /// Fuzzy matcher for file filtering.
    fuzzy_matcher: FuzzyMatcher,

    // Highlighting
    /// Syntax highlighter cache.
    pub highlighter: HighlighterCache,
    /// Cached per-line highlights for the old pane.
    pub old_highlights: FileHighlightCache,
    /// Cached per-line highlights for the new pane.
    pub new_highlights: FileHighlightCache,
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

    // File watching
    /// File system watcher for live reload.
    watcher: Option<RepoWatcher>,

    // PR worker state
    pr_worker: PrWorker,
    next_pr_request_id: u64,
    pending_pr_list_id: Option<u64>,
    pending_pr_load_id: Option<u64>,
    pending_pr: Option<crate::core::PullRequest>,

    /// PR mode state.
    pub pr: PrState,
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

fn build_view_rows(diff: &DiffResult, mode: DiffViewMode) -> Vec<usize> {
    if mode != DiffViewMode::HunksOnly {
        return Vec::new();
    }

    let mut rows = Vec::new();
    for hunk in diff.hunks() {
        rows.extend(hunk.start_row..(hunk.start_row + hunk.row_count));
    }
    rows
}

fn map_diff_row_to_view_row(view_rows: &[usize], diff_row: usize) -> Option<usize> {
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
            sidebar: SidebarState::default(),
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
            viewer: ViewerState {
                view_mode: DiffViewMode::HunksOnly,
                pane_mode: DiffPaneMode::Both,
                ..Default::default()
            },
            ui: UiState {
                dirty: true,
                ..Default::default()
            },
            comments: CommentsState::default(),
            fuzzy_matcher: FuzzyMatcher::new(),
            highlighter: HighlighterCache::new(),
            old_highlights: FileHighlightCache::new(),
            new_highlights: FileHighlightCache::new(),
            current_lang: LanguageId::Plain,
            old_scopes: Vec::new(),
            new_scopes: Vec::new(),
            theme,
            theme_list: Theme::list(),
            theme_selector_idx: 0,
            theme_original: theme_name.unwrap_or("default").to_string(),
            watcher: None,
            pr_worker,
            next_pr_request_id: 1,
            pending_pr_list_id: None,
            pending_pr_load_id: None,
            pending_pr: None,
            pr: PrState::default(),
        };

        // Build path cache for sidebar
        app.rebuild_path_cache();

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
                    app.sidebar.selected_idx = idx;
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
        self.files.get(self.sidebar.selected_idx)
    }

    /// Rebuild the cached truncated paths for sidebar.
    fn rebuild_path_cache(&mut self) {
        self.sidebar.path_cache = build_path_cache(self.files.iter().map(|f| f.path.as_str()));
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
            self.pending_request_id = None;
            self.queued_request = None;
            self.old_highlights.clear();
            self.new_highlights.clear();
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
        self.viewer.hunk_view_rows.clear();
        self.old_buffer = None;
        self.new_buffer = None;
        self.old_scopes.clear();
        self.new_scopes.clear();
        self.old_highlights.clear();
        self.new_highlights.clear();
        self.viewer.scroll_y = 0;
        self.viewer.scroll_x = 0;

        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        self.pending_request_id = Some(id);
        self.loading = true;
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

        // Update last selected (only for working tree mode)
        if matches!(self.source, DiffSource::WorkingTree) {
            self.viewed
                .set_last_selected(Some(file.path.as_str().to_string()));
        }
    }

    fn enqueue_diff_request(&mut self, req: DiffLoadRequest) -> bool {
        let Some(tx) = self.worker.request_tx.as_ref() else {
            self.ui.error = Some("Diff worker stopped".to_string());
            self.loading = false;
            self.pending_request_id = None;
            self.queued_request = None;
            self.ui.dirty = true;
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
                self.ui.error = Some("Diff worker stopped".to_string());
                self.loading = false;
                self.pending_request_id = None;
                self.queued_request = None;
                self.ui.dirty = true;
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
            self.ui.error = Some("PR worker stopped".to_string());
            self.pr.loading = false;
            self.loading = false;
            self.pending_pr_list_id = None;
            self.pending_pr_load_id = None;
            self.pending_pr = None;
            self.ui.dirty = true;
            return false;
        };

        if tx.send(req).is_err() {
            self.ui.error = Some("PR worker stopped".to_string());
            self.pr.loading = false;
            self.loading = false;
            self.pending_pr_list_id = None;
            self.pending_pr_load_id = None;
            self.pending_pr = None;
            self.ui.dirty = true;
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

                    // Compute scopes for sticky line display
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
                    if self.pending_request_id != Some(id) {
                        continue;
                    }

                    self.pending_request_id = None;
                    self.loading = false;
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

    /// Apply any completed PR loads.
    pub fn poll_pr_worker(&mut self) {
        while let Ok(msg) = self.pr_worker.response_rx.try_recv() {
            match msg {
                PrResponse::List { id, prs } => {
                    if self.pending_pr_list_id != Some(id) {
                        continue;
                    }

                    self.pending_pr_list_id = None;
                    self.pr.list = prs;
                    self.pr.loading = false;
                    self.ui.error = None;

                    if self.pr.list.is_empty() {
                        self.ui.status = Some("No PRs found".to_string());
                    }

                    self.ui.dirty = true;
                }
                PrResponse::ListError { id, message } => {
                    if self.pending_pr_list_id != Some(id) {
                        continue;
                    }

                    self.pending_pr_list_id = None;
                    self.pr.list.clear();
                    self.pr.loading = false;
                    self.ui.error = Some(format!("Failed to fetch PRs: {}", message));
                    self.ui.dirty = true;
                }
                PrResponse::Diff { id, diff } => {
                    if self.pending_pr_load_id != Some(id) {
                        continue;
                    }

                    self.pending_pr_load_id = None;
                    self.pr.loading = false;
                    self.loading = false;
                    self.ui.error = None;

                    let pr = match self.pending_pr.take().or_else(|| self.pr.current.clone()) {
                        Some(pr) => pr,
                        None => {
                            self.ui.error = Some("No PR selected".to_string());
                            self.ui.dirty = true;
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
                    if self.pending_pr_load_id != Some(id) {
                        continue;
                    }

                    self.pending_pr_load_id = None;
                    self.pending_pr = None;
                    self.pr.loading = false;
                    self.loading = false;
                    self.ui.error = Some(format!("Failed to load PR diff: {}", message));
                    self.ui.dirty = true;
                }
            }
        }
    }

    /// Move selection up in sidebar.
    pub fn select_prev(&mut self) {
        if self.sidebar.filtered_indices.is_empty() {
            // No filter - simple prev
            if self.sidebar.selected_idx > 0 {
                self.sidebar.selected_idx -= 1;
                self.request_current_diff();
                self.ui.dirty = true;
            }
        } else {
            // Find current position in filtered list and move to prev
            if let Some(pos) = self
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
    }

    /// Move selection down in sidebar.
    pub fn select_next(&mut self) {
        if self.sidebar.filtered_indices.is_empty() {
            // No filter - simple next
            if self.sidebar.selected_idx + 1 < self.files.len() {
                self.sidebar.selected_idx += 1;
                self.request_current_diff();
                self.ui.dirty = true;
            }
        } else {
            // Find current position in filtered list and move to next
            if let Some(pos) = self
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
            self.ui.dirty = true;
        }
    }

    /// Advance to next unviewed file, respecting filter.
    fn advance_to_next_unviewed(&mut self) {
        let visible = if self.sidebar.filtered_indices.is_empty() {
            (0..self.files.len()).collect::<Vec<_>>()
        } else {
            self.sidebar.filtered_indices.clone()
        };

        // Find current position in visible list
        let cur_pos = visible
            .iter()
            .position(|&i| i == self.sidebar.selected_idx)
            .unwrap_or(0);

        // Search forward from current position, wrapping around
        for offset in 1..=visible.len() {
            let pos = (cur_pos + offset) % visible.len();
            let idx = visible[pos];
            if !self.viewed.is_viewed(&self.files[idx].path) {
                self.sidebar.selected_idx = idx;
                self.request_current_diff();
                return;
            }
        }
        // All files viewed - stay on current
    }

    fn rebuild_view_rows(&mut self) {
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

    fn diff_row_to_view_row(&self, diff_row: usize) -> Option<usize> {
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

    /// Scroll diff view.
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

    /// Jump to next hunk.
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

    /// Jump to previous hunk.
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
    /// Returns None if no diff or not on a hunk.
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

    /// Switch focus.
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sidebar => Focus::Diff,
            Focus::Diff => Focus::Sidebar,
        };
        self.ui.dirty = true;
    }

    /// Set focus directly.
    pub fn set_focus(&mut self, focus: Focus) {
        if self.focus != focus {
            self.focus = focus;
            self.ui.dirty = true;
        }
    }

    /// Toggle old pane fullscreen mode (old-only <-> both).
    pub fn toggle_old_fullscreen(&mut self) {
        let next = match self.viewer.pane_mode {
            DiffPaneMode::OldOnly => DiffPaneMode::Both,
            _ => DiffPaneMode::OldOnly,
        };
        self.set_diff_pane_mode(next);
    }

    /// Toggle new pane fullscreen mode (new-only <-> both).
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

    /// Open the selected file in the user's configured editor.
    pub fn open_selected_in_editor(&mut self) {
        let Some(file) = self.selected_file() else {
            self.ui.error = Some("No file selected to open".to_string());
            self.ui.dirty = true;
            return;
        };

        let path = file.path.to_absolute(&self.repo);
        let command_parts = match Self::editor_command() {
            Ok(parts) => parts,
            Err(msg) => {
                self.ui.error = Some(msg);
                self.ui.dirty = true;
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
            self.ui.error = Some(format!("Failed to release terminal: {}", e));
            self.ui.dirty = true;
            return;
        }

        let status = cmd.status();

        if let Err(e) = Self::resume_terminal_after_external() {
            self.ui.error = Some(format!("Failed to restore terminal: {}", e));
            self.ui.dirty = true;
            return;
        }

        match status {
            Ok(status) => {
                if status.success() {
                    self.ui.status = Some(format!("Editor closed for {}", file.path.as_str()));
                    self.ui.error = None;
                } else {
                    self.ui.error = Some(format!("Editor exited with code {:?}", status.code()));
                }
            }
            Err(e) => {
                self.ui.error = Some(format!("Failed to launch editor: {}", e));
            }
        }

        self.ui.dirty = true;
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
            self.ui.error = Some("No file selected to copy".to_string());
            self.ui.dirty = true;
            return;
        };

        match Clipboard::new() {
            Ok(mut clipboard) => {
                if let Err(e) = clipboard.set_text(file.path.as_str().to_string()) {
                    self.ui.error = Some(format!("Clipboard error: {}", e));
                } else {
                    self.ui.status = Some(format!("Copied {} to clipboard", file.path));
                }
            }
            Err(e) => {
                self.ui.error = Some(format!("Clipboard unavailable: {}", e));
            }
        }

        self.ui.dirty = true;
    }

    /// Reload the current diff or refresh file list manually.
    pub fn manual_reload(&mut self) {
        match self.source {
            DiffSource::WorkingTree | DiffSource::Base(_) => {
                self.refresh_file_list();
            }
            DiffSource::Commit(_) | DiffSource::Range { .. } => {
                if self.files.is_empty() {
                    self.ui.status = Some("No files to reload".to_string());
                } else {
                    self.request_current_diff();
                    self.ui.status = Some("Reloaded current diff".to_string());
                }
                self.ui.dirty = true;
            }
            DiffSource::PullRequest { .. } => {
                // PR reload handled by refresh_pr() in PR mode
                self.request_current_diff();
                self.ui.status = Some("Reloaded PR diff".to_string());
                self.ui.dirty = true;
            }
        }
    }

    /// Open the in-app help overlay.
    pub fn open_help(&mut self) {
        self.ui.mode = Mode::Help;
        self.ui.dirty = true;
    }

    /// Close the help overlay.
    pub fn close_help(&mut self) {
        if self.ui.mode == Mode::Help {
            self.ui.mode = Mode::Normal;
            self.ui.dirty = true;
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
        self.ui.dirty = true;
    }

    /// Clear dirty flag after drawing.
    pub fn clear_dirty(&mut self) {
        self.ui.dirty = false;
    }

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

    // ========================================================================
    // Sidebar filter
    // ========================================================================

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

    /// Recompute filtered indices from current query (for live filtering).
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
        // Reset selection to first match if current selection is filtered out
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
        self.ui.mode = Mode::SelectTheme;
        self.ui.dirty = true;
    }

    /// Close theme selector without applying.
    pub fn close_theme_selector(&mut self) {
        // Restore original theme
        self.theme = Theme::load(&self.theme_original);
        self.ui.mode = Mode::Normal;
        self.ui.dirty = true;
    }

    /// Move selection up in theme list.
    pub fn theme_select_prev(&mut self) {
        if self.theme_selector_idx > 0 {
            self.theme_selector_idx -= 1;
            // Live preview
            self.theme = Theme::load(&self.theme_list[self.theme_selector_idx]);
            self.ui.dirty = true;
        }
    }

    /// Move selection down in theme list.
    pub fn theme_select_next(&mut self) {
        if self.theme_selector_idx + 1 < self.theme_list.len() {
            self.theme_selector_idx += 1;
            // Live preview
            self.theme = Theme::load(&self.theme_list[self.theme_selector_idx]);
            self.ui.dirty = true;
        }
    }

    /// Apply selected theme and close selector.
    pub fn theme_apply(&mut self) {
        self.theme_original = self.theme_list[self.theme_selector_idx].clone();
        self.ui.mode = Mode::Normal;
        self.ui.dirty = true;
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
        self.rebuild_path_cache();

        // Restore selection if file still exists
        if let Some(ref path) = current_path {
            if let Some(idx) = self.files.iter().position(|f| &f.path == path) {
                self.sidebar.selected_idx = idx;
                // Re-request diff (file content may have changed)
                self.request_current_diff();
            } else {
                // File removed - select next valid or first
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

        // Update viewed count
        self.viewed_in_changeset = self
            .files
            .iter()
            .filter(|f| self.viewed.is_viewed(&f.path))
            .count();

        // Update open comment counts
        self.open_comment_counts = load_open_comment_counts(&self.repo, &self.comment_context);

        // Clear filter (file indices may have shifted)
        if !self.sidebar.filtered_indices.is_empty() {
            self.sidebar.filtered_indices.clear();
            self.sidebar.filter.clear();
        }

        self.ui.status = Some("Refreshed".to_string());
        self.ui.dirty = true;
    }

    // ========================================================================
    // PR Picker
    // ========================================================================

    /// Open PR picker mode.
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

        // Fetch PRs synchronously for now (could be backgrounded later)
        self.fetch_pr_list();
    }

    /// Fetch PR list from GitHub.
    pub fn fetch_pr_list(&mut self) {
        self.pr.loading = true;
        self.ui.error = None;

        let id = self.next_pr_request_id;
        self.next_pr_request_id = self.next_pr_request_id.wrapping_add(1);
        self.pending_pr_list_id = Some(id);

        if !self.send_pr_request(PrRequest::List {
            id,
            filter: self.pr.filter,
        }) {
            self.pending_pr_list_id = None;
            self.pr.loading = false;
        }

        self.ui.dirty = true;
    }

    /// Close PR picker and return to normal mode.
    pub fn close_pr_picker(&mut self) {
        self.ui.mode = Mode::Normal;
        self.pr.list.clear();
        self.pr.picker_selected = 0;
        self.pr.picker_scroll = 0;
        self.pr.loading = false;
        self.pending_pr_list_id = None;
        self.ui.dirty = true;
    }

    /// Select next PR in picker.
    pub fn pr_picker_next(&mut self) {
        if !self.pr.list.is_empty() {
            self.pr.picker_selected = (self.pr.picker_selected + 1).min(self.pr.list.len() - 1);
            self.ui.dirty = true;
        }
    }

    /// Select previous PR in picker.
    pub fn pr_picker_prev(&mut self) {
        self.pr.picker_selected = self.pr.picker_selected.saturating_sub(1);
        self.ui.dirty = true;
    }

    /// Cycle to next PR filter.
    pub fn pr_picker_next_filter(&mut self) {
        self.pr.filter = match self.pr.filter {
            crate::core::PRFilter::All => crate::core::PRFilter::Mine,
            crate::core::PRFilter::Mine => crate::core::PRFilter::ReviewRequested,
            crate::core::PRFilter::ReviewRequested => crate::core::PRFilter::All,
        };
        self.pr.picker_selected = 0;
        self.fetch_pr_list();
    }

    /// Cycle to previous PR filter.
    pub fn pr_picker_prev_filter(&mut self) {
        self.pr.filter = match self.pr.filter {
            crate::core::PRFilter::All => crate::core::PRFilter::ReviewRequested,
            crate::core::PRFilter::Mine => crate::core::PRFilter::All,
            crate::core::PRFilter::ReviewRequested => crate::core::PRFilter::Mine,
        };
        self.pr.picker_selected = 0;
        self.fetch_pr_list();
    }

    /// Select the highlighted PR and load its diff.
    pub fn pr_picker_select(&mut self) {
        if self.pr.list.is_empty() {
            return;
        }

        let pr = self.pr.list[self.pr.picker_selected].clone();
        self.load_pr(pr);
    }

    /// Load a specific PR's diff.
    pub fn load_pr(&mut self, pr: crate::core::PullRequest) {
        self.pr.loading = true;
        self.loading = true;
        self.ui.error = None;
        self.ui.mode = Mode::Normal;
        self.ui.dirty = true;

        let id = self.next_pr_request_id;
        self.next_pr_request_id = self.next_pr_request_id.wrapping_add(1);
        self.pending_pr_load_id = Some(id);
        self.pending_pr = Some(pr.clone());

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
            self.pending_pr_load_id = None;
            self.pending_pr = None;
            self.pr.loading = false;
            self.loading = false;
        }
    }

    /// Request diff for currently selected file in PR mode.
    fn request_current_pr_diff(&mut self) {
        if !self.pr.active || self.pr.files.is_empty() {
            return;
        }

        let Some(pr_file) = self.pr.files.get(self.sidebar.selected_idx) else {
            return;
        };

        // Detect language for highlighting
        self.current_lang = pr_file
            .path
            .extension()
            .map(crate::highlight::LanguageId::from_extension)
            .unwrap_or(crate::highlight::LanguageId::Plain);

        self.old_scopes.clear();
        self.new_scopes.clear();
        self.old_highlights.clear();
        self.new_highlights.clear();

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

        // Jump to first hunk
        if let Some(diff) = self.diff.as_ref() {
            if let Some(first) = diff.hunks().first() {
                if let Some(view_row) = self.diff_row_to_view_row(first.start_row) {
                    self.viewer.scroll_y = view_row;
                }
            }
        }

        self.viewer.scroll_x = 0;
        self.ui.error = None;
        self.loading = false;
        self.ui.dirty = true;
    }

    /// Exit PR mode and return to working tree.
    pub fn exit_pr_mode(&mut self) {
        if !self.pr.active {
            return;
        }

        self.pr.active = false;
        self.pr.current = None;
        self.pr.files.clear();
        self.pending_pr_load_id = None;
        self.pending_pr = None;
        self.pr.loading = false;
        self.loading = false;
        self.source = DiffSource::WorkingTree;

        // Reload working tree files
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

    /// Refresh current PR.
    pub fn refresh_pr(&mut self) {
        if let Some(pr) = self.pr.current.clone() {
            self.load_pr(pr);
        }
    }

    /// Open the current PR in browser.
    pub fn open_pr_in_browser(&mut self) {
        let Some(pr) = &self.pr.current else {
            self.ui.error = Some("No PR selected".to_string());
            self.ui.dirty = true;
            return;
        };

        let pr_number = pr.number;
        let repo_path = self.repo.path().to_path_buf();

        match crate::core::open_pr_in_browser(&repo_path, pr_number) {
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

    // ========================================================================
    // PR Actions
    // ========================================================================

    /// Start approve action.
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

    /// Start comment action.
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

    /// Start request-changes action.
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

    /// Cancel current PR action.
    pub fn cancel_pr_action(&mut self) {
        self.ui.mode = Mode::Normal;
        self.pr.action_type = None;
        self.pr.action_text.clear();
        self.ui.dirty = true;
    }

    /// Submit the current PR action.
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
                crate::core::approve_pr(&repo_path, pr_number, body)
            }
            Some(PRActionType::Comment) => {
                if self.pr.action_text.trim().is_empty() {
                    self.ui.error = Some("Comment cannot be empty".to_string());
                    self.ui.dirty = true;
                    return;
                }
                crate::core::comment_pr(&repo_path, pr_number, &self.pr.action_text)
            }
            Some(PRActionType::RequestChanges) => {
                if self.pr.action_text.trim().is_empty() {
                    self.ui.error = Some("Message cannot be empty".to_string());
                    self.ui.dirty = true;
                    return;
                }
                crate::core::request_changes_pr(&repo_path, pr_number, &self.pr.action_text)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{DiffResult, TextBuffer};

    #[test]
    fn test_extract_content_from_patch_simple() {
        let patch = r#"@@ -1,3 +1,3 @@
 line1
-line2
+modified2
 line3"#;

        let (old, new) = extract_content_from_patch(patch);
        assert_eq!(old, "line1\nline2\nline3");
        assert_eq!(new, "line1\nmodified2\nline3");
    }

    #[test]
    fn test_extract_content_from_patch_multi_hunk() {
        let patch = r#"@@ -1,2 +1,2 @@
 header
-old_first
+new_first
@@ -10,2 +10,2 @@
 middle
-old_second
+new_second"#;

        let (old, new) = extract_content_from_patch(patch);
        assert_eq!(old, "header\nold_first\nmiddle\nold_second");
        assert_eq!(new, "header\nnew_first\nmiddle\nnew_second");
    }

    #[test]
    fn test_extract_content_from_patch_with_rename_header() {
        // Rename patches have extra header lines before the hunk
        let patch = r#"diff --git a/old.rs b/new.rs
similarity index 95%
rename from old.rs
rename to new.rs
--- a/old.rs
+++ b/new.rs
@@ -1,2 +1,2 @@
 content
-old
+new"#;

        let (old, new) = extract_content_from_patch(patch);
        assert_eq!(old, "content\nold");
        assert_eq!(new, "content\nnew");
    }

    #[test]
    fn test_extract_content_from_patch_empty() {
        // Empty patch (no hunks)
        let patch = r#"diff --git a/file.rs b/file.rs
--- a/file.rs
+++ b/file.rs"#;

        let (old, new) = extract_content_from_patch(patch);
        assert_eq!(old, "");
        assert_eq!(new, "");
    }

    #[test]
    fn test_extract_content_from_patch_add_only() {
        let patch = r#"@@ -0,0 +1,2 @@
+new line 1
+new line 2"#;

        let (old, new) = extract_content_from_patch(patch);
        assert_eq!(old, "");
        assert_eq!(new, "new line 1\nnew line 2");
    }

    #[test]
    fn test_extract_content_from_patch_delete_only() {
        let patch = r#"@@ -1,2 +0,0 @@
-deleted line 1
-deleted line 2"#;

        let (old, new) = extract_content_from_patch(patch);
        assert_eq!(old, "deleted line 1\ndeleted line 2");
        assert_eq!(new, "");
    }

    #[test]
    fn view_rows_use_hunks_only() {
        let old = TextBuffer::new(b"l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\n");
        let new = TextBuffer::new(b"l1\nx\nl3\nl4\nl5\nl6\nl7\ny\nl9\nl10\n");
        let diff = DiffResult::compute_with_context(&old, &new, 1);

        assert_eq!(diff.hunks().len(), 2);

        let view_rows = build_view_rows(&diff, DiffViewMode::HunksOnly);
        let total_rows: usize = diff.hunks().iter().map(|h| h.row_count).sum();
        assert_eq!(view_rows.len(), total_rows);

        let first = diff.hunks().first().unwrap();
        let second = diff.hunks().last().unwrap();
        let gap_row = first.start_row + first.row_count;
        let mapped = map_diff_row_to_view_row(&view_rows, gap_row).unwrap();
        assert_eq!(view_rows[mapped], second.start_row);

        let after_last = second.start_row + second.row_count + 5;
        let mapped_last = map_diff_row_to_view_row(&view_rows, after_last).unwrap();
        assert_eq!(
            view_rows[mapped_last],
            second.start_row + second.row_count - 1
        );
    }
}
