//! Diff model and hunk navigation.

use std::sync::Arc;

use similar::{ChangeTag, TextDiff};

use crate::core::TextBuffer;

/// A span within a line indicating changed/unchanged regions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSpan {
    /// Byte start offset in the line.
    pub start: usize,
    /// Byte end offset in the line.
    pub end: usize,
    /// Whether this span represents changed content.
    pub changed: bool,
}

/// A single line reference in the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRef {
    /// Original line number (0-indexed).
    pub line_num: usize,
    /// The line content (without trailing newline).
    pub content: String,
    /// Inline diff spans for word-level highlighting (only for Replace rows).
    pub inline_spans: Option<Vec<InlineSpan>>,
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
    /// All render rows (Arc for cheap cloning).
    rows: Arc<[RenderRow]>,
    /// Hunk index for navigation (sorted by start_row).
    hunks: Arc<[Hunk]>,
}

impl DiffResult {
    /// Compute diff between old and new text buffers.
    ///
    /// Uses 3 lines of context by default. For custom context, use
    /// [`compute_with_context`](Self::compute_with_context).
    ///
    /// # Examples
    ///
    /// ```
    /// use quickdiff::core::{DiffResult, TextBuffer};
    ///
    /// let old = TextBuffer::new(b"hello\nworld\n");
    /// let new = TextBuffer::new(b"hello\nrust\n");
    /// let diff = DiffResult::compute(&old, &new);
    ///
    /// assert!(diff.has_changes());
    /// assert_eq!(diff.hunks().len(), 1);
    /// ```
    pub fn compute(old: &TextBuffer, new: &TextBuffer) -> Self {
        let _timer = crate::metrics::Timer::start("diff_compute");
        compute_diff(old, new, 3) // 3 lines of context by default
    }

    /// Compute diff with custom context lines.
    pub fn compute_with_context(old: &TextBuffer, new: &TextBuffer, context: usize) -> Self {
        let _timer = crate::metrics::Timer::start("diff_compute_with_context");
        compute_diff(old, new, context)
    }

    /// Get all render rows.
    #[must_use]
    pub fn rows(&self) -> &[RenderRow] {
        &self.rows
    }

    /// Get all hunks.
    #[must_use]
    pub fn hunks(&self) -> &[Hunk] {
        &self.hunks
    }

    /// Total number of render rows.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Get render rows for a viewport.
    pub fn render_rows(&self, start_row: usize, height: usize) -> impl Iterator<Item = &RenderRow> {
        self.rows.iter().skip(start_row).take(height)
    }

    /// Find the next hunk after the given row (for `}` navigation).
    /// Returns the start row of the next hunk, or None if no more hunks.
    #[must_use]
    pub fn next_hunk_row(&self, current_row: usize) -> Option<usize> {
        let idx = self.hunks.partition_point(|h| h.start_row <= current_row);
        self.hunks.get(idx).map(|h| h.start_row)
    }

    /// Find the previous hunk before the given row (for `{` navigation).
    /// Returns the start row of the previous hunk, or None if no earlier hunks.
    #[must_use]
    pub fn prev_hunk_row(&self, current_row: usize) -> Option<usize> {
        let idx = self.hunks.partition_point(|h| h.start_row < current_row);
        if idx > 0 {
            Some(self.hunks[idx - 1].start_row)
        } else {
            None
        }
    }

    /// Check if there are any changes.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        self.rows.iter().any(|r| r.kind != ChangeKind::Equal)
    }

    /// Find the hunk containing a given row.
    /// Returns the hunk index (0-based) or None if row is not within any hunk.
    #[must_use]
    pub fn hunk_at_row(&self, row: usize) -> Option<usize> {
        let idx = self.hunks.partition_point(|h| h.start_row <= row);
        if idx == 0 {
            return None;
        }

        let candidate_idx = idx - 1;
        let h = &self.hunks[candidate_idx];
        if row < h.start_row + h.row_count {
            Some(candidate_idx)
        } else {
            None
        }
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
    let old_lines = old.lines();
    let new_lines = new.lines();
    let old_refs: Vec<&str> = old_lines.iter().map(|s| s.as_ref()).collect();
    let new_refs: Vec<&str> = new_lines.iter().map(|s| s.as_ref()).collect();

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
    let rows = pair_changes(changes);

    // Build hunks from rows
    let hunks = build_hunks(&rows, context);

    DiffResult {
        rows: rows.into(),
        hunks: hunks.into(),
    }
}

/// Convert changes to render rows.
///
/// Pairing strategy:
/// - Consecutive deletes and inserts are paired positionally (1st delete with 1st insert, etc.)
/// - Paired lines get word-level inline diff highlighting
/// - Unpaired lines (more deletes than inserts or vice versa) show as pure Delete/Insert
fn pair_changes(changes: Vec<Change>) -> Vec<RenderRow> {
    let mut rows = Vec::new();
    let mut iter = changes.into_iter().peekable();

    while let Some(change) = iter.next() {
        match change {
            Change::Equal {
                old_line,
                new_line,
                content,
            } => {
                let content_new = content.clone();
                rows.push(RenderRow {
                    old: Some(LineRef {
                        line_num: old_line,
                        content,
                        inline_spans: None,
                    }),
                    new: Some(LineRef {
                        line_num: new_line,
                        content: content_new,
                        inline_spans: None,
                    }),
                    kind: ChangeKind::Equal,
                });
            }
            Change::Delete { old_line, content } => {
                let mut deletes = Vec::new();
                let mut inserts = Vec::new();
                deletes.push((old_line, content));
                collect_change_run(&mut iter, &mut deletes, &mut inserts);
                emit_paired_changes(deletes, inserts, &mut rows);
            }
            Change::Insert { new_line, content } => {
                let mut deletes = Vec::new();
                let mut inserts = Vec::new();
                inserts.push((new_line, content));
                collect_change_run(&mut iter, &mut deletes, &mut inserts);
                emit_paired_changes(deletes, inserts, &mut rows);
            }
        }
    }

    rows
}

fn collect_change_run(
    iter: &mut std::iter::Peekable<std::vec::IntoIter<Change>>,
    deletes: &mut Vec<(usize, String)>,
    inserts: &mut Vec<(usize, String)>,
) {
    while let Some(next) = iter.peek() {
        match next {
            Change::Delete { .. } => {
                let Some(Change::Delete { old_line, content }) = iter.next() else {
                    unreachable!();
                };
                deletes.push((old_line, content));
            }
            Change::Insert { .. } => {
                let Some(Change::Insert { new_line, content }) = iter.next() else {
                    unreachable!();
                };
                inserts.push((new_line, content));
            }
            Change::Equal { .. } => break,
        }
    }
}

fn emit_paired_changes(
    deletes: Vec<(usize, String)>,
    inserts: Vec<(usize, String)>,
    rows: &mut Vec<RenderRow>,
) {
    let max_len = deletes.len().max(inserts.len());
    let mut del_iter = deletes.into_iter();
    let mut ins_iter = inserts.into_iter();

    for _ in 0..max_len {
        match (del_iter.next(), ins_iter.next()) {
            (Some((old_line, old_content)), Some((new_line, new_content))) => {
                let (old_spans, new_spans) = compute_inline_diff(&old_content, &new_content);
                rows.push(RenderRow {
                    old: Some(LineRef {
                        line_num: old_line,
                        content: old_content,
                        inline_spans: old_spans,
                    }),
                    new: Some(LineRef {
                        line_num: new_line,
                        content: new_content,
                        inline_spans: new_spans,
                    }),
                    kind: ChangeKind::Replace,
                });
            }
            (Some((old_line, content)), None) => {
                rows.push(RenderRow {
                    old: Some(LineRef {
                        line_num: old_line,
                        content,
                        inline_spans: None,
                    }),
                    new: None,
                    kind: ChangeKind::Delete,
                });
            }
            (None, Some((new_line, content))) => {
                rows.push(RenderRow {
                    old: None,
                    new: Some(LineRef {
                        line_num: new_line,
                        content,
                        inline_spans: None,
                    }),
                    kind: ChangeKind::Insert,
                });
            }
            (None, None) => break,
        }
    }
}

/// Check if a string contains meaningful (non-whitespace) content.
fn has_meaningful_content(s: &str) -> bool {
    s.chars().any(|c| !c.is_whitespace())
}

/// Max line length for computing inline diff (skip for very long lines).
const MAX_INLINE_DIFF_LEN: usize = 500;

/// Compute word-level inline diff between two lines.
/// Returns (old_spans, new_spans) or (None, None) if lines are too long.
fn compute_inline_diff(old: &str, new: &str) -> (Option<Vec<InlineSpan>>, Option<Vec<InlineSpan>>) {
    // Skip very long lines for performance
    if old.len() > MAX_INLINE_DIFF_LEN || new.len() > MAX_INLINE_DIFF_LEN {
        return (None, None);
    }

    // Skip if lines are identical (shouldn't happen for Replace, but defensive)
    if old == new {
        return (None, None);
    }

    let diff = TextDiff::configure().diff_unicode_words(old, new);

    let mut old_spans = Vec::new();
    let mut new_spans = Vec::new();
    let mut old_pos = 0usize;
    let mut new_pos = 0usize;
    let mut unchanged_len = 0usize;

    for change in diff.iter_all_changes() {
        let value = change.value();
        let len = value.len();

        match change.tag() {
            ChangeTag::Equal => {
                if has_meaningful_content(value) {
                    unchanged_len += value.trim().len();
                }
                // Unchanged content - exists in both
                old_spans.push(InlineSpan {
                    start: old_pos,
                    end: old_pos + len,
                    changed: false,
                });
                new_spans.push(InlineSpan {
                    start: new_pos,
                    end: new_pos + len,
                    changed: false,
                });
                old_pos += len;
                new_pos += len;
            }
            ChangeTag::Delete => {
                // Only in old
                old_spans.push(InlineSpan {
                    start: old_pos,
                    end: old_pos + len,
                    changed: true,
                });
                old_pos += len;
            }
            ChangeTag::Insert => {
                // Only in new
                new_spans.push(InlineSpan {
                    start: new_pos,
                    end: new_pos + len,
                    changed: true,
                });
                new_pos += len;
            }
        }
    }

    let total_len = old.trim().len().max(new.trim().len());
    const MIN_UNCHANGED_RATIO: f64 = 0.20;
    if total_len == 0 {
        return (None, None);
    }
    let total_len_f = f64::from(u32::try_from(total_len).unwrap_or(0));
    let unchanged_len_f = f64::from(u32::try_from(unchanged_len).unwrap_or(0));
    if total_len_f == 0.0 || (unchanged_len_f / total_len_f) < MIN_UNCHANGED_RATIO {
        return (None, None);
    }

    // Merge adjacent spans with same changed status for cleaner rendering
    let old_spans = merge_adjacent_spans(old_spans);
    let new_spans = merge_adjacent_spans(new_spans);

    (Some(old_spans), Some(new_spans))
}

/// Merge adjacent spans with the same `changed` status.
fn merge_adjacent_spans(spans: Vec<InlineSpan>) -> Vec<InlineSpan> {
    if spans.is_empty() {
        return spans;
    }

    let mut merged = Vec::with_capacity(spans.len());
    let mut current = spans[0].clone();

    for span in spans.into_iter().skip(1) {
        if span.changed == current.changed && span.start == current.end {
            // Extend current span
            current.end = span.end;
        } else {
            merged.push(current);
            current = span;
        }
    }
    merged.push(current);
    merged
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

    let mut old_min: Option<usize> = None;
    let mut old_max: usize = 0;
    let mut new_min: Option<usize> = None;
    let mut new_max: usize = 0;

    for row in slice {
        if let Some(old) = row.old.as_ref() {
            old_min = Some(old_min.map_or(old.line_num, |m| m.min(old.line_num)));
            old_max = old_max.max(old.line_num);
        }
        if let Some(new) = row.new.as_ref() {
            new_min = Some(new_min.map_or(new.line_num, |m| m.min(new.line_num)));
            new_max = new_max.max(new.line_num);
        }
    }

    let old_range = match old_min {
        Some(min) => (min, old_max - min + 1),
        None => (0, 0),
    };

    let new_range = match new_min {
        Some(min) => (min, new_max - min + 1),
        None => (0, 0),
    };

    Hunk {
        start_row: start,
        row_count: end - start,
        old_range,
        new_range,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Property-based tests
    mod proptest_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Diff of identical content should have no changes.
            #[test]
            fn identical_content_no_changes(content in "[a-z\n]{0,200}") {
                let buf = TextBuffer::new(content.as_bytes());
                let result = DiffResult::compute(&buf, &buf);
                prop_assert!(!result.has_changes());
            }

            /// Diff should be symmetric in detecting changes.
            #[test]
            fn diff_detects_changes(
                old in "[a-z]{0,50}",
                new in "[a-z]{0,50}"
            ) {
                let old_buf = TextBuffer::new(old.as_bytes());
                let new_buf = TextBuffer::new(new.as_bytes());
                let result = DiffResult::compute(&old_buf, &new_buf);

                // If content differs, there should be changes (unless both empty)
                if old != new && !(old.is_empty() && new.is_empty()) {
                    prop_assert!(result.has_changes() || result.row_count() == 0);
                }
            }

            /// Row count should be non-negative and reasonable.
            #[test]
            fn row_count_reasonable(
                old in "[a-z\n]{0,100}",
                new in "[a-z\n]{0,100}"
            ) {
                let old_buf = TextBuffer::new(old.as_bytes());
                let new_buf = TextBuffer::new(new.as_bytes());
                let result = DiffResult::compute(&old_buf, &new_buf);

                // Row count should be at most sum of lines (loose bound)
                let max_lines = old_buf.line_count() + new_buf.line_count() + 1;
                prop_assert!(result.row_count() <= max_lines);
            }

            /// Hunk navigation should be consistent.
            #[test]
            fn hunk_navigation_consistent(
                old in "[a-z\n]{0,50}",
                new in "[a-z\n]{0,50}"
            ) {
                let old_buf = TextBuffer::new(old.as_bytes());
                let new_buf = TextBuffer::new(new.as_bytes());
                let result = DiffResult::compute(&old_buf, &new_buf);

                // next_hunk from start should give first hunk (if any)
                if let Some(first) = result.next_hunk_row(0) {
                    prop_assert!(first < result.row_count());
                }

                // prev_hunk from end should give last hunk (if any)
                if result.row_count() > 0 {
                    if let Some(last) = result.prev_hunk_row(result.row_count()) {
                        prop_assert!(last < result.row_count());
                    }
                }
            }
        }
    }

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
    fn inline_diff_similarity_gate() {
        let old = TextBuffer::new(b"alpha beta gamma\n");
        let new = TextBuffer::new(b"delta epsilon zeta\n");
        let result = DiffResult::compute(&old, &new);

        let replace = result
            .rows
            .iter()
            .find(|r| r.kind == ChangeKind::Replace)
            .unwrap();
        assert!(replace.old.as_ref().unwrap().inline_spans.is_none());
        assert!(replace.new.as_ref().unwrap().inline_spans.is_none());
    }

    #[test]
    fn inline_diff_similarity_kept() {
        let old = TextBuffer::new(b"let value = 1\n");
        let new = TextBuffer::new(b"let value = 2\n");
        let result = DiffResult::compute(&old, &new);

        let replace = result
            .rows
            .iter()
            .find(|r| r.kind == ChangeKind::Replace)
            .unwrap();
        let old_spans = replace.old.as_ref().unwrap().inline_spans.as_ref().unwrap();
        let new_spans = replace.new.as_ref().unwrap().inline_spans.as_ref().unwrap();
        assert!(old_spans.iter().any(|s| s.changed));
        assert!(new_spans.iter().any(|s| s.changed));
    }

    #[test]
    fn inline_diff_unicode_words() {
        let old = TextBuffer::new("let cafe = \"naive\"\n".as_bytes());
        let new = TextBuffer::new("let caf\u{00E9} = \"naive\"\n".as_bytes());
        let result = DiffResult::compute(&old, &new);

        let replace = result
            .rows
            .iter()
            .find(|r| r.kind == ChangeKind::Replace)
            .unwrap();
        let new_spans = replace.new.as_ref().unwrap().inline_spans.as_ref().unwrap();
        assert!(new_spans.iter().any(|s| s.changed));
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

    #[test]
    fn hunk_at_row_basic() {
        let old = TextBuffer::new(b"line1\nold\nline3\n");
        let new = TextBuffer::new(b"line1\nnew\nline3\n");
        let result = DiffResult::compute(&old, &new);

        assert!(!result.hunks.is_empty());
        let hunk = &result.hunks[0];

        // Row within hunk should return Some(0)
        assert_eq!(result.hunk_at_row(hunk.start_row), Some(0));
        assert_eq!(
            result.hunk_at_row(hunk.start_row + hunk.row_count - 1),
            Some(0)
        );
    }

    #[test]
    fn hunk_at_row_outside() {
        let old = TextBuffer::new(b"same\n");
        let new = TextBuffer::new(b"same\n");
        let result = DiffResult::compute(&old, &new);

        // No hunks, so any row should return None
        assert!(result.hunk_at_row(0).is_none());
    }
}
