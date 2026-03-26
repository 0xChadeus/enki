use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::llm::types::{Message, Role, UsageStats};

/// A single turn in the conversation (for persistence)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub timestamp: DateTime<Utc>,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub token_usage: Option<TurnUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// Manages the conversation state and persistence
pub struct Conversation {
    /// The LLM message history (used for API calls)
    pub messages: Vec<Message>,
    /// Full history for persistence (includes metadata)
    pub turns: Vec<ConversationTurn>,
    /// Session ID
    pub session_id: String,
    /// Total tokens used in this conversation
    pub total_prompt_tokens: u32,
    pub total_completion_tokens: u32,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            turns: Vec::new(),
            session_id: uuid::Uuid::new_v4().to_string(),
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
        }
    }

    /// Add a user message
    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(Message {
            role: Role::User,
            content: content.to_string(),
            tool_calls: None,
            tool_name: None,
        });
        self.turns.push(ConversationTurn {
            timestamp: Utc::now(),
            role: "user".to_string(),
            content: content.to_string(),
            tool_name: None,
            token_usage: None,
        });
    }

    /// Add an assistant message
    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages.push(Message {
            role: Role::Assistant,
            content: content.to_string(),
            tool_calls: None,
            tool_name: None,
        });
        self.turns.push(ConversationTurn {
            timestamp: Utc::now(),
            role: "assistant".to_string(),
            content: content.to_string(),
            tool_name: None,
            token_usage: None,
        });
    }

    /// Add an assistant message with tool calls (native path)
    pub fn add_assistant_tool_call(&mut self, tool_name: &str, arguments: &serde_json::Value) {
        let content = format!("Calling tool: {} with args: {}", tool_name, arguments);
        self.messages.push(Message {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: Some(vec![crate::llm::types::ToolCallResponse {
                function: crate::llm::types::ToolCallFunction {
                    name: tool_name.to_string(),
                    arguments: arguments.clone(),
                },
            }]),
            tool_name: None,
        });
        self.turns.push(ConversationTurn {
            timestamp: Utc::now(),
            role: "assistant".to_string(),
            content,
            tool_name: Some(tool_name.to_string()),
            token_usage: None,
        });
    }

    /// Add a tool result message
    pub fn add_tool_result(&mut self, tool_name: &str, result: &str) {
        self.messages.push(Message {
            role: Role::Tool,
            content: result.to_string(),
            tool_calls: None,
            tool_name: Some(tool_name.to_string()),
        });
        self.turns.push(ConversationTurn {
            timestamp: Utc::now(),
            role: "tool".to_string(),
            content: result.to_string(),
            tool_name: Some(tool_name.to_string()),
            token_usage: None,
        });
    }

    /// Record usage stats from a completed LLM call
    pub fn record_usage(&mut self, stats: &UsageStats) {
        self.total_prompt_tokens += stats.prompt_tokens;
        self.total_completion_tokens += stats.completion_tokens;
    }

    /// Save conversation to disk
    pub fn save(&self, data_dir: &PathBuf) -> anyhow::Result<()> {
        let sessions_dir = data_dir.join("sessions");
        std::fs::create_dir_all(&sessions_dir)?;

        let file_path = sessions_dir.join(format!("{}.json", self.session_id));
        let json = serde_json::to_string_pretty(&self.turns)?;
        std::fs::write(&file_path, json)?;
        Ok(())
    }

    /// Get the number of messages
    pub fn len(&self) -> usize {
        self.messages.len()
    }
}
