//! Input handling.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::app::{App, Focus, Mode};

/// Handle a crossterm event.
/// Returns true if the event was handled.
pub fn handle_input(app: &mut App, event: Event) -> bool {
    match event {
        Event::Key(key) => handle_key(app, key),
        Event::Mouse(mouse) => handle_mouse(app, mouse),
        _ => false,
    }
}

/// Handle a key event.
fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    // Handle input modes first
    match app.mode {
        Mode::AddComment => return handle_add_comment_key(app, key),
        Mode::ViewComments => return handle_view_comments_key(app, key),
        Mode::FilterFiles => return handle_filter_key(app, key),
        Mode::SelectTheme => return handle_theme_selector_key(app, key),
        Mode::Normal => {}
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
        KeyCode::Char('T') => {
            app.open_theme_selector();
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
        KeyCode::Char('/') => {
            app.start_filter();
            true
        }
        KeyCode::Esc => {
            app.clear_filter();
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
        KeyCode::Char('c') if app.is_worktree_mode() => {
            app.start_add_comment();
            true
        }
        KeyCode::Char('C') if app.is_worktree_mode() => {
            app.show_comments();
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

/// Handle keys when viewing comments overlay.
fn handle_view_comments_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('C') => {
            app.close_comments();
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.comments_select_next();
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.comments_select_prev();
            true
        }
        KeyCode::Enter => {
            app.comments_jump_to_selected();
            true
        }
        KeyCode::Char('r') => {
            app.comments_resolve_selected();
            true
        }
        KeyCode::Char('a') => {
            app.comments_toggle_include_resolved();
            true
        }
        _ => true, // consume all keys in overlay
    }
}

/// Handle keys when filtering files.
fn handle_filter_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.cancel_filter();
            true
        }
        KeyCode::Enter => {
            app.apply_filter();
            true
        }
        KeyCode::Backspace => {
            app.sidebar_filter.pop();
            app.update_filter_live();
            true
        }
        KeyCode::Char(c) => {
            app.sidebar_filter.push(c);
            app.update_filter_live();
            true
        }
        _ => false,
    }
}

/// Handle keys when theme selector is open.
fn handle_theme_selector_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('T') => {
            app.close_theme_selector();
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.theme_select_next();
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.theme_select_prev();
            true
        }
        KeyCode::Enter => {
            app.theme_apply();
            true
        }
        _ => true, // consume all keys in overlay
    }
}

// ============================================================================
// Mouse Handling
// ============================================================================

/// Sidebar width including borders (matches render.rs layout).
const SIDEBAR_WIDTH: u16 = 32;

/// Handle a mouse event.
fn handle_mouse(app: &mut App, event: MouseEvent) -> bool {
    // Don't handle mouse in modal modes
    if !matches!(app.mode, Mode::Normal) {
        return false;
    }

    match event.kind {
        MouseEventKind::ScrollUp => {
            handle_scroll(app, -3, event.column);
            true
        }
        MouseEventKind::ScrollDown => {
            handle_scroll(app, 3, event.column);
            true
        }
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            handle_click(app, event.column, event.row);
            true
        }
        _ => false,
    }
}

/// Handle scroll wheel. Direction: negative = up, positive = down.
fn handle_scroll(app: &mut App, delta: isize, x: u16) {
    if x < SIDEBAR_WIDTH {
        // Scroll in sidebar - navigate files
        if delta < 0 {
            for _ in 0..(-delta) {
                app.select_prev();
            }
        } else {
            for _ in 0..delta {
                app.select_next();
            }
        }
    } else {
        // Scroll in diff pane
        app.scroll_diff(delta, 0);
    }
}

/// Handle left click.
fn handle_click(app: &mut App, x: u16, y: u16) {
    // Layout: row 0 = top bar, rows 1..n-1 = main, row n-1 = bottom bar
    // Main area: sidebar is x < SIDEBAR_WIDTH, diff is x >= SIDEBAR_WIDTH
    //
    // Sidebar inner area (content):
    //   x: 1..SIDEBAR_WIDTH-1 (excluding borders)
    //   y: 2..height-2 (top bar=0, sidebar border=1, content, bottom border, bottom bar)

    if y == 0 {
        // Top bar - ignore
        return;
    }

    if x < SIDEBAR_WIDTH {
        // Click in sidebar region -> focus sidebar
        app.set_focus(Focus::Sidebar);

        // Calculate which file was clicked
        // Sidebar inner area starts at y=2 (top bar + border)
        if y >= 2 && (1..SIDEBAR_WIDTH - 1).contains(&x) {
            let row_in_sidebar = (y - 2) as usize;
            let clicked_visible_idx = app.sidebar_scroll + row_in_sidebar;

            // Get visible files
            let visible: Vec<usize> = if app.filtered_indices.is_empty() {
                (0..app.files.len()).collect()
            } else {
                app.filtered_indices.clone()
            };

            if clicked_visible_idx < visible.len() {
                let file_idx = visible[clicked_visible_idx];
                if file_idx != app.selected_idx {
                    app.selected_idx = file_idx;
                    app.request_current_diff();
                }
            }
        }
    } else {
        // Click in diff region -> focus diff
        app.set_focus(Focus::Diff);
    }
}
