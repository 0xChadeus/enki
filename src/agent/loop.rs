use anyhow::Result;
use tokio::sync::mpsc;

use crate::agent::context::ContextManager;
use crate::agent::conversation::Conversation;
use crate::agent::system_prompt;
use crate::config::Settings;
use crate::llm::capabilities::ModelCapabilities;
use crate::llm::client::OllamaClient;
use crate::llm::types::*;
use crate::tools::ToolRegistry;

/// Events emitted by the agent loop for the TUI to render
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Streaming text from the assistant
    TextDelta(String),
    /// Thinking text (from JSON fallback)
    Thinking(String),
    /// Agent is about to call a tool
    ToolCallStart { name: String, arguments: serde_json::Value },
    /// Tool execution completed
    ToolResult { name: String, result: String, is_error: bool },
    /// Tool requires user approval
    NeedsApproval {
        tool_name: String,
        description: String,
        preview: Option<String>,
        arguments: serde_json::Value,
    },
    /// Agent is asking the user a question
    AskUser(String),
    /// Agent has completed the task
    Complete(String),
    /// Token usage update
    UsageUpdate { prompt_tokens: u32, completion_tokens: u32, context_pct: f64 },
    /// An error occurred
    Error(String),
    /// Agent turn finished (no more events for this turn)
    TurnComplete,
}

/// The core agentic loop
pub struct AgentLoop {
    client: OllamaClient,
    capabilities: ModelCapabilities,
    settings: Settings,
    tools: ToolRegistry,
    conversation: Conversation,
    context_manager: ContextManager,
    system_prompt: String,
    working_dir: std::path::PathBuf,
}

impl AgentLoop {
    pub fn new(
        client: OllamaClient,
        capabilities: ModelCapabilities,
        settings: Settings,
        working_dir: std::path::PathBuf,
    ) -> Self {
        let mut tools = ToolRegistry::new();
        tools.register_defaults(working_dir.clone());

        let project_instructions = system_prompt::load_project_instructions(&working_dir);
        let sys_prompt = system_prompt::build_system_prompt(
            &working_dir,
            &capabilities,
            &tools,
            &project_instructions,
        );

        let sys_prompt_tokens = ContextManager::estimate_tokens(&sys_prompt);
        let context_manager = ContextManager::new(
            capabilities.history_budget(),
            sys_prompt_tokens,
        );

        Self {
            client,
            capabilities,
            settings,
            tools,
            conversation: Conversation::new(),
            context_manager,
            system_prompt: sys_prompt,
            working_dir,
        }
    }

    /// Process a user message and run the agentic loop.
    /// Emits AgentEvents through the returned receiver.
    pub async fn process_message(
        &mut self,
        user_message: &str,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) {
        self.conversation.add_user_message(user_message);

        // Truncate history if needed
        if self.context_manager.needs_compaction(&self.conversation.messages) {
            self.context_manager.truncate_history(&mut self.conversation.messages);
        }

        let mut iteration = 0;
        let max_iterations = self.settings.max_iterations;

        loop {
            iteration += 1;
            if iteration > max_iterations {
                let _ = event_tx.send(AgentEvent::Error(format!(
                    "Max iterations ({}) reached. Stopping.",
                    max_iterations
                )));
                break;
            }

            // Build the request
            let request = self.build_chat_request();

            // Call the LLM
            match self.execute_llm_turn(&request, &event_tx).await {
                Ok(TurnOutcome::Continue) => continue,
                Ok(TurnOutcome::Complete) => break,
                Ok(TurnOutcome::WaitForUser) => break,
                Err(e) => {
                    let _ = event_tx.send(AgentEvent::Error(e.to_string()));
                    break;
                }
            }
        }

        // Send usage update
        let ctx_pct = self.context_manager.usage_percentage(&self.conversation.messages);
        let _ = event_tx.send(AgentEvent::UsageUpdate {
            prompt_tokens: self.conversation.total_prompt_tokens,
            completion_tokens: self.conversation.total_completion_tokens,
            context_pct: ctx_pct,
        });

        let _ = event_tx.send(AgentEvent::TurnComplete);
    }

    fn build_chat_request(&self) -> ChatRequest {
        let mut messages = vec![Message {
            role: Role::System,
            content: self.system_prompt.clone(),
            tool_calls: None,
            tool_name: None,
        }];
        messages.extend(self.conversation.messages.clone());

        let (tools, format) = if self.capabilities.supports_tools {
            (Some(self.tools.tool_definitions()), None)
        } else {
            // JSON fallback: use format parameter to force structured output
            (None, Some(tool_call_fallback_schema()))
        };

        let options = Some(ModelOptions {
            temperature: Some(0.1),
            num_ctx: Some(self.capabilities.context_length),
            num_predict: Some(-1), // no limit
            top_p: None,
            top_k: None,
            seed: None,
            stop: None,
        });

        ChatRequest {
            model: self.capabilities.model_name.clone(),
            messages,
            tools,
            format,
            stream: Some(true),
            options,
            keep_alive: None,
        }
    }

    async fn execute_llm_turn(
        &mut self,
        request: &ChatRequest,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<TurnOutcome> {
        let mut rx = self.client.chat_stream(request).await?;
        let mut full_text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        // Collect the streaming response
        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::TextDelta(text) => {
                    full_text.push_str(&text);
                    let _ = event_tx.send(AgentEvent::TextDelta(text));
                }
                StreamEvent::ToolCall(tc) => {
                    tool_calls.push(tc);
                }
                StreamEvent::Done(stats) => {
                    self.conversation.record_usage(&stats);
                }
                StreamEvent::Error(e) => {
                    return Err(anyhow::anyhow!("LLM stream error: {}", e));
                }
            }
        }

        // Determine what happened: native tool call, JSON fallback, or text response

        // Path A: Native tool calling
        if !tool_calls.is_empty() {
            return self.handle_tool_calls(tool_calls, &full_text, event_tx).await;
        }

        // Path B: JSON fallback (no native tool support)
        if !self.capabilities.supports_tools && !full_text.is_empty() {
            return self.handle_fallback_response(&full_text, event_tx).await;
        }

        // Path C: Pure text response (no tool calls)
        if !full_text.is_empty() {
            self.conversation.add_assistant_message(&full_text);
        }

        Ok(TurnOutcome::Complete)
    }

    async fn handle_tool_calls(
        &mut self,
        tool_calls: Vec<ToolCall>,
        assistant_text: &str,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<TurnOutcome> {
        // Add assistant message with text (if any)
        if !assistant_text.is_empty() {
            self.conversation.add_assistant_message(assistant_text);
        }

        for tc in tool_calls {
            let _ = event_tx.send(AgentEvent::ToolCallStart {
                name: tc.name.clone(),
                arguments: tc.arguments.clone(),
            });

            self.conversation.add_assistant_tool_call(&tc.name, &tc.arguments);

            // Execute the tool
            let result = self.tools.execute(&tc.name, tc.arguments.clone()).await;
            let outcome = self.handle_tool_result(&tc.name, result, &tc.arguments, event_tx).await?;

            match outcome {
                TurnOutcome::Continue => {}
                other => return Ok(other),
            }
        }

        Ok(TurnOutcome::Continue)
    }

    async fn handle_fallback_response(
        &mut self,
        response_text: &str,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<TurnOutcome> {
        match parse_fallback_tool_call(response_text) {
            Ok(fallback) => {
                // Emit thinking
                if !fallback.thinking.is_empty() {
                    let _ = event_tx.send(AgentEvent::Thinking(fallback.thinking.clone()));
                }

                if let Some(tc) = fallback.tool_call {
                    // It's a tool call
                    let _ = event_tx.send(AgentEvent::ToolCallStart {
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    });

                    self.conversation.add_assistant_tool_call(&tc.name, &tc.arguments);

                    let result = self.tools.execute(&tc.name, tc.arguments.clone()).await;
                    self.handle_tool_result(&tc.name, result, &tc.arguments, event_tx).await
                } else if let Some(response) = fallback.response {
                    // It's a text response
                    self.conversation.add_assistant_message(&response);
                    Ok(TurnOutcome::Complete)
                } else {
                    // Just thinking with no action
                    self.conversation.add_assistant_message(&fallback.thinking);
                    Ok(TurnOutcome::Complete)
                }
            }
            Err(e) => {
                // Failed to parse as JSON — treat as plain text
                self.conversation.add_assistant_message(response_text);
                let _ = event_tx.send(AgentEvent::Error(format!(
                    "Failed to parse model response as JSON: {}",
                    e
                )));
                Ok(TurnOutcome::Complete)
            }
        }
    }

    async fn handle_tool_result(
        &mut self,
        tool_name: &str,
        result: crate::tools::types::ToolResult,
        _arguments: &serde_json::Value,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<TurnOutcome> {
        match result {
            crate::tools::types::ToolResult::Success(output) => {
                let truncated = ContextManager::truncate_tool_result(&output, 4096);
                let _ = event_tx.send(AgentEvent::ToolResult {
                    name: tool_name.to_string(),
                    result: truncated.clone(),
                    is_error: false,
                });
                self.conversation.add_tool_result(tool_name, &truncated);
                Ok(TurnOutcome::Continue)
            }
            crate::tools::types::ToolResult::Error(err) => {
                let _ = event_tx.send(AgentEvent::ToolResult {
                    name: tool_name.to_string(),
                    result: err.clone(),
                    is_error: true,
                });
                self.conversation.add_tool_result(tool_name, &format!("ERROR: {}", err));
                Ok(TurnOutcome::Continue)
            }
            crate::tools::types::ToolResult::AskUser(question) => {
                let _ = event_tx.send(AgentEvent::AskUser(question));
                Ok(TurnOutcome::WaitForUser)
            }
            crate::tools::types::ToolResult::Complete(result) => {
                let _ = event_tx.send(AgentEvent::Complete(result));
                Ok(TurnOutcome::Complete)
            }
            crate::tools::types::ToolResult::NeedsApproval(pending) => {
                let _ = event_tx.send(AgentEvent::NeedsApproval {
                    tool_name: pending.tool_name,
                    description: pending.description,
                    preview: pending.preview,
                    arguments: pending.arguments,
                });
                // The TUI will handle approval and call execute_approved_action
                Ok(TurnOutcome::WaitForUser)
            }
        }
    }

    /// Execute an approved action (called by TUI after user approves)
    pub async fn execute_approved_action(
        &mut self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> crate::tools::types::ToolResult {
        match tool_name {
            "write_file" => crate::tools::file_write::execute_write(&self.working_dir, arguments),
            "edit_file" => crate::tools::file_edit::execute_edit(&self.working_dir, arguments),
            "bash" => crate::tools::bash::execute_bash(&self.working_dir, arguments).await,
            _ => crate::tools::types::ToolResult::Error(format!("Unknown tool: {}", tool_name)),
        }
    }

    /// Add an approved tool result back and continue the loop
    pub fn add_tool_result_and_continue(&mut self, tool_name: &str, result: &str) {
        self.conversation.add_tool_result(tool_name, result);
    }

    /// Get conversation reference
    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// Get context usage percentage
    pub fn context_usage(&self) -> f64 {
        self.context_manager.usage_percentage(&self.conversation.messages)
    }

    /// Get the current model name
    pub fn model_name(&self) -> &str {
        &self.capabilities.model_name
    }
}

enum TurnOutcome {
    /// The agent wants to continue (made tool calls, needs to process results)
    Continue,
    /// The agent is done (text response or attempt_completion)
    Complete,
    /// The agent is waiting for user input (ask_user or needs_approval)
    WaitForUser,
}
