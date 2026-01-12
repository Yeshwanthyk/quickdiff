use crate::core::{CommentStatus, PRChangedFile, PRFilter, PullRequest};

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
    pub current: Option<PullRequest>,
    /// PR file list.
    pub files: Vec<PRChangedFile>,
    /// Available PRs.
    pub list: Vec<PullRequest>,
    /// Loading state.
    pub loading: bool,
    /// Filter for PR list.
    pub filter: PRFilter,
    /// Picker selection.
    pub picker_selected: usize,
    /// Picker scroll.
    pub picker_scroll: usize,
    /// Action text.
    pub action_text: String,
    /// Action type.
    pub action_type: Option<PRActionType>,
}
