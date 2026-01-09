//! Integration tests with real git repositories.

use std::path::Path;
use git2::{Repository, Signature};
use tempfile::TempDir;

/// Create a temporary git repo with some commits using git2.
fn create_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path();

    // Initialize repo with git2
    let repo = Repository::init(path).expect("failed to init repo");

    // Configure git for commits
    let mut config = repo.config().expect("failed to get config");
    config
        .set_str("user.email", "test@test.com")
        .expect("failed to set email");
    config
        .set_str("user.name", "Test")
        .expect("failed to set name");

    // Create initial file
    std::fs::write(path.join("file.txt"), "initial content\n").expect("failed to write file");

    // Stage file
    let mut index = repo.index().expect("failed to get index");
    index
        .add_path(Path::new("file.txt"))
        .expect("failed to add file");
    index.write().expect("failed to write index");

    // Create tree from index
    let tree_oid = index.write_tree().expect("failed to write tree");
    let tree = repo.find_tree(tree_oid).expect("failed to find tree");

    // Create signature and commit
    let sig = Signature::now("Test", "test@test.com").expect("failed to create signature");
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "initial",
        &tree,
        &[], // No parents for initial commit
    )
    .expect("failed to create commit");

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
