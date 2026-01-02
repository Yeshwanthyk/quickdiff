//! Parser for unified diff output from `gh pr diff`.

use crate::core::{FileChangeKind, RelPath};

/// A file changed in a PR with its patch content.
#[derive(Debug, Clone)]
pub struct PRChangedFile {
    /// Path to the file.
    pub path: RelPath,
    /// Original path (for renames).
    pub old_path: Option<RelPath>,
    /// Type of change.
    pub kind: FileChangeKind,
    /// Number of lines added.
    pub additions: usize,
    /// Number of lines deleted.
    pub deletions: usize,
    /// Raw unified diff patch for this file.
    pub patch: String,
}

/// Parse unified diff output into a list of changed files.
///
/// Splits on `diff --git` boundaries and extracts file info + patch.
pub fn parse_unified_diff(raw_diff: &str) -> Vec<PRChangedFile> {
    let mut files = Vec::new();

    // Split on "diff --git " but keep the delimiter
    let chunks: Vec<&str> = raw_diff.split("diff --git ").collect();

    for chunk in chunks.iter().skip(1) {
        // Skip empty chunks
        if chunk.trim().is_empty() {
            continue;
        }

        // Reconstruct full chunk for patch
        let full_chunk = format!("diff --git {}", chunk);

        if let Some(file) = parse_file_chunk(&full_chunk) {
            files.push(file);
        }
    }

    // Sort by path for consistent ordering
    files.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    files
}

fn parse_file_chunk(chunk: &str) -> Option<PRChangedFile> {
    let lines: Vec<&str> = chunk.lines().collect();
    let first_line = lines.first()?;

    // Parse header: diff --git a/path b/path
    let header_match = parse_diff_header(first_line)?;
    let (old_path_str, new_path_str) = header_match;

    // Determine status from diff metadata
    let mut kind = FileChangeKind::Modified;
    let mut old_path = None;

    for line in lines.iter().take(10) {
        if line.starts_with("new file mode") {
            kind = FileChangeKind::Added;
        } else if line.starts_with("deleted file mode") {
            kind = FileChangeKind::Deleted;
        } else if line.starts_with("rename from ") {
            kind = FileChangeKind::Renamed;
            let from = line.strip_prefix("rename from ").unwrap_or("");
            old_path = Some(RelPath::new(from));
        }
    }

    // For renames without explicit "rename from", use the a/ path
    if kind == FileChangeKind::Renamed && old_path.is_none() && old_path_str != new_path_str {
        old_path = Some(RelPath::new(old_path_str));
    }

    // Count additions and deletions
    let mut additions = 0;
    let mut deletions = 0;
    let mut in_hunk = false;

    for line in &lines {
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }

        if in_hunk {
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }
    }

    Some(PRChangedFile {
        path: RelPath::new(new_path_str),
        old_path,
        kind,
        additions,
        deletions,
        patch: chunk.to_string(),
    })
}

/// Parse "diff --git a/path b/path" header.
/// Returns (old_path, new_path) without a/ b/ prefixes.
fn parse_diff_header(line: &str) -> Option<(&str, &str)> {
    // Format: "diff --git a/old/path b/new/path"
    let rest = line.strip_prefix("diff --git ")?;

    // Find the split point - look for " b/" pattern
    // Handle paths with spaces by finding last " b/" occurrence
    let b_idx = rest.rfind(" b/")?;

    let a_part = &rest[..b_idx];
    let b_part = &rest[b_idx + 1..];

    // Strip "a/" and "b/" prefixes
    let old_path = a_part.strip_prefix("a/").unwrap_or(a_part);
    let new_path = b_part.strip_prefix("b/").unwrap_or(b_part);

    Some((old_path, new_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_diff() {
        let diff = r#"diff --git a/src/main.rs b/src/main.rs
index abc123..def456 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!("Hello");
     println!("World");
 }
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "src/main.rs");
        assert_eq!(files[0].kind, FileChangeKind::Modified);
        assert_eq!(files[0].additions, 1);
        assert_eq!(files[0].deletions, 0);
    }

    #[test]
    fn parse_new_file() {
        let diff = r#"diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..abc123
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+line 1
+line 2
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "new.txt");
        assert_eq!(files[0].kind, FileChangeKind::Added);
        assert_eq!(files[0].additions, 2);
    }

    #[test]
    fn parse_deleted_file() {
        let diff = r#"diff --git a/old.txt b/old.txt
deleted file mode 100644
index abc123..0000000
--- a/old.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-line 1
-line 2
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "old.txt");
        assert_eq!(files[0].kind, FileChangeKind::Deleted);
        assert_eq!(files[0].deletions, 2);
    }

    #[test]
    fn parse_rename() {
        let diff = r#"diff --git a/old_name.rs b/new_name.rs
similarity index 95%
rename from old_name.rs
rename to new_name.rs
index abc123..def456 100644
--- a/old_name.rs
+++ b/new_name.rs
@@ -1,3 +1,3 @@
 fn main() {
-    old();
+    new();
 }
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "new_name.rs");
        assert_eq!(files[0].kind, FileChangeKind::Renamed);
        assert_eq!(
            files[0].old_path.as_ref().map(|p| p.as_str()),
            Some("old_name.rs")
        );
    }

    #[test]
    fn parse_multiple_files() {
        let diff = r#"diff --git a/a.rs b/a.rs
index 111..222 100644
--- a/a.rs
+++ b/a.rs
@@ -1 +1 @@
-old
+new
diff --git a/b.rs b/b.rs
index 333..444 100644
--- a/b.rs
+++ b/b.rs
@@ -1 +1,2 @@
 existing
+added
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 2);
        // Sorted by path
        assert_eq!(files[0].path.as_str(), "a.rs");
        assert_eq!(files[1].path.as_str(), "b.rs");
    }

    #[test]
    fn parse_diff_header_simple() {
        let header = "diff --git a/src/main.rs b/src/main.rs";
        let (old, new) = parse_diff_header(header).unwrap();
        assert_eq!(old, "src/main.rs");
        assert_eq!(new, "src/main.rs");
    }

    #[test]
    fn parse_diff_header_rename() {
        let header = "diff --git a/old/path.rs b/new/path.rs";
        let (old, new) = parse_diff_header(header).unwrap();
        assert_eq!(old, "old/path.rs");
        assert_eq!(new, "new/path.rs");
    }
}
