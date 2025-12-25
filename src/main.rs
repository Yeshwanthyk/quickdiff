//! quickdiff - A git-first terminal diff viewer.

use std::io::{self, Write};
use std::panic;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use quickdiff::cli::run_comments_command;
use quickdiff::core::{DiffSource, RepoRoot};
use quickdiff::ui::{handle_input, render, App};

/// A git-first terminal diff viewer.
#[derive(Parser, Debug)]
#[command(name = "quickdiff", version, about)]
struct Cli {
    /// Show changes from a specific commit
    #[arg(short = 'c', long = "commit")]
    commit: Option<String>,

    /// Compare against a base branch (e.g., origin/main)
    #[arg(short = 'b', long = "base")]
    base: Option<String>,

    /// Revision or range (e.g., HEAD~3, abc123..def456, origin/main)
    #[arg(value_name = "REV")]
    revision: Option<String>,

    /// Filter to specific file(s)
    #[arg(short = 'f', long = "file", value_name = "PATH")]
    file: Option<String>,

    /// Color theme (default, dracula, catppuccin, nord, gruvbox, tokyonight, rosepine, onedark, solarized)
    #[arg(short = 't', long = "theme", value_name = "THEME")]
    theme: Option<String>,

    /// Comments subcommand
    #[arg(trailing_var_arg = true, hide = true)]
    rest: Vec<String>,
}

/// RAII guard for terminal state. Restores terminal on drop (including panic).
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Disable mouse capture first (while still in raw mode)
        let _ = execute!(io::stdout(), DisableMouseCapture);
        let _ = io::stdout().flush();
        // Then leave alternate screen and disable raw mode
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
        let _ = io::stdout().flush();
    }
}

fn main() -> ExitCode {
    // Check for comments subcommand first (before clap parsing)
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("comments") {
        return run_cli_comments(&args[2..]);
    }

    // Parse CLI args
    let cli = Cli::parse();

    // Determine diff source
    let source = parse_diff_source(&cli);

    // Run TUI
    match run_tui(source, cli.file, cli.theme) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(1)
        }
    }
}

/// Parse CLI arguments into a DiffSource.
fn parse_diff_source(cli: &Cli) -> DiffSource {
    // Explicit flags take precedence
    if let Some(ref commit) = cli.commit {
        return DiffSource::Commit(commit.clone());
    }
    if let Some(ref base) = cli.base {
        return DiffSource::Base(base.clone());
    }

    // Check positional argument
    if let Some(ref rev) = cli.revision {
        // Check for range syntax (contains ..)
        if let Some(idx) = rev.find("..") {
            let from = &rev[..idx];
            let to = &rev[idx + 2..];
            // Handle ... (three dots) as well
            let to = to.strip_prefix('.').unwrap_or(to);
            return DiffSource::Range {
                from: if from.is_empty() {
                    "HEAD".to_string()
                } else {
                    from.to_string()
                },
                to: if to.is_empty() {
                    "HEAD".to_string()
                } else {
                    to.to_string()
                },
            };
        }

        // Check if it looks like a remote branch
        if rev.contains('/') && !rev.contains(':') {
            return DiffSource::Base(rev.clone());
        }

        // Default: treat as a commit
        return DiffSource::Commit(rev.clone());
    }

    // Default: working tree changes
    DiffSource::WorkingTree
}

/// Run CLI comments command.
fn run_cli_comments(args: &[String]) -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to get current directory: {}", e);
            return ExitCode::from(1);
        }
    };

    let repo = match RepoRoot::discover(&cwd) {
        Ok(repo) => repo,
        Err(quickdiff::core::RepoError::NotARepo) => {
            eprintln!("Error: Not inside a git repository");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(1);
        }
    };

    run_comments_command(&repo, args)
}

/// Run the TUI application.
fn run_tui(source: DiffSource, file_filter: Option<String>, theme: Option<String>) -> Result<()> {
    // Set panic hook to ensure terminal cleanup
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Restore terminal before printing panic
        let _ = execute!(io::stdout(), DisableMouseCapture);
        let _ = io::stdout().flush();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
        let _ = io::stdout().flush();
        default_hook(info);
    }));

    // Discover repository
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = match RepoRoot::discover(&cwd) {
        Ok(repo) => repo,
        Err(quickdiff::core::RepoError::NotARepo) => {
            eprintln!("Error: Not inside a git repository");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Create app with diff source and file filter
    let mut app = App::new(repo, source, file_filter, theme.as_deref())?;

    // Check for empty changeset
    if app.files.is_empty() {
        println!("No changes detected");
        return Ok(());
    }

    // Setup terminal with RAII guard
    let _guard = TerminalGuard::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    let result = run_loop(&mut terminal, &mut app);

    // Save state (guard will cleanup terminal on drop)
    if let Err(e) = app.save_state() {
        eprintln!("Warning: Failed to save state: {}", e);
    }

    result
}

fn run_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        app.poll_worker();

        // Only redraw if dirty or on resize
        if app.dirty {
            terminal.draw(|frame| render(frame, app))?;
            app.clear_dirty();
        }

        // Poll for events with timeout
        if event::poll(Duration::from_millis(50))? {
            let event = event::read()?;

            // Resize always triggers redraw
            if matches!(event, crossterm::event::Event::Resize(_, _)) {
                app.mark_dirty();
            }

            handle_input(app, event);
        }

        if app.should_quit {
            // Disable mouse capture before exiting
            let _ = execute!(io::stdout(), DisableMouseCapture);
            let _ = io::stdout().flush();
            break;
        }
    }

    Ok(())
}
