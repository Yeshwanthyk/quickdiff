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
use quickdiff::core::{
    load_preferences, looks_like_unified_diff, read_stdin_text, ConfigOverrides, DiffSource,
    RepoRoot, VcsPreference,
};
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

    /// Force VCS backend (default: auto-detect, prefers jj)
    #[arg(long = "vcs", value_name = "TYPE", value_parser = parse_vcs_preference)]
    vcs: Option<VcsPreference>,

    /// Comments subcommand
    #[arg(trailing_var_arg = true, hide = true)]
    rest: Vec<String>,
}

fn parse_vcs_preference(s: &str) -> Result<VcsPreference, String> {
    match s.to_lowercase().as_str() {
        "git" => Ok(VcsPreference::Git),
        "jj" => Ok(VcsPreference::Jj),
        "auto" => Ok(VcsPreference::Auto),
        _ => Err(format!("invalid VCS type '{}', expected: git, jj, auto", s)),
    }
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
        restore_terminal_state("terminal guard cleanup");
    }
}

fn disable_mouse_capture_with_warning(context: &str) {
    let mut stdout = io::stdout();
    if let Err(e) = execute!(stdout, DisableMouseCapture) {
        eprintln!("Warning: failed to disable mouse capture ({context}): {e}");
    }
    if let Err(e) = stdout.flush() {
        eprintln!("Warning: failed to flush stdout ({context}): {e}");
    }
}

fn restore_terminal_state(context: &str) {
    disable_mouse_capture_with_warning(context);

    let mut stdout = io::stdout();
    if let Err(e) = execute!(stdout, LeaveAlternateScreen) {
        eprintln!("Warning: failed to leave alternate screen ({context}): {e}");
    }
    if let Err(e) = disable_raw_mode() {
        eprintln!("Warning: failed to disable raw mode ({context}): {e}");
    }
    if let Err(e) = stdout.flush() {
        eprintln!("Warning: failed to flush stdout ({context}): {e}");
    }
}

fn open_html_in_browser(path: &str) {
    #[cfg(target_os = "macos")]
    {
        match std::process::Command::new("open").arg(path).status() {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("Warning: open exited with status: {}", status),
            Err(e) => eprintln!("Warning: failed to launch open: {}", e),
        }
    }

    #[cfg(target_os = "linux")]
    {
        match std::process::Command::new("xdg-open").arg(path).status() {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("Warning: xdg-open exited with status: {}", status),
            Err(e) => eprintln!("Warning: failed to launch xdg-open: {}", e),
        }
    }

    #[cfg(target_os = "windows")]
    {
        match std::process::Command::new("start").arg(path).status() {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("Warning: start exited with status: {}", status),
            Err(e) => eprintln!("Warning: failed to launch start: {}", e),
        }
    }
}

/// Find a subcommand in args, skipping flags and their values.
/// Returns (index, subcommand) if found.
fn find_subcommand(args: &[String]) -> Option<(usize, &str)> {
    const SUBCOMMANDS: &[&str] = &["comments", "web", "pager", "difftool"];
    const FLAGS_WITH_VALUES: &[&str] = &[
        "-c", "--commit", "-b", "--base", "-f", "--file", "-t", "--theme", "--pr", "--vcs",
    ];

    let mut i = 1; // skip program name
    while i < args.len() {
        let arg = &args[i];
        if arg.starts_with('-') {
            // Check if this flag takes a value
            if FLAGS_WITH_VALUES.contains(&arg.as_str()) {
                i += 2; // skip flag and its value
            } else {
                i += 1; // skip flag only
            }
        } else if SUBCOMMANDS.contains(&arg.as_str()) {
            return Some((i, arg.as_str()));
        } else {
            // Positional arg that isn't a subcommand - stop looking
            return None;
        }
    }
    None
}

fn main() -> ExitCode {
    // Check for subcommands first (before clap parsing)
    // Scan past any flags to find subcommand position
    let args: Vec<String> = std::env::args().collect();
    if let Some((idx, cmd)) = find_subcommand(&args) {
        match cmd {
            "comments" => return run_cli_comments(&args[idx + 1..]),
            "web" => return run_cli_web(&args[idx + 1..]),
            "pager" => return run_cli_pager(&args[idx + 1..]),
            "difftool" => return run_cli_difftool(&args[idx + 1..]),
            _ => {}
        }
    }

    // Parse CLI args
    let cli = Cli::parse();

    // Initialize metrics if enabled
    quickdiff::metrics::init();

    if cli.stdin {
        return run_tui_patch(cli.theme, cli.vcs.unwrap_or(VcsPreference::Auto));
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
    let vcs = cli.vcs.unwrap_or(VcsPreference::Auto);
    match run_tui(source, cli.file, cli.theme, pr_number, vcs) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(1)
        }
    }
}

/// Parse CLI arguments into a DiffSource.
fn parse_diff_source(cli: &Cli) -> DiffSource {
    // Explicit flags take precedence.
    if let Some(ref commit) = cli.commit {
        return DiffSource::Commit(commit.clone());
    }
    if let Some(ref base) = cli.base {
        return DiffSource::Base(base.clone());
    }

    if let Some(left) = cli.revision.as_ref().filter(|_| !cli.rest.is_empty()) {
        let right = &cli.rest[0];
        let display_path = cli.rest.get(1).cloned();
        return DiffSource::FilePair {
            left: left.into(),
            right: right.into(),
            display_path,
        };
    }

    if let Some(ref rev) = cli.revision {
        if let Some(idx) = rev.find("..") {
            let from = &rev[..idx];
            let to = &rev[idx + 2..];
            let to = to.strip_prefix('.').unwrap_or(to);
            return DiffSource::Range {
                from: from.to_string(),
                to: to.to_string(),
            };
        }

        if rev.contains('/') && !rev.contains(':') {
            return DiffSource::Base(rev.clone());
        }

        return DiffSource::Commit(rev.clone());
    }

    DiffSource::WorkingTree
}

fn validate_file_input(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        anyhow::bail!("file not found: {}", path.display());
    }
    if !path.is_file() {
        anyhow::bail!(
            "unsupported input (expected regular file): {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_source_inputs(source: &DiffSource) -> Result<()> {
    match source {
        DiffSource::FilePair { left, right, .. } | DiffSource::DiffTool { left, right, .. } => {
            validate_file_input(left)?;
            validate_file_input(right)?;
        }
        _ => {}
    }
    Ok(())
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

    let repo = match RepoRoot::discover(&cwd, VcsPreference::Auto) {
        Ok(repo) => repo,
        Err(quickdiff::core::RepoError::NotARepo(vcs)) => {
            eprintln!("Error: Not inside a {} repository", vcs);
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(1);
        }
    };

    run_comments_command(&repo, args)
}

/// Embedded web template (compiled into binary).
const WEB_TEMPLATE: &str = include_str!("../web/template.html");

/// Run CLI web preview command.
fn run_cli_web(args: &[String]) -> ExitCode {
    use base64::Engine;

    // Parse web subcommand args
    let mut stdin_mode = false;
    let mut open_browser = false;
    let mut output_path: Option<String> = None;
    let mut file_filter: Option<String> = None;
    let mut diffspec: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--stdin" => stdin_mode = true,
            "--open" | "-o" => open_browser = true,
            "--output" => {
                i += 1;
                output_path = args.get(i).cloned();
            }
            "--file" | "-f" => {
                i += 1;
                file_filter = args.get(i).cloned();
            }
            arg if !arg.starts_with('-') => {
                diffspec = Some(arg.to_string());
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                return ExitCode::from(1);
            }
        }
        i += 1;
    }

    // Read stdin patch if requested
    let stdin_patch = if stdin_mode {
        let patch = match read_stdin_text() {
            Ok(text) => text,
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                return ExitCode::from(1);
            }
        };
        if patch.trim().is_empty() {
            eprintln!("Error: empty input from stdin");
            return ExitCode::from(1);
        }
        Some(patch)
    } else {
        None
    };

    // Discover repository
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to get current directory: {}", e);
            return ExitCode::from(1);
        }
    };

    let repo = match RepoRoot::discover(&cwd, VcsPreference::Auto) {
        Ok(repo) => repo,
        Err(quickdiff::core::RepoError::NotARepo(vcs)) => {
            eprintln!("Error: Not inside a {} repository", vcs);
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(1);
        }
    };

    // Determine diff source from diffspec
    let source = if let Some(ref spec) = diffspec {
        if let Some(idx) = spec.find("..") {
            let from = &spec[..idx];
            let to = &spec[idx + 2..];
            let to = to.strip_prefix('.').unwrap_or(to);
            DiffSource::Range {
                from: from.to_string(),
                to: to.to_string(),
            }
        } else if spec.contains('/') && !spec.contains(':') {
            DiffSource::Base(spec.clone())
        } else {
            DiffSource::Commit(spec.clone())
        }
    } else {
        DiffSource::WorkingTree
    };

    // Build review data
    let input = quickdiff::web::WebInput {
        source,
        stdin_patch,
        file_filter,
        label: diffspec.unwrap_or_default(),
    };

    let review_data = match quickdiff::web::build_review_data(&repo, input) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error building review data: {}", e);
            return ExitCode::from(1);
        }
    };

    // Determine output path
    let out_file = output_path.unwrap_or_else(|| {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("/tmp/quickdiff-{}.html", timestamp)
    });

    // Serialize and base64 encode JSON
    let json_data = match serde_json::to_string(&review_data) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Error serializing review data: {}", e);
            return ExitCode::from(1);
        }
    };
    let b64_data = base64::engine::general_purpose::STANDARD.encode(&json_data);

    // Render template
    let html = WEB_TEMPLATE
        .replace("{{REVIEW_DATA_B64}}", &b64_data)
        .replace("{{BRANCH}}", &review_data.branch)
        .replace("{{COMMIT}}", &review_data.commit);

    // Write output
    if let Err(e) = std::fs::write(&out_file, &html) {
        eprintln!("Error writing HTML: {}", e);
        return ExitCode::from(1);
    }

    println!("Generated: {}", out_file);

    if open_browser {
        open_html_in_browser(&out_file);
    }

    ExitCode::SUCCESS
}

fn run_cli_pager(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("Error: pager does not accept positional arguments");
        return ExitCode::from(1);
    }

    let input = match read_stdin_text() {
        Ok(text) => text,
        Err(err) => {
            eprintln!("Error reading stdin: {}", err);
            return ExitCode::from(1);
        }
    };

    if input.trim().is_empty() {
        eprintln!("Error: empty input from stdin");
        return ExitCode::from(1);
    }

    if looks_like_unified_diff(&input) {
        run_tui_patch_from_text(input, None, VcsPreference::Auto, "stdin")
    } else {
        print!("{}", input);
        ExitCode::SUCCESS
    }
}

fn run_cli_difftool(args: &[String]) -> ExitCode {
    if args.len() < 2 || args.len() > 3 {
        eprintln!("Usage: quickdiff difftool <left> <right> [display-path]");
        return ExitCode::from(1);
    }

    let source = DiffSource::DiffTool {
        left: args[0].clone().into(),
        right: args[1].clone().into(),
        display_path: args.get(2).cloned().unwrap_or_else(|| args[1].clone()),
    };

    match run_tui(source, None, None, None, VcsPreference::Auto) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {}", err);
            ExitCode::from(1)
        }
    }
}

/// Run TUI in patch mode (stdin input).
fn run_tui_patch(theme: Option<String>, vcs: VcsPreference) -> ExitCode {
    let patch = match read_stdin_text() {
        Ok(text) => text,
        Err(err) => {
            eprintln!("Error reading stdin: {}", err);
            return ExitCode::from(1);
        }
    };

    if patch.trim().is_empty() {
        eprintln!("Error: empty input from stdin");
        return ExitCode::from(1);
    }

    run_tui_patch_from_text(patch, theme, vcs, "stdin")
}

fn run_tui_patch_from_text(
    patch: String,
    theme: Option<String>,
    vcs: VcsPreference,
    label: &str,
) -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to get current directory: {}", e);
            return ExitCode::from(1);
        }
    };

    let repo = match RepoRoot::discover(&cwd, vcs) {
        Ok(repo) => repo,
        Err(quickdiff::core::RepoError::NotARepo(vcs)) => {
            eprintln!("Error: Not inside a {} repository", vcs);
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(1);
        }
    };

    let loaded = load_preferences(
        repo.path(),
        &ConfigOverrides {
            theme: theme.clone(),
        },
    );
    let mut app = match App::new(repo, DiffSource::WorkingTree, None, loaded.prefs) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Error: {}", e);
            return ExitCode::from(1);
        }
    };
    if let Some(warning) = loaded.warnings.into_iter().next() {
        app.ui.status = Some(warning);
    }

    app.load_patch(patch, label.to_string());

    if app.files.is_empty() {
        eprintln!("Error: patch contains no files");
        return ExitCode::from(1);
    }

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

    // SAFETY: Duplicating file descriptor 0 is thread-safe and does not alias Rust references.
    // We validate the returned fd before using it.
    let orig_stdin = unsafe { libc::dup(0) };
    if orig_stdin < 0 {
        return Err(anyhow::anyhow!("Failed to dup stdin"));
    }

    // SAFETY: `tty_fd` and target fd 0 are valid descriptors at this point.
    // `dup2` atomically replaces stdin with `/dev/tty` for the current process.
    if unsafe { libc::dup2(tty_fd, 0) } < 0 {
        // SAFETY: `orig_stdin` was returned by `dup` above and has not been closed yet.
        unsafe { libc::close(orig_stdin) };
        return Err(anyhow::anyhow!("Failed to redirect stdin to /dev/tty"));
    }

    let result = run_tui_loop(app);

    // SAFETY: `orig_stdin` is a valid duplicated descriptor and 0 is stdin.
    // Restoring via `dup2` and then closing `orig_stdin` returns fd ownership to the OS.
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
    vcs: VcsPreference,
) -> Result<()> {
    // Set panic hook to ensure terminal cleanup
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Restore terminal before printing panic
        restore_terminal_state("panic hook");
        default_hook(info);
    }));

    // Discover repository
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = match RepoRoot::discover(&cwd, vcs) {
        Ok(repo) => repo,
        Err(quickdiff::core::RepoError::NotARepo(vcs)) => {
            eprintln!("Error: Not inside a {} repository", vcs);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut source = source;
    source.apply_defaults(repo.working_copy_parent_ref());
    validate_source_inputs(&source)?;

    if repo.is_jj() && pr_number.is_some() {
        eprintln!("Error: PR mode requires a git repository");
        std::process::exit(1);
    }

    let loaded = load_preferences(
        repo.path(),
        &ConfigOverrides {
            theme: theme.clone(),
        },
    );

    // Create app with diff source and file filter
    let mut app = App::new(repo.clone(), source, file_filter, loaded.prefs)?;
    if let Some(warning) = loaded.warnings.into_iter().next() {
        app.ui.status = Some(warning);
    }

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
            disable_mouse_capture_with_warning("event loop shutdown");
            break;
        }
    }

    Ok(())
}
