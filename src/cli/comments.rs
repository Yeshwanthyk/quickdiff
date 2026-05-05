//! CLI commands for comment management.

use std::collections::{HashMap, HashSet};
use std::process::ExitCode;

use serde::Deserialize;

use crate::core::{
    Anchor, ChangeKind, ChangedFile, Comment, CommentContext, CommentStatus, CommentStore,
    DiffResult, DiffSource, FileCommentStore, RelPath, RepoError, RepoRoot, Selector, TextBuffer,
    format_anchor_summary, list_changed_files, list_changed_files_between,
    list_changed_files_from_base_with_merge_base, list_commit_files, load_diff_contents,
    resolve_revision, selector_from_hunk,
};

/// Run a comments subcommand.
/// Returns ExitCode for the process.
pub fn run_comments_command(repo: &RepoRoot, args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: quickdiff comments <command>");
        eprintln!("Commands:");
        eprintln!(
            "  list [--all] [--json] [--path <path>] [--worktree|--base <ref>|--commit <rev>|--range <from>..<to>]"
        );
        eprintln!(
            "  add  [--worktree|--base <ref>|--commit <rev>|--range <from>..<to>] --path <path> (--hunk <n>|--old-line <n>|--new-line <n>) --message <text>"
        );
        eprintln!(
            "  import --json <file> [--worktree|--base <ref>|--commit <rev>|--range <from>..<to>]"
        );
        eprintln!("  next [--json] [--worktree|--base <ref>|--commit <rev>|--range <from>..<to>]");
        eprintln!("  resolve <id>");
        return ExitCode::from(1);
    }

    let cmd = args[0].as_str();
    let cmd_args = &args[1..];

    match cmd {
        "list" => cmd_list(repo, cmd_args),
        "add" => cmd_add(repo, cmd_args),
        "import" => cmd_import(repo, cmd_args),
        "next" => cmd_next(repo, cmd_args),
        "resolve" => cmd_resolve(repo, cmd_args),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            ExitCode::from(1)
        }
    }
}

/// Flags that take a value argument. Used to skip values when parsing.
const FLAGS_WITH_VALUES: &[&str] = &[
    "--base",
    "--commit",
    "--range",
    "--path",
    "--hunk",
    "--old-line",
    "--new-line",
    "--message",
    "--json",
    "-m",
];

/// Check if arg is a known flag that takes a value.
fn takes_value(arg: &str) -> bool {
    let flag_name = arg.split_once('=').map_or(arg, |(name, _)| name);
    FLAGS_WITH_VALUES.contains(&flag_name)
}

fn parse_context(
    repo: &RepoRoot,
    args: &[String],
) -> Result<Option<(CommentContext, DiffSource)>, String> {
    let mut worktree = false;
    let mut base: Option<String> = None;
    let mut commit: Option<String> = None;
    let mut range: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--worktree" => {
                worktree = true;
            }
            "--base" => {
                i += 1;
                if i >= args.len() {
                    return Err("--base requires a value".to_string());
                }
                base = Some(args[i].clone());
            }
            "--commit" => {
                i += 1;
                if i >= args.len() {
                    return Err("--commit requires a value".to_string());
                }
                commit = Some(args[i].clone());
            }
            "--range" => {
                i += 1;
                if i >= args.len() {
                    return Err("--range requires a value".to_string());
                }
                range = Some(args[i].clone());
            }
            // Skip values for other known flags to avoid misinterpreting them
            other if takes_value(other) => {
                i += 1; // Skip the value
            }
            _ => {}
        }
        i += 1;
    }

    let set_count =
        worktree as u8 + base.is_some() as u8 + commit.is_some() as u8 + range.is_some() as u8;
    if set_count == 0 {
        return Ok(None);
    }
    if set_count > 1 {
        return Err("Only one of --worktree/--base/--commit/--range may be specified".to_string());
    }

    if worktree {
        return Ok(Some((CommentContext::Worktree, DiffSource::WorkingTree)));
    }

    if let Some(base) = base {
        return Ok(Some((
            CommentContext::Base { base: base.clone() },
            DiffSource::Base(base),
        )));
    }

    if let Some(commit) = commit {
        let sha = resolve_revision(repo, &commit)
            .map_err(|e| format!("Failed to resolve commit {}: {}", commit, e))?;
        return Ok(Some((
            CommentContext::Commit {
                commit: sha.clone(),
            },
            DiffSource::Commit(sha),
        )));
    }

    if let Some(range) = range {
        let (mut from, mut to) = parse_range(&range)?;
        let default_ref = repo.working_copy_parent_ref();
        if from.is_empty() {
            from = default_ref.to_string();
        }
        if to.is_empty() {
            to = default_ref.to_string();
        }

        let from_sha = resolve_revision(repo, &from)
            .map_err(|e| format!("Failed to resolve revision {}: {}", from, e))?;
        let to_sha = resolve_revision(repo, &to)
            .map_err(|e| format!("Failed to resolve revision {}: {}", to, e))?;
        return Ok(Some((
            CommentContext::Range {
                from: from_sha.clone(),
                to: to_sha.clone(),
            },
            DiffSource::Range {
                from: from_sha,
                to: to_sha,
            },
        )));
    }

    Ok(None)
}

fn parse_range(s: &str) -> Result<(String, String), String> {
    let Some(idx) = s.find("..") else {
        return Err("--range must contain '..' (e.g. a..b)".to_string());
    };

    let from = &s[..idx];
    let to = &s[idx + 2..];

    let to = to.strip_prefix('.').unwrap_or(to);

    Ok((from.to_string(), to.to_string()))
}

fn list_files_for_source(
    repo: &RepoRoot,
    source: &DiffSource,
) -> Result<(Vec<ChangedFile>, Option<String>), RepoError> {
    match source {
        DiffSource::WorkingTree => Ok((list_changed_files(repo)?, None)),
        DiffSource::Commit(commit) => Ok((list_commit_files(repo, commit)?, None)),
        DiffSource::Range { from, to } => Ok((list_changed_files_between(repo, from, to)?, None)),
        DiffSource::Base(base) => {
            let result = list_changed_files_from_base_with_merge_base(repo, base)?;
            Ok((result.files, Some(result.merge_base)))
        }
        DiffSource::FilePair { .. } | DiffSource::DiffTool { .. } => Ok((Vec::new(), None)),
        DiffSource::PullRequest { .. } => {
            // PR files come from parsed diff output, not this function
            Ok((Vec::new(), None))
        }
    }
}

fn context_summary(ctx: &CommentContext) -> String {
    match ctx {
        CommentContext::Unscoped => "any".to_string(),
        CommentContext::Worktree => "worktree".to_string(),
        CommentContext::Base { base } => format!("base:{}", base),
        CommentContext::Commit { commit } => {
            let short: String = commit.chars().take(7).collect();
            format!("commit:{}", short)
        }
        CommentContext::Range { from, to } => {
            let short_from: String = from.chars().take(7).collect();
            let short_to: String = to.chars().take(7).collect();
            format!("range:{}..{}", short_from, short_to)
        }
    }
}

/// List comments.
fn cmd_list(repo: &RepoRoot, args: &[String]) -> ExitCode {
    let mut include_resolved = false;
    let mut json_output = false;
    let mut filter_path: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--all" => include_resolved = true,
            "--json" => json_output = true,
            "--path" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--path requires a value");
                    return ExitCode::from(1);
                }
                filter_path = Some(args[i].clone());
            }
            // Skip values for other known flags
            other if takes_value(other) => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let context_filter = match parse_context(repo, args) {
        Ok(v) => v.map(|(ctx, _src)| ctx),
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::from(1);
        }
    };

    let store = match FileCommentStore::open(repo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open comment store: {}", e);
            return ExitCode::from(1);
        }
    };

    let mut comments: Vec<_> = if let Some(ref path) = filter_path {
        let rel_path = match RelPath::try_new(path.clone()) {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Invalid path: must be relative (not absolute): {}", path);
                return ExitCode::from(1);
            }
        };
        store.list_for_path(&rel_path, include_resolved)
    } else {
        store.list(include_resolved)
    };

    if let Some(ctx) = context_filter {
        comments.retain(|c| c.context.matches(&ctx));
    }

    if json_output {
        let json_comments: Vec<_> = comments
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "path": c.path.as_str(),
                    "context": &c.context,
                    "context_summary": context_summary(&c.context),
                    "status": c.status,
                    "message": &c.message,
                    "anchor": &c.anchor,
                    "anchor_summary": format_anchor_summary(&c.anchor),
                    "created_at_ms": c.created_at_ms,
                    "resolved_at_ms": c.resolved_at_ms,
                })
            })
            .collect();

        match serde_json::to_string_pretty(&json_comments) {
            Ok(s) => println!("{}", s),
            Err(e) => {
                eprintln!("JSON serialization error: {}", e);
                return ExitCode::from(1);
            }
        }
    } else if comments.is_empty() {
        println!("No comments found");
    } else {
        for c in comments {
            let status = if c.status == CommentStatus::Open {
                "OPEN"
            } else {
                "RESOLVED"
            };
            println!(
                "[{}] {} ({}, {}) - {}",
                c.id,
                c.path.as_str(),
                status,
                context_summary(&c.context),
                c.message
            );
            println!("    {}", format_anchor_summary(&c.anchor));
        }
    }

    ExitCode::SUCCESS
}

#[derive(Debug, serde::Serialize)]
struct NextComment<'a> {
    id: u64,
    path: &'a str,
    context: &'a CommentContext,
    context_summary: String,
    status: CommentStatus,
    message: &'a str,
    anchor: &'a Anchor,
    anchor_summary: String,
    hunk_digest: &'a str,
    hunk_index: usize,
    hunk: NextHunkContext,
}

#[derive(Debug, serde::Serialize)]
struct NextHunkContext {
    header: String,
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<NextHunkLine>,
}

#[derive(Debug, serde::Serialize)]
struct NextHunkLine {
    kind: &'static str,
    old: Option<usize>,
    new: Option<usize>,
    text: String,
}

#[derive(Debug, Deserialize)]
struct ImportComment {
    path: String,
    hunk_digest: String,
    message: String,
}

#[derive(Debug, serde::Serialize)]
struct ImportRejected {
    index: usize,
    reason: String,
}

#[derive(Debug, serde::Serialize)]
struct ImportReport {
    accepted: usize,
    rejected: Vec<ImportRejected>,
}

fn comment_hunk_digest(comment: &Comment) -> Option<&str> {
    comment
        .anchor
        .selectors
        .first()
        .map(|selector| match selector {
            Selector::DiffHunkV1(hunk) => hunk.digest_hex.as_str(),
        })
}

fn build_hunk_context(diff: &DiffResult, hunk_index: usize) -> Option<NextHunkContext> {
    let hunk = diff.hunks().get(hunk_index)?;
    let rows = diff
        .rows()
        .get(hunk.start_row..hunk.start_row + hunk.row_count)?;
    let lines = rows
        .iter()
        .map(|row| {
            let kind = match row.kind {
                ChangeKind::Equal => "context",
                ChangeKind::Delete => "delete",
                ChangeKind::Insert => "insert",
                ChangeKind::Replace => "replace",
            };
            let text = row
                .new
                .as_ref()
                .or(row.old.as_ref())
                .map(|line| line.content.clone())
                .unwrap_or_default();
            NextHunkLine {
                kind,
                old: row.old.as_ref().map(|line| line.line_num + 1),
                new: row.new.as_ref().map(|line| line.line_num + 1),
                text,
            }
        })
        .collect();

    Some(NextHunkContext {
        header: format!(
            "@@ -{},{} +{},{} @@",
            hunk.old_range.0 + 1,
            hunk.old_range.1,
            hunk.new_range.0 + 1,
            hunk.new_range.1
        ),
        old_start: hunk.old_range.0 + 1,
        old_count: hunk.old_range.1,
        new_start: hunk.new_range.0 + 1,
        new_count: hunk.new_range.1,
        lines,
    })
}

#[derive(Debug, Clone, Copy)]
enum HunkSelectorArg {
    Index(usize),
    OldLine(usize),
    NewLine(usize),
}

/// Add a comment.
fn cmd_add(repo: &RepoRoot, args: &[String]) -> ExitCode {
    let mut path: Option<String> = None;
    let mut selector: Option<HunkSelectorArg> = None;
    let mut message: Option<String> = None;

    let (context, source) = match parse_context(repo, args) {
        Ok(Some(v)) => v,
        Ok(None) => (CommentContext::Worktree, DiffSource::WorkingTree),
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::from(1);
        }
    };

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--path" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--path requires a value");
                    return ExitCode::from(1);
                }
                path = Some(args[i].clone());
            }
            "--hunk" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--hunk requires a value");
                    return ExitCode::from(1);
                }
                let n = match args[i].parse::<usize>() {
                    Ok(n) if n >= 1 => n,
                    _ => {
                        eprintln!("--hunk must be a positive integer");
                        return ExitCode::from(1);
                    }
                };
                selector = Some(HunkSelectorArg::Index(n - 1));
            }
            "--old-line" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--old-line requires a value");
                    return ExitCode::from(1);
                }
                let n = match args[i].parse::<usize>() {
                    Ok(n) if n >= 1 => n,
                    _ => {
                        eprintln!("--old-line must be a positive integer");
                        return ExitCode::from(1);
                    }
                };
                selector = Some(HunkSelectorArg::OldLine(n - 1));
            }
            "--new-line" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--new-line requires a value");
                    return ExitCode::from(1);
                }
                let n = match args[i].parse::<usize>() {
                    Ok(n) if n >= 1 => n,
                    _ => {
                        eprintln!("--new-line must be a positive integer");
                        return ExitCode::from(1);
                    }
                };
                selector = Some(HunkSelectorArg::NewLine(n - 1));
            }
            "--message" | "-m" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--message requires a value");
                    return ExitCode::from(1);
                }
                message = Some(args[i].clone());
            }
            // Skip values for context flags (handled by parse_context)
            other if takes_value(other) => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let Some(path) = path else {
        eprintln!("--path is required");
        return ExitCode::from(1);
    };

    let Some(selector_arg) = selector else {
        eprintln!("One of --hunk/--old-line/--new-line is required");
        return ExitCode::from(1);
    };

    let Some(message) = message else {
        eprintln!("--message is required");
        return ExitCode::from(1);
    };

    let rel_path = match RelPath::try_new(&path) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Invalid path: must be relative (not absolute): {}", path);
            return ExitCode::from(1);
        }
    };

    let (files, merge_base) = match list_files_for_source(repo, &source) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to list files for source: {}", e);
            return ExitCode::from(1);
        }
    };

    let Some(file) = files.into_iter().find(|f| f.path == rel_path) else {
        eprintln!(
            "Path {} is not part of the current changeset ({})",
            rel_path.as_str(),
            context_summary(&context)
        );
        return ExitCode::from(1);
    };

    let (old_bytes, new_bytes) =
        match load_diff_contents(repo, &source, &file, merge_base.as_deref()) {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("Failed to load diff content: {}", e);
                return ExitCode::from(1);
            }
        };

    let old_buffer = TextBuffer::new(&old_bytes);
    let new_buffer = TextBuffer::new(&new_bytes);
    let diff = DiffResult::compute(&old_buffer, &new_buffer);

    if diff.hunks().is_empty() {
        eprintln!("No hunks found for {}", rel_path.as_str());
        return ExitCode::from(1);
    }

    let hunk_idx = match selector_arg {
        HunkSelectorArg::Index(idx) => {
            if idx >= diff.hunks().len() {
                eprintln!(
                    "Hunk {} does not exist (file has {} hunks)",
                    idx + 1,
                    diff.hunks().len()
                );
                return ExitCode::from(1);
            }
            idx
        }
        HunkSelectorArg::OldLine(old_line) => match diff
            .hunks()
            .iter()
            .position(|h| old_line >= h.old_range.0 && old_line < h.old_range.0 + h.old_range.1)
        {
            Some(idx) => idx,
            None => {
                eprintln!("No hunk matches --old-line {}", old_line + 1);
                return ExitCode::from(1);
            }
        },
        HunkSelectorArg::NewLine(new_line) => match diff
            .hunks()
            .iter()
            .position(|h| new_line >= h.new_range.0 && new_line < h.new_range.0 + h.new_range.1)
        {
            Some(idx) => idx,
            None => {
                eprintln!("No hunk matches --new-line {}", new_line + 1);
                return ExitCode::from(1);
            }
        },
    };

    let Some(hunk_selector) = selector_from_hunk(&diff, hunk_idx) else {
        eprintln!("Failed to create selector for hunk");
        return ExitCode::from(1);
    };

    let anchor = Anchor {
        selectors: vec![Selector::DiffHunkV1(hunk_selector)],
    };

    let mut store = match FileCommentStore::open(repo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open comment store: {}", e);
            return ExitCode::from(1);
        }
    };

    match store.add(rel_path, context, message, anchor) {
        Ok(id) => {
            let digest_prefix = diff
                .hunk_digest(hunk_idx)
                .map(|d| d.get(..8).unwrap_or(d).to_string())
                .unwrap_or_else(|| "????????".to_string());
            println!("Created comment {} [{}]", id, digest_prefix);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Failed to create comment: {}", e);
            ExitCode::from(1)
        }
    }
}

fn cmd_import(repo: &RepoRoot, args: &[String]) -> ExitCode {
    let (context, source) = match parse_context(repo, args) {
        Ok(Some(v)) => v,
        Ok(None) => (CommentContext::Worktree, DiffSource::WorkingTree),
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::from(1);
        }
    };

    let mut json_path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--json requires a value");
                    return ExitCode::from(1);
                }
                json_path = Some(args[i].clone());
            }
            other if takes_value(other) => i += 1,
            _ => {}
        }
        i += 1;
    }

    let Some(json_path) = json_path else {
        eprintln!("Usage: quickdiff comments import --json <file>");
        return ExitCode::from(1);
    };

    let input = match std::fs::read_to_string(&json_path) {
        Ok(input) => input,
        Err(e) => {
            eprintln!("Failed to read {}: {}", json_path, e);
            return ExitCode::from(1);
        }
    };
    let imports: Vec<ImportComment> = match serde_json::from_str(&input) {
        Ok(imports) => imports,
        Err(e) => {
            eprintln!("Failed to parse JSON: {}", e);
            return ExitCode::from(1);
        }
    };

    let (files, merge_base) = match list_files_for_source(repo, &source) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to list files for source: {}", e);
            return ExitCode::from(1);
        }
    };
    let files_by_path: HashMap<_, _> = files
        .iter()
        .map(|file| (file.path.as_str().to_string(), file))
        .collect();

    let mut store = match FileCommentStore::open(repo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open comment store: {}", e);
            return ExitCode::from(1);
        }
    };

    let existing = store.list(true).into_iter().cloned().collect::<Vec<_>>();
    let mut seen: HashSet<(String, String, String)> = existing
        .iter()
        .filter(|comment| comment.context.matches(&context))
        .flat_map(|comment| {
            comment
                .anchor
                .selectors
                .iter()
                .map(|selector| match selector {
                    Selector::DiffHunkV1(hunk) => (
                        comment.path.as_str().to_string(),
                        hunk.digest_hex.clone(),
                        comment.message.trim().to_string(),
                    ),
                })
        })
        .collect();
    let mut report = ImportReport {
        accepted: 0,
        rejected: Vec::new(),
    };
    let mut diff_cache: HashMap<String, DiffResult> = HashMap::new();

    for (index, item) in imports.into_iter().enumerate() {
        let message = item.message.trim();
        if message.is_empty() {
            report.rejected.push(ImportRejected {
                index,
                reason: "message is empty".to_string(),
            });
            continue;
        }

        let Ok(rel_path) = RelPath::try_new(item.path.clone()) else {
            report.rejected.push(ImportRejected {
                index,
                reason: "path must be relative".to_string(),
            });
            continue;
        };
        let Some(file) = files_by_path.get(rel_path.as_str()) else {
            report.rejected.push(ImportRejected {
                index,
                reason: "path is not part of current changeset".to_string(),
            });
            continue;
        };

        let seen_key = (
            rel_path.as_str().to_string(),
            item.hunk_digest.clone(),
            message.to_string(),
        );
        if seen.contains(&seen_key) {
            report.rejected.push(ImportRejected {
                index,
                reason: "duplicate comment".to_string(),
            });
            continue;
        }

        if !diff_cache.contains_key(rel_path.as_str()) {
            let (old_bytes, new_bytes) =
                match load_diff_contents(repo, &source, file, merge_base.as_deref()) {
                    Ok(pair) => pair,
                    Err(e) => {
                        report.rejected.push(ImportRejected {
                            index,
                            reason: format!("failed to load diff: {}", e),
                        });
                        continue;
                    }
                };
            diff_cache.insert(
                rel_path.as_str().to_string(),
                DiffResult::compute(&TextBuffer::new(&old_bytes), &TextBuffer::new(&new_bytes)),
            );
        }
        let Some(diff) = diff_cache.get(rel_path.as_str()) else {
            continue;
        };
        let Some((_, hunk)) = diff
            .hunks()
            .iter()
            .enumerate()
            .find(|(_, hunk)| hunk.digest_hex == item.hunk_digest)
        else {
            report.rejected.push(ImportRejected {
                index,
                reason: "hunk digest not found".to_string(),
            });
            continue;
        };

        let anchor = Anchor {
            selectors: vec![Selector::DiffHunkV1(crate::core::DiffHunkSelectorV1 {
                old_range: hunk.old_range,
                new_range: hunk.new_range,
                digest_hex: hunk.digest_hex.clone(),
            })],
        };
        match store.add(rel_path, context.clone(), message.to_string(), anchor) {
            Ok(_) => {
                seen.insert(seen_key);
                report.accepted += 1;
            }
            Err(e) => report.rejected.push(ImportRejected {
                index,
                reason: format!("failed to store comment: {}", e),
            }),
        }
    }

    match serde_json::to_string_pretty(&report) {
        Ok(output) => println!("{}", output),
        Err(e) => {
            eprintln!("JSON serialization error: {}", e);
            return ExitCode::from(1);
        }
    }
    if report.rejected.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}

fn cmd_next(repo: &RepoRoot, args: &[String]) -> ExitCode {
    let mut json_output = false;
    for arg in args {
        if arg == "--json" {
            json_output = true;
        }
    }

    let (context, source) = match parse_context(repo, args) {
        Ok(Some(v)) => v,
        Ok(None) => (CommentContext::Worktree, DiffSource::WorkingTree),
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::from(1);
        }
    };

    let store = match FileCommentStore::open(repo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open comment store: {}", e);
            return ExitCode::from(1);
        }
    };

    let Some(comment) = store
        .list(false)
        .into_iter()
        .filter(|comment| comment.context.matches(&context))
        .min_by_key(|comment| comment.id)
    else {
        if json_output {
            println!("null");
        } else {
            println!("No open comments");
        }
        return ExitCode::from(2);
    };

    let Some(hunk_digest) = comment_hunk_digest(comment) else {
        eprintln!("Comment {} has no diff hunk selector", comment.id);
        return ExitCode::from(1);
    };

    let (files, merge_base) = match list_files_for_source(repo, &source) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to list files for source: {}", e);
            return ExitCode::from(1);
        }
    };

    let Some(file) = files.iter().find(|file| file.path == comment.path) else {
        eprintln!(
            "Comment {} is stale: {} is not part of the current changeset ({})",
            comment.id,
            comment.path.as_str(),
            context_summary(&context)
        );
        return ExitCode::from(3);
    };

    let (old_bytes, new_bytes) =
        match load_diff_contents(repo, &source, file, merge_base.as_deref()) {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("Failed to load diff content: {}", e);
                return ExitCode::from(1);
            }
        };
    let diff = DiffResult::compute(&TextBuffer::new(&old_bytes), &TextBuffer::new(&new_bytes));
    let Some((hunk_index, _)) = diff
        .hunks()
        .iter()
        .enumerate()
        .find(|(_, hunk)| hunk.digest_hex == hunk_digest)
    else {
        eprintln!(
            "Comment {} is stale: hunk digest {} was not found in {}",
            comment.id,
            hunk_digest,
            comment.path.as_str()
        );
        return ExitCode::from(3);
    };

    let Some(hunk) = build_hunk_context(&diff, hunk_index) else {
        eprintln!("Failed to build hunk context for comment {}", comment.id);
        return ExitCode::from(1);
    };

    if json_output {
        let payload = NextComment {
            id: comment.id,
            path: comment.path.as_str(),
            context: &comment.context,
            context_summary: context_summary(&comment.context),
            status: comment.status,
            message: &comment.message,
            anchor: &comment.anchor,
            anchor_summary: format_anchor_summary(&comment.anchor),
            hunk_digest,
            hunk_index,
            hunk,
        };
        match serde_json::to_string_pretty(&payload) {
            Ok(output) => println!("{}", output),
            Err(e) => {
                eprintln!("JSON serialization error: {}", e);
                return ExitCode::from(1);
            }
        }
    } else {
        println!(
            "[{}] {} - {}",
            comment.id,
            comment.path.as_str(),
            comment.message
        );
        println!("    {}", format_anchor_summary(&comment.anchor));
    }

    ExitCode::SUCCESS
}

/// Resolve a comment.
fn cmd_resolve(repo: &RepoRoot, args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: quickdiff comments resolve <id>");
        return ExitCode::from(1);
    }

    let id: u64 = match args[0].parse() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("Invalid comment ID: {}", args[0]);
            return ExitCode::from(1);
        }
    };

    let mut store = match FileCommentStore::open(repo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open comment store: {}", e);
            return ExitCode::from(1);
        }
    };

    match store.resolve(id) {
        Ok(true) => {
            println!("Resolved comment {}", id);
            ExitCode::SUCCESS
        }
        Ok(false) => {
            eprintln!("Comment {} not found", id);
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("Failed to resolve comment: {}", e);
            ExitCode::from(1)
        }
    }
}
