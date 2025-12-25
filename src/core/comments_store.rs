//! Comment persistence with repo-local storage.

use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::{Anchor, Comment, CommentContext, CommentId, CommentStatus, RelPath, RepoRoot};

/// Persisted state schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentsState {
    /// Schema version for migration.
    pub version: u32,
    /// Next available comment ID.
    pub next_id: CommentId,
    /// All stored comments.
    pub comments: Vec<Comment>,
}

impl Default for CommentsState {
    fn default() -> Self {
        Self {
            version: 1,
            next_id: 1,
            comments: Vec::new(),
        }
    }
}

/// Trait for comment storage operations.
pub trait CommentStore {
    /// List all comments, optionally filtering by status.
    fn list(&self, include_resolved: bool) -> Vec<&Comment>;

    /// List comments for a specific file.
    fn list_for_path(&self, path: &RelPath, include_resolved: bool) -> Vec<&Comment>;

    /// Add a new comment, returns the assigned ID.
    fn add(
        &mut self,
        path: RelPath,
        context: CommentContext,
        message: String,
        anchor: Anchor,
    ) -> io::Result<CommentId>;

    /// Resolve a comment by ID.
    fn resolve(&mut self, id: CommentId) -> io::Result<bool>;

    /// Get a comment by ID.
    fn get(&self, id: CommentId) -> Option<&Comment>;
}

/// File-backed comment store under `.quickdiff/comments.json`.
pub struct FileCommentStore {
    state_path: PathBuf,
    state: CommentsState,
}

impl FileCommentStore {
    /// Open or create a comment store for the given repo.
    #[must_use = "this returns a Result that should be checked"]
    pub fn open(repo_root: &RepoRoot) -> io::Result<Self> {
        let dir = repo_root.path().join(".quickdiff");
        let state_path = dir.join("comments.json");

        let state = if state_path.exists() {
            let content = std::fs::read_to_string(&state_path)?;
            serde_json::from_str(&content).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid comments.json: {}", e),
                )
            })?
        } else {
            CommentsState::default()
        };

        Ok(Self { state_path, state })
    }

    /// Save state to disk using atomic write.
    #[must_use = "this returns a Result that should be checked"]
    pub fn save(&self) -> io::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Atomic write: temp file + rename
        let temp_path = self.state_path.with_extension("json.tmp");
        let content = serde_json::to_string_pretty(&self.state)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(&temp_path, content)?;
        std::fs::rename(&temp_path, &self.state_path)?;

        Ok(())
    }

    /// Get current timestamp in milliseconds.
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

impl CommentStore for FileCommentStore {
    fn list(&self, include_resolved: bool) -> Vec<&Comment> {
        self.state
            .comments
            .iter()
            .filter(|c| include_resolved || c.status == CommentStatus::Open)
            .collect()
    }

    fn list_for_path(&self, path: &RelPath, include_resolved: bool) -> Vec<&Comment> {
        self.state
            .comments
            .iter()
            .filter(|c| c.path == *path && (include_resolved || c.status == CommentStatus::Open))
            .collect()
    }

    fn add(
        &mut self,
        path: RelPath,
        context: CommentContext,
        message: String,
        anchor: Anchor,
    ) -> io::Result<CommentId> {
        let id = self.state.next_id;
        self.state.next_id += 1;

        let comment = Comment {
            id,
            path,
            context,
            message,
            status: CommentStatus::Open,
            anchor,
            created_at_ms: Some(Self::now_ms()),
            resolved_at_ms: None,
        };

        self.state.comments.push(comment);
        self.save()?;

        Ok(id)
    }

    fn resolve(&mut self, id: CommentId) -> io::Result<bool> {
        if let Some(comment) = self.state.comments.iter_mut().find(|c| c.id == id) {
            comment.status = CommentStatus::Resolved;
            comment.resolved_at_ms = Some(Self::now_ms());
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get(&self, id: CommentId) -> Option<&Comment> {
        self.state.comments.iter().find(|c| c.id == id)
    }
}

/// In-memory comment store (for testing).
#[derive(Debug, Default)]
pub struct MemoryCommentStore {
    state: CommentsState,
}

impl MemoryCommentStore {
    /// Create a new empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl CommentStore for MemoryCommentStore {
    fn list(&self, include_resolved: bool) -> Vec<&Comment> {
        self.state
            .comments
            .iter()
            .filter(|c| include_resolved || c.status == CommentStatus::Open)
            .collect()
    }

    fn list_for_path(&self, path: &RelPath, include_resolved: bool) -> Vec<&Comment> {
        self.state
            .comments
            .iter()
            .filter(|c| c.path == *path && (include_resolved || c.status == CommentStatus::Open))
            .collect()
    }

    fn add(
        &mut self,
        path: RelPath,
        context: CommentContext,
        message: String,
        anchor: Anchor,
    ) -> io::Result<CommentId> {
        let id = self.state.next_id;
        self.state.next_id += 1;

        let comment = Comment {
            id,
            path,
            context,
            message,
            status: CommentStatus::Open,
            anchor,
            created_at_ms: None,
            resolved_at_ms: None,
        };

        self.state.comments.push(comment);
        Ok(id)
    }

    fn resolve(&mut self, id: CommentId) -> io::Result<bool> {
        if let Some(comment) = self.state.comments.iter_mut().find(|c| c.id == id) {
            comment.status = CommentStatus::Resolved;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get(&self, id: CommentId) -> Option<&Comment> {
        self.state.comments.iter().find(|c| c.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{DiffHunkSelectorV1, Selector};

    fn test_anchor() -> Anchor {
        Anchor {
            selectors: vec![Selector::DiffHunkV1(DiffHunkSelectorV1 {
                old_range: (1, 5),
                new_range: (1, 6),
                digest_hex: "deadbeef00000000".to_string(),
            })],
        }
    }

    #[test]
    fn memory_store_add_and_list() {
        let mut store = MemoryCommentStore::new();

        let id = store
            .add(
                RelPath::new("src/main.rs"),
                CommentContext::Worktree,
                "Fix this".to_string(),
                test_anchor(),
            )
            .unwrap();

        assert_eq!(id, 1);
        assert_eq!(store.list(false).len(), 1);
        assert_eq!(store.list(false)[0].message, "Fix this");
        assert!(store.list(false)[0]
            .context
            .matches(&CommentContext::Worktree));
    }

    #[test]
    fn memory_store_resolve() {
        let mut store = MemoryCommentStore::new();

        let id = store
            .add(
                RelPath::new("test.rs"),
                CommentContext::Worktree,
                "TODO".to_string(),
                test_anchor(),
            )
            .unwrap();

        assert_eq!(store.list(false).len(), 1);

        let resolved = store.resolve(id).unwrap();
        assert!(resolved);

        // Should not appear in open list
        assert_eq!(store.list(false).len(), 0);
        // Should appear in all list
        assert_eq!(store.list(true).len(), 1);
    }

    #[test]
    fn memory_store_resolve_nonexistent() {
        let mut store = MemoryCommentStore::new();
        let resolved = store.resolve(999).unwrap();
        assert!(!resolved);
    }

    #[test]
    fn file_store_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_path = temp_dir.path().to_path_buf();

        // Create a mock RepoRoot by using the temp dir
        // We'll create the store directly with the path
        let state_path = repo_path.join(".quickdiff").join("comments.json");

        // Add a comment
        {
            let mut store = FileCommentStore {
                state_path: state_path.clone(),
                state: CommentsState::default(),
            };

            let id = store
                .add(
                    RelPath::new("file.rs"),
                    CommentContext::Worktree,
                    "Review this".to_string(),
                    test_anchor(),
                )
                .unwrap();

            assert_eq!(id, 1);
        }

        // Reload and verify
        {
            let content = std::fs::read_to_string(&state_path).unwrap();
            let state: CommentsState = serde_json::from_str(&content).unwrap();

            assert_eq!(state.version, 1);
            assert_eq!(state.next_id, 2);
            assert_eq!(state.comments.len(), 1);
            assert_eq!(state.comments[0].message, "Review this");
            assert!(state.comments[0].context.matches(&CommentContext::Worktree));
        }
    }
}
