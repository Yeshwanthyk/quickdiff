//! Input handling.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, Focus, Mode};

/// Handle a crossterm event.
/// Returns true if the event was handled.
pub fn handle_input(app: &mut App, event: Event) -> bool {
    match event {
        Event::Key(key) => handle_key(app, key),
        _ => false,
    }
}

/// Handle a key event.
fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    // Handle input mode first
    if app.mode == Mode::AddComment {
        return handle_add_comment_key(app, key);
    }

    // Global keys (only in Normal mode)
    match key.code {
        KeyCode::Char('q') => {
            app.should_quit = true;
            return true;
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
            return true;
        }
        KeyCode::Tab => {
            app.toggle_focus();
            return true;
        }
        KeyCode::Char('1') => {
            app.set_focus(Focus::Sidebar);
            return true;
        }
        KeyCode::Char('2') => {
            app.set_focus(Focus::Diff);
            return true;
        }
        KeyCode::Char(' ') => {
            app.toggle_viewed();
            return true;
        }
        _ => {}
    }

    // Focus-specific keys
    match app.focus {
        Focus::Sidebar => handle_sidebar_key(app, key),
        Focus::Diff => handle_diff_key(app, key),
    }
}

/// Handle keys when sidebar is focused.
fn handle_sidebar_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next();
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev();
            true
        }
        KeyCode::Enter => {
            app.set_focus(Focus::Diff);
            true
        }
        _ => false,
    }
}

/// Handle keys when diff view is focused.
fn handle_diff_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.scroll_diff(1, 0);
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.scroll_diff(-1, 0);
            true
        }
        KeyCode::Char('h') | KeyCode::Left => {
            app.scroll_diff(0, -1);
            true
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.scroll_diff(0, 1);
            true
        }
        KeyCode::Char('}') => {
            app.next_hunk();
            true
        }
        KeyCode::Char('{') => {
            app.prev_hunk();
            true
        }
        KeyCode::PageDown => {
            app.scroll_diff(20, 0);
            true
        }
        KeyCode::PageUp => {
            app.scroll_diff(-20, 0);
            true
        }
        KeyCode::Char('G') => {
            // Go to end
            if let Some(diff) = &app.diff {
                app.scroll_y = diff.row_count().saturating_sub(1);
            }
            true
        }
        KeyCode::Char('g') => {
            // Go to start
            app.scroll_y = 0;
            true
        }
        KeyCode::Char('c') => {
            app.start_add_comment();
            true
        }
        _ => false,
    }
}

/// Handle keys when in AddComment mode.
fn handle_add_comment_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.cancel_add_comment();
            true
        }
        KeyCode::Enter => {
            app.save_comment();
            true
        }
        KeyCode::Backspace => {
            app.draft_comment.pop();
            app.mark_dirty();
            true
        }
        KeyCode::Char(c) => {
            app.draft_comment.push(c);
            app.mark_dirty();
            true
        }
        _ => false,
    }
}
