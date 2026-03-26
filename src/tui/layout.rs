use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// The main layout of the TUI.
/// ┌────────────────────────────────┐
/// │         Status Bar             │
/// ├────────────────────────────────┤
/// │                                │
/// │         Chat Area              │
/// │                                │
/// ├────────────────────────────────┤
/// │         Input Area             │
/// └────────────────────────────────┘
pub fn main_layout(area: Rect) -> MainLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Status bar
            Constraint::Min(3),      // Chat area
            Constraint::Length(3),   // Input area
        ])
        .split(area);

    MainLayout {
        status: chunks[0],
        chat: chunks[1],
        input: chunks[2],
    }
}

pub struct MainLayout {
    pub status: Rect,
    pub chat: Rect,
    pub input: Rect,
}

/// Layout for the approval prompt overlay
pub fn approval_overlay(area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
        ])
        .split(area);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Percentage(70),
            Constraint::Percentage(15),
        ])
        .split(vertical[1]);

    horizontal[1]
}
