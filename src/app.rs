use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers, KeyEventKind};
use ratatui::prelude::*;
use tokio::sync::mpsc;

use crate::agent::r#loop::{AgentEvent, AgentLoop};
use crate::config::Settings;
use crate::llm::capabilities::ModelCapabilities;
use crate::llm::client::OllamaClient;
use crate::tui::chat::{ChatMessage, render_chat, render_approval};
use crate::tui::input::{InputState, SlashCommand, parse_slash_command, render_input};
use crate::tui::layout::{main_layout, approval_overlay};
use crate::tui::status::{AppState, render_status};

/// Pending approval state
struct PendingApproval {
    tool_name: String,
    description: String,
    preview: Option<String>,
    arguments: serde_json::Value,
}

/// Main application state
pub struct App {
    agent: AgentLoop,
    messages: Vec<ChatMessage>,
    input: InputState,
    state: AppState,
    scroll_offset: u16,
    model_name: String,
    context_pct: f64,
    total_tokens: u32,
    pending_approval: Option<PendingApproval>,
    event_rx: Option<mpsc::UnboundedReceiver<AgentEvent>>,
    should_quit: bool,
}

impl App {
    pub async fn new(settings: Settings, working_dir: PathBuf) -> Result<Self> {
        let client = OllamaClient::new(&settings.ollama_url);

        // Health check
        if let Err(e) = client.health_check().await {
            eprintln!("Warning: Cannot connect to Ollama at {}: {}", settings.ollama_url, e);
            eprintln!("Make sure Ollama is running. Continuing anyway...");
        }

        // Detect model capabilities
        let capabilities = ModelCapabilities::detect(&client, &settings.default_model).await
            .unwrap_or_else(|_| ModelCapabilities::fallback(&settings.default_model));
        let model_name = capabilities.model_name.clone();

        let agent = AgentLoop::new(client, capabilities, settings, working_dir);

        let mut app = Self {
            agent,
            messages: Vec::new(),
            input: InputState::new(),
            state: AppState::Idle,
            scroll_offset: 0,
            model_name,
            context_pct: 0.0,
            total_tokens: 0,
            pending_approval: None,
            event_rx: None,
            should_quit: false,
        };

        app.messages.push(ChatMessage::system(
            "Welcome to Enki — your local AI coding assistant.\nType a message or /help for commands.",
        ));

        Ok(app)
    }

    pub async fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
        loop {
            // Draw
            terminal.draw(|frame| self.render(frame))?;

            // Process agent events if any
            self.drain_agent_events();

            // Handle input events (with a small timeout to allow agent events to flow)
            if event::poll(std::time::Duration::from_millis(16))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    self.handle_key(key.code, key.modifiers).await?;
                }
            }

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let layout = main_layout(frame.area());

        // Status bar
        render_status(
            frame,
            layout.status,
            &self.model_name,
            self.context_pct,
            &self.state,
            self.total_tokens,
        );

        // Chat area
        render_chat(frame, layout.chat, &self.messages, self.scroll_offset);

        // Input area
        let is_active = self.state == AppState::Idle;
        render_input(
            frame,
            layout.input,
            &self.input.buffer,
            self.input.cursor,
            is_active,
        );

        // Approval overlay
        if let Some(ref approval) = self.pending_approval {
            let overlay_area = approval_overlay(frame.area());
            render_approval(
                frame,
                overlay_area,
                &approval.tool_name,
                &approval.description,
                approval.preview.as_deref(),
            );
        }
    }

    fn drain_agent_events(&mut self) {
        let mut events = Vec::new();
        if let Some(ref mut rx) = self.event_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            self.handle_agent_event(event);
        }
    }

    fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(text) => {
                self.state = AppState::Streaming;
                // Append to last streaming message or create one
                if let Some(last) = self.messages.last_mut() {
                    if last.is_streaming {
                        last.content.push_str(&text);
                        self.scroll_offset = 0; // auto-scroll
                        return;
                    }
                }
                let mut msg = ChatMessage::assistant_streaming();
                msg.content = text;
                self.messages.push(msg);
                self.scroll_offset = 0;
            }
            AgentEvent::Thinking(text) => {
                self.messages.push(ChatMessage::system(&format!("💭 {}", text)));
                self.scroll_offset = 0;
            }
            AgentEvent::ToolCallStart { name, arguments: _ } => {
                self.state = AppState::ToolExec;
                // Finalize any streaming message
                self.finalize_streaming();
                self.messages.push(ChatMessage::system(&format!("Calling {}...", name)));
                self.scroll_offset = 0;
            }
            AgentEvent::ToolResult { name, result, is_error } => {
                self.messages.push(ChatMessage::tool(&name, &result, is_error));
                self.scroll_offset = 0;
            }
            AgentEvent::NeedsApproval { tool_name, description, preview, arguments } => {
                self.state = AppState::WaitingApproval;
                self.finalize_streaming();
                self.pending_approval = Some(PendingApproval {
                    tool_name,
                    description,
                    preview,
                    arguments,
                });
            }
            AgentEvent::AskUser(question) => {
                self.state = AppState::Idle;
                self.finalize_streaming();
                self.messages.push(ChatMessage::assistant(&question));
                self.scroll_offset = 0;
            }
            AgentEvent::Complete(result) => {
                self.finalize_streaming();
                if !result.is_empty() {
                    self.messages.push(ChatMessage::assistant(&result));
                }
                self.scroll_offset = 0;
            }
            AgentEvent::UsageUpdate { prompt_tokens, completion_tokens, context_pct } => {
                self.total_tokens = prompt_tokens + completion_tokens;
                self.context_pct = context_pct;
            }
            AgentEvent::Error(err) => {
                self.messages.push(ChatMessage::error(&err));
                self.scroll_offset = 0;
            }
            AgentEvent::TurnComplete => {
                self.state = AppState::Idle;
                self.finalize_streaming();
            }
        }
    }

    fn finalize_streaming(&mut self) {
        if let Some(last) = self.messages.last_mut() {
            if last.is_streaming {
                last.is_streaming = false;
            }
        }
    }

    async fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        // Global keybindings
        match (code, modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.state != AppState::Idle {
                    // Cancel current operation
                    self.state = AppState::Idle;
                    self.event_rx = None;
                    self.finalize_streaming();
                    self.messages.push(ChatMessage::system("Cancelled."));
                } else {
                    self.should_quit = true;
                }
                return Ok(());
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return Ok(());
            }
            _ => {}
        }

        // Approval mode keybindings
        if self.state == AppState::WaitingApproval {
            if let Some(approval) = self.pending_approval.take() {
                match code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        self.state = AppState::ToolExec;
                        let result = self.agent.execute_approved_action(
                            &approval.tool_name,
                            &approval.arguments,
                        ).await;

                        let (content, is_error) = match &result {
                            crate::tools::types::ToolResult::Success(s) => (s.clone(), false),
                            crate::tools::types::ToolResult::Error(e) => (e.clone(), true),
                            _ => ("Done.".to_string(), false),
                        };

                        self.messages.push(ChatMessage::tool(&approval.tool_name, &content, is_error));
                        self.agent.add_tool_result_and_continue(&approval.tool_name, &content);

                        // Continue the agent loop
                        self.spawn_agent_continuation();
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.agent.add_tool_result_and_continue(
                            &approval.tool_name,
                            "DENIED: User rejected this action.",
                        );
                        self.messages.push(ChatMessage::system("Action denied."));
                        self.spawn_agent_continuation();
                    }
                    _ => {
                        // Put it back
                        self.pending_approval = Some(approval);
                    }
                }
            }
            return Ok(());
        }

        // Normal input mode
        if self.state != AppState::Idle {
            return Ok(()); // Ignore input while agent is working
        }

        match (code, modifiers) {
            (KeyCode::Enter, _) => {
                let text = self.input.submit();
                if text.is_empty() {
                    return Ok(());
                }

                // Check for slash commands
                if let Some(cmd) = parse_slash_command(&text) {
                    self.handle_slash_command(cmd);
                    return Ok(());
                }

                // Send to agent
                self.messages.push(ChatMessage::user(&text));
                self.scroll_offset = 0;
                self.state = AppState::Thinking;

                let (tx, rx) = mpsc::unbounded_channel();
                self.event_rx = Some(rx);
                self.agent.process_message(&text, tx).await;
            }
            (KeyCode::Backspace, _) => self.input.delete_char(),
            (KeyCode::Delete, _) => self.input.delete_forward(),
            (KeyCode::Left, _) => self.input.move_left(),
            (KeyCode::Right, _) => self.input.move_right(),
            (KeyCode::Home, _) => self.input.move_home(),
            (KeyCode::End, _) => self.input.move_end(),
            (KeyCode::Up, _) => self.input.history_up(),
            (KeyCode::Down, _) => self.input.history_down(),
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => self.input.move_home(),
            (KeyCode::Char('e'), KeyModifiers::CONTROL) => self.input.move_end(),
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => self.input.kill_line(),
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => self.input.kill_word_back(),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.input.clear(),
            (KeyCode::PageUp, _) => {
                self.scroll_offset = self.scroll_offset.saturating_add(10);
            }
            (KeyCode::PageDown, _) => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
            }
            (KeyCode::Char(c), _) => self.input.insert_char(c),
            _ => {}
        }
        Ok(())
    }

    fn handle_slash_command(&mut self, cmd: SlashCommand) {
        match cmd {
            SlashCommand::Help => {
                self.messages.push(ChatMessage::system(
                    "Commands:\n  /help     — Show this help\n  /clear    — Clear chat\n  /model <n> — Switch model\n  /compact  — Compact context\n  /save     — Save session\n  /quit     — Exit Enki",
                ));
            }
            SlashCommand::Clear => {
                self.messages.clear();
                self.messages.push(ChatMessage::system("Chat cleared."));
            }
            SlashCommand::Model(name) => {
                if name.is_empty() {
                    self.messages.push(ChatMessage::system(&format!(
                        "Current model: {}",
                        self.model_name,
                    )));
                } else {
                    self.messages.push(ChatMessage::system(&format!(
                        "Model switching will be implemented. Current: {}",
                        self.model_name,
                    )));
                }
            }
            SlashCommand::Compact => {
                self.messages.push(ChatMessage::system("Context compaction triggered."));
            }
            SlashCommand::Save => {
                let data_dir = crate::config::Settings::data_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from(".enki"));
                match self.agent.conversation().save(&data_dir) {
                    Ok(()) => self.messages.push(ChatMessage::system(
                        "Session saved."
                    )),
                    Err(e) => self.messages.push(ChatMessage::error(&format!(
                        "Failed to save: {}",
                        e
                    ))),
                }
            }
            SlashCommand::Quit => {
                self.should_quit = true;
            }
            SlashCommand::Unknown(cmd) => {
                self.messages.push(ChatMessage::error(&format!(
                    "Unknown command: /{}. Type /help for available commands.",
                    cmd
                )));
            }
        }
        self.scroll_offset = 0;
    }

    fn spawn_agent_continuation(&mut self) {
        // After approval/denial, the agent needs to continue processing
        // This will be handled by the main loop picking up remaining events
        self.state = AppState::Thinking;
    }
}
