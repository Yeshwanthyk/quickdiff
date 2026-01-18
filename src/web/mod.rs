//! Web preview generation for quickdiff.
//!
//! Generates standalone HTML using `@pierre/diffs` for client-side rendering.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::core::{
    diff_source_display, get_pr_diff, list_changed_files, parse_unified_diff, DiffSource,
    FileChangeKind, RepoRoot,
};

/// Review data for web template rendering.
#[derive(Serialize)]
pub struct ReviewData {
    /// Branch name.
    pub branch: String,
    /// Short commit hash.
    pub commit: String,
    /// Summary/title.
    pub summary: String,
    /// Full unified diff patch.
    pub patch: String,
    /// List of changed files with stats.
    pub files: Vec<ReviewFile>,
    /// Aggregate statistics.
    pub stats: ReviewStats,
}

/// A file in the review with its statistics.
#[derive(Serialize)]
pub struct ReviewFile {
    /// File path.
    pub path: String,
    /// File summary (empty for now).
    pub summary: String,
    /// Lines added.
    pub additions: usize,
    /// Lines deleted.
    pub deletions: usize,
    /// Comments on this file.
    pub comments: Vec<ReviewComment>,
}

/// A comment on a file.
#[derive(Serialize)]
pub struct ReviewComment {
    /// Start line number.
    #[serde(rename = "startLine")]
    pub start_line: usize,
    /// End line number.
    #[serde(rename = "endLine")]
    pub end_line: usize,
    /// Comment type (bug, warning, suggestion, good).
    #[serde(rename = "type")]
    pub kind: String,
    /// Comment text.
    pub text: String,
}

/// Aggregate review statistics.
#[derive(Serialize, Default)]
pub struct ReviewStats {
    /// Number of bugs found.
    pub bugs: usize,
    /// Number of warnings.
    pub warnings: usize,
    /// Number of suggestions.
    pub suggestions: usize,
    /// Number of positive notes.
    pub good: usize,
}

/// Input configuration for web preview generation.
pub struct WebInput {
    /// Diff source specification.
    pub source: DiffSource,
    /// Pre-loaded patch from stdin (if any).
    pub stdin_patch: Option<String>,
    /// File path filter.
    pub file_filter: Option<String>,
    /// Display label for the diff.
    pub label: String,
}

/// Build review data from the given input.
pub fn build_review_data(repo: &RepoRoot, input: WebInput) -> Result<ReviewData> {
    let patch = if let Some(patch) = input.stdin_patch {
        patch
    } else {
        build_patch_from_source(repo, &input.source, input.file_filter.as_deref())?
    };

    let files = parse_unified_diff(&patch);
    let review_files = files
        .iter()
        .map(|f| ReviewFile {
            path: f.path.as_str().to_string(),
            summary: String::new(),
            additions: f.additions,
            deletions: f.deletions,
            comments: Vec::new(),
        })
        .collect::<Vec<_>>();

    let summary = if input.label.is_empty() {
        diff_source_display(&input.source, repo)
    } else {
        input.label
    };

    Ok(ReviewData {
        branch: current_git_branch(repo.path()).unwrap_or_else(|| summary.clone()),
        commit: current_git_short(repo.path()).unwrap_or_default(),
        summary,
        patch,
        files: review_files,
        stats: ReviewStats::default(),
    })
}

fn build_patch_from_source(
    repo: &RepoRoot,
    source: &DiffSource,
    file_filter: Option<&str>,
) -> Result<String> {
    if repo.is_jj() {
        return build_jj_patch(repo, source, file_filter);
    }
    build_git_patch(repo, source, file_filter)
}

fn build_git_patch(
    repo: &RepoRoot,
    source: &DiffSource,
    file_filter: Option<&str>,
) -> Result<String> {
    let base = repo.path();
    let mut patch = match source {
        DiffSource::WorkingTree => run_git(base, &["diff", "--no-color", "HEAD"])?,
        DiffSource::Commit(commit) => {
            let parent = crate::core::get_parent_revision(repo, commit)?;
            run_git(base, &["diff", "--no-color", &parent, commit])?
        }
        DiffSource::Range { from, to } => run_git(base, &["diff", "--no-color", from, to])?,
        DiffSource::Base(base_ref) => {
            let merge_base = crate::core::resolve_merge_base(repo, base_ref)?;
            run_git(base, &["diff", "--no-color", &merge_base])?
        }
        DiffSource::PullRequest { number, .. } => {
            get_pr_diff(base, *number).map_err(|e| anyhow::anyhow!(e.to_string()))?
        }
    };

    patch = append_untracked(repo, patch)?;
    Ok(apply_file_filter(patch, file_filter))
}

fn append_untracked(repo: &RepoRoot, mut patch: String) -> Result<String> {
    let files = list_changed_files(repo)?;
    for f in files
        .into_iter()
        .filter(|f| f.kind == FileChangeKind::Untracked)
    {
        let path = repo.path().join(f.path.as_str());
        let path_str = path.to_str().unwrap_or("");
        // Use --no-index to diff against /dev/null for untracked files
        let extra = run_git(
            repo.path(),
            &["diff", "--no-color", "--no-index", "/dev/null", path_str],
        );
        if let Ok(extra) = extra {
            if !extra.trim().is_empty() {
                patch.push('\n');
                patch.push_str(&extra);
            }
        }
    }
    Ok(patch)
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .with_context(|| format!("git {:?} failed", args))?;

    // For diff commands, exit code 1 means differences found (not an error)
    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            return Err(anyhow::anyhow!(stderr.trim().to_string()));
        }
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn build_jj_patch(
    repo: &RepoRoot,
    source: &DiffSource,
    file_filter: Option<&str>,
) -> Result<String> {
    let base = repo.path();
    let args: Vec<&str> = match source {
        DiffSource::WorkingTree => vec!["diff", "--git"],
        DiffSource::Commit(commit) => vec!["diff", "--git", "-r", commit],
        DiffSource::Range { from, to } => {
            // jj range syntax
            return run_jj_range(base, from, to, file_filter);
        }
        DiffSource::Base(base_ref) => {
            // jj diff from base to working copy
            return run_jj_range(base, base_ref, "@", file_filter);
        }
        DiffSource::PullRequest { .. } => {
            return Err(anyhow::anyhow!("PR web mode requires git"));
        }
    };

    let output = Command::new("jj")
        .args(&args)
        .current_dir(base)
        .output()
        .context("jj diff failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(stderr.trim().to_string()));
    }

    let patch = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(apply_file_filter(patch, file_filter))
}

fn run_jj_range(base: &Path, from: &str, to: &str, file_filter: Option<&str>) -> Result<String> {
    let range = format!("{}..{}", from, to);
    let output = Command::new("jj")
        .args(["diff", "--git", "-r", &range])
        .current_dir(base)
        .output()
        .context("jj diff failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(stderr.trim().to_string()));
    }

    let patch = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(apply_file_filter(patch, file_filter))
}

fn apply_file_filter(patch: String, filter: Option<&str>) -> String {
    let Some(filter) = filter else {
        return patch;
    };

    let files = parse_unified_diff(&patch);
    let filtered: Vec<_> = files
        .into_iter()
        .filter(|f| f.path.as_str().contains(filter))
        .map(|f| f.patch)
        .collect();

    filtered.join("\n")
}

fn current_git_branch(repo: &Path) -> Option<String> {
    run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
}

fn current_git_short(repo: &Path) -> Option<String> {
    run_git(repo, &["rev-parse", "--short", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_file_filter_keeps_matching_files() {
        let patch = r#"diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1 +1 @@
-old
+new
diff --git a/tests/test.rs b/tests/test.rs
--- a/tests/test.rs
+++ b/tests/test.rs
@@ -1 +1 @@
-test old
+test new
"#;
        let filtered = apply_file_filter(patch.to_string(), Some("src/"));
        assert!(filtered.contains("src/main.rs"));
        assert!(!filtered.contains("tests/test.rs"));
    }

    #[test]
    fn apply_file_filter_none_returns_all() {
        let patch = "diff --git a/file.rs b/file.rs\n";
        let result = apply_file_filter(patch.to_string(), None);
        assert_eq!(result, patch);
    }
}
