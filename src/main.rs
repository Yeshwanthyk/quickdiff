//! quickdiff - A git/jj-first terminal diff viewer.

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

/// A git/jj-first terminal diff viewer.
#[derive(Parser, Debug)]
#[command(name = "quickdiff", version, about)]
struct Cli {
    /// Show changes from a specific commit
    #[arg(short = 'c', long = "commit")]
    commit: Option<String>,

    /// Compare against a base branch (e.g., origin/main)
    #[arg(short = 'b', long = "base")]
    base: Option<String>,

    /// Revision or range (e.g., HEAD~3, abc123..def456, origin/main, @-..@)
    #[arg(value_name = "REV")]
    revision: Option<String>,

    /// Filter to specific file(s)
    #[arg(short = 'f', long = "file", value_name = "PATH")]
    file: Option<String>,

    /// Color theme (default, dracula, catppuccin, nord, gruvbox, tokyonight, rosepine, onedark, solarized)
    #[arg(short = 't', long = "theme", value_name = "THEME")]
    theme: Option<String>,

    /// Browse and review GitHub pull requests (optionally specify PR number)
    #[arg(long = "pr", value_name = "NUMBER")]
    pr: Option<Option<u32>>,

    /// Read unified diff from stdin and render in TUI (pager mode)
    #[arg(long = "stdin")]
    stdin: bool,

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

    // Initialize metrics if enabled
    quickdiff::metrics::init();

    if cli.stdin {
        return run_tui_patch(cli.theme);
    }

    // Determine diff source
    let source = parse_diff_source(&cli);

    // Handle --pr flag
    let pr_number = match cli.pr {
        Some(Some(n)) => Some(n), // --pr 123
        Some(None) => Some(0),    // --pr (picker mode, 0 = open picker)
        None => None,             // no flag
    };

    // Run TUI
    match run_tui(source, cli.file, cli.theme, pr_number) {
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
                from: from.to_string(),
                to: to.to_string(),
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
            eprintln!("Error: Not inside a git or jj repository");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(1);
        }
    };

    run_comments_command(&repo, args)
}

/// Run TUI in patch mode (stdin input).
fn run_tui_patch(theme: Option<String>) -> ExitCode {
    use std::io::Read;

    // Read patch from stdin
    let mut patch = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut patch) {
        eprintln!("Error reading stdin: {}", e);
        return ExitCode::from(1);
    }

    if patch.trim().is_empty() {
        eprintln!("Error: empty input from stdin");
        return ExitCode::from(1);
    }

    // Discover repository (optional for patch mode, but needed for theme/state)
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
            eprintln!("Error: Not inside a git or jj repository");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(1);
        }
    };

    // Create app in patch mode
    let mut app = match App::new(repo, DiffSource::WorkingTree, None, theme.as_deref()) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(1);
        }
    };

    // Load patch
    app.load_patch(patch, "stdin".to_string());

    if app.files.is_empty() {
        eprintln!("Error: patch contains no files");
        return ExitCode::from(1);
    }

    // Run TUI (reopen /dev/tty if stdin was piped)
    match run_tui_loop_tty(&mut app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(1)
        }
    }
}

/// Run TUI loop, reopening /dev/tty for input when stdin is piped.
#[cfg(unix)]
fn run_tui_loop_tty(app: &mut App) -> Result<()> {
    use std::fs::File;
    use std::os::unix::io::AsRawFd;

    // Check if stdin is a TTY
    if crossterm::tty::IsTty::is_tty(&io::stdin()) {
        return run_tui_loop(app);
    }

    // stdin is piped, reopen /dev/tty for terminal input
    let tty = File::open("/dev/tty")
        .context("Failed to open /dev/tty - stdin mode requires a terminal")?;
    let tty_fd = tty.as_raw_fd();

    // Duplicate stdin to restore later
    let orig_stdin = unsafe { libc::dup(0) };
    if orig_stdin < 0 {
        return Err(anyhow::anyhow!("Failed to dup stdin"));
    }

    // Redirect stdin to /dev/tty
    if unsafe { libc::dup2(tty_fd, 0) } < 0 {
        unsafe { libc::close(orig_stdin) };
        return Err(anyhow::anyhow!("Failed to redirect stdin to /dev/tty"));
    }

    let result = run_tui_loop(app);

    // Restore original stdin
    unsafe {
        libc::dup2(orig_stdin, 0);
        libc::close(orig_stdin);
    }

    result
}

#[cfg(not(unix))]
fn run_tui_loop_tty(app: &mut App) -> Result<()> {
    // On non-Unix, just try regular TUI loop
    run_tui_loop(app)
}

/// Run the TUI application.
fn run_tui(
    source: DiffSource,
    file_filter: Option<String>,
    theme: Option<String>,
    pr_number: Option<u32>,
) -> Result<()> {
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
            eprintln!("Error: Not inside a git or jj repository");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut source = source;
    source.apply_defaults(repo.working_copy_parent_ref());

    if repo.is_jj() && pr_number.is_some() {
        eprintln!("Error: PR mode requires a git repository");
        std::process::exit(1);
    }

    // Create app with diff source and file filter
    let mut app = App::new(repo.clone(), source, file_filter, theme.as_deref())?;

    // Handle PR mode initialization
    if let Some(n) = pr_number {
        if n == 0 {
            // Open PR picker
            app.open_pr_picker();
        } else {
            // Load specific PR
            if !quickdiff::core::is_gh_available() {
                eprintln!("Error: GitHub CLI not available. Run 'gh auth login'");
                std::process::exit(1);
            }
            match quickdiff::core::list_prs(repo.path(), quickdiff::core::PRFilter::All) {
                Ok(prs) => {
                    if let Some(pr) = prs.into_iter().find(|p| p.number == n) {
                        app.load_pr(pr);
                    } else {
                        eprintln!("Error: PR #{} not found", n);
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    // Check for empty changeset (skip in PR mode - files come from PR)
    if app.files.is_empty() && !app.pr.active && app.ui.mode != quickdiff::ui::Mode::PRPicker {
        println!("No changes detected");
        return Ok(());
    }

    run_tui_loop(&mut app)
}

/// Run the TUI loop with terminal setup and cleanup.
fn run_tui_loop(app: &mut App) -> Result<()> {
    // Setup terminal with RAII guard
    let _guard = TerminalGuard::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    let result = run_event_loop(&mut terminal, app);

    // Save state (guard will cleanup terminal on drop)
    if let Err(e) = app.save_state() {
        eprintln!("Warning: Failed to save state: {}", e);
    }

    result
}

fn run_event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        app.poll_worker();
        app.poll_pr_worker();
        app.poll_watcher();

        // Only redraw if dirty or on resize
        if app.ui.dirty {
            terminal.draw(|frame| render(frame, app))?;
            app.clear_dirty();
        }

        // Poll for events with timeout, then drain all pending events
        if event::poll(Duration::from_millis(50))? {
            // Drain all available events to reduce poll syscalls and batch processing
            let mut events = Vec::with_capacity(8);
            while event::poll(Duration::ZERO)? {
                events.push(event::read()?);
            }

            // Process all events
            for ev in events {
                if matches!(ev, crossterm::event::Event::Resize(_, _)) {
                    app.mark_dirty();
                }
                handle_input(app, ev);
                if app.should_quit {
                    break;
                }
            }
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
