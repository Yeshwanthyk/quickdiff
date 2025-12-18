//! quickdiff - A git-first terminal diff viewer.

use std::io::{self, Write};
use std::panic;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event, execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use quickdiff::cli::run_comments_command;
use quickdiff::core::RepoRoot;
use quickdiff::ui::{handle_input, render, App};

/// RAII guard for terminal state. Restores terminal on drop (including panic).
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = io::stdout().flush();
    }
}

fn main() -> ExitCode {
    // Parse args for CLI dispatch
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Check for CLI subcommands
    if args.first().map(|s| s.as_str()) == Some("comments") {
        return run_cli_comments(&args[1..]);
    }

    // Run TUI
    match run_tui() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(1)
        }
    }
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
fn run_tui() -> Result<()> {
    // Set panic hook to ensure terminal cleanup
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Restore terminal before printing panic
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
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

    // Create app
    let mut app = App::new(repo)?;

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
            break;
        }
    }

    Ok(())
}
