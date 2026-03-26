use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::text::{Line, Span};

/// A displayable chat message in the TUI
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    /// If this is a tool call, the tool name
    pub tool_name: Option<String>,
    /// Whether this message is still streaming
    pub is_streaming: bool,
    /// Whether this is an error
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
    System,
}

impl ChatMessage {
    pub fn user(content: &str) -> Self {
        Self {
            role: ChatRole::User,
            content: content.to_string(),
            tool_name: None,
            is_streaming: false,
            is_error: false,
        }
    }

    pub fn assistant(content: &str) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.to_string(),
            tool_name: None,
            is_streaming: false,
            is_error: false,
        }
    }

    pub fn assistant_streaming() -> Self {
        Self {
            role: ChatRole::Assistant,
            content: String::new(),
            tool_name: None,
            is_streaming: true,
            is_error: false,
        }
    }

    pub fn tool(name: &str, result: &str, is_error: bool) -> Self {
        Self {
            role: ChatRole::Tool,
            content: result.to_string(),
            tool_name: Some(name.to_string()),
            is_streaming: false,
            is_error,
        }
    }

    pub fn system(content: &str) -> Self {
        Self {
            role: ChatRole::System,
            content: content.to_string(),
            tool_name: None,
            is_streaming: false,
            is_error: false,
        }
    }

    pub fn error(content: &str) -> Self {
        Self {
            role: ChatRole::System,
            content: content.to_string(),
            tool_name: None,
            is_streaming: false,
            is_error: true,
        }
    }
}

/// Render the chat area with all messages
pub fn render_chat(
    frame: &mut Frame,
    area: Rect,
    messages: &[ChatMessage],
    scroll_offset: u16,
) {
    let mut lines: Vec<Line> = Vec::new();

    for msg in messages {
        let (prefix, style) = match (&msg.role, msg.is_error) {
            (_, true) => ("✗ ", Style::default().fg(Color::Red)),
            (ChatRole::User, _) => ("❯ ", Style::default().fg(Color::Green).bold()),
            (ChatRole::Assistant, _) => ("⚡ ", Style::default().fg(Color::Cyan)),
            (ChatRole::Tool, _) => {
                let name = msg.tool_name.as_deref().unwrap_or("tool");
                let header_style = if msg.is_error {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("⟐ {} ", name), header_style),
                ]));
                // Show content with dimmed style
                for line in msg.content.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", line),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                lines.push(Line::from(""));
                continue;
            }
            (ChatRole::System, _) => ("◆ ", Style::default().fg(Color::Magenta)),
        };

        // Render message header and content
        let content: String = if msg.is_streaming && msg.content.is_empty() {
            "▌".to_string()
        } else if msg.is_streaming {
            format!("{}▌", msg.content)
        } else {
            msg.content.clone()
        };

        let text_lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut first = true;
        for line in &text_lines {
            if first {
                lines.push(Line::from(vec![
                    Span::styled(prefix.to_string(), style),
                    Span::styled(line.clone(), style),
                ]));
                first = false;
            } else {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    style,
                )));
            }
        }
        if first {
            // Empty content with cursor
            lines.push(Line::from(Span::styled(prefix, style)));
        }
        lines.push(Line::from(""));
    }

    let total_lines = lines.len() as u16;
    let visible = area.height.saturating_sub(2); // account for borders
    let scroll = if scroll_offset == 0 {
        // Auto-scroll to bottom
        total_lines.saturating_sub(visible)
    } else {
        total_lines.saturating_sub(visible).saturating_sub(scroll_offset)
    };

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Enki "))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}

/// Render the approval prompt
pub fn render_approval(
    frame: &mut Frame,
    area: Rect,
    tool_name: &str,
    description: &str,
    preview: Option<&str>,
) {
    let mut lines = vec![
        Line::from(Span::styled(
            format!("Tool: {}", tool_name),
            Style::default().fg(Color::Yellow).bold(),
        )),
        Line::from(""),
        Line::from(description.to_string()),
    ];

    if let Some(prev) = preview {
        lines.push(Line::from(""));
        for line in prev.lines().take(20) {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[y]", Style::default().fg(Color::Green).bold()),
        Span::raw(" Approve  "),
        Span::styled("[n]", Style::default().fg(Color::Red).bold()),
        Span::raw(" Deny  "),
        Span::styled("[a]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Always approve"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ⚠ Permission Required ")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}
