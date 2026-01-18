//! Application state and lifecycle.

use std::collections::{HashMap, HashSet};

use crate::core::{
    diff_source_display, list_changed_files, list_changed_files_between,
    list_changed_files_from_base_with_merge_base, list_commit_files, resolve_revision, ChangedFile,
    CommentContext, CommentStore, DiffResult, DiffSource, FileCommentStore, FileViewedStore,
    FuzzyMatcher, RelPath, RepoRoot, RepoWatcher, TextBuffer, ViewedStore,
};
use crate::highlight::{FileHighlightCache, HighlighterCache, LanguageId, ScopeInfo};
use crate::theme::Theme;

use super::render::{build_path_cache, ThemeStyles};

mod comments;
mod diff;
mod external;
mod filter;
mod navigation;
mod patch;
mod pr;
mod state;
mod theme;
mod watcher;
mod worker_state;

pub use state::{
    CommentViewItem, CommentsState, DiffPaneMode, DiffViewMode, Focus, Mode, PRActionType,
    PatchState, PrState, SidebarState, UiState, ViewerState,
};
use worker_state::WorkerState;

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

    /// Background worker state.
    worker: WorkerState,

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
    /// Cached theme styles for rendering.
    pub theme_styles: ThemeStyles,
    /// Available theme names for selector.
    pub theme_list: Vec<String>,
    /// Selected index in theme selector.
    pub theme_selector_idx: usize,
    /// Original theme name (for cancel).
    pub theme_original: String,

    /// PR mode state.
    pub pr: PrState,
    /// Patch mode state.
    pub patch: PatchState,
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
        let theme_styles = ThemeStyles::from_theme(&theme);
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

        let worker = WorkerState::new(&repo);

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
            theme_styles,
            theme_list: Theme::list(),
            theme_selector_idx: 0,
            theme_original: theme_name.unwrap_or("default").to_string(),
            pr: PrState::default(),
            patch: PatchState::default(),
        };

        // Build path cache for sidebar
        app.rebuild_path_cache();

        // Initialize file watcher for live-reload modes (WorkingTree, Base)
        if matches!(app.source, DiffSource::WorkingTree | DiffSource::Base(_)) {
            match RepoWatcher::new(&app.repo) {
                Ok(w) => app.worker.watcher = Some(w),
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
        if self.patch.active {
            return format!("Patch ({})", self.patch.label);
        }
        diff_source_display(&self.source, &self.repo)
    }

    /// Get the currently selected file.
    pub fn selected_file(&self) -> Option<&ChangedFile> {
        self.files.get(self.sidebar.selected_idx)
    }

    pub(crate) fn diff_loading(&self) -> bool {
        self.worker.loading
    }

    /// Rebuild the cached truncated paths for sidebar.
    fn rebuild_path_cache(&mut self) {
        self.sidebar.path_cache = build_path_cache(self.files.iter().map(|f| f.path.as_str()));
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
        matches!(self.source, DiffSource::WorkingTree) && !self.patch.active
    }

    /// Mark dirty for redraw.
    pub fn mark_dirty(&mut self) {
        self.ui.dirty = true;
    }

    /// Clear dirty flag after drawing.
    pub fn clear_dirty(&mut self) {
        self.ui.dirty = false;
    }

    // ========================================================================
    // Sidebar filter
    // ========================================================================

    // ========================================================================
    // Theme selector
    // ========================================================================

    // ========================================================================
    // File watching
    // ========================================================================
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
    use super::{
        diff::{build_view_rows, map_diff_row_to_view_row},
        *,
    };
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

    #[test]
    fn worker_state_initializes_defaults() {
        let repo = RepoRoot::discover(std::path::Path::new(".")).unwrap();
        let worker = WorkerState::new(&repo);

        assert_eq!(worker.next_request_id, 1);
        assert!(worker.pending_request_id.is_none());
        assert!(worker.queued_request.is_none());
        assert!(!worker.loading);
        assert!(worker.diff.request_tx.is_some());

        assert_eq!(worker.next_pr_request_id, 1);
        assert!(worker.pending_pr_list_id.is_none());
        assert!(worker.pending_pr_load_id.is_none());
        assert!(worker.pending_pr.is_none());
        assert!(worker.pr_worker.request_tx.is_some());
        assert!(worker.watcher.is_none());
    }
}
