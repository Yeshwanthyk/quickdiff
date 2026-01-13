use git2::{IndexAddOption, Repository, Signature};
use quickdiff::core::{DiffSource, RepoRoot};
use quickdiff::ui::{App, DiffPaneMode, Focus, Mode};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[cfg(unix)]
use serde_json::json;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

const FILE_ALPHA: &str = "alpha.txt";
const FILE_RUST: &str = "src/lib.rs";
const FILE_NOTES: &str = "docs/notes.md";

static ENV_GUARD: Mutex<()> = Mutex::new(());

struct TestEnv {
    _guard: MutexGuard<'static, ()>,
    prev_home: Option<String>,
    prev_xdg: Option<String>,
    #[allow(dead_code)]
    home_dir: TempDir,
}

impl TestEnv {
    fn new() -> Self {
        let guard = ENV_GUARD
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let prev_home = std::env::var("HOME").ok();
        let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let home_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", home_dir.path());
        std::env::set_var("XDG_CONFIG_HOME", home_dir.path());
        Self {
            _guard: guard,
            prev_home,
            prev_xdg,
            home_dir,
        }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        if let Some(prev) = &self.prev_home {
            std::env::set_var("HOME", prev);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(prev) = &self.prev_xdg {
            std::env::set_var("XDG_CONFIG_HOME", prev);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }
}

struct RepoHarness {
    _env: TestEnv,
    _dir: TempDir,
    repo: RepoRoot,
}

impl RepoHarness {
    fn new() -> Self {
        let env = TestEnv::new();
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let repo = RepoRoot::discover(dir.path()).unwrap();
        Self {
            _env: env,
            _dir: dir,
            repo,
        }
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

#[cfg(unix)]
static GH_GUARD: Mutex<()> = Mutex::new(());

#[cfg(unix)]
struct GhFixture {
    _guard: MutexGuard<'static, ()>,
    dir: TempDir,
    log_path: PathBuf,
    prev_path: Option<String>,
    prev_data_dir: Option<String>,
}

#[cfg(unix)]
impl GhFixture {
    fn new() -> Self {
        let guard = GH_GUARD.lock().unwrap_or_else(|poison| poison.into_inner());
        let dir = TempDir::new().unwrap();
        let script_path = dir.path().join("gh");
        std::fs::write(&script_path, GH_SCRIPT).unwrap();
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();

        let log_path = dir.path().join("gh.log");
        std::fs::write(&log_path, "").unwrap();

        let prev_path = std::env::var("PATH").ok();
        let new_path = match &prev_path {
            Some(path) if !path.is_empty() => format!("{}:{}", dir.path().display(), path),
            _ => dir.path().display().to_string(),
        };
        std::env::set_var("PATH", &new_path);

        let prev_data_dir = std::env::var("QUICKDIFF_TEST_GH_DIR").ok();
        std::env::set_var("QUICKDIFF_TEST_GH_DIR", dir.path());

        Self {
            _guard: guard,
            dir,
            log_path,
            prev_path,
            prev_data_dir,
        }
    }

    fn write_pr_list(&self, value: serde_json::Value) {
        let path = self.dir.path().join("pr_list.json");
        let content = serde_json::to_string(&value).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn write_diff(&self, number: u32, patch: &str) {
        let path = self.dir.path().join(format!("diff_{}.patch", number));
        std::fs::write(path, patch).unwrap();
    }

    fn log_lines(&self) -> Vec<String> {
        let content = std::fs::read_to_string(&self.log_path).unwrap_or_default();
        content
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect()
    }
}

#[cfg(unix)]
impl Drop for GhFixture {
    fn drop(&mut self) {
        if let Some(prev) = &self.prev_path {
            std::env::set_var("PATH", prev);
        } else {
            std::env::remove_var("PATH");
        }

        if let Some(prev) = &self.prev_data_dir {
            std::env::set_var("QUICKDIFF_TEST_GH_DIR", prev);
        } else {
            std::env::remove_var("QUICKDIFF_TEST_GH_DIR");
        }
    }
}

#[cfg(unix)]
const GH_SCRIPT: &str = r#"#!/bin/bash
set -euo pipefail
BASE="${QUICKDIFF_TEST_GH_DIR:?}"
LOG="$BASE/gh.log"
CMD="${1:-}"
shift || true
case "$CMD" in
  auth)
    SUB="${1:-}"
    if [[ "$SUB" == "status" ]]; then
      exit 0
    fi
    ;;
  pr)
    ACTION="${1:-}"
    shift || true
    case "$ACTION" in
      list)
        cat "$BASE/pr_list.json"
        exit 0
        ;;
      diff)
        PRNUM="${1:-}"
        cat "$BASE/diff_${PRNUM}.patch"
        exit 0
        ;;
      review)
        PRNUM="${1:-}"
        shift || true
        echo "review ${PRNUM} $*" >> "$LOG"
        exit 0
        ;;
      view)
        PRNUM="${1:-}"
        echo "view ${PRNUM}" >> "$LOG"
        exit 0
        ;;
    esac
    ;;
esac

echo "unexpected gh invocation: cmd=${CMD} args=$*" >> "$LOG"
exit 1
"#;

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

#[cfg(unix)]
fn wait_for_pr_list(app: &mut App) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        app.poll_pr_worker();
        if !app.pr.loading && !app.pr.list.is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("pr list timed out");
}

#[cfg(unix)]
fn wait_for_pr_loaded(app: &mut App) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        app.poll_pr_worker();
        if !app.pr.loading && app.pr.active && !app.files.is_empty() && app.diff.is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("pr diff timed out");
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
    assert!(app.diff.as_ref().is_some_and(|d| !d.rows().is_empty()));
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
fn apply_filter_reselects_first_match() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    select_file(&mut app, FILE_RUST);
    app.start_filter();
    app.sidebar.filter = "notes".to_string();
    app.apply_filter();
    let selected_path = app.selected_file().unwrap().path.clone();
    assert_eq!(selected_path.as_str(), FILE_NOTES);
    assert_eq!(app.sidebar.filtered_indices.len(), 1);
}

#[test]
fn cancel_filter_clears_state() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    app.start_filter();
    app.sidebar.filter = "notes".to_string();
    app.cancel_filter();
    assert_eq!(app.sidebar.filter, "");
    assert!(app.sidebar.filtered_indices.is_empty());
    assert_eq!(app.ui.mode, Mode::Normal);
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
    assert!(!app.open_comment_counts.contains_key(&path));
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

#[test]
fn focus_toggle_switches_modes() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    assert_eq!(app.focus, Focus::Sidebar);
    app.toggle_focus();
    assert_eq!(app.focus, Focus::Diff);
    app.toggle_focus();
    assert_eq!(app.focus, Focus::Sidebar);
    app.set_focus(Focus::Diff);
    assert_eq!(app.focus, Focus::Diff);
}

#[test]
fn pane_fullscreen_toggles_cycle() {
    let harness = RepoHarness::new();
    let mut app = harness.app();
    assert_eq!(app.viewer.pane_mode, DiffPaneMode::Both);
    app.toggle_old_fullscreen();
    assert_eq!(app.viewer.pane_mode, DiffPaneMode::OldOnly);
    app.toggle_old_fullscreen();
    assert_eq!(app.viewer.pane_mode, DiffPaneMode::Both);
    app.toggle_new_fullscreen();
    assert_eq!(app.viewer.pane_mode, DiffPaneMode::NewOnly);
    app.toggle_new_fullscreen();
    assert_eq!(app.viewer.pane_mode, DiffPaneMode::Both);
}

#[cfg(unix)]
#[test]
fn pr_picker_loads_pr_and_diff() {
    let harness = RepoHarness::new();
    let gh = GhFixture::new();
    gh.write_pr_list(json!([
        {
            "number": 7,
            "title": "Update alpha",
            "headRefName": "feature/alpha",
            "baseRefName": "main",
            "author": {"login": "alice"},
            "additions": 3,
            "deletions": 1,
            "changedFiles": 1,
            "isDraft": false
        }
    ]));
    gh.write_diff(
        7,
        r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
-pub fn meaning() -> i32 {
-    41
-}
+pub fn meaning() -> i32 {
+    42
+}
"#,
    );

    let mut app = harness.app();
    app.open_pr_picker();
    wait_for_pr_list(&mut app);
    assert_eq!(app.pr.list.len(), 1);
    assert_eq!(app.ui.mode, Mode::PRPicker);
    app.pr_picker_select();
    wait_for_pr_loaded(&mut app);
    assert!(app.pr.active);
    assert!(matches!(
        app.source,
        DiffSource::PullRequest { number: 7, .. }
    ));
    assert_eq!(app.files.len(), 1);
    assert_eq!(app.files[0].path.as_str(), "src/lib.rs");
    assert!(app.diff.is_some());
}

#[cfg(unix)]
#[test]
fn pr_actions_issue_commands() {
    let harness = RepoHarness::new();
    let gh = GhFixture::new();
    gh.write_pr_list(json!([
        {
            "number": 9,
            "title": "Review me",
            "headRefName": "feature/pr",
            "baseRefName": "main",
            "author": {"login": "bob"},
            "additions": 2,
            "deletions": 1,
            "changedFiles": 1,
            "isDraft": false
        }
    ]));
    gh.write_diff(
        9,
        r#"diff --git a/docs/notes.md b/docs/notes.md
--- a/docs/notes.md
+++ b/docs/notes.md
@@ -1,3 +1,3 @@
-# Notes
-
-Original
+# Notes
+
+Updated body
"#,
    );

    let mut app = harness.app();
    app.open_pr_picker();
    wait_for_pr_list(&mut app);
    app.pr_picker_select();
    wait_for_pr_loaded(&mut app);

    app.start_pr_approve();
    app.submit_pr_action();

    app.start_pr_comment();
    app.pr.action_text = "thanks".into();
    app.submit_pr_action();

    app.start_pr_request_changes();
    app.pr.action_text = "redo".into();
    app.submit_pr_action();

    assert_eq!(app.ui.mode, Mode::Normal);
    assert!(app.pr.action_type.is_none());

    let log = gh.log_lines();
    assert_eq!(
        log,
        vec![
            "review 9 --approve",
            "review 9 --comment -b thanks",
            "review 9 --request-changes -b redo"
        ]
    );
}

#[cfg(unix)]
#[test]
fn gh_fixture_handles_list_and_diff() {
    let gh = GhFixture::new();
    gh.write_pr_list(json!([
        {
            "number": 3,
            "title": "Test",
            "headRefName": "feature/test",
            "baseRefName": "main",
            "author": {"login": "ci"},
            "additions": 1,
            "deletions": 1,
            "changedFiles": 1,
            "isDraft": false
        }
    ]));
    gh.write_diff(3, "diff --git a/a b/b\n");

    let list = Command::new("gh").args(["pr", "list"]).output().unwrap();
    assert!(list.status.success());
    let stdout = String::from_utf8(list.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed[0]["number"], 3);

    let diff = Command::new("gh")
        .args(["pr", "diff", "3"])
        .output()
        .unwrap();
    assert!(diff.status.success());
    assert!(String::from_utf8(diff.stdout)
        .unwrap()
        .contains("diff --git"));
}

#[cfg(unix)]
#[test]
fn gh_fixture_logs_review_commands() {
    let gh = GhFixture::new();
    let status = Command::new("gh")
        .args(["pr", "review", "12", "--comment", "-b", "hello"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(gh.log_lines(), vec!["review 12 --comment -b hello"]);
}

#[cfg(unix)]
#[test]
fn gh_fixture_view_command_records_log() {
    let gh = GhFixture::new();
    let status = Command::new("gh")
        .args(["pr", "view", "21"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(gh.log_lines(), vec!["view 21"]);
}

#[cfg(unix)]
#[test]
fn gh_fixture_auth_status_succeeds_without_logging() {
    let gh = GhFixture::new();
    let status = Command::new("gh")
        .args(["auth", "status"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(gh.log_lines().is_empty());
}

#[cfg(unix)]
#[test]
fn gh_fixture_unknown_command_fails_and_logs() {
    let gh = GhFixture::new();
    let status = Command::new("gh").args(["issue", "list"]).output().unwrap();
    assert!(!status.status.success());
    let logs = gh.log_lines();
    assert_eq!(logs.len(), 1);
    assert!(logs[0].starts_with("unexpected gh invocation"));
}
