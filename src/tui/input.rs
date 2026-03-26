use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render the input area
pub fn render_input(
    frame: &mut Frame,
    area: Rect,
    input: &str,
    cursor_position: usize,
    is_active: bool,
) {
    let style = if is_active {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let display = if input.is_empty() && is_active {
        "Type a message... (/help for commands)".to_string()
    } else {
        input.to_string()
    };

    let border_style = if is_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let paragraph = Paragraph::new(display.as_str())
        .style(if input.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            style
        })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Input ")
                .border_style(border_style),
        );

    frame.render_widget(paragraph, area);

    // Place cursor
    if is_active {
        let cursor_x = area.x + 1 + cursor_position as u16;
        let cursor_y = area.y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Slash commands that the user can invoke
pub enum SlashCommand {
    Help,
    Clear,
    Model(String),
    Compact,
    Save,
    Quit,
    Unknown(String),
}

pub fn parse_slash_command(input: &str) -> Option<SlashCommand> {
    if !input.starts_with('/') {
        return None;
    }

    let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
    let cmd = parts[0].to_lowercase();
    let arg = parts.get(1).map(|s| s.to_string());

    Some(match cmd.as_str() {
        "help" | "h" => SlashCommand::Help,
        "clear" => SlashCommand::Clear,
        "model" | "m" => SlashCommand::Model(arg.unwrap_or_default()),
        "compact" => SlashCommand::Compact,
        "save" => SlashCommand::Save,
        "quit" | "q" | "exit" => SlashCommand::Quit,
        _ => SlashCommand::Unknown(cmd),
    })
}

/// Input buffer state
pub struct InputState {
    pub buffer: String,
    pub cursor: usize,
    /// History of previous inputs
    pub history: Vec<String>,
    pub history_index: Option<usize>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.buffer.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn delete_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
        }
    }

    pub fn delete_forward(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_index = None;
    }

    pub fn submit(&mut self) -> String {
        let text = self.buffer.clone();
        if !text.is_empty() {
            self.history.push(text.clone());
        }
        self.clear();
        text
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_index {
            Some(i) if i > 0 => i - 1,
            Some(i) => i,
            None => self.history.len() - 1,
        };
        self.history_index = Some(idx);
        self.buffer = self.history[idx].clone();
        self.cursor = self.buffer.len();
    }

    pub fn history_down(&mut self) {
        match self.history_index {
            Some(i) if i < self.history.len() - 1 => {
                self.history_index = Some(i + 1);
                self.buffer = self.history[i + 1].clone();
                self.cursor = self.buffer.len();
            }
            Some(_) => {
                self.history_index = None;
                self.buffer.clear();
                self.cursor = 0;
            }
            None => {}
        }
    }

    pub fn kill_line(&mut self) {
        self.buffer.truncate(self.cursor);
    }

    pub fn kill_word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut end = self.cursor;
        // Skip whitespace
        while end > 0 && self.buffer.as_bytes()[end - 1] == b' ' {
            end -= 1;
        }
        // Skip word chars
        while end > 0 && self.buffer.as_bytes()[end - 1] != b' ' {
            end -= 1;
        }
        self.buffer.drain(end..self.cursor);
        self.cursor = end;
    }
}
