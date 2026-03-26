//! Helpers for stdin-backed diff and pager workflows.

use std::io::{self, Read};

/// Read all stdin into a string.
pub fn read_stdin_text() -> io::Result<String> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    Ok(text)
}

/// Heuristic unified-diff detector.
pub fn looks_like_unified_diff(text: &str) -> bool {
    let mut saw_file_header = false;
    let mut saw_hunk = false;

    for line in text.lines().take(200) {
        if line.starts_with("diff --git ") || line.starts_with("--- ") || line.starts_with("+++ ") {
            saw_file_header = true;
        }
        if line.starts_with("@@ ") || line.starts_with("@@-") || line.starts_with("@@") {
            saw_hunk = true;
        }
        if saw_file_header && saw_hunk {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unified_diff() {
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        assert!(looks_like_unified_diff(diff));
    }

    #[test]
    fn rejects_plain_text() {
        assert!(!looks_like_unified_diff("hello\nworld\n"));
    }
}
