//! Git repository discovery and file operations.

use std::path::{Path, PathBuf};

use thiserror::Error;

use git2::{DiffFindOptions, DiffOptions, Repository, Status, StatusOptions};

/// Maximum file size to load (50 MiB). Prevents OOM on huge files.
pub const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Errors from repository operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RepoError {
    /// Path is not inside a git repository.
    #[error("not inside a git repository")]
    NotARepo,
    /// Git command failed with an error message.
    #[error("git command failed: {0}")]
    GitError(String),
    /// I/O error during git operation.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Git output contained invalid UTF-8.
    #[error("invalid utf-8 in git output")]
    InvalidUtf8,
    /// Invalid revision specified.
    #[error("invalid revision: {0}")]
    InvalidRevision(String),
    /// File exceeds maximum allowed size.
    #[error("file too large: {size} bytes (max {max} bytes)")]
    FileTooLarge {
        /// Actual file size.
        size: u64,
        /// Maximum allowed size.
        max: u64,
    },
    /// Operation not supported for PR diff sources.
    #[error("operation not supported for PR diff sources; use patch extraction instead")]
    UnsupportedForPR,
}

/// Error when constructing a RelPath with an absolute path.
#[derive(Debug, Clone, thiserror::Error)]
#[error("path must be relative, got: {0}")]
pub struct InvalidRelPath(pub String);

/// Source specification for diff comparison.
#[derive(Debug, Clone)]
pub enum DiffSource {
    /// Working tree changes vs HEAD (default behavior).
    WorkingTree,
    /// Single commit (show changes introduced by that commit).
    Commit(String),
    /// Range of commits (from..to).
    Range {
        /// Starting commit.
        from: String,
        /// Ending commit.
        to: String,
    },
    /// Compare against a base ref (e.g., origin/main).
    Base(String),
    /// GitHub Pull Request.
    PullRequest {
        /// PR number.
        number: u32,
        /// Head branch name.
        head: String,
        /// Base branch name.
        base: String,
    },
}

impl Default for DiffSource {
    fn default() -> Self {
        Self::WorkingTree
    }
}

/// Canonicalized path to a git repository root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoRoot(PathBuf);

impl RepoRoot {
    /// Discover the git repository containing the given path.
    ///
    /// Walks up the directory tree to find a `.git` directory.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use quickdiff::core::RepoRoot;
    /// use std::path::Path;
    ///
    /// let repo = RepoRoot::discover(Path::new(".")).expect("not in a git repo");
    /// println!("Repo at: {}", repo.path().display());
    /// ```
    #[must_use = "this returns a Result that should be checked"]
    pub fn discover(path: &Path) -> Result<Self, RepoError> {
        let repo = Repository::discover(path).map_err(|_| RepoError::NotARepo)?;
        let root = repo
            .workdir()
            .ok_or(RepoError::NotARepo)?
            .canonicalize()
            .map_err(|_| RepoError::NotARepo)?;
        Ok(Self(root))
    }

    /// Get the repository root path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Get the repository root as a string (for persistence keys).
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.to_str().unwrap_or("")
    }
}

/// A git repository handle using libgit2.
/// Provides native git operations without subprocess overhead.
pub struct GitRepo {
    inner: Repository,
    root: PathBuf,
}

impl std::fmt::Debug for GitRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitRepo")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl GitRepo {
    /// Open a git repository at the exact path.
    pub fn open(path: &Path) -> Result<Self, RepoError> {
        let repo = Repository::open(path).map_err(|_| RepoError::NotARepo)?;
        let root = repo
            .workdir()
            .ok_or(RepoError::NotARepo)?
            .to_path_buf();
        Ok(Self { inner: repo, root })
    }

    /// Discover the git repository containing the given path.
    pub fn discover(path: &Path) -> Result<Self, RepoError> {
        let repo = Repository::discover(path).map_err(|_| RepoError::NotARepo)?;
        let root = repo
            .workdir()
            .ok_or(RepoError::NotARepo)?
            .canonicalize()
            .map_err(|_| RepoError::NotARepo)?;
        Ok(Self { inner: repo, root })
    }

    /// Get the repository root path.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get a reference to the underlying git2 Repository.
    #[must_use]
    pub fn repo(&self) -> &Repository {
        &self.inner
    }
}

/// Validate that a reference doesn't look like a flag (defense in depth).
fn validate_ref_format(reference: &str) -> Result<(), RepoError> {
    let reference = reference.trim();
    if reference.starts_with('-') {
        return Err(RepoError::InvalidRevision(format!(
            "references cannot start with '-': {}",
            reference
        )));
    }
    Ok(())
}

/// Look up a blob at a given revision and path.
/// Returns None if the file doesn't exist at that revision.
fn lookup_blob<'a>(
    repo: &'a Repository,
    revision: &str,
    path: &RelPath,
) -> Result<Option<git2::Blob<'a>>, RepoError> {
    let obj = match repo.revparse_single(revision) {
        Ok(obj) => obj,
        Err(_) => return Err(RepoError::InvalidRevision(revision.to_string())),
    };

    let commit = match obj.peel_to_commit() {
        Ok(c) => c,
        Err(_) => return Err(RepoError::InvalidRevision(revision.to_string())),
    };

    let tree = commit
        .tree()
        .map_err(|e| RepoError::GitError(format!("failed to get tree: {}", e)))?;

    let entry = match tree.get_path(Path::new(path.as_str())) {
        Ok(entry) => entry,
        Err(_) => return Ok(None), // File doesn't exist at this revision
    };

    let blob = repo
        .find_blob(entry.id())
        .map_err(|e| RepoError::GitError(format!("failed to find blob: {}", e)))?;

    Ok(Some(blob))
}

/// A repository-relative path. Never absolute.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct RelPath(String);

impl RelPath {
    /// Create a new RelPath from a string.
    ///
    /// Returns an error if the path is absolute (starts with `/`).
    ///
    /// # Examples
    ///
    /// ```
    /// use quickdiff::core::RelPath;
    ///
    /// let path = RelPath::try_new("src/main.rs").unwrap();
    /// assert_eq!(path.as_str(), "src/main.rs");
    ///
    /// // Absolute paths are rejected
    /// assert!(RelPath::try_new("/absolute/path").is_err());
    /// ```
    #[must_use = "this returns a Result that should be checked"]
    pub fn try_new(path: impl Into<String>) -> Result<Self, InvalidRelPath> {
        let path = path.into();
        if path.starts_with('/') {
            return Err(InvalidRelPath(path));
        }
        Ok(Self(path))
    }

    /// Create a new RelPath without validation.
    ///
    /// # Safety (logical)
    /// Caller must ensure `path` is relative (does not start with `/`).
    /// Used for trusted input from git commands.
    pub fn new_unchecked(path: impl Into<String>) -> Self {
        let path = path.into();
        debug_assert!(
            !path.starts_with('/'),
            "RelPath must not be absolute: {}",
            path
        );
        Self(path)
    }

    /// Convenience alias for `new_unchecked` — use when path is from git output.
    #[inline]
    pub fn new(path: impl Into<String>) -> Self {
        Self::new_unchecked(path)
    }

    /// Get the path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to an absolute path given a repo root.
    #[must_use]
    pub fn to_absolute(&self, root: &RepoRoot) -> PathBuf {
        root.path().join(&self.0)
    }

    /// Get the file extension, if any.
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        Path::new(&self.0).extension().and_then(|s| s.to_str())
    }

    /// Get the file name.
    #[must_use]
    pub fn file_name(&self) -> &str {
        Path::new(&self.0)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&self.0)
    }
}

impl std::fmt::Display for RelPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Kind of file change detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    /// File was newly added.
    Added,
    /// File was modified.
    Modified,
    /// File was deleted.
    Deleted,
    /// File is untracked.
    Untracked,
    /// File was renamed (best-effort; may show as delete+add).
    Renamed,
}

/// A changed file in the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// Path to the file.
    pub path: RelPath,
    /// Type of change.
    pub kind: FileChangeKind,
    /// For renames, the original path.
    pub old_path: Option<RelPath>,
}

impl ChangedFile {
    /// Create a new changed file entry.
    pub fn new(path: RelPath, kind: FileChangeKind) -> Self {
        Self {
            path,
            kind,
            old_path: None,
        }
    }

    /// Create a renamed file entry.
    pub fn renamed(old_path: RelPath, new_path: RelPath) -> Self {
        Self {
            path: new_path,
            kind: FileChangeKind::Renamed,
            old_path: Some(old_path),
        }
    }
}

/// List changed files in the working tree vs HEAD.
#[must_use = "this returns a Result that should be checked"]
pub fn list_changed_files(root: &RepoRoot) -> Result<Vec<ChangedFile>, RepoError> {
    let repo = Repository::open(root.path()).map_err(|_| RepoError::NotARepo)?;

    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .include_ignored(false)
        .exclude_submodules(true);

    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| RepoError::GitError(format!("failed to get status: {}", e)))?;

    let mut files = Vec::new();

    for entry in statuses.iter() {
        let path = match entry.path() {
            Some(p) => p,
            None => continue,
        };

        // Skip .quickdiff/ internal files
        if path.starts_with(".quickdiff/") {
            continue;
        }

        let status = entry.status();
        let kind = status_to_change_kind(status);

        // Handle renames (check for both old and new paths)
        if status.contains(Status::INDEX_RENAMED) || status.contains(Status::WT_RENAMED) {
            if let Some(diff_delta) = entry.head_to_index() {
                if let (Some(old), Some(new)) = (
                    diff_delta.old_file().path().and_then(|p| p.to_str()),
                    diff_delta.new_file().path().and_then(|p| p.to_str()),
                ) {
                    if old != new {
                        files.push(ChangedFile::renamed(
                            RelPath::new(old),
                            RelPath::new(new),
                        ));
                        continue;
                    }
                }
            }
        }

        files.push(ChangedFile::new(RelPath::new(path), kind));
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Convert git2 Status flags to FileChangeKind.
fn status_to_change_kind(status: Status) -> FileChangeKind {
    if status.contains(Status::WT_NEW) || status.contains(Status::INDEX_NEW) {
        if status.contains(Status::INDEX_NEW) {
            FileChangeKind::Added
        } else {
            FileChangeKind::Untracked
        }
    } else if status.contains(Status::WT_DELETED) || status.contains(Status::INDEX_DELETED) {
        FileChangeKind::Deleted
    } else if status.contains(Status::WT_RENAMED) || status.contains(Status::INDEX_RENAMED) {
        FileChangeKind::Renamed
    } else {
        // Modified, typechange, or any other status defaults to Modified
        FileChangeKind::Modified
    }
}

/// Parse `git status --porcelain=v1 -z` output.
/// Retained for unit test coverage of parsing logic.
#[cfg(test)]
fn parse_porcelain_status(output: &[u8]) -> Result<Vec<ChangedFile>, RepoError> {
    let text = std::str::from_utf8(output).map_err(|_| RepoError::InvalidUtf8)?;
    let mut files = Vec::new();
    let mut parts = text.split('\0').peekable();

    while let Some(entry) = parts.next() {
        if entry.is_empty() {
            continue;
        }
        if entry.len() < 4 {
            continue;
        }

        let status = &entry[0..2];
        let path = &entry[3..];

        // Parse the two-character status code
        // First char = index status, second char = worktree status
        let kind = match status {
            "??" => FileChangeKind::Untracked,
            " M" | "MM" | "AM" => FileChangeKind::Modified,
            " D" | "MD" | "AD" => FileChangeKind::Deleted,
            "A " | " A" => FileChangeKind::Added,
            "M " => FileChangeKind::Modified,
            "D " => FileChangeKind::Deleted,
            s if s.starts_with('R') || s.starts_with('C') => {
                // Rename/Copy: next entry is the old/original path
                if let Some(old) = parts.next() {
                    if path.starts_with(".quickdiff/") || old.starts_with(".quickdiff/") {
                        continue;
                    }
                    files.push(ChangedFile::renamed(RelPath::new(old), RelPath::new(path)));
                    continue;
                }
                FileChangeKind::Modified
            }
            _ => FileChangeKind::Modified, // fallback
        };

        // Skip .quickdiff/ internal files
        if path.starts_with(".quickdiff/") {
            continue;
        }

        files.push(ChangedFile::new(RelPath::new(path), kind));
    }

    // Sort by path for consistent ordering
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Get the size of a blob at a given revision without reading it.
#[allow(dead_code)]
fn get_blob_size(
    root: &RepoRoot,
    revision: &str,
    path: &RelPath,
) -> Result<Option<u64>, RepoError> {
    let repo = Repository::open(root.path()).map_err(|_| RepoError::NotARepo)?;

    let result = lookup_blob(&repo, revision, path)?;
    Ok(result.map(|blob| blob.size() as u64))
}

/// Load content from HEAD for a given path.
/// Returns error if file exceeds `MAX_FILE_SIZE`.
#[must_use = "this returns a Result that should be checked"]
pub fn load_head_content(root: &RepoRoot, path: &RelPath) -> Result<Vec<u8>, RepoError> {
    let repo = Repository::open(root.path()).map_err(|_| RepoError::NotARepo)?;

    let blob = match lookup_blob(&repo, "HEAD", path)? {
        Some(b) => b,
        None => return Ok(Vec::new()), // File doesn't exist in HEAD (new file)
    };

    // Preflight size check to avoid OOM on huge blobs
    let size = blob.size() as u64;
    if size > MAX_FILE_SIZE {
        return Err(RepoError::FileTooLarge {
            size,
            max: MAX_FILE_SIZE,
        });
    }

    Ok(blob.content().to_vec())
}

/// Load content from the working tree.
/// Returns empty content for directories, symlinks, or missing files.
/// Returns error if file exceeds `MAX_FILE_SIZE`.
#[must_use = "this returns a Result that should be checked"]
pub fn load_working_content(root: &RepoRoot, path: &RelPath) -> Result<Vec<u8>, RepoError> {
    let full_path = path.to_absolute(root);

    // Use symlink_metadata to avoid following symlinks.
    // Symlinks are treated as empty to avoid escaping the repo.
    match std::fs::symlink_metadata(&full_path) {
        Ok(meta) if meta.is_file() => {
            // Check file size limit
            if meta.len() > MAX_FILE_SIZE {
                return Err(RepoError::FileTooLarge {
                    size: meta.len(),
                    max: MAX_FILE_SIZE,
                });
            }
        }
        Ok(_) => return Ok(Vec::new()), // Directory, symlink, etc.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    }

    match std::fs::read(&full_path) {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

/// Load content from a specific git revision.
/// Returns error if file exceeds `MAX_FILE_SIZE`.
#[must_use = "this returns a Result that should be checked"]
pub fn load_revision_content(
    root: &RepoRoot,
    revision: &str,
    path: &RelPath,
) -> Result<Vec<u8>, RepoError> {
    validate_ref_format(revision)?;

    let repo = Repository::open(root.path()).map_err(|_| RepoError::NotARepo)?;

    let blob = match lookup_blob(&repo, revision, path)? {
        Some(b) => b,
        None => return Ok(Vec::new()), // File doesn't exist in this revision
    };

    // Preflight size check to avoid OOM on huge blobs
    let size = blob.size() as u64;
    if size > MAX_FILE_SIZE {
        return Err(RepoError::FileTooLarge {
            size,
            max: MAX_FILE_SIZE,
        });
    }

    Ok(blob.content().to_vec())
}

/// Resolve the merge-base between a base ref and HEAD.
#[must_use = "this returns a Result that should be checked"]
pub fn resolve_merge_base(root: &RepoRoot, base: &str) -> Result<String, RepoError> {
    let base = base.trim();
    validate_ref_format(base)?;

    let repo = Repository::open(root.path()).map_err(|_| RepoError::NotARepo)?;

    // Resolve base ref
    let base_obj = repo
        .revparse_single(base)
        .map_err(|_| RepoError::InvalidRevision(base.to_string()))?;
    let base_commit = base_obj
        .peel_to_commit()
        .map_err(|_| RepoError::InvalidRevision(base.to_string()))?;

    // Resolve HEAD
    let head = repo
        .head()
        .map_err(|e| RepoError::GitError(format!("failed to get HEAD: {}", e)))?;
    let head_commit = head
        .peel_to_commit()
        .map_err(|e| RepoError::GitError(format!("failed to peel HEAD: {}", e)))?;

    // Find merge base
    match repo.merge_base(base_commit.id(), head_commit.id()) {
        Ok(oid) => Ok(oid.to_string()),
        Err(_) => {
            // Fall back to using the base ref directly (same as before)
            Ok(base_commit.id().to_string())
        }
    }
}

/// Load the old/new content for a specific file given a diff source.
///
/// For `DiffSource::Base`, provide `merge_base` to avoid recomputing it per file.
#[must_use = "this returns a Result that should be checked"]
pub fn load_diff_contents(
    root: &RepoRoot,
    source: &DiffSource,
    file: &ChangedFile,
    merge_base: Option<&str>,
) -> Result<(Vec<u8>, Vec<u8>), RepoError> {
    let path = &file.path;
    let kind = file.kind;
    let old_path = file.old_path.as_ref();

    match source {
        DiffSource::WorkingTree => match kind {
            FileChangeKind::Added | FileChangeKind::Untracked => {
                Ok((Vec::new(), load_working_content(root, path)?))
            }
            FileChangeKind::Deleted => Ok((load_head_content(root, path)?, Vec::new())),
            FileChangeKind::Modified | FileChangeKind::Renamed => {
                let old_p = old_path.unwrap_or(path);
                Ok((
                    load_head_content(root, old_p)?,
                    load_working_content(root, path)?,
                ))
            }
        },
        DiffSource::Commit(commit) => {
            let parent = get_parent_revision(root, commit)?;
            match kind {
                FileChangeKind::Added => {
                    Ok((Vec::new(), load_revision_content(root, commit, path)?))
                }
                FileChangeKind::Deleted => {
                    Ok((load_revision_content(root, &parent, path)?, Vec::new()))
                }
                FileChangeKind::Modified | FileChangeKind::Renamed | FileChangeKind::Untracked => {
                    let old_p = old_path.unwrap_or(path);
                    Ok((
                        load_revision_content(root, &parent, old_p)?,
                        load_revision_content(root, commit, path)?,
                    ))
                }
            }
        }
        DiffSource::Range { from, to } => match kind {
            FileChangeKind::Added => Ok((Vec::new(), load_revision_content(root, to, path)?)),
            FileChangeKind::Deleted => Ok((load_revision_content(root, from, path)?, Vec::new())),
            FileChangeKind::Modified | FileChangeKind::Renamed | FileChangeKind::Untracked => {
                let old_p = old_path.unwrap_or(path);
                Ok((
                    load_revision_content(root, from, old_p)?,
                    load_revision_content(root, to, path)?,
                ))
            }
        },
        DiffSource::Base(base) => {
            let merge_base = match merge_base {
                Some(mb) => mb.to_string(),
                None => resolve_merge_base(root, base)?,
            };

            match kind {
                FileChangeKind::Added | FileChangeKind::Untracked => {
                    Ok((Vec::new(), load_working_content(root, path)?))
                }
                FileChangeKind::Deleted => {
                    Ok((load_revision_content(root, &merge_base, path)?, Vec::new()))
                }
                FileChangeKind::Modified | FileChangeKind::Renamed => {
                    let old_p = old_path.unwrap_or(path);
                    Ok((
                        load_revision_content(root, &merge_base, old_p)?,
                        load_working_content(root, path)?,
                    ))
                }
            }
        }
        DiffSource::PullRequest { .. } => {
            // PR mode uses patch extraction, not git show.
            // This should not be called for PR sources.
            Err(RepoError::UnsupportedForPR)
        }
    }
}

/// Resolve a revision to its full SHA.
#[must_use = "this returns a Result that should be checked"]
pub fn resolve_revision(root: &RepoRoot, revision: &str) -> Result<String, RepoError> {
    let revision = revision.trim();
    validate_ref_format(revision)?;

    let repo = Repository::open(root.path()).map_err(|_| RepoError::NotARepo)?;
    let obj = repo
        .revparse_single(revision)
        .map_err(|_| RepoError::InvalidRevision(revision.to_string()))?;

    // Peel to commit to ensure we have a valid commit-ish
    let commit = obj
        .peel_to_commit()
        .map_err(|_| RepoError::InvalidRevision(revision.to_string()))?;

    Ok(commit.id().to_string())
}

/// Git's well-known empty tree SHA.
const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Get the parent commit of a revision.
#[must_use = "this returns a Result that should be checked"]
pub fn get_parent_revision(root: &RepoRoot, revision: &str) -> Result<String, RepoError> {
    let revision = revision.trim();
    validate_ref_format(revision)?;

    let repo = Repository::open(root.path()).map_err(|_| RepoError::NotARepo)?;
    let obj = repo
        .revparse_single(revision)
        .map_err(|_| RepoError::InvalidRevision(revision.to_string()))?;
    let commit = obj
        .peel_to_commit()
        .map_err(|_| RepoError::InvalidRevision(revision.to_string()))?;

    if commit.parent_count() > 0 {
        Ok(commit.parent_id(0)
            .map_err(|e| RepoError::GitError(format!("failed to get parent: {}", e)))?
            .to_string())
    } else {
        // No parent (initial commit) - return empty tree
        Ok(EMPTY_TREE_SHA.to_string())
    }
}

/// List changed files between two revisions.
#[must_use = "this returns a Result that should be checked"]
pub fn list_changed_files_between(
    root: &RepoRoot,
    from: &str,
    to: &str,
) -> Result<Vec<ChangedFile>, RepoError> {
    validate_ref_format(from)?;
    validate_ref_format(to)?;

    let repo = Repository::open(root.path()).map_err(|_| RepoError::NotARepo)?;

    // Resolve commits
    let from_obj = repo
        .revparse_single(from)
        .map_err(|_| RepoError::InvalidRevision(from.to_string()))?;
    let from_tree = from_obj
        .peel_to_tree()
        .map_err(|_| RepoError::InvalidRevision(from.to_string()))?;

    let to_obj = repo
        .revparse_single(to)
        .map_err(|_| RepoError::InvalidRevision(to.to_string()))?;
    let to_tree = to_obj
        .peel_to_tree()
        .map_err(|_| RepoError::InvalidRevision(to.to_string()))?;

    // Create diff with rename/copy detection
    let mut opts = DiffOptions::new();
    let mut diff = repo
        .diff_tree_to_tree(Some(&from_tree), Some(&to_tree), Some(&mut opts))
        .map_err(|e| RepoError::GitError(format!("failed to create diff: {}", e)))?;

    // Enable rename detection
    let mut find_opts = DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| RepoError::GitError(format!("failed to find renames: {}", e)))?;

    let mut files = Vec::new();

    for delta in diff.deltas() {
        let new_path = delta
            .new_file()
            .path()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        let old_path = delta
            .old_file()
            .path()
            .and_then(|p| p.to_str());

        if new_path.is_empty() {
            continue;
        }

        let kind = match delta.status() {
            git2::Delta::Added => FileChangeKind::Added,
            git2::Delta::Deleted => FileChangeKind::Deleted,
            git2::Delta::Modified => FileChangeKind::Modified,
            git2::Delta::Renamed | git2::Delta::Copied => {
                if let Some(old) = old_path {
                    if old != new_path {
                        files.push(ChangedFile::renamed(
                            RelPath::new(old),
                            RelPath::new(new_path),
                        ));
                        continue;
                    }
                }
                FileChangeKind::Modified
            }
            _ => FileChangeKind::Modified,
        };

        files.push(ChangedFile::new(RelPath::new(new_path), kind));
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// List changed files in a single commit.
#[must_use = "this returns a Result that should be checked"]
pub fn list_commit_files(root: &RepoRoot, commit: &str) -> Result<Vec<ChangedFile>, RepoError> {
    let parent = get_parent_revision(root, commit)?;
    list_changed_files_between(root, &parent, commit)
}

/// Result of a base comparison.
#[derive(Debug, Clone)]
pub struct BaseComparison {
    /// The computed merge-base commit SHA.
    pub merge_base: String,
    /// Files changed between merge-base and HEAD.
    pub files: Vec<ChangedFile>,
}

/// List changed files between a base ref and HEAD (including working tree), returning the merge-base SHA.
#[must_use = "this returns a Result that should be checked"]
pub fn list_changed_files_from_base_with_merge_base(
    root: &RepoRoot,
    base: &str,
) -> Result<BaseComparison, RepoError> {
    let merge_base = resolve_merge_base(root, base)?;

    // Get files changed between merge-base and HEAD
    let committed = list_changed_files_between(root, &merge_base, "HEAD")?;

    // Also get working tree changes
    let working = list_changed_files(root)?;

    // Merge: working tree changes take precedence
    let mut files: std::collections::HashMap<String, ChangedFile> = committed
        .into_iter()
        .map(|f| (f.path.as_str().to_string(), f))
        .collect();

    for f in working {
        files.insert(f.path.as_str().to_string(), f);
    }

    let mut result: Vec<_> = files.into_values().collect();
    result.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(BaseComparison {
        merge_base,
        files: result,
    })
}

/// List changed files between a base ref and HEAD (including working tree).
#[must_use = "this returns a Result that should be checked"]
pub fn list_changed_files_from_base(
    root: &RepoRoot,
    base: &str,
) -> Result<Vec<ChangedFile>, RepoError> {
    Ok(list_changed_files_from_base_with_merge_base(root, base)?.files)
}

/// Parse `git diff --name-status -z` output.
/// Retained for unit test coverage of parsing logic.
#[cfg(test)]
#[allow(dead_code)]
fn parse_diff_name_status(output: &[u8]) -> Result<Vec<ChangedFile>, RepoError> {
    let text = std::str::from_utf8(output).map_err(|_| RepoError::InvalidUtf8)?;
    let mut files = Vec::new();
    let mut parts = text.split('\0').peekable();

    while let Some(status) = parts.next() {
        if status.is_empty() {
            continue;
        }

        let status_char = status.chars().next().unwrap_or('M');
        let path = parts.next().unwrap_or("");

        if path.is_empty() {
            continue;
        }

        let kind = match status_char {
            'A' => FileChangeKind::Added,
            'D' => FileChangeKind::Deleted,
            'M' => FileChangeKind::Modified,
            'R' | 'C' => {
                // Rename/Copy: next part is the new path
                if let Some(new_path) = parts.next() {
                    files.push(ChangedFile::renamed(
                        RelPath::new(path),
                        RelPath::new(new_path),
                    ));
                    continue;
                }
                FileChangeKind::Modified
            }
            _ => FileChangeKind::Modified,
        };

        files.push(ChangedFile::new(RelPath::new(path), kind));
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Truncate a string to at most `max_chars` Unicode characters.
/// If truncated, appends "..." so total display width is roughly `max_chars + 3`.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

/// Take at most `n` characters from a string (no ellipsis).
fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Get a display name for a DiffSource.
pub fn diff_source_display(source: &DiffSource, root: &RepoRoot) -> String {
    match source {
        DiffSource::WorkingTree => "Working Tree".to_string(),
        DiffSource::Commit(c) => {
            // Try to get short commit message using git2
            if let Ok(repo) = Repository::open(root.path()) {
                if let Ok(obj) = repo.revparse_single(c) {
                    if let Ok(commit) = obj.peel_to_commit() {
                        let short_id = &commit.id().to_string()[..7];
                        let summary = commit.summary().unwrap_or("");
                        let msg = format!("{} {}", short_id, summary);
                        return truncate_chars(&msg, 50);
                    }
                }
            }
            format!("Commit {}", take_chars(c, 7))
        }
        DiffSource::Range { from, to } => {
            format!("{}..{}", take_chars(from, 7), take_chars(to, 7))
        }
        DiffSource::Base(base) => format!("vs {}", base),
        DiffSource::PullRequest { number, head, base } => {
            format!("PR #{} ({} → {})", number, head, base)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relpath_basics() {
        let p = RelPath::new("src/main.rs");
        assert_eq!(p.as_str(), "src/main.rs");
        assert_eq!(p.extension(), Some("rs"));
        assert_eq!(p.file_name(), "main.rs");
    }

    #[test]
    fn relpath_try_new_rejects_absolute() {
        assert!(RelPath::try_new("/absolute/path").is_err());
        assert!(RelPath::try_new("relative/path").is_ok());
    }

    #[test]
    fn parse_porcelain_modified() {
        let output = b" M src/lib.rs\0";
        let files = parse_porcelain_status(output).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "src/lib.rs");
        assert_eq!(files[0].kind, FileChangeKind::Modified);
    }

    #[test]
    fn parse_porcelain_untracked() {
        let output = b"?? newfile.txt\0";
        let files = parse_porcelain_status(output).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].kind, FileChangeKind::Untracked);
    }

    #[test]
    fn parse_porcelain_multiple() {
        let output = b" M file1.rs\0?? file2.txt\0A  file3.rs\0";
        let files = parse_porcelain_status(output).unwrap();
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn parse_porcelain_ignores_quickdiff_state() {
        let output = b"?? .quickdiff/comments.json\0 M src/lib.rs\0";
        let files = parse_porcelain_status(output).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "src/lib.rs");
    }

    #[test]
    fn parse_porcelain_empty() {
        let output = b"";
        let files = parse_porcelain_status(output).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn parse_porcelain_rename() {
        // Format: "R  new_path\0old_path\0"
        // The status line contains the NEW path, followed by NUL, then OLD path
        let output = b"R  new_name.rs\0old_name.rs\0";
        let files = parse_porcelain_status(output).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].kind, FileChangeKind::Renamed);
        assert_eq!(files[0].path.as_str(), "new_name.rs");
        assert_eq!(
            files[0].old_path.as_ref().map(|p| p.as_str()),
            Some("old_name.rs")
        );
    }

    #[test]
    fn parse_porcelain_copy() {
        // Copy uses same format as rename
        let output = b"C  copied.rs\0original.rs\0";
        let files = parse_porcelain_status(output).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].kind, FileChangeKind::Renamed); // Treated as rename
        assert_eq!(files[0].path.as_str(), "copied.rs");
        assert_eq!(
            files[0].old_path.as_ref().map(|p| p.as_str()),
            Some("original.rs")
        );
    }

    #[test]
    fn truncate_chars_ascii() {
        assert_eq!(truncate_chars("short", 10), "short");
        assert_eq!(truncate_chars("exactly ten", 11), "exactly ten");
        assert_eq!(truncate_chars("this is way too long", 10), "this is wa...");
    }

    #[test]
    fn truncate_chars_unicode() {
        // Japanese: each char is 3 bytes but 1 char
        let jp = "日本語テスト"; // 6 chars, 18 bytes
        assert_eq!(truncate_chars(jp, 10), jp); // fits
        assert_eq!(truncate_chars(jp, 4), "日本語テ..."); // truncated at char boundary

        // Emoji: multi-byte
        let emoji = "🎉🎊🎁🎂🎈"; // 5 chars
        assert_eq!(truncate_chars(emoji, 3), "🎉🎊🎁...");
    }

    #[test]
    fn take_chars_unicode() {
        assert_eq!(take_chars("abc", 2), "ab");
        assert_eq!(take_chars("日本語", 2), "日本");
        assert_eq!(take_chars("🎉🎊🎁", 2), "🎉🎊");
        assert_eq!(take_chars("short", 100), "short"); // no panic on over-take
    }
}
