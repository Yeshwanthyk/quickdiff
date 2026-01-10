//! File system watching for live reload.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};

use super::RepoRoot;

/// Events emitted by the repo watcher.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// Files changed, refresh needed.
    Changed,
}

/// Watches a repository for file changes.
pub struct RepoWatcher {
    /// Receiver for watch events.
    rx: Receiver<WatchEvent>,
    /// Keep watcher alive. Dropping this stops watching.
    _watcher: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl RepoWatcher {
    /// Create a new watcher for the given repository root.
    ///
    /// Watches recursively, excluding `.git/`, `.jj/`, and `.quickdiff/` directories.
    /// Events are debounced (200ms window) and coalesced into `WatchEvent::Changed`.
    pub fn new(root: &RepoRoot) -> Result<Self, notify::Error> {
        let (tx, rx) = mpsc::channel();
        let repo_path = root.path().to_path_buf();

        // Create debouncer with 200ms timeout
        let mut debouncer = new_debouncer(
            Duration::from_millis(200),
            move |res: DebounceEventResult| {
                if let Ok(events) = res {
                    // Filter out .git/, .jj/, and .quickdiff/ events
                    let relevant = events.iter().any(|e| !is_ignored_path(&e.path, &repo_path));

                    if relevant {
                        // Coalesce all events into single Changed signal
                        let _ = tx.send(WatchEvent::Changed);
                    }
                }
            },
        )?;

        // Watch repo root recursively
        debouncer
            .watcher()
            .watch(root.path(), RecursiveMode::Recursive)?;

        Ok(Self {
            rx,
            _watcher: debouncer,
        })
    }

    /// Poll for watch events without blocking.
    ///
    /// Returns `Some(WatchEvent)` if files changed, `None` if no events pending.
    pub fn poll(&self) -> Option<WatchEvent> {
        match self.rx.try_recv() {
            Ok(event) => {
                // Drain any additional pending events (debouncer may send multiple)
                while self.rx.try_recv().is_ok() {}
                Some(event)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }
}

/// Check if a path should be ignored for watching.
fn is_ignored_path(path: &Path, repo_root: &Path) -> bool {
    // Get path relative to repo root
    let rel = match path.strip_prefix(repo_root) {
        Ok(r) => r,
        Err(_) => return false,
    };

    // Check each component
    for component in rel.components() {
        if let std::path::Component::Normal(name) = component {
            let name = name.to_string_lossy();
            if name == ".git" || name == ".jj" || name == ".quickdiff" {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_ignored_path() {
        let root = PathBuf::from("/repo");

        // Should ignore
        assert!(is_ignored_path(Path::new("/repo/.git/objects/abc"), &root));
        assert!(is_ignored_path(Path::new("/repo/.git/HEAD"), &root));
        assert!(is_ignored_path(Path::new("/repo/.jj/store/abc"), &root));
        assert!(is_ignored_path(
            Path::new("/repo/.quickdiff/state.json"),
            &root
        ));

        // Should not ignore
        assert!(!is_ignored_path(Path::new("/repo/src/main.rs"), &root));
        assert!(!is_ignored_path(Path::new("/repo/file.txt"), &root));
        assert!(!is_ignored_path(Path::new("/repo/some/.gitignore"), &root));
    }
}
