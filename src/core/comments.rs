//! Comment model and anchor types.

use serde::{Deserialize, Serialize};

use crate::core::{ChangeKind, DiffResult, Hunk, RelPath};

/// Comment identifier.
pub type CommentId = u64;

/// Context in which a comment was created (which diff it refers to).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommentContext {
    /// Legacy comments created before context scoping existed.
    Unscoped,
    /// HEAD vs working tree.
    Worktree,
    /// Merge-base(base, HEAD) vs working tree.
    Base {
        /// The base ref (e.g., "origin/main").
        base: String,
    },
    /// Parent(commit) vs commit.
    Commit {
        /// The commit SHA.
        commit: String,
    },
    /// from..to comparison.
    Range {
        /// Starting commit.
        from: String,
        /// Ending commit.
        to: String,
    },
}

impl Default for CommentContext {
    fn default() -> Self {
        Self::Unscoped
    }
}

impl CommentContext {
    /// Whether this stored context should be considered relevant for the current view.
    ///
    /// `Unscoped` is treated as matching all contexts for backward compatibility.
    pub fn matches(&self, current: &CommentContext) -> bool {
        matches!(self, CommentContext::Unscoped) || self == current
    }

    fn is_unscoped(ctx: &CommentContext) -> bool {
        matches!(ctx, CommentContext::Unscoped)
    }
}

/// Status of a comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentStatus {
    /// Comment is unresolved.
    Open,
    /// Comment has been resolved.
    Resolved,
}

/// A hunk-level comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    /// Unique identifier.
    pub id: CommentId,
    /// Path to the file.
    pub path: RelPath,
    /// Context in which the comment was created.
    #[serde(default, skip_serializing_if = "CommentContext::is_unscoped")]
    pub context: CommentContext,
    /// Comment text.
    pub message: String,
    /// Current status.
    pub status: CommentStatus,
    /// Location anchor.
    pub anchor: Anchor,
    /// Creation timestamp (milliseconds since epoch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<u64>,
    /// Resolution timestamp (milliseconds since epoch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at_ms: Option<u64>,
}

/// Anchor describing where a comment is attached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor {
    /// Selectors for locating the comment (tried in order).
    pub selectors: Vec<Selector>,
}

/// Selector type for locating a comment target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Selector {
    /// V1 hunk-based selector using line ranges and content digest.
    DiffHunkV1(DiffHunkSelectorV1),
}

/// V1 selector: line ranges + content digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunkSelectorV1 {
    /// Old file line range (start, count).
    pub old_range: (usize, usize),
    /// New file line range (start, count).
    pub new_range: (usize, usize),
    /// FNV-1a 64-bit hash of changed rows, lowercase hex.
    pub digest_hex: String,
}

/// Build a selector from a hunk in a diff.
pub fn selector_from_hunk(diff: &DiffResult, hunk_idx: usize) -> Option<DiffHunkSelectorV1> {
    let hunk = diff.hunks().get(hunk_idx)?;
    let digest = digest_hunk_changed_rows(diff, hunk);
    Some(DiffHunkSelectorV1 {
        old_range: hunk.old_range,
        new_range: hunk.new_range,
        digest_hex: digest,
    })
}

/// Compute FNV-1a 64-bit digest of changed rows in a hunk.
/// Feeds `-<old_line>\n` for deletions/replaces (old side)
/// and `+<new_line>\n` for insertions/replaces (new side).
pub fn digest_hunk_changed_rows(diff: &DiffResult, hunk: &Hunk) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;

    let start = hunk.start_row;
    let end = start + hunk.row_count;

    for row in diff.rows().iter().skip(start).take(end - start) {
        match row.kind {
            ChangeKind::Delete | ChangeKind::Replace => {
                if let Some(ref old) = row.old {
                    // Feed "-<content>\n"
                    for byte in b"-".iter().chain(old.content.as_bytes()).chain(b"\n") {
                        hash ^= *byte as u64;
                        hash = hash.wrapping_mul(FNV_PRIME);
                    }
                }
            }
            _ => {}
        }
        match row.kind {
            ChangeKind::Insert | ChangeKind::Replace => {
                if let Some(ref new) = row.new {
                    // Feed "+<content>\n"
                    for byte in b"+".iter().chain(new.content.as_bytes()).chain(b"\n") {
                        hash ^= *byte as u64;
                        hash = hash.wrapping_mul(FNV_PRIME);
                    }
                }
            }
            _ => {}
        }
    }

    format!("{:016x}", hash)
}

/// Format an anchor into a human-friendly single-line summary.
pub fn format_anchor_summary(anchor: &Anchor) -> String {
    anchor
        .selectors
        .iter()
        .map(|s| match s {
            Selector::DiffHunkV1(h) => format!(
                "@@ -{},{} +{},{} @@ [{}]",
                h.old_range.0 + 1,
                h.old_range.1,
                h.new_range.0 + 1,
                h.new_range.1,
                &h.digest_hex[..8]
            ),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::TextBuffer;

    #[test]
    fn digest_determinism() {
        let old = TextBuffer::new(b"a\nb\nc\n");
        let new = TextBuffer::new(b"a\nB\nc\n");
        let diff1 = DiffResult::compute(&old, &new);
        let diff2 = DiffResult::compute(&old, &new);

        assert!(!diff1.hunks().is_empty());
        let d1 = digest_hunk_changed_rows(&diff1, &diff1.hunks()[0]);
        let d2 = digest_hunk_changed_rows(&diff2, &diff2.hunks()[0]);
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 16); // 64-bit hex
    }

    #[test]
    fn selector_from_valid_hunk() {
        let old = TextBuffer::new(b"line1\nold\nline3\n");
        let new = TextBuffer::new(b"line1\nnew\nline3\n");
        let diff = DiffResult::compute(&old, &new);

        let sel = selector_from_hunk(&diff, 0);
        assert!(sel.is_some());
        let sel = sel.unwrap();
        assert!(!sel.digest_hex.is_empty());
    }

    #[test]
    fn selector_from_invalid_hunk() {
        let old = TextBuffer::new(b"same\n");
        let new = TextBuffer::new(b"same\n");
        let diff = DiffResult::compute(&old, &new);

        // No hunks
        assert!(selector_from_hunk(&diff, 0).is_none());
    }

    #[test]
    fn context_unscoped_matches_all() {
        assert!(CommentContext::Unscoped.matches(&CommentContext::Worktree));
        assert!(CommentContext::Unscoped.matches(&CommentContext::Base {
            base: "origin/main".to_string(),
        }));
    }

    #[test]
    fn context_scoped_matches_exact() {
        assert!(CommentContext::Worktree.matches(&CommentContext::Worktree));
        assert!(!CommentContext::Worktree.matches(&CommentContext::Base {
            base: "origin/main".to_string(),
        }));
    }
}
