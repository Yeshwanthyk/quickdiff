//! GitHub CLI (`gh`) wrapper for PR operations.

use std::io::Read;
use std::process::{Command, Stdio};

use serde::Deserialize;
use thiserror::Error;

/// Maximum diff size to load (50 MiB). Matches repo::MAX_FILE_SIZE.
pub const MAX_DIFF_SIZE: usize = 50 * 1024 * 1024;

/// Errors from GitHub CLI operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GhError {
    /// `gh` CLI not found or not authenticated.
    #[error("GitHub CLI not available or not authenticated. Run 'gh auth login'")]
    NotAvailable,
    /// GitHub API/CLI error.
    #[error("GitHub error: {0}")]
    ApiError(String),
    /// Failed to parse CLI output.
    #[error("Failed to parse gh output: {0}")]
    ParseError(String),
    /// I/O error running command.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Diff exceeds maximum allowed size.
    #[error("PR diff too large: exceeded {max} bytes")]
    DiffTooLarge {
        /// Maximum allowed size.
        max: usize,
    },
}

/// Filter for PR listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PRFilter {
    /// All open PRs.
    #[default]
    All,
    /// PRs authored by current user.
    Mine,
    /// PRs where review is requested from current user.
    ReviewRequested,
}

/// PR author info.
#[derive(Debug, Clone, Deserialize)]
pub struct PRAuthor {
    /// GitHub username.
    pub login: String,
}

/// A pull request summary (from list).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    /// PR number.
    pub number: u32,
    /// PR title.
    pub title: String,
    /// Head branch name.
    pub head_ref_name: String,
    /// Base branch name.
    pub base_ref_name: String,
    /// PR author.
    pub author: PRAuthor,
    /// Lines added.
    pub additions: u32,
    /// Lines deleted.
    pub deletions: u32,
    /// Number of files changed.
    pub changed_files: u32,
    /// Whether this is a draft PR.
    #[serde(default)]
    pub is_draft: bool,
}

/// Check if `gh` CLI is available and authenticated.
pub fn is_gh_available() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// List open (non-draft) PRs for the repository at `repo_path`.
pub fn list_prs(
    repo_path: &std::path::Path,
    filter: PRFilter,
) -> Result<Vec<PullRequest>, GhError> {
    let mut args = vec![
        "pr",
        "list",
        "--state",
        "open",
        "--json",
        "number,title,headRefName,baseRefName,author,additions,deletions,changedFiles,isDraft",
    ];

    match filter {
        PRFilter::All => {}
        PRFilter::Mine => {
            args.push("--author");
            args.push("@me");
        }
        PRFilter::ReviewRequested => {
            args.push("--search");
            args.push("review-requested:@me");
        }
    }

    let output = Command::new("gh")
        .args(&args)
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("auth") || stderr.contains("login") {
            return Err(GhError::NotAvailable);
        }
        return Err(GhError::ApiError(stderr.trim().to_string()));
    }

    let prs: Vec<PullRequest> =
        serde_json::from_slice(&output.stdout).map_err(|e| GhError::ParseError(e.to_string()))?;

    // Filter out drafts
    Ok(prs.into_iter().filter(|pr| !pr.is_draft).collect())
}

/// Get the unified diff for a PR.
/// Returns error if diff exceeds `MAX_DIFF_SIZE`.
pub fn get_pr_diff(repo_path: &std::path::Path, pr_number: u32) -> Result<String, GhError> {
    let mut child = Command::new("gh")
        .args(["pr", "diff", &pr_number.to_string()])
        .current_dir(repo_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Read with a hard cap to prevent OOM
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut buffer = Vec::with_capacity(64 * 1024); // 64KB initial
    let mut total_read = 0;
    let mut temp = [0u8; 8192];

    loop {
        match stdout.read(&mut temp) {
            Ok(0) => break,
            Ok(n) => {
                total_read += n;
                if total_read > MAX_DIFF_SIZE {
                    // Kill the process and return error
                    let _ = child.kill();
                    return Err(GhError::DiffTooLarge { max: MAX_DIFF_SIZE });
                }
                buffer.extend_from_slice(&temp[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }

    let status = child.wait()?;
    if !status.success() {
        // Try to read stderr for error message
        let mut stderr = child.stderr.take().expect("stderr piped");
        let mut stderr_buf = Vec::new();
        let _ = stderr.read_to_end(&mut stderr_buf);
        let stderr_str = String::from_utf8_lossy(&stderr_buf);
        return Err(GhError::ApiError(stderr_str.trim().to_string()));
    }

    String::from_utf8(buffer).map_err(|e| GhError::ParseError(format!("Invalid UTF-8: {}", e)))
}

/// Approve a PR.
pub fn approve_pr(
    repo_path: &std::path::Path,
    pr_number: u32,
    body: Option<&str>,
) -> Result<(), GhError> {
    let pr_num_str = pr_number.to_string();
    let mut args = vec!["pr", "review", &pr_num_str, "--approve"];

    let body_owned;
    if let Some(b) = body {
        body_owned = b.to_string();
        args.push("-b");
        args.push(&body_owned);
    }

    let output = Command::new("gh")
        .args(&args)
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError::ApiError(stderr.trim().to_string()));
    }

    Ok(())
}

/// Add a comment review to a PR.
pub fn comment_pr(repo_path: &std::path::Path, pr_number: u32, body: &str) -> Result<(), GhError> {
    let pr_num_str = pr_number.to_string();
    let output = Command::new("gh")
        .args(["pr", "review", &pr_num_str, "--comment", "-b", body])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError::ApiError(stderr.trim().to_string()));
    }

    Ok(())
}

/// Request changes on a PR.
pub fn request_changes_pr(
    repo_path: &std::path::Path,
    pr_number: u32,
    body: &str,
) -> Result<(), GhError> {
    let pr_num_str = pr_number.to_string();
    let output = Command::new("gh")
        .args(["pr", "review", &pr_num_str, "--request-changes", "-b", body])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GhError::ApiError(stderr.trim().to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_filter_default_is_all() {
        assert_eq!(PRFilter::default(), PRFilter::All);
    }
}
