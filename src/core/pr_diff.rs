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
/// Parses line-by-line to avoid false splits on file content containing
/// "diff --git" strings.
pub fn parse_unified_diff(raw_diff: &str) -> Vec<PRChangedFile> {
    let mut files = Vec::new();
    let mut current_chunk = String::new();

    for line in raw_diff.lines() {
        // "diff --git" at the start of a line marks a new file boundary
        if line.starts_with("diff --git ") {
            // Process previous chunk if non-empty
            if !current_chunk.is_empty() {
                if let Some(file) = parse_file_chunk(&current_chunk) {
                    files.push(file);
                }
            }
            current_chunk = line.to_string();
            current_chunk.push('\n');
        } else {
            current_chunk.push_str(line);
            current_chunk.push('\n');
        }
    }

    // Process final chunk
    if !current_chunk.is_empty() {
        if let Some(file) = parse_file_chunk(&current_chunk) {
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
    let (old_path_str, new_path_str) = parse_diff_header(first_line)?;

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

/// Unquote a C-style quoted string if present.
/// Git uses C-style quoting for paths with special chars.
fn unquote_path(s: &str) -> String {
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        // Basic C-escape handling
        let mut result = String::with_capacity(inner.len());
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => result.push('\n'),
                    Some('t') => result.push('\t'),
                    Some('\\') => result.push('\\'),
                    Some('"') => result.push('"'),
                    Some(other) => {
                        result.push('\\');
                        result.push(other);
                    }
                    None => result.push('\\'),
                }
            } else {
                result.push(c);
            }
        }
        result
    } else {
        s.to_string()
    }
}

/// Parse "diff --git a/path b/path" header.
/// Returns (old_path, new_path) without a/ b/ prefixes.
/// Handles quoted paths for special characters.
fn parse_diff_header(line: &str) -> Option<(String, String)> {
    // Format: "diff --git a/old/path b/new/path"
    // Or with quotes: diff --git "a/path with spaces" "b/path with spaces"
    let rest = line.strip_prefix("diff --git ")?;

    // Try to handle quoted paths first
    if rest.starts_with('"') {
        // Find matching quote for first path
        let first_end = find_closing_quote(rest, 0)?;
        let first_quoted = &rest[..=first_end];

        // Skip space and find second quoted path
        let remainder = rest.get(first_end + 2..)?;
        if remainder.starts_with('"') {
            let second_end = find_closing_quote(remainder, 0)?;
            let second_quoted = &remainder[..=second_end];

            let old_path = unquote_path(first_quoted);
            let new_path = unquote_path(second_quoted);

            // Strip a/ and b/ prefixes
            let old_path = old_path.strip_prefix("a/").unwrap_or(&old_path).to_string();
            let new_path = new_path.strip_prefix("b/").unwrap_or(&new_path).to_string();

            return Some((old_path, new_path));
        }
    }

    // Non-quoted paths: find the split point - look for " b/" pattern
    // Handle paths with spaces by finding last " b/" occurrence
    let b_idx = rest.rfind(" b/")?;

    let a_part = &rest[..b_idx];
    let b_part = &rest[b_idx + 1..];

    // Strip "a/" and "b/" prefixes
    let old_path = a_part.strip_prefix("a/").unwrap_or(a_part);
    let new_path = b_part.strip_prefix("b/").unwrap_or(b_part);

    Some((old_path.to_string(), new_path.to_string()))
}

/// Find the index of the closing quote, accounting for escapes.
fn find_closing_quote(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        return None;
    }
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // Skip escaped char
        } else if bytes[i] == b'"' {
            return Some(i);
        } else {
            i += 1;
        }
    }
    None
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

    #[test]
    fn parse_file_content_with_diff_git_line() {
        // A file containing "diff --git" in its content should not cause false splits
        let diff = r#"diff --git a/test.md b/test.md
index abc123..def456 100644
--- a/test.md
+++ b/test.md
@@ -1,3 +1,5 @@
 # Example
+This line shows: diff --git a/fake b/fake
+Another line with diff --git in content
 End of file
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1); // Should be 1 file, not 3
        assert_eq!(files[0].path.as_str(), "test.md");
        assert_eq!(files[0].additions, 2);
    }

    #[test]
    fn parse_quoted_path() {
        // Git quotes paths with special characters
        let diff = r#"diff --git "a/path with spaces.txt" "b/path with spaces.txt"
new file mode 100644
index 0000000..abc123
--- /dev/null
+++ "b/path with spaces.txt"
@@ -0,0 +1 @@
+content
"#;
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.as_str(), "path with spaces.txt");
        assert_eq!(files[0].kind, FileChangeKind::Added);
    }

    #[test]
    fn unquote_path_escapes() {
        assert_eq!(unquote_path(r#""simple""#), "simple");
        assert_eq!(unquote_path(r#""with\\backslash""#), "with\\backslash");
        assert_eq!(unquote_path(r#""with\ttab""#), "with\ttab");
        assert_eq!(unquote_path(r#""with\"quote""#), "with\"quote");
        // Unquoted strings pass through
        assert_eq!(unquote_path("unquoted"), "unquoted");
    }
}
