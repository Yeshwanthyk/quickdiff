//! Integration tests with real git repositories.

use std::process::Command;
use tempfile::TempDir;

/// Create a temporary git repo with some commits.
fn create_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .unwrap();

    // Configure git for commits
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .unwrap();

    // Create initial commit
    std::fs::write(path.join("file.txt"), "initial content\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(path)
        .output()
        .unwrap();

    dir
}

#[test]
fn test_repo_discovery() {
    let dir = create_test_repo();
    let repo = quickdiff::core::RepoRoot::discover(dir.path()).unwrap();
    assert!(repo.path().exists());
}

#[test]
fn test_list_changed_files_empty() {
    let dir = create_test_repo();
    let repo = quickdiff::core::RepoRoot::discover(dir.path()).unwrap();
    let files = quickdiff::core::list_changed_files(&repo).unwrap();
    assert!(files.is_empty(), "Clean repo should have no changes");
}

#[test]
fn test_list_changed_files_modified() {
    let dir = create_test_repo();
    let path = dir.path();

    // Modify file
    std::fs::write(path.join("file.txt"), "modified content\n").unwrap();

    let repo = quickdiff::core::RepoRoot::discover(path).unwrap();
    let files = quickdiff::core::list_changed_files(&repo).unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path.as_str(), "file.txt");
    assert_eq!(files[0].kind, quickdiff::core::FileChangeKind::Modified);
}

#[test]
fn test_list_changed_files_untracked() {
    let dir = create_test_repo();
    let path = dir.path();

    // Add new untracked file
    std::fs::write(path.join("new.txt"), "new file\n").unwrap();

    let repo = quickdiff::core::RepoRoot::discover(path).unwrap();
    let files = quickdiff::core::list_changed_files(&repo).unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path.as_str(), "new.txt");
    assert_eq!(files[0].kind, quickdiff::core::FileChangeKind::Untracked);
}
