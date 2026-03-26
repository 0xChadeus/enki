use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::text::{Line, Span};

/// Render the status bar
pub fn render_status(
    frame: &mut Frame,
    area: Rect,
    model_name: &str,
    context_pct: f64,
    state: &AppState,
    token_count: u32,
) {
    let state_indicator = match state {
        AppState::Idle => Span::styled(" ● READY ", Style::default().fg(Color::Green)),
        AppState::Thinking => Span::styled(" ◉ THINKING ", Style::default().fg(Color::Yellow)),
        AppState::ToolExec => Span::styled(" ⟐ TOOL ", Style::default().fg(Color::Cyan)),
        AppState::WaitingApproval => Span::styled(" ⚠ APPROVAL ", Style::default().fg(Color::Red)),
        AppState::Streaming => Span::styled(" ◉ STREAMING ", Style::default().fg(Color::Blue)),
    };

    let ctx_color = if context_pct > 80.0 {
        Color::Red
    } else if context_pct > 60.0 {
        Color::Yellow
    } else {
        Color::Green
    };

    let line = Line::from(vec![
        state_indicator,
        Span::raw(" │ "),
        Span::styled(
            format!("⬡ {}", model_name),
            Style::default().fg(Color::White).bold(),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("ctx {:.0}%", context_pct),
            Style::default().fg(ctx_color),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("{}tok", format_tokens(token_count)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 𒂗𒆠 Enki ")
                .border_style(Style::default().fg(Color::DarkGray)),
        );

    frame.render_widget(paragraph, area);
}

fn format_tokens(count: u32) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Current state of the application
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Idle,
    Thinking,
    ToolExec,
    WaitingApproval,
    Streaming,
}
