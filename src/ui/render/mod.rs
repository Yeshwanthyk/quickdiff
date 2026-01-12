//! UI rendering with ratatui.
//!
//! Design: Themeable dark UI with editorial minimalism.
//! - Muted UI chrome lets syntax highlighting pop
//! - Subtle diff backgrounds don't compete with code colors
//! - Single accent color for focus states

mod bars;
mod diff;
mod helpers;
mod overlays;
mod sidebar;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    widgets::Block,
    Frame,
};

use super::app::{App, Mode};

// Re-export for potential external use
#[allow(unused_imports)]
pub use helpers::{
    build_path_cache, SpanBuilder, ThemeStyles, GUTTER_WIDTH, SIDEBAR_PATH_WIDTH, TAB_WIDTH,
};

/// Main render function.
pub fn render(frame: &mut Frame, app: &mut App) {
    let _timer = crate::metrics::Timer::start("render_frame");

    // Fill background
    let bg_block = Block::default().style(app.theme_styles.bg_dark);
    frame.render_widget(bg_block, frame.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top bar
            Constraint::Min(0),    // Main content
            Constraint::Length(1), // Bottom bar
        ])
        .split(frame.area());

    bars::render_top_bar(frame, app, chunks[0]);
    render_main(frame, app, chunks[1]);
    bars::render_bottom_bar(frame, app, chunks[2]);

    // Overlays
    match app.ui.mode {
        Mode::ViewComments => overlays::render_comments_overlay(frame, app),
        Mode::SelectTheme => overlays::render_theme_selector(frame, app),
        Mode::Help => overlays::render_help_overlay(frame, app),
        Mode::PRPicker => overlays::render_pr_picker_overlay(frame, app),
        Mode::PRAction => overlays::render_pr_action_overlay(frame, app),
        _ => {}
    }
}

fn render_main(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(32), // Sidebar
            Constraint::Min(0),     // Diff view
        ])
        .split(area);

    sidebar::render_sidebar(frame, app, chunks[0]);
    diff::render_diff(frame, app, chunks[1]);
}
