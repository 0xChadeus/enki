use std::path::PathBuf;

use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

use crate::agent::r#loop::{AgentEvent, AgentLoop};
use crate::config::Settings;
use crate::llm::capabilities::ModelCapabilities;
use crate::llm::client::OllamaClient;

/// Commands sent from the handle to the agent task
pub enum AgentCommand {
    /// Process a user message; events stream back on the provided sender
    ProcessMessage {
        text: String,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    },
    /// Execute an approved tool action
    ExecuteApproved {
        tool_name: String,
        arguments: serde_json::Value,
        reply: oneshot::Sender<crate::tools::types::ToolResult>,
    },
    /// Add a tool result and let the agent continue
    AddToolResult {
        tool_name: String,
        result: String,
    },
    /// Get the session id
    GetSessionId {
        reply: oneshot::Sender<String>,
    },
    /// Save the conversation
    SaveConversation {
        data_dir: PathBuf,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Get context usage percentage
    GetContextUsage {
        reply: oneshot::Sender<f64>,
    },
    /// Shut down the agent task
    Shutdown,
}

/// A handle to an `AgentLoop` running in a background Tokio task.
///
/// All interaction goes through channel messages, so the TUI event loop
/// is never blocked by LLM calls or tool execution.
#[derive(Clone)]
pub struct AgentHandle {
    cmd_tx: mpsc::UnboundedSender<AgentCommand>,
    model_name: String,
}

impl AgentHandle {
    /// Create a new agent and spawn it on a background Tokio task.
    /// Returns a handle for sending commands.
    pub async fn spawn(settings: Settings, working_dir: PathBuf) -> Result<Self> {
        let client = OllamaClient::new(&settings.ollama_url);

        // Health check (non-fatal)
        if let Err(e) = client.health_check().await {
            eprintln!(
                "Warning: Cannot connect to Ollama at {}: {}",
                settings.ollama_url, e
            );
            eprintln!("Make sure Ollama is running. Continuing anyway...");
        }

        // Detect model capabilities
        let capabilities =
            ModelCapabilities::detect(&client, &settings.default_model)
                .await
                .unwrap_or_else(|_| ModelCapabilities::fallback(&settings.default_model));
        let model_name = capabilities.model_name.clone();

        let agent = AgentLoop::new(client, capabilities, settings, working_dir);

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        tokio::spawn(agent_task(agent, cmd_rx));

        Ok(Self { cmd_tx, model_name })
    }

    /// Create a handle from an already-constructed AgentLoop (used by daemon sessions).
    pub fn spawn_from(agent: AgentLoop) -> Self {
        let model_name = agent.model_name().to_string();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        tokio::spawn(agent_task(agent, cmd_rx));
        Self { cmd_tx, model_name }
    }

    /// Send a user message. Events stream back on the returned receiver.
    pub fn send_message(&self, text: String) -> mpsc::UnboundedReceiver<AgentEvent> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let _ = self.cmd_tx.send(AgentCommand::ProcessMessage { text, event_tx });
        event_rx
    }

    /// Execute an approved tool action, returning the tool result.
    pub async fn execute_approved(
        &self,
        tool_name: String,
        arguments: serde_json::Value,
    ) -> Result<crate::tools::types::ToolResult> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self.cmd_tx.send(AgentCommand::ExecuteApproved {
            tool_name,
            arguments,
            reply: reply_tx,
        });
        Ok(reply_rx.await?)
    }

    /// Add a tool result and let the agent continue on the next message.
    pub fn add_tool_result(&self, tool_name: String, result: String) {
        let _ = self.cmd_tx.send(AgentCommand::AddToolResult { tool_name, result });
    }

    /// Get the current session id.
    pub async fn session_id(&self) -> Result<String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self.cmd_tx.send(AgentCommand::GetSessionId { reply: reply_tx });
        Ok(reply_rx.await?)
    }

    /// Save the conversation to disk.
    pub async fn save_conversation(&self, data_dir: PathBuf) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self.cmd_tx.send(AgentCommand::SaveConversation {
            data_dir,
            reply: reply_tx,
        });
        reply_rx.await?
    }

    /// Get context usage percentage.
    pub async fn context_usage(&self) -> Result<f64> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self.cmd_tx.send(AgentCommand::GetContextUsage { reply: reply_tx });
        Ok(reply_rx.await?)
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Shut down the background agent task.
    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(AgentCommand::Shutdown);
    }
}

/// Background task that owns the AgentLoop and processes commands sequentially.
async fn agent_task(mut agent: AgentLoop, mut cmd_rx: mpsc::UnboundedReceiver<AgentCommand>) {
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            AgentCommand::ProcessMessage { text, event_tx } => {
                agent.process_message(&text, event_tx).await;
            }
            AgentCommand::ExecuteApproved {
                tool_name,
                arguments,
                reply,
            } => {
                let result = agent.execute_approved_action(&tool_name, &arguments).await;
                let _ = reply.send(result);
            }
            AgentCommand::AddToolResult { tool_name, result } => {
                agent.add_tool_result_and_continue(&tool_name, &result);
            }
            AgentCommand::GetSessionId { reply } => {
                let _ = reply.send(agent.conversation().session_id.clone());
            }
            AgentCommand::SaveConversation { data_dir, reply } => {
                let _ = reply.send(agent.conversation().save(&data_dir));
            }
            AgentCommand::GetContextUsage { reply } => {
                let _ = reply.send(agent.context_usage());
            }
            AgentCommand::Shutdown => break,
        }
    }
}
