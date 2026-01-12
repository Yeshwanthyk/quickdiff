use git2::{IndexAddOption, Repository, Signature};
use quickdiff::core::{DiffSource, RepoRoot};
use quickdiff::ui::{App, Mode};
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const FILE_ALPHA: &str = "alpha.txt";
const FILE_RUST: &str = "src/lib.rs";
const FILE_NOTES: &str = "docs/notes.md";

fn ensure_test_config_dir() -> &'static std::path::PathBuf {
    static CONFIG_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    CONFIG_DIR.get_or_init(|| {
        let base = std::env::temp_dir().join(format!("quickdiff-ui-tests-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let config_dir = base.join(".config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("HOME", &base);
        std::env::set_var("XDG_CONFIG_HOME", &config_dir);
        config_dir
    })
}

fn reset_persistent_state() {
    let config_dir = ensure_test_config_dir().clone();
    let quickdiff_dir = config_dir.join("quickdiff");
    let _ = std::fs::remove_dir_all(&quickdiff_dir);
}

struct RepoHarness {
    _dir: TempDir,
    repo: RepoRoot,
}

impl RepoHarness {
    fn new() -> Self {
        reset_persistent_state();
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let repo = RepoRoot::discover(dir.path()).unwrap();
        Self { _dir: dir, repo }
    }

    fn app(&self) -> App {
        App::new(
            self.repo.clone(),
            DiffSource::WorkingTree,
            None,
            Some("default"),
        )
        .unwrap()
    }
}

fn init_repo(path: &Path) {
    let repo = Repository::init(path).unwrap();
    {
        let mut config = repo.config().unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        config.set_str("user.name", "Test").unwrap();
    }

    std::fs::create_dir_all(path.join("src")).unwrap();
    std::fs::create_dir_all(path.join("docs")).unwrap();

    std::fs::write(path.join(FILE_ALPHA), "alpha line one\nalpha line two\n").unwrap();
    std::fs::write(
        path.join(FILE_RUST),
        "pub fn meaning() -> i32 {\n    41\n}\n",
    )
    .unwrap();
    std::fs::write(path.join(FILE_NOTES), "# Notes\n\nOriginal\n").unwrap();

    let mut index = repo.index().unwrap();
    index.add_all(["."], IndexAddOption::DEFAULT, None).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = Signature::now("Test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
        .unwrap();

    std::fs::write(path.join(FILE_ALPHA), "alpha line one\nchanged line\n").unwrap();
    std::fs::write(
        path.join(FILE_RUST),
        "pub fn meaning() -> i32 {\n    42\n}\n",
    )
    .unwrap();
    std::fs::write(path.join(FILE_NOTES), "# Notes\n\nUpdated body\n").unwrap();
}

fn wait_for_diff(app: &mut App) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        app.poll_worker();
        if app.diff.is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("diff load timed out");
}

fn select_file(app: &mut App, path: &str) {
    let idx = app
        .files
        .iter()
        .position(|f| f.path.as_str() == path)
        .unwrap_or_else(|| panic!("missing file {}", path));
    app.sidebar.selected_idx = idx;
    app.request_current_diff();
    wait_for_diff(app);
}

#[test]
fn app_loads_diff_for_selected_file() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    select_file(&mut app, FILE_RUST);
    assert!(app.diff.as_ref().is_some_and(|d| d.rows().len() > 0));
    assert!(app.old_buffer.is_some());
    assert!(app.new_buffer.is_some());
}

#[test]
fn toggle_viewed_advances_to_next_file() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    select_file(&mut app, FILE_ALPHA);
    let initial_idx = app.sidebar.selected_idx;
    let initial_viewed = app.viewed_in_changeset;
    app.toggle_viewed();
    wait_for_diff(&mut app);
    assert_eq!(app.viewed_in_changeset, initial_viewed + 1);
    if app.files.len() > 1 {
        assert_ne!(app.sidebar.selected_idx, initial_idx);
    }
}

#[test]
fn sidebar_filter_limits_visible_files() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    app.start_filter();
    app.sidebar.filter = "notes".to_string();
    app.apply_filter();
    let visible = app.visible_files();
    assert_eq!(visible.len(), 1);
    assert!(visible[0].1.path.as_str().contains("notes"));
    app.clear_filter();
    assert!(app.sidebar.filtered_indices.is_empty());
}

#[test]
fn comment_flow_updates_counts() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    select_file(&mut app, FILE_RUST);
    app.start_add_comment();
    app.comments.draft = "nit".into();
    app.save_comment();
    let path = app.selected_file().unwrap().path.clone();
    assert_eq!(app.open_comment_counts.get(&path).copied(), Some(1));
    app.show_comments();
    assert_eq!(app.ui.mode, Mode::ViewComments);
    assert_eq!(app.comments.viewing.len(), 1);
    app.comments_resolve_selected();
    assert!(app.open_comment_counts.get(&path).is_none());
}

#[test]
fn theme_selector_preview_and_revert() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    let original_color = app.theme.bg_dark;
    app.open_theme_selector();
    let initial_idx = app.theme_selector_idx;
    if app.theme_list.len() > 1 {
        app.theme_select_next();
        assert_ne!(app.theme_selector_idx, initial_idx);
    }
    app.close_theme_selector();
    assert_eq!(app.ui.mode, Mode::Normal);
    assert_eq!(app.theme.bg_dark, original_color);
}

#[test]
fn diff_view_toggle_preserves_position() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    select_file(&mut app, FILE_ALPHA);
    let initial_row = app.viewer.scroll_y;
    app.toggle_diff_view_mode();
    let after_toggle = app.viewer.scroll_y;
    app.toggle_diff_view_mode();
    assert_eq!(app.viewer.scroll_y, initial_row);
    assert!(after_toggle <= initial_row || app.diff.is_some());
}
