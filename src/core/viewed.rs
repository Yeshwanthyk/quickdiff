//! Viewed state storage.

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::core::RelPath;

/// Cached config directory path.
static CONFIG_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Get the quickdiff config directory (cached).
fn config_dir() -> &'static std::path::Path {
    CONFIG_DIR.get_or_init(|| {
        directories::ProjectDirs::from("", "", "quickdiff")
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(dirs_fallback)
    })
}

/// Trait for viewed state storage.
pub trait ViewedStore {
    /// Check if a file is marked as viewed.
    fn is_viewed(&self, path: &RelPath) -> bool;

    /// Mark a file as viewed.
    fn mark_viewed(&mut self, path: RelPath);

    /// Unmark a file as viewed.
    fn unmark_viewed(&mut self, path: &RelPath);

    /// Toggle viewed state, returns new state.
    fn toggle_viewed(&mut self, path: RelPath) -> bool {
        if self.is_viewed(&path) {
            self.unmark_viewed(&path);
            false
        } else {
            self.mark_viewed(path);
            true
        }
    }

    /// Get count of viewed files.
    fn viewed_count(&self) -> usize;
}

/// In-memory viewed state (no persistence).
#[derive(Debug, Default, Clone)]
pub struct MemoryViewedStore {
    viewed: HashSet<String>,
}

impl MemoryViewedStore {
    /// Create a new empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ViewedStore for MemoryViewedStore {
    fn is_viewed(&self, path: &RelPath) -> bool {
        self.viewed.contains(path.as_str())
    }

    fn mark_viewed(&mut self, path: RelPath) {
        self.viewed.insert(path.as_str().to_string());
    }

    fn unmark_viewed(&mut self, path: &RelPath) {
        self.viewed.remove(path.as_str());
    }

    fn viewed_count(&self) -> usize {
        self.viewed.len()
    }
}

/// Persisted state schema.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedState {
    /// Schema version for migration.
    pub version: u32,
    /// Per-repository state, keyed by repo path.
    pub repos: std::collections::HashMap<String, RepoState>,
}

/// Per-repository viewed state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepoState {
    /// List of viewed file paths.
    pub viewed: Vec<String>,
    /// Last selected file path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_selected: Option<String>,
}

impl PersistedState {
    /// Current schema version.
    pub const VERSION: u32 = 1;

    /// Create a new empty persisted state.
    pub fn new() -> Self {
        Self {
            version: Self::VERSION,
            repos: std::collections::HashMap::new(),
        }
    }
}

/// Persistent viewed store backed by a JSON file.
#[derive(Debug)]
pub struct FileViewedStore {
    /// Path to the state file.
    state_path: std::path::PathBuf,
    /// Repository root key.
    repo_key: String,
    /// In-memory state.
    viewed: HashSet<String>,
    /// Last selected file.
    last_selected: Option<String>,
}

impl FileViewedStore {
    /// Create a new FileViewedStore.
    /// Loads existing state if available.
    #[must_use = "this returns a Result that should be checked"]
    pub fn new(repo_root: &str) -> std::io::Result<Self> {
        let state_path = Self::default_state_path()?;
        let mut store = Self {
            state_path,
            repo_key: repo_root.to_string(),
            viewed: HashSet::new(),
            last_selected: None,
        };
        store.load()?;
        Ok(store)
    }

    /// Create with a custom state path (for testing).
    #[must_use = "this returns a Result that should be checked"]
    pub fn with_path(repo_root: &str, state_path: std::path::PathBuf) -> std::io::Result<Self> {
        let mut store = Self {
            state_path,
            repo_key: repo_root.to_string(),
            viewed: HashSet::new(),
            last_selected: None,
        };
        store.load()?;
        Ok(store)
    }

    /// Get the default state file path.
    fn default_state_path() -> std::io::Result<std::path::PathBuf> {
        Ok(config_dir().join("state.json"))
    }

    /// Load state from disk.
    fn load(&mut self) -> std::io::Result<()> {
        if !self.state_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.state_path)?;
        let state: PersistedState = serde_json::from_str(&content).unwrap_or_default();

        if let Some(repo_state) = state.repos.get(&self.repo_key) {
            self.viewed = repo_state.viewed.iter().cloned().collect();
            self.last_selected = repo_state.last_selected.clone();
        }

        Ok(())
    }

    /// Save state to disk (atomic write).
    #[must_use = "this returns a Result that should be checked"]
    pub fn save(&self) -> std::io::Result<()> {
        // Load existing state to preserve other repos
        let mut state = if self.state_path.exists() {
            let content = std::fs::read_to_string(&self.state_path)?;
            serde_json::from_str(&content).unwrap_or_else(|_| PersistedState::new())
        } else {
            PersistedState::new()
        };

        // Update this repo's state
        state.repos.insert(
            self.repo_key.clone(),
            RepoState {
                viewed: self.viewed.iter().cloned().collect(),
                last_selected: self.last_selected.clone(),
            },
        );

        // Ensure parent directory exists
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Atomic write: temp file + rename
        let temp_path = self.state_path.with_extension("json.tmp");
        let content = serde_json::to_string_pretty(&state)?;
        std::fs::write(&temp_path, content)?;
        std::fs::rename(&temp_path, &self.state_path)?;

        Ok(())
    }

    /// Get last selected file.
    pub fn last_selected(&self) -> Option<&str> {
        self.last_selected.as_deref()
    }

    /// Set last selected file.
    pub fn set_last_selected(&mut self, path: Option<String>) {
        self.last_selected = path;
    }
}

impl ViewedStore for FileViewedStore {
    fn is_viewed(&self, path: &RelPath) -> bool {
        self.viewed.contains(path.as_str())
    }

    fn mark_viewed(&mut self, path: RelPath) {
        self.viewed.insert(path.as_str().to_string());
    }

    fn unmark_viewed(&mut self, path: &RelPath) {
        self.viewed.remove(path.as_str());
    }

    fn viewed_count(&self) -> usize {
        self.viewed.len()
    }
}

/// Fallback config directory if `directories` fails.
fn dirs_fallback() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(|h| Path::new(&h).join(".config").join("quickdiff"))
        .unwrap_or_else(|_| std::path::PathBuf::from(".quickdiff"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_toggle() {
        let mut store = MemoryViewedStore::new();
        let path = RelPath::new("src/main.rs");

        assert!(!store.is_viewed(&path));
        assert_eq!(store.viewed_count(), 0);

        let result = store.toggle_viewed(path.clone());
        assert!(result);
        assert!(store.is_viewed(&path));
        assert_eq!(store.viewed_count(), 1);

        let result = store.toggle_viewed(path.clone());
        assert!(!result);
        assert!(!store.is_viewed(&path));
        assert_eq!(store.viewed_count(), 0);
    }

    #[test]
    fn persisted_state_serialization() {
        let mut state = PersistedState::new();
        state.repos.insert(
            "/path/to/repo".to_string(),
            RepoState {
                viewed: vec!["file1.rs".to_string(), "file2.rs".to_string()],
                last_selected: Some("file1.rs".to_string()),
            },
        );

        let json = serde_json::to_string(&state).unwrap();
        let parsed: PersistedState = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, PersistedState::VERSION);
        assert!(parsed.repos.contains_key("/path/to/repo"));
    }

    #[test]
    fn file_store_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let state_path = temp_dir.path().join("state.json");

        // Create and populate store
        {
            let mut store = FileViewedStore::with_path("/repo", state_path.clone()).unwrap();
            store.mark_viewed(RelPath::new("file1.rs"));
            store.set_last_selected(Some("file1.rs".to_string()));
            store.save().unwrap();
        }

        // Load in new instance
        {
            let store = FileViewedStore::with_path("/repo", state_path).unwrap();
            assert!(store.is_viewed(&RelPath::new("file1.rs")));
            assert_eq!(store.last_selected(), Some("file1.rs"));
        }
    }
}
