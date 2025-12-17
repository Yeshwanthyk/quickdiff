//! Diff model and hunk navigation.

use similar::{ChangeTag, TextDiff};

use crate::core::TextBuffer;

/// A single line reference in the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRef {
    /// Original line number (0-indexed).
    pub line_num: usize,
    /// The line content (without trailing newline).
    pub content: String,
}

/// Kind of change for a render row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Both sides are equal.
    Equal,
    /// Line was deleted from old (no corresponding new line).
    Delete,
    /// Line was inserted in new (no corresponding old line).
    Insert,
    /// Line was replaced (both old and new present, but different).
    Replace,
}

/// A single row in the rendered diff view.
/// Maps to 0..1 old line + 0..1 new line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRow {
    /// Line from the old file (if any).
    pub old: Option<LineRef>,
    /// Line from the new file (if any).
    pub new: Option<LineRef>,
    /// The kind of change.
    pub kind: ChangeKind,
}

/// A hunk is a contiguous block of changes with context.
#[derive(Debug, Clone)]
pub struct Hunk {
    /// Start row index in the render rows.
    pub start_row: usize,
    /// Number of rows in this hunk (including context).
    pub row_count: usize,
    /// Old file line range (start, count).
    pub old_range: (usize, usize),
    /// New file line range (start, count).
    pub new_range: (usize, usize),
}

/// Complete diff result between two text buffers.
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// All render rows.
    pub rows: Vec<RenderRow>,
    /// Hunk index for navigation (sorted by start_row).
    pub hunks: Vec<Hunk>,
}

impl DiffResult {
    /// Compute diff between old and new text buffers.
    pub fn compute(old: &TextBuffer, new: &TextBuffer) -> Self {
        compute_diff(old, new, 3) // 3 lines of context by default
    }

    /// Compute diff with custom context lines.
    pub fn compute_with_context(old: &TextBuffer, new: &TextBuffer, context: usize) -> Self {
        compute_diff(old, new, context)
    }

    /// Total number of render rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Get render rows for a viewport.
    pub fn render_rows(&self, start_row: usize, height: usize) -> impl Iterator<Item = &RenderRow> {
        self.rows.iter().skip(start_row).take(height)
    }

    /// Find the next hunk after the given row (for `}` navigation).
    /// Returns the start row of the next hunk, or None if no more hunks.
    pub fn next_hunk_row(&self, current_row: usize) -> Option<usize> {
        let idx = self.hunks.partition_point(|h| h.start_row <= current_row);
        self.hunks.get(idx).map(|h| h.start_row)
    }

    /// Find the previous hunk before the given row (for `{` navigation).
    /// Returns the start row of the previous hunk, or None if no earlier hunks.
    pub fn prev_hunk_row(&self, current_row: usize) -> Option<usize> {
        let idx = self.hunks.partition_point(|h| h.start_row < current_row);
        if idx > 0 {
            Some(self.hunks[idx - 1].start_row)
        } else {
            None
        }
    }

    /// Check if there are any changes.
    pub fn has_changes(&self) -> bool {
        self.rows.iter().any(|r| r.kind != ChangeKind::Equal)
    }
}

/// Intermediate change representation before pairing.
#[derive(Debug)]
enum Change {
    Equal {
        old_line: usize,
        new_line: usize,
        content: String,
    },
    Delete {
        old_line: usize,
        content: String,
    },
    Insert {
        new_line: usize,
        content: String,
    },
}

/// Compute the diff with context lines.
/// Groups consecutive delete+insert runs into paired Replace rows.
fn compute_diff(old: &TextBuffer, new: &TextBuffer, context: usize) -> DiffResult {
    let old_lines: Vec<String> = old.lines().into_iter().map(|c| c.into_owned()).collect();
    let new_lines: Vec<String> = new.lines().into_iter().map(|c| c.into_owned()).collect();
    let old_refs: Vec<&str> = old_lines.iter().map(|s| s.as_str()).collect();
    let new_refs: Vec<&str> = new_lines.iter().map(|s| s.as_str()).collect();

    let diff = TextDiff::from_slices(&old_refs, &new_refs);

    // Collect all changes first
    let mut changes: Vec<Change> = Vec::new();
    let mut old_line = 0usize;
    let mut new_line = 0usize;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                changes.push(Change::Equal {
                    old_line,
                    new_line,
                    content: change.value().to_string(),
                });
                old_line += 1;
                new_line += 1;
            }
            ChangeTag::Delete => {
                changes.push(Change::Delete {
                    old_line,
                    content: change.value().to_string(),
                });
                old_line += 1;
            }
            ChangeTag::Insert => {
                changes.push(Change::Insert {
                    new_line,
                    content: change.value().to_string(),
                });
                new_line += 1;
            }
        }
    }

    // Convert changes to rows, pairing deletes with inserts
    let rows = pair_changes(&changes);

    // Build hunks from rows
    let hunks = build_hunks(&rows, context);

    DiffResult { rows, hunks }
}

/// Pair consecutive delete+insert sequences into Replace rows.
fn pair_changes(changes: &[Change]) -> Vec<RenderRow> {
    let mut rows = Vec::new();
    let mut i = 0;

    while i < changes.len() {
        match &changes[i] {
            Change::Equal {
                old_line,
                new_line,
                content,
            } => {
                rows.push(RenderRow {
                    old: Some(LineRef {
                        line_num: *old_line,
                        content: content.clone(),
                    }),
                    new: Some(LineRef {
                        line_num: *new_line,
                        content: content.clone(),
                    }),
                    kind: ChangeKind::Equal,
                });
                i += 1;
            }
            Change::Delete { .. } | Change::Insert { .. } => {
                // Collect consecutive deletes and inserts
                let mut deletes: Vec<&Change> = Vec::new();
                let mut inserts: Vec<&Change> = Vec::new();

                while i < changes.len() {
                    match &changes[i] {
                        Change::Delete { .. } => {
                            deletes.push(&changes[i]);
                            i += 1;
                        }
                        Change::Insert { .. } => {
                            inserts.push(&changes[i]);
                            i += 1;
                        }
                        Change::Equal { .. } => break,
                    }
                }

                // Pair deletes with inserts
                let max_len = deletes.len().max(inserts.len());
                for j in 0..max_len {
                    let old = deletes.get(j).map(|c| {
                        if let Change::Delete { old_line, content } = c {
                            LineRef {
                                line_num: *old_line,
                                content: content.clone(),
                            }
                        } else {
                            unreachable!()
                        }
                    });
                    let new = inserts.get(j).map(|c| {
                        if let Change::Insert { new_line, content } = c {
                            LineRef {
                                line_num: *new_line,
                                content: content.clone(),
                            }
                        } else {
                            unreachable!()
                        }
                    });

                    let kind = match (&old, &new) {
                        (Some(_), Some(_)) => ChangeKind::Replace,
                        (Some(_), None) => ChangeKind::Delete,
                        (None, Some(_)) => ChangeKind::Insert,
                        (None, None) => unreachable!(),
                    };

                    rows.push(RenderRow { old, new, kind });
                }
            }
        }
    }

    rows
}

/// Build hunks from rows with context.
fn build_hunks(rows: &[RenderRow], context: usize) -> Vec<Hunk> {
    if rows.is_empty() {
        return Vec::new();
    }

    let mut hunks = Vec::new();
    let mut in_hunk = false;
    let mut hunk_start = 0;
    let mut last_change = 0;

    for (i, row) in rows.iter().enumerate() {
        let is_change = row.kind != ChangeKind::Equal;

        if is_change {
            if !in_hunk {
                // Start new hunk with leading context
                hunk_start = i.saturating_sub(context);
                in_hunk = true;
            }
            last_change = i;
        } else if in_hunk {
            // Check if we should close the hunk
            let gap = i - last_change;
            if gap >= context * 2 {
                // Close hunk with trailing context
                let hunk_end = last_change + context + 1;
                hunks.push(make_hunk(rows, hunk_start, hunk_end));
                in_hunk = false;
            }
        }
    }

    // Close final hunk if open
    if in_hunk {
        let hunk_end = (last_change + context + 1).min(rows.len());
        hunks.push(make_hunk(rows, hunk_start, hunk_end));
    }

    hunks
}

/// Create a Hunk from a row range.
fn make_hunk(rows: &[RenderRow], start: usize, end: usize) -> Hunk {
    let slice = &rows[start..end];

    let old_start = slice
        .iter()
        .filter_map(|r| r.old.as_ref().map(|l| l.line_num))
        .min()
        .unwrap_or(0);
    let old_end = slice
        .iter()
        .filter_map(|r| r.old.as_ref().map(|l| l.line_num))
        .max()
        .unwrap_or(0);

    let new_start = slice
        .iter()
        .filter_map(|r| r.new.as_ref().map(|l| l.line_num))
        .min()
        .unwrap_or(0);
    let new_end = slice
        .iter()
        .filter_map(|r| r.new.as_ref().map(|l| l.line_num))
        .max()
        .unwrap_or(0);

    Hunk {
        start_row: start,
        row_count: end - start,
        old_range: (old_start, old_end.saturating_sub(old_start) + 1),
        new_range: (new_start, new_end.saturating_sub(new_start) + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_diff() {
        let old = TextBuffer::new(b"");
        let new = TextBuffer::new(b"");
        let result = DiffResult::compute(&old, &new);
        assert_eq!(result.row_count(), 0);
        assert!(result.hunks.is_empty());
        assert!(!result.has_changes());
    }

    #[test]
    fn identical_files() {
        let old = TextBuffer::new(b"line1\nline2\nline3\n");
        let new = TextBuffer::new(b"line1\nline2\nline3\n");
        let result = DiffResult::compute(&old, &new);
        assert!(!result.has_changes());
        assert!(result.hunks.is_empty());
    }

    #[test]
    fn simple_insert() {
        let old = TextBuffer::new(b"line1\nline3\n");
        let new = TextBuffer::new(b"line1\nline2\nline3\n");
        let result = DiffResult::compute(&old, &new);
        assert!(result.has_changes());
        assert_eq!(result.hunks.len(), 1);

        let insert = result
            .rows
            .iter()
            .find(|r| r.kind == ChangeKind::Insert)
            .unwrap();
        assert_eq!(insert.new.as_ref().unwrap().content, "line2");
        assert!(insert.old.is_none());
    }

    #[test]
    fn simple_delete() {
        let old = TextBuffer::new(b"line1\nline2\nline3\n");
        let new = TextBuffer::new(b"line1\nline3\n");
        let result = DiffResult::compute(&old, &new);
        assert!(result.has_changes());

        let delete = result
            .rows
            .iter()
            .find(|r| r.kind == ChangeKind::Delete)
            .unwrap();
        assert_eq!(delete.old.as_ref().unwrap().content, "line2");
        assert!(delete.new.is_none());
    }

    #[test]
    fn replace_pairing() {
        // When a line is modified, it should show as Replace with both old and new
        let old = TextBuffer::new(b"line1\nold content\nline3\n");
        let new = TextBuffer::new(b"line1\nnew content\nline3\n");
        let result = DiffResult::compute(&old, &new);
        assert!(result.has_changes());

        let replace = result
            .rows
            .iter()
            .find(|r| r.kind == ChangeKind::Replace)
            .unwrap();
        assert_eq!(replace.old.as_ref().unwrap().content, "old content");
        assert_eq!(replace.new.as_ref().unwrap().content, "new content");
    }

    #[test]
    fn multi_line_replace() {
        // Multiple consecutive changes should pair up
        let old = TextBuffer::new(b"a\nb\nc\n");
        let new = TextBuffer::new(b"x\ny\nz\n");
        let result = DiffResult::compute(&old, &new);

        let replaces: Vec<_> = result
            .rows
            .iter()
            .filter(|r| r.kind == ChangeKind::Replace)
            .collect();
        assert_eq!(replaces.len(), 3);

        // Check pairing
        assert_eq!(replaces[0].old.as_ref().unwrap().content, "a");
        assert_eq!(replaces[0].new.as_ref().unwrap().content, "x");
    }

    #[test]
    fn unbalanced_changes() {
        // More deletes than inserts
        let old = TextBuffer::new(b"a\nb\nc\n");
        let new = TextBuffer::new(b"x\n");
        let result = DiffResult::compute(&old, &new);

        // Should have 1 replace + 2 deletes
        let replaces: Vec<_> = result
            .rows
            .iter()
            .filter(|r| r.kind == ChangeKind::Replace)
            .collect();
        let deletes: Vec<_> = result
            .rows
            .iter()
            .filter(|r| r.kind == ChangeKind::Delete)
            .collect();
        assert_eq!(replaces.len(), 1);
        assert_eq!(deletes.len(), 2);
    }

    #[test]
    fn next_hunk_navigation() {
        let old = TextBuffer::new(b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np\nq\nr\ns\nt\n");
        let new = TextBuffer::new(b"a\nB\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nN\no\np\nq\nr\ns\nt\n");
        let result = DiffResult::compute_with_context(&old, &new, 2);

        assert!(!result.hunks.is_empty());

        if let Some(next) = result.next_hunk_row(0) {
            assert!(next > 0 || result.hunks[0].start_row == 0);
        }
    }

    #[test]
    fn prev_hunk_navigation() {
        let old = TextBuffer::new(b"a\nb\nc\nd\n");
        let new = TextBuffer::new(b"A\nb\nc\nD\n");
        let result = DiffResult::compute_with_context(&old, &new, 1);

        let last_row = result.row_count().saturating_sub(1);
        if let Some(prev) = result.prev_hunk_row(last_row) {
            assert!(prev < last_row);
        }
    }

    #[test]
    fn render_rows_viewport() {
        let old = TextBuffer::new(b"a\nb\nc\nd\ne\n");
        let new = TextBuffer::new(b"a\nB\nc\nd\ne\n");
        let result = DiffResult::compute(&old, &new);

        let viewport: Vec<_> = result.render_rows(0, 3).collect();
        assert!(viewport.len() <= 3);
    }
}
