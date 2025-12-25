//! Git repository discovery and file operations.

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

/// Errors from repository operations.
#[derive(Debug, Error)]
pub enum RepoError {
    #[error("not inside a git repository")]
    NotARepo,
    #[error("git command failed: {0}")]
    GitError(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid utf-8 in git output")]
    InvalidUtf8,
    #[error("invalid revision: {0}")]
    InvalidRevision(String),
}

/// Source specification for diff comparison.
#[derive(Debug, Clone)]
pub enum DiffSource {
    /// Working tree changes vs HEAD (default behavior).
    WorkingTree,
    /// Single commit (show changes introduced by that commit).
    Commit(String),
    /// Range of commits (from..to).
    Range { from: String, to: String },
    /// Compare against a base ref (e.g., origin/main).
    Base(String),
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
    pub fn discover(path: &Path) -> Result<Self, RepoError> {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--show-toplevel")
            .current_dir(path)
            .output()?;

        if !output.status.success() {
            return Err(RepoError::NotARepo);
        }

        let root = std::str::from_utf8(&output.stdout)
            .map_err(|_| RepoError::InvalidUtf8)?
            .trim();

        let canonical = PathBuf::from(root)
            .canonicalize()
            .map_err(|_| RepoError::NotARepo)?;

        Ok(Self(canonical))
    }

    /// Get the repository root path.
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Get the repository root as a string (for persistence keys).
    pub fn as_str(&self) -> &str {
        self.0.to_str().unwrap_or("")
    }
}

/// A repository-relative path. Never absolute.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct RelPath(String);

impl RelPath {
    /// Create a new RelPath from a string.
    /// Panics if the path is absolute.
    pub fn new(path: impl Into<String>) -> Self {
        let path = path.into();
        debug_assert!(
            !path.starts_with('/'),
            "RelPath must not be absolute: {}",
            path
        );
        Self(path)
    }

    /// Get the path as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to an absolute path given a repo root.
    pub fn to_absolute(&self, root: &RepoRoot) -> PathBuf {
        root.path().join(&self.0)
    }

    /// Get the file extension, if any.
    pub fn extension(&self) -> Option<&str> {
        Path::new(&self.0).extension().and_then(|s| s.to_str())
    }

    /// Get the file name.
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
    Added,
    Modified,
    Deleted,
    Untracked,
    /// Renamed is best-effort; may show as delete+add.
    Renamed,
}

/// A changed file in the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: RelPath,
    pub kind: FileChangeKind,
    /// For renames, the original path.
    pub old_path: Option<RelPath>,
}

impl ChangedFile {
    pub fn new(path: RelPath, kind: FileChangeKind) -> Self {
        Self {
            path,
            kind,
            old_path: None,
        }
    }

    pub fn renamed(old_path: RelPath, new_path: RelPath) -> Self {
        Self {
            path: new_path,
            kind: FileChangeKind::Renamed,
            old_path: Some(old_path),
        }
    }
}

/// List changed files in the working tree vs HEAD.
pub fn list_changed_files(root: &RepoRoot) -> Result<Vec<ChangedFile>, RepoError> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "-z"])
        .current_dir(root.path())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RepoError::GitError(stderr.to_string()));
    }

    parse_porcelain_status(&output.stdout)
}

/// Parse `git status --porcelain=v1 -z` output.
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
                    files.push(ChangedFile::renamed(RelPath::new(old), RelPath::new(path)));
                    continue;
                }
                FileChangeKind::Modified
            }
            _ => FileChangeKind::Modified, // fallback
        };

        files.push(ChangedFile::new(RelPath::new(path), kind));
    }

    // Sort by path for consistent ordering
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Load content from HEAD for a given path.
pub fn load_head_content(root: &RepoRoot, path: &RelPath) -> Result<Vec<u8>, RepoError> {
    let output = Command::new("git")
        .args(["show", &format!("HEAD:{}", path.as_str())])
        .current_dir(root.path())
        .output()?;

    if !output.status.success() {
        // File might not exist in HEAD (new file)
        return Ok(Vec::new());
    }

    Ok(output.stdout)
}

/// Load content from the working tree.
/// Returns empty content for directories, symlinks, or missing files.
pub fn load_working_content(root: &RepoRoot, path: &RelPath) -> Result<Vec<u8>, RepoError> {
    let full_path = path.to_absolute(root);

    // Check if it's a regular file first
    match std::fs::metadata(&full_path) {
        Ok(meta) if meta.is_file() => {}
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
pub fn load_revision_content(
    root: &RepoRoot,
    revision: &str,
    path: &RelPath,
) -> Result<Vec<u8>, RepoError> {
    let output = Command::new("git")
        .args(["show", &format!("{}:{}", revision, path.as_str())])
        .current_dir(root.path())
        .output()?;

    if !output.status.success() {
        // File might not exist in this revision
        return Ok(Vec::new());
    }

    Ok(output.stdout)
}

/// Resolve a revision to its full SHA.
pub fn resolve_revision(root: &RepoRoot, revision: &str) -> Result<String, RepoError> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", revision])
        .current_dir(root.path())
        .output()?;

    if !output.status.success() {
        return Err(RepoError::InvalidRevision(revision.to_string()));
    }

    let sha = std::str::from_utf8(&output.stdout)
        .map_err(|_| RepoError::InvalidUtf8)?
        .trim()
        .to_string();

    Ok(sha)
}

/// Get the parent commit of a revision.
pub fn get_parent_revision(root: &RepoRoot, revision: &str) -> Result<String, RepoError> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", &format!("{}^", revision)])
        .current_dir(root.path())
        .output()?;

    if !output.status.success() {
        // No parent (initial commit) - return empty tree
        return Ok("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string()); // git empty tree
    }

    let sha = std::str::from_utf8(&output.stdout)
        .map_err(|_| RepoError::InvalidUtf8)?
        .trim()
        .to_string();

    Ok(sha)
}

/// List changed files between two revisions.
pub fn list_changed_files_between(
    root: &RepoRoot,
    from: &str,
    to: &str,
) -> Result<Vec<ChangedFile>, RepoError> {
    let output = Command::new("git")
        .args([
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            "--find-copies",
            from,
            to,
        ])
        .current_dir(root.path())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RepoError::GitError(stderr.to_string()));
    }

    parse_diff_name_status(&output.stdout)
}

/// List changed files in a single commit.
pub fn list_commit_files(root: &RepoRoot, commit: &str) -> Result<Vec<ChangedFile>, RepoError> {
    let parent = get_parent_revision(root, commit)?;
    list_changed_files_between(root, &parent, commit)
}

/// Result of a base comparison.
#[derive(Debug, Clone)]
pub struct BaseComparison {
    pub merge_base: String,
    pub files: Vec<ChangedFile>,
}

/// List changed files between a base ref and HEAD (including working tree), returning the merge-base SHA.
pub fn list_changed_files_from_base_with_merge_base(
    root: &RepoRoot,
    base: &str,
) -> Result<BaseComparison, RepoError> {
    // Get merge-base to find common ancestor
    let output = Command::new("git")
        .args(["merge-base", base, "HEAD"])
        .current_dir(root.path())
        .output()?;

    let merge_base = if output.status.success() {
        std::str::from_utf8(&output.stdout)
            .map_err(|_| RepoError::InvalidUtf8)?
            .trim()
            .to_string()
    } else {
        // Fall back to using the base ref directly
        resolve_revision(root, base)?
    };

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
pub fn list_changed_files_from_base(
    root: &RepoRoot,
    base: &str,
) -> Result<Vec<ChangedFile>, RepoError> {
    Ok(list_changed_files_from_base_with_merge_base(root, base)?.files)
}

/// Parse `git diff --name-status -z` output.
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

/// Get a display name for a DiffSource.
pub fn diff_source_display(source: &DiffSource, root: &RepoRoot) -> String {
    match source {
        DiffSource::WorkingTree => "Working Tree".to_string(),
        DiffSource::Commit(c) => {
            // Try to get short commit message
            let output = Command::new("git")
                .args(["log", "-1", "--format=%h %s", c])
                .current_dir(root.path())
                .output();

            if let Ok(out) = output {
                if out.status.success() {
                    if let Ok(msg) = std::str::from_utf8(&out.stdout) {
                        let msg = msg.trim();
                        if msg.len() > 50 {
                            return format!("{}...", &msg[..47]);
                        }
                        return msg.to_string();
                    }
                }
            }
            format!("Commit {}", &c[..7.min(c.len())])
        }
        DiffSource::Range { from, to } => {
            format!("{}..{}", &from[..7.min(from.len())], &to[..7.min(to.len())])
        }
        DiffSource::Base(base) => format!("vs {}", base),
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
}
