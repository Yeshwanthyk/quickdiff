//! Application state and lifecycle.

use crate::core::{
    diff_source_display, get_parent_revision, list_changed_files, list_changed_files_between,
    list_changed_files_from_base_with_merge_base, list_commit_files, load_head_content,
    load_revision_content, load_working_content, ChangedFile, DiffResult, DiffSource,
    FileChangeKind, FileViewedStore, RepoRoot, TextBuffer, ViewedStore,
};
use crate::highlight::{HighlighterCache, LanguageId};

/// Focus state for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Diff,
}

/// UI mode (normal vs input modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    AddComment,
    ViewComments,
}

/// Application state.
pub struct App {
    /// Repository root.
    pub repo: RepoRoot,
    /// Diff source specification.
    pub source: DiffSource,
    /// Cached merge-base SHA for `DiffSource::Base`.
    pub cached_merge_base: Option<String>,
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
    /// Should the app quit?
    pub should_quit: bool,

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
    pub viewing_comments: Vec<(u64, String)>,
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
}

impl App {
    /// Create a new App from a repository root with optional diff source and file filter.
    pub fn new(
        repo: RepoRoot,
        source: DiffSource,
        file_filter: Option<String>,
    ) -> anyhow::Result<Self> {
        // Load files based on diff source
        let (mut files, cached_merge_base) = match &source {
            DiffSource::WorkingTree => (list_changed_files(&repo)?, None),
            DiffSource::Commit(commit) => (list_commit_files(&repo, commit)?, None),
            DiffSource::Range { from, to } => (list_changed_files_between(&repo, from, to)?, None),
            DiffSource::Base(base) => {
                let result = list_changed_files_from_base_with_merge_base(&repo, base)?;
                (result.files, Some(result.merge_base))
            }
        };

        // Apply file filter if set
        if let Some(ref filter) = file_filter {
            files.retain(|f| f.path.as_str().contains(filter));
        }

        let viewed = FileViewedStore::new(repo.as_str())?;
        let viewed_in_changeset = files.iter().filter(|f| viewed.is_viewed(&f.path)).count();

        let mut app = Self {
            repo,
            source,
            cached_merge_base,
            files,
            file_filter,
            selected_idx: 0,
            sidebar_scroll: 0,
            focus: Focus::Sidebar,
            viewed,
            viewed_in_changeset,
            should_quit: false,
            diff: None,
            old_buffer: None,
            new_buffer: None,
            is_binary: false,
            scroll_y: 0,
            scroll_x: 0,
            mode: Mode::default(),
            draft_comment: String::new(),
            viewing_comments: Vec::new(),
            error_msg: None,
            status_msg: None,
            dirty: true,
            highlighter: HighlighterCache::new(),
            current_lang: LanguageId::Plain,
        };

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
            app.load_current_diff();
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

    /// Load diff for the currently selected file.
    /// Errors are stored in error_msg rather than propagated.
    pub fn load_current_diff(&mut self) {
        self.error_msg = None;
        self.is_binary = false;

        let Some(file) = self.selected_file() else {
            self.diff = None;
            self.old_buffer = None;
            self.new_buffer = None;
            return;
        };

        // Clone what we need to avoid borrow issues
        let path = file.path.clone();
        let kind = file.kind;
        let old_path = file.old_path.clone();
        let source = self.source.clone();

        // Detect language for highlighting
        self.current_lang = path
            .extension()
            .map(LanguageId::from_extension)
            .unwrap_or(LanguageId::Plain);

        // Load content based on diff source and change kind
        let (old_bytes, new_bytes) = match source {
            DiffSource::WorkingTree => {
                // Working tree: compare HEAD to working directory
                self.load_working_tree_content(&path, kind, old_path.as_ref())
            }
            DiffSource::Commit(ref commit) => {
                // Single commit: compare parent to commit
                self.load_commit_content(commit, &path, kind, old_path.as_ref())
            }
            DiffSource::Range { ref from, ref to } => {
                // Range: compare from to to
                self.load_range_content(from, to, &path, kind, old_path.as_ref())
            }
            DiffSource::Base(ref base) => {
                // Base: compare merge-base to working tree
                let merge_base = self.cached_merge_base.as_deref().unwrap_or(base.as_str());
                self.load_base_content(merge_base, &path, kind, old_path.as_ref())
            }
        };

        let (old_bytes, new_bytes) = match (old_bytes, new_bytes) {
            (Ok(old), Ok(new)) => (old, new),
            (Err(e), _) | (_, Err(e)) => {
                self.error_msg = Some(format!("Failed to load content: {}", e));
                return;
            }
        };

        let old_buffer = TextBuffer::new(&old_bytes);
        let new_buffer = TextBuffer::new(&new_bytes);

        // Check for binary content
        self.is_binary = old_buffer.is_binary() || new_buffer.is_binary();

        let diff = DiffResult::compute(&old_buffer, &new_buffer);

        self.old_buffer = Some(old_buffer);
        self.new_buffer = Some(new_buffer);
        self.diff = Some(diff);
        self.scroll_y = 0;
        self.scroll_x = 0;
        self.dirty = true;

        // Update last selected (only for working tree mode)
        if matches!(self.source, DiffSource::WorkingTree) {
            self.viewed
                .set_last_selected(Some(path.as_str().to_string()));
        }
    }

    /// Load content for working tree diff (HEAD vs working directory).
    fn load_working_tree_content(
        &self,
        path: &crate::core::RelPath,
        kind: FileChangeKind,
        old_path: Option<&crate::core::RelPath>,
    ) -> (
        Result<Vec<u8>, crate::core::RepoError>,
        Result<Vec<u8>, crate::core::RepoError>,
    ) {
        match kind {
            FileChangeKind::Added | FileChangeKind::Untracked => {
                (Ok(Vec::new()), load_working_content(&self.repo, path))
            }
            FileChangeKind::Deleted => (load_head_content(&self.repo, path), Ok(Vec::new())),
            FileChangeKind::Modified | FileChangeKind::Renamed => {
                let old_p = old_path.unwrap_or(path);
                (
                    load_head_content(&self.repo, old_p),
                    load_working_content(&self.repo, path),
                )
            }
        }
    }

    /// Load content for a single commit diff (parent vs commit).
    fn load_commit_content(
        &self,
        commit: &str,
        path: &crate::core::RelPath,
        kind: FileChangeKind,
        old_path: Option<&crate::core::RelPath>,
    ) -> (
        Result<Vec<u8>, crate::core::RepoError>,
        Result<Vec<u8>, crate::core::RepoError>,
    ) {
        let parent = match get_parent_revision(&self.repo, commit) {
            Ok(p) => p,
            Err(e) => {
                let err_msg = e.to_string();
                return (
                    Err(crate::core::RepoError::GitError(err_msg.clone())),
                    Err(crate::core::RepoError::GitError(err_msg)),
                );
            }
        };

        match kind {
            FileChangeKind::Added => (
                Ok(Vec::new()),
                load_revision_content(&self.repo, commit, path),
            ),
            FileChangeKind::Deleted => (
                load_revision_content(&self.repo, &parent, path),
                Ok(Vec::new()),
            ),
            FileChangeKind::Modified | FileChangeKind::Renamed | FileChangeKind::Untracked => {
                let old_p = old_path.unwrap_or(path);
                (
                    load_revision_content(&self.repo, &parent, old_p),
                    load_revision_content(&self.repo, commit, path),
                )
            }
        }
    }

    /// Load content for a range diff (from..to).
    fn load_range_content(
        &self,
        from: &str,
        to: &str,
        path: &crate::core::RelPath,
        kind: FileChangeKind,
        old_path: Option<&crate::core::RelPath>,
    ) -> (
        Result<Vec<u8>, crate::core::RepoError>,
        Result<Vec<u8>, crate::core::RepoError>,
    ) {
        match kind {
            FileChangeKind::Added => (Ok(Vec::new()), load_revision_content(&self.repo, to, path)),
            FileChangeKind::Deleted => (
                load_revision_content(&self.repo, from, path),
                Ok(Vec::new()),
            ),
            FileChangeKind::Modified | FileChangeKind::Renamed | FileChangeKind::Untracked => {
                let old_p = old_path.unwrap_or(path);
                (
                    load_revision_content(&self.repo, from, old_p),
                    load_revision_content(&self.repo, to, path),
                )
            }
        }
    }

    /// Load content for base comparison (merge-base vs working tree).
    fn load_base_content(
        &self,
        merge_base: &str,
        path: &crate::core::RelPath,
        kind: FileChangeKind,
        old_path: Option<&crate::core::RelPath>,
    ) -> (
        Result<Vec<u8>, crate::core::RepoError>,
        Result<Vec<u8>, crate::core::RepoError>,
    ) {
        match kind {
            FileChangeKind::Added | FileChangeKind::Untracked => {
                (Ok(Vec::new()), load_working_content(&self.repo, path))
            }
            FileChangeKind::Deleted => (
                load_revision_content(&self.repo, merge_base, path),
                Ok(Vec::new()),
            ),
            FileChangeKind::Modified | FileChangeKind::Renamed => {
                let old_p = old_path.unwrap_or(path);
                (
                    load_revision_content(&self.repo, merge_base, old_p),
                    load_working_content(&self.repo, path),
                )
            }
        }
    }

    /// Move selection up in sidebar.
    pub fn select_prev(&mut self) {
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
            self.load_current_diff();
            self.dirty = true;
        }
    }

    /// Move selection down in sidebar.
    pub fn select_next(&mut self) {
        if self.selected_idx + 1 < self.files.len() {
            self.selected_idx += 1;
            self.load_current_diff();
            self.dirty = true;
        }
    }

    /// Toggle viewed state for current file.
    pub fn toggle_viewed(&mut self) {
        if let Some(file) = self.selected_file() {
            let path = file.path.clone();
            let now_viewed = self.viewed.toggle_viewed(path);
            if now_viewed {
                self.viewed_in_changeset += 1;
            } else {
                self.viewed_in_changeset = self.viewed_in_changeset.saturating_sub(1);
            }
            self.dirty = true;
        }
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
        // Check if we have a diff and are on a hunk
        let Some(diff) = &self.diff else {
            self.error_msg = Some("No diff available".to_string());
            self.dirty = true;
            return;
        };

        if diff.hunks.is_empty() {
            self.error_msg = Some("No hunks to comment on".to_string());
            self.dirty = true;
            return;
        }

        // Check if current scroll position is within a hunk
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
        use crate::core::{selector_from_hunk, Anchor, CommentStore, FileCommentStore, Selector};

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

        // Build selector from hunk
        let Some(selector) = selector_from_hunk(diff, hunk_idx) else {
            self.error_msg = Some("Failed to create comment anchor".to_string());
            self.mode = Mode::Normal;
            self.dirty = true;
            return;
        };

        let anchor = Anchor {
            selectors: vec![Selector::DiffHunkV1(selector)],
        };

        // Save to store
        let mut store = match FileCommentStore::open(&self.repo) {
            Ok(s) => s,
            Err(e) => {
                self.error_msg = Some(format!("Failed to open comment store: {}", e));
                self.mode = Mode::Normal;
                self.dirty = true;
                return;
            }
        };

        match store.add(path, self.draft_comment.clone(), anchor) {
            Ok(id) => {
                self.status_msg = Some(format!("Comment {} saved", id));
                self.error_msg = None;
            }
            Err(e) => {
                self.error_msg = Some(format!("Failed to save comment: {}", e));
            }
        }

        self.mode = Mode::Normal;
        self.draft_comment.clear();
        self.dirty = true;
    }

    /// Show comments overlay for current hunk.
    pub fn show_comments(&mut self) {
        use crate::core::{CommentStore, FileCommentStore};

        let Some(file) = self.selected_file() else {
            return;
        };
        let path = file.path.clone();

        let store = match FileCommentStore::open(&self.repo) {
            Ok(s) => s,
            Err(_) => return,
        };

        // Get all open comments for this file
        let comments = store.list_for_path(&path, false);

        if comments.is_empty() {
            self.status_msg = Some("No comments on this file".to_string());
            self.dirty = true;
            return;
        }

        // Collect id + message pairs
        self.viewing_comments = comments.iter().map(|c| (c.id, c.message.clone())).collect();

        self.mode = Mode::ViewComments;
        self.dirty = true;
    }

    /// Close comments overlay.
    pub fn close_comments(&mut self) {
        self.mode = Mode::Normal;
        self.viewing_comments.clear();
        self.dirty = true;
    }
}
