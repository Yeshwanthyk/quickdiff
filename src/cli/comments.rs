//! CLI commands for comment management.

use std::process::ExitCode;

use crate::core::{
    load_head_content, load_working_content, selector_from_hunk, Anchor, CommentStore, DiffResult,
    FileCommentStore, RelPath, RepoRoot, Selector, TextBuffer,
};

/// Run a comments subcommand.
/// Returns ExitCode for the process.
pub fn run_comments_command(repo: &RepoRoot, args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: quickdiff comments <command>");
        eprintln!("Commands:");
        eprintln!("  list [--all] [--json] [--path <path>]");
        eprintln!("  add --path <path> --hunk <n> --message <text>");
        eprintln!("  resolve <id>");
        return ExitCode::from(1);
    }

    let cmd = args[0].as_str();
    let cmd_args = &args[1..];

    match cmd {
        "list" => cmd_list(repo, cmd_args),
        "add" => cmd_add(repo, cmd_args),
        "resolve" => cmd_resolve(repo, cmd_args),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            ExitCode::from(1)
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
        match args[i].as_str() {
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
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                return ExitCode::from(1);
            }
        }
        i += 1;
    }

    let store = match FileCommentStore::open(repo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open comment store: {}", e);
            return ExitCode::from(1);
        }
    };

    let comments = if let Some(ref path) = filter_path {
        let rel_path = RelPath::new(path.clone());
        store.list_for_path(&rel_path, include_resolved)
    } else {
        store.list(include_resolved)
    };

    if json_output {
        // Output as JSON array
        let json_comments: Vec<_> = comments
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "path": c.path.as_str(),
                    "status": format!("{:?}", c.status).to_lowercase(),
                    "message": c.message,
                    "anchor": format_anchor_summary(&c.anchor),
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
            let status = if c.status == crate::core::CommentStatus::Open {
                "OPEN"
            } else {
                "RESOLVED"
            };
            println!(
                "[{}] {} ({}) - {}",
                c.id,
                c.path.as_str(),
                status,
                c.message
            );
            println!("    {}", format_anchor_summary(&c.anchor));
        }
    }

    ExitCode::SUCCESS
}

/// Add a comment.
fn cmd_add(repo: &RepoRoot, args: &[String]) -> ExitCode {
    let mut path: Option<String> = None;
    let mut hunk_num: Option<usize> = None;
    let mut message: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
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
                match args[i].parse::<usize>() {
                    Ok(n) if n >= 1 => hunk_num = Some(n),
                    _ => {
                        eprintln!("--hunk must be a positive integer");
                        return ExitCode::from(1);
                    }
                }
            }
            "--message" | "-m" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--message requires a value");
                    return ExitCode::from(1);
                }
                message = Some(args[i].clone());
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                return ExitCode::from(1);
            }
        }
        i += 1;
    }

    let Some(path) = path else {
        eprintln!("--path is required");
        return ExitCode::from(1);
    };

    let Some(hunk_num) = hunk_num else {
        eprintln!("--hunk is required");
        return ExitCode::from(1);
    };

    let Some(message) = message else {
        eprintln!("--message is required");
        return ExitCode::from(1);
    };

    let rel_path = RelPath::new(path);

    // Load content and compute diff
    let old_bytes = match load_head_content(repo, &rel_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to load HEAD content: {}", e);
            return ExitCode::from(1);
        }
    };

    let new_bytes = match load_working_content(repo, &rel_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to load working content: {}", e);
            return ExitCode::from(1);
        }
    };

    let old_buffer = TextBuffer::new(&old_bytes);
    let new_buffer = TextBuffer::new(&new_bytes);
    let diff = DiffResult::compute(&old_buffer, &new_buffer);

    // Check hunk exists (1-indexed)
    let hunk_idx = hunk_num - 1;
    if hunk_idx >= diff.hunks.len() {
        eprintln!(
            "Hunk {} does not exist (file has {} hunks)",
            hunk_num,
            diff.hunks.len()
        );
        return ExitCode::from(1);
    }

    // Build selector
    let selector = match selector_from_hunk(&diff, hunk_idx) {
        Some(s) => s,
        None => {
            eprintln!("Failed to create selector for hunk");
            return ExitCode::from(1);
        }
    };

    let anchor = Anchor {
        selectors: vec![Selector::DiffHunkV1(selector)],
    };

    // Add comment
    let mut store = match FileCommentStore::open(repo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open comment store: {}", e);
            return ExitCode::from(1);
        }
    };

    match store.add(rel_path, message, anchor) {
        Ok(id) => {
            println!("Created comment {}", id);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Failed to create comment: {}", e);
            ExitCode::from(1)
        }
    }
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

/// Format anchor summary for display.
fn format_anchor_summary(anchor: &Anchor) -> String {
    anchor
        .selectors
        .iter()
        .map(|s| match s {
            Selector::DiffHunkV1(h) => {
                format!(
                    "@@ -{},{} +{},{} @@ [{}]",
                    h.old_range.0 + 1,
                    h.old_range.1,
                    h.new_range.0 + 1,
                    h.new_range.1,
                    &h.digest_hex[..8]
                )
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}
