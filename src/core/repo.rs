//! VCS repository discovery and file operations.

use std::path::{Path, PathBuf};

#[cfg(feature = "jj")]
use std::collections::HashMap;
#[cfg(feature = "jj")]
use std::sync::Arc;

use thiserror::Error;

use git2::{DiffFindOptions, DiffOptions, Repository, Status, StatusOptions};

#[cfg(feature = "jj")]
use chrono::Local;
#[cfg(feature = "jj")]
use futures::StreamExt;
#[cfg(feature = "jj")]
use jj_lib::backend::TreeValue;
#[cfg(feature = "jj")]
use jj_lib::commit::Commit as JjCommit;
#[cfg(feature = "jj")]
use jj_lib::config::StackedConfig;
#[cfg(feature = "jj")]
use jj_lib::conflicts::{
    materialize_merge_result_to_bytes, try_materialize_file_conflict_value, ConflictMarkerStyle,
    ConflictMaterializeOptions,
};
#[cfg(feature = "jj")]
use jj_lib::files::FileMergeHunkLevel;
#[cfg(feature = "jj")]
use jj_lib::matchers::EverythingMatcher;
#[cfg(feature = "jj")]
use jj_lib::merge::{MergedTreeValue, SameChange};
#[cfg(feature = "jj")]
use jj_lib::object_id::ObjectId;
#[cfg(feature = "jj")]
use jj_lib::repo::{ReadonlyRepo, Repo, StoreFactories};
#[cfg(feature = "jj")]
use jj_lib::repo_path::{RepoPath, RepoPathBuf, RepoPathUiConverter};
#[cfg(feature = "jj")]
use jj_lib::revset::{
    RevsetAliasesMap, RevsetDiagnostics, RevsetExtensions, RevsetParseContext,
    RevsetWorkspaceContext, SymbolResolver, SymbolResolverExtension,
};
#[cfg(feature = "jj")]
use jj_lib::settings::UserSettings;
#[cfg(feature = "jj")]
use jj_lib::time_util::DatePatternContext;
#[cfg(feature = "jj")]
use jj_lib::tree_merge::MergeOptions;
#[cfg(feature = "jj")]
use jj_lib::workspace::{default_working_copy_factories, Workspace};
#[cfg(feature = "jj")]
use pollster::FutureExt;
#[cfg(feature = "jj")]
use tokio::io::AsyncReadExt;

/// Maximum file size to load (50 MiB). Prevents OOM on huge files.
pub const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Errors from repository operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RepoError {
    /// Path is not inside a git or jj repository.
    #[error("not inside a git or jj repository")]
    NotARepo,
    /// Git command failed with an error message.
    #[error("git command failed: {0}")]
    GitError(String),
    /// Jujutsu operation failed with an error message.
    #[error("jj operation failed: {0}")]
    JjError(String),
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
#[derive(Debug, Clone, Default)]
pub enum DiffSource {
    /// Working tree changes vs parent (HEAD/@-).
    #[default]
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

impl DiffSource {
    /// Apply default refs for open-ended ranges.
    pub fn apply_defaults(&mut self, default_ref: &str) {
        if let DiffSource::Range { from, to } = self {
            if from.is_empty() {
                *from = default_ref.to_string();
            }
            if to.is_empty() {
                *to = default_ref.to_string();
            }
        }
    }
}

/// Detected VCS type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VcsType {
    /// Git repository (.git).
    Git,
    /// Jujutsu repository (.jj).
    Jj,
}

#[cfg(feature = "jj")]
fn find_jj_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };

    loop {
        if current.join(".jj").is_dir() {
            return Some(current.to_path_buf());
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => return None,
        }
    }
}

#[cfg(not(feature = "jj"))]
fn find_jj_root(_start: &Path) -> Option<PathBuf> {
    None
}

/// Canonicalized path to a VCS repository root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoRoot {
    root: PathBuf,
    vcs: VcsType,
}

impl RepoRoot {
    /// Discover the git/jj repository containing the given path.
    ///
    /// Walks up the directory tree to find a `.jj` or `.git` directory.
    /// Prefers `.jj` when both are present (colocated repo).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use quickdiff::core::RepoRoot;
    /// use std::path::Path;
    ///
    /// let repo = RepoRoot::discover(Path::new(".")).expect("not in a repo");
    /// println!("Repo at: {}", repo.path().display());
    /// ```
    #[must_use = "this returns a Result that should be checked"]
    pub fn discover(path: &Path) -> Result<Self, RepoError> {
        if let Some(root) = find_jj_root(path) {
            let root = root.canonicalize().map_err(|_| RepoError::NotARepo)?;
            return Ok(Self {
                root,
                vcs: VcsType::Jj,
            });
        }

        let repo = Repository::discover(path).map_err(|_| RepoError::NotARepo)?;
        let root = repo
            .workdir()
            .ok_or(RepoError::NotARepo)?
            .canonicalize()
            .map_err(|_| RepoError::NotARepo)?;
        Ok(Self {
            root,
            vcs: VcsType::Git,
        })
    }

    /// Get the repository root path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Get the repository root as a string (for persistence keys).
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.root.to_str().unwrap_or("")
    }

    /// Get the detected VCS type.
    #[must_use]
    pub fn vcs(&self) -> VcsType {
        self.vcs
    }

    /// Returns true when the repository is Git-backed.
    #[must_use]
    pub fn is_git(&self) -> bool {
        matches!(self.vcs, VcsType::Git)
    }

    /// Returns true when the repository is Jujutsu-backed.
    #[must_use]
    pub fn is_jj(&self) -> bool {
        matches!(self.vcs, VcsType::Jj)
    }

    /// Default parent reference for working copy comparisons.
    #[must_use]
    pub fn working_copy_parent_ref(&self) -> &'static str {
        match self.vcs {
            VcsType::Git => "HEAD",
            VcsType::Jj => "@-",
        }
    }

    /// Default reference for the current working copy commit.
    #[must_use]
    pub fn working_copy_ref(&self) -> &'static str {
        match self.vcs {
            VcsType::Git => "HEAD",
            VcsType::Jj => "@",
        }
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
        let root = repo.workdir().ok_or(RepoError::NotARepo)?.to_path_buf();
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

#[cfg(feature = "jj")]
struct JjRepo {
    workspace: Workspace,
    repo: Arc<ReadonlyRepo>,
    settings: UserSettings,
    workspace_path: PathBuf,
}

#[cfg(feature = "jj")]
impl JjRepo {
    fn open(path: &Path) -> Result<Self, RepoError> {
        let config = StackedConfig::with_defaults();
        let settings = UserSettings::from_config(config)
            .map_err(|e| RepoError::JjError(format!("failed to create settings: {}", e)))?;

        let workspace = Workspace::load(
            &settings,
            path,
            &StoreFactories::default(),
            &default_working_copy_factories(),
        )
        .map_err(|e| RepoError::JjError(format!("failed to load workspace: {}", e)))?;

        let repo = workspace
            .repo_loader()
            .load_at_head()
            .map_err(|e| RepoError::JjError(format!("failed to load repo: {}", e)))?;

        Ok(Self {
            workspace,
            repo,
            settings,
            workspace_path: path.to_path_buf(),
        })
    }

    fn with_revset_context<T, F>(&self, f: F) -> Result<T, RepoError>
    where
        F: FnOnce(&RevsetParseContext) -> Result<T, RepoError>,
    {
        let path_converter = RepoPathUiConverter::Fs {
            cwd: self.workspace_path.clone(),
            base: self.workspace_path.clone(),
        };
        let workspace_ctx = RevsetWorkspaceContext {
            path_converter: &path_converter,
            workspace_name: self.workspace.workspace_name(),
        };

        let context = RevsetParseContext {
            aliases_map: &RevsetAliasesMap::default(),
            local_variables: HashMap::new(),
            user_email: self.settings.user_email(),
            date_pattern_context: DatePatternContext::from(Local::now()),
            default_ignored_remote: None,
            use_glob_by_default: true,
            extensions: &RevsetExtensions::default(),
            workspace: Some(workspace_ctx),
        };

        f(&context)
    }

    fn resolve_single_commit(&self, revset_str: &str) -> Result<JjCommit, RepoError> {
        let repo = self.repo.as_ref();

        self.with_revset_context(|context| {
            let mut diagnostics = RevsetDiagnostics::new();
            let expression = jj_lib::revset::parse(&mut diagnostics, revset_str, context)
                .map_err(|e| RepoError::InvalidRevision(format!("parse error: {}", e)))?;

            let symbol_resolver =
                SymbolResolver::new(repo, &([] as [&Box<dyn SymbolResolverExtension>; 0]));

            let resolved = expression
                .resolve_user_expression(repo, &symbol_resolver)
                .map_err(|e| RepoError::InvalidRevision(format!("resolution error: {}", e)))?;

            let revset = resolved
                .evaluate(repo)
                .map_err(|e| RepoError::JjError(format!("evaluation error: {}", e)))?;

            let mut iter = revset.iter();
            let commit_id = iter
                .next()
                .ok_or_else(|| {
                    RepoError::InvalidRevision(format!("revision '{}' not found", revset_str))
                })?
                .map_err(|e| RepoError::JjError(format!("iterator error: {}", e)))?;

            let commit = repo
                .store()
                .get_commit(&commit_id)
                .map_err(|e| RepoError::JjError(format!("failed to load commit: {}", e)))?;

            Ok(commit)
        })
    }

    fn get_content_from_value(
        &self,
        repo: &dyn Repo,
        path: &RepoPath,
        value: &MergedTreeValue,
    ) -> Result<Option<Vec<u8>>, RepoError> {
        if let Some(resolved) = value.as_resolved() {
            match resolved {
                Some(TreeValue::File { id, .. }) => {
                    let mut content = Vec::new();
                    let mut reader =
                        repo.store().read_file(path, id).block_on().map_err(|e| {
                            RepoError::JjError(format!("failed to read file: {}", e))
                        })?;

                    async { reader.read_to_end(&mut content).await }
                        .block_on()
                        .map_err(|e| {
                            RepoError::JjError(format!("failed to read content: {}", e))
                        })?;

                    Ok(Some(content))
                }
                None => Ok(None),
                _ => Ok(None),
            }
        } else {
            self.materialize_conflict(repo, path, value)
        }
    }

    fn materialize_conflict(
        &self,
        repo: &dyn Repo,
        path: &RepoPath,
        value: &MergedTreeValue,
    ) -> Result<Option<Vec<u8>>, RepoError> {
        let file_conflict = try_materialize_file_conflict_value(repo.store(), path, value)
            .block_on()
            .map_err(|e| RepoError::JjError(format!("failed to materialize conflict: {}", e)))?;

        match file_conflict {
            Some(file) => {
                let options = ConflictMaterializeOptions {
                    marker_style: ConflictMarkerStyle::Git,
                    marker_len: None,
                    merge: MergeOptions {
                        hunk_level: FileMergeHunkLevel::Line,
                        same_change: SameChange::Accept,
                    },
                };

                let content = materialize_merge_result_to_bytes(&file.contents, &options);
                Ok(Some(content.as_slice().to_vec()))
            }
            None => Ok(Some(
                b"<<<<<<< Conflict (non-file)\n(complex conflict - file vs non-file)\n>>>>>>>\n"
                    .to_vec(),
            )),
        }
    }
}

#[cfg(feature = "jj")]
fn jj_value_exists(value: &MergedTreeValue) -> bool {
    match value.as_resolved() {
        Some(resolved) => resolved.is_some(),
        None => true,
    }
}

#[cfg(feature = "jj")]
fn list_changed_files_jj(root: &RepoRoot) -> Result<Vec<ChangedFile>, RepoError> {
    let repo = JjRepo::open(root.path())?;
    let wc_commit = repo.resolve_single_commit("@")?;
    let repo_ref = repo.repo.as_ref();

    let parent_tree = if wc_commit.parent_ids().is_empty() {
        repo_ref.store().empty_merged_tree()
    } else {
        let parent_id = &wc_commit.parent_ids()[0];
        let parent = repo_ref
            .store()
            .get_commit(parent_id)
            .map_err(|e| RepoError::JjError(format!("failed to get parent: {}", e)))?;
        parent.tree()
    };

    let wc_tree = wc_commit.tree();
    let diff_stream = parent_tree.diff_stream(&wc_tree, &EverythingMatcher);
    let entries: Vec<_> = async { diff_stream.collect().await }.block_on();

    let mut files = Vec::new();

    for entry in entries {
        let diff = entry
            .values
            .map_err(|e| RepoError::JjError(format!("diff iteration error: {}", e)))?;

        let path_str = entry.path.as_internal_file_string();

        if path_str.is_empty()
            || path_str.starts_with(".quickdiff/")
            || path_str.starts_with(".jj/")
        {
            continue;
        }

        let before_exists = jj_value_exists(&diff.before);
        let after_exists = jj_value_exists(&diff.after);

        let kind = match (before_exists, after_exists) {
            (false, true) => FileChangeKind::Added,
            (true, false) => FileChangeKind::Deleted,
            (true, true) => FileChangeKind::Modified,
            (false, false) => continue,
        };

        files.push(ChangedFile::new(RelPath::new(path_str), kind));
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[cfg(feature = "jj")]
fn list_changed_files_between_jj(
    root: &RepoRoot,
    from: &str,
    to: &str,
) -> Result<Vec<ChangedFile>, RepoError> {
    let repo = JjRepo::open(root.path())?;
    let from_commit = repo.resolve_single_commit(from)?;
    let to_commit = repo.resolve_single_commit(to)?;

    let from_tree = from_commit.tree();
    let to_tree = to_commit.tree();

    let diff_stream = from_tree.diff_stream(&to_tree, &EverythingMatcher);
    let entries: Vec<_> = async { diff_stream.collect().await }.block_on();

    let mut files = Vec::new();

    for entry in entries {
        let diff = entry
            .values
            .map_err(|e| RepoError::JjError(format!("diff iteration error: {}", e)))?;

        let path_str = entry.path.as_internal_file_string();

        if path_str.is_empty()
            || path_str.starts_with(".quickdiff/")
            || path_str.starts_with(".jj/")
        {
            continue;
        }

        let before_exists = jj_value_exists(&diff.before);
        let after_exists = jj_value_exists(&diff.after);

        let kind = match (before_exists, after_exists) {
            (false, true) => FileChangeKind::Added,
            (true, false) => FileChangeKind::Deleted,
            (true, true) => FileChangeKind::Modified,
            (false, false) => continue,
        };

        files.push(ChangedFile::new(RelPath::new(path_str), kind));
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[cfg(feature = "jj")]
fn load_revision_content_jj(
    root: &RepoRoot,
    revision: &str,
    path: &RelPath,
) -> Result<Vec<u8>, RepoError> {
    let repo = JjRepo::open(root.path())?;
    let commit = repo.resolve_single_commit(revision)?;
    let tree = commit.tree();

    let repo_path = RepoPathBuf::from_internal_string(path.as_str().to_string())
        .map_err(|e| RepoError::InvalidRevision(format!("invalid path: {}", e)))?;

    let value = tree
        .path_value(&repo_path)
        .map_err(|e| RepoError::JjError(format!("failed to get path value: {}", e)))?;

    let content = repo.get_content_from_value(repo.repo.as_ref(), &repo_path, &value)?;
    let content = content.unwrap_or_default();

    if content.len() as u64 > MAX_FILE_SIZE {
        return Err(RepoError::FileTooLarge {
            size: content.len() as u64,
            max: MAX_FILE_SIZE,
        });
    }

    Ok(content)
}

#[cfg(feature = "jj")]
fn resolve_merge_base_jj(root: &RepoRoot, base: &str) -> Result<String, RepoError> {
    let repo = JjRepo::open(root.path())?;
    let revset = format!("heads(::({}) & ::({}))", base, root.working_copy_ref());
    let commit = repo.resolve_single_commit(&revset)?;
    Ok(commit.id().hex())
}

#[cfg(feature = "jj")]
fn resolve_revision_jj(root: &RepoRoot, revision: &str) -> Result<String, RepoError> {
    let repo = JjRepo::open(root.path())?;
    let commit = repo.resolve_single_commit(revision)?;
    Ok(commit.id().hex())
}

#[cfg(feature = "jj")]
fn get_parent_revision_jj(root: &RepoRoot, revision: &str) -> Result<String, RepoError> {
    let repo = JjRepo::open(root.path())?;
    let commit = repo.resolve_single_commit(revision)?;

    if commit.parent_ids().is_empty() {
        return Ok("root()".to_string());
    }

    Ok(commit.parent_ids()[0].hex())
}

/// Validate that a git reference doesn't look like a flag (defense in depth).
fn validate_git_ref_format(reference: &str) -> Result<(), RepoError> {
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

/// List changed files in the working tree vs HEAD/@-.
#[must_use = "this returns a Result that should be checked"]
pub fn list_changed_files(root: &RepoRoot) -> Result<Vec<ChangedFile>, RepoError> {
    if root.is_jj() {
        #[cfg(feature = "jj")]
        {
            return list_changed_files_jj(root);
        }
        #[cfg(not(feature = "jj"))]
        {
            return Err(RepoError::JjError("jj support not enabled".to_string()));
        }
    }

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

        // Skip internal files
        if path.starts_with(".quickdiff/") || path.starts_with(".jj/") {
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
                        files.push(ChangedFile::renamed(RelPath::new(old), RelPath::new(new)));
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
                    if path.starts_with(".quickdiff/")
                        || path.starts_with(".jj/")
                        || old.starts_with(".quickdiff/")
                        || old.starts_with(".jj/")
                    {
                        continue;
                    }
                    files.push(ChangedFile::renamed(RelPath::new(old), RelPath::new(path)));
                    continue;
                }
                FileChangeKind::Modified
            }
            _ => FileChangeKind::Modified, // fallback
        };

        // Skip internal files
        if path.starts_with(".quickdiff/") || path.starts_with(".jj/") {
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
    if root.is_jj() {
        return load_revision_content(root, root.working_copy_parent_ref(), path);
    }

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

/// Load content from a specific revision.
/// Returns error if file exceeds `MAX_FILE_SIZE`.
#[must_use = "this returns a Result that should be checked"]
pub fn load_revision_content(
    root: &RepoRoot,
    revision: &str,
    path: &RelPath,
) -> Result<Vec<u8>, RepoError> {
    if root.is_jj() {
        #[cfg(feature = "jj")]
        {
            return load_revision_content_jj(root, revision, path);
        }
        #[cfg(not(feature = "jj"))]
        {
            return Err(RepoError::JjError("jj support not enabled".to_string()));
        }
    }

    validate_git_ref_format(revision)?;

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

/// Resolve the merge-base between a base ref and the current commit.
#[must_use = "this returns a Result that should be checked"]
pub fn resolve_merge_base(root: &RepoRoot, base: &str) -> Result<String, RepoError> {
    let base = base.trim();
    if root.is_jj() {
        #[cfg(feature = "jj")]
        {
            return resolve_merge_base_jj(root, base);
        }
        #[cfg(not(feature = "jj"))]
        {
            return Err(RepoError::JjError("jj support not enabled".to_string()));
        }
    }

    validate_git_ref_format(base)?;

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

/// Resolve a revision to its full commit id.
#[must_use = "this returns a Result that should be checked"]
pub fn resolve_revision(root: &RepoRoot, revision: &str) -> Result<String, RepoError> {
    let revision = revision.trim();
    if root.is_jj() {
        #[cfg(feature = "jj")]
        {
            return resolve_revision_jj(root, revision);
        }
        #[cfg(not(feature = "jj"))]
        {
            return Err(RepoError::JjError("jj support not enabled".to_string()));
        }
    }

    validate_git_ref_format(revision)?;

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
    if root.is_jj() {
        #[cfg(feature = "jj")]
        {
            return get_parent_revision_jj(root, revision);
        }
        #[cfg(not(feature = "jj"))]
        {
            return Err(RepoError::JjError("jj support not enabled".to_string()));
        }
    }

    validate_git_ref_format(revision)?;

    let repo = Repository::open(root.path()).map_err(|_| RepoError::NotARepo)?;
    let obj = repo
        .revparse_single(revision)
        .map_err(|_| RepoError::InvalidRevision(revision.to_string()))?;
    let commit = obj
        .peel_to_commit()
        .map_err(|_| RepoError::InvalidRevision(revision.to_string()))?;

    if commit.parent_count() > 0 {
        Ok(commit
            .parent_id(0)
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
    if root.is_jj() {
        #[cfg(feature = "jj")]
        {
            return list_changed_files_between_jj(root, from, to);
        }
        #[cfg(not(feature = "jj"))]
        {
            return Err(RepoError::JjError("jj support not enabled".to_string()));
        }
    }

    validate_git_ref_format(from)?;
    validate_git_ref_format(to)?;

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
        let old_path = delta.old_file().path().and_then(|p| p.to_str());

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
            if root.is_jj() {
                #[cfg(feature = "jj")]
                {
                    if let Ok(repo) = JjRepo::open(root.path()) {
                        if let Ok(commit) = repo.resolve_single_commit(c) {
                            let change_id = commit.change_id().hex();
                            let summary = commit.description().lines().next().unwrap_or("");
                            let msg = format!("{} {}", take_chars(&change_id, 8), summary);
                            return truncate_chars(&msg, 50);
                        }
                    }
                }
                return format!("Commit {}", take_chars(c, 7));
            }

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

    #[cfg(feature = "jj")]
    use std::fs;
    #[cfg(feature = "jj")]
    use std::path::Path;
    #[cfg(feature = "jj")]
    use std::process::Command;
    #[cfg(feature = "jj")]
    use tempfile::TempDir;

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

    #[cfg(feature = "jj")]
    fn jj_available() -> bool {
        Command::new("jj")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[cfg(feature = "jj")]
    fn jj(dir: &Path, args: &[&str]) -> bool {
        Command::new("jj")
            .current_dir(dir)
            .args(args)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[cfg(feature = "jj")]
    struct JjRepoGuard {
        dir: TempDir,
    }

    #[cfg(feature = "jj")]
    impl JjRepoGuard {
        fn new() -> Option<Self> {
            if !jj_available() {
                return None;
            }

            let dir = TempDir::new().ok()?;
            if !jj(dir.path(), &["git", "init"]) {
                return None;
            }

            jj(
                dir.path(),
                &["config", "set", "--repo", "user.email", "test@example.com"],
            );
            jj(
                dir.path(),
                &["config", "set", "--repo", "user.name", "Test User"],
            );

            fs::write(dir.path().join("README.md"), "hello\n").ok()?;
            jj(dir.path(), &["status"]);

            Some(Self { dir })
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    #[test]
    #[cfg(feature = "jj")]
    fn jj_repo_root_discovery() {
        let Some(repo) = JjRepoGuard::new() else {
            eprintln!("Skipping test: jj not available");
            return;
        };

        let root = RepoRoot::discover(repo.path()).expect("should discover jj repo");
        assert!(root.is_jj());
    }

    #[test]
    #[cfg(feature = "jj")]
    fn jj_list_changed_files_includes_readme() {
        let Some(repo) = JjRepoGuard::new() else {
            eprintln!("Skipping test: jj not available");
            return;
        };

        let root = RepoRoot::discover(repo.path()).expect("should discover jj repo");
        let files = list_changed_files(&root).expect("list_changed_files should succeed");

        assert!(files.iter().any(|f| f.path.as_str() == "README.md"));
    }

    #[test]
    #[cfg(feature = "jj")]
    fn jj_load_revision_content_reads_file() {
        let Some(repo) = JjRepoGuard::new() else {
            eprintln!("Skipping test: jj not available");
            return;
        };

        let root = RepoRoot::discover(repo.path()).expect("should discover jj repo");
        let content = load_revision_content(&root, "@", &RelPath::new("README.md"))
            .expect("load_revision_content should succeed");

        assert_eq!(content, b"hello\n");
    }

    #[test]
    #[cfg(feature = "jj")]
    fn jj_resolve_revision_returns_hex_id() {
        let Some(repo) = JjRepoGuard::new() else {
            eprintln!("Skipping test: jj not available");
            return;
        };

        let root = RepoRoot::discover(repo.path()).expect("should discover jj repo");
        let revision = resolve_revision(&root, "@").expect("resolve_revision should succeed");

        assert!(!revision.is_empty());
        assert!(revision.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
