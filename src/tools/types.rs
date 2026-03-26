use async_trait::async_trait;
use serde_json::Value;

use crate::llm::types::ToolDefinition;

/// Result of executing a tool
#[derive(Debug, Clone)]
pub enum ToolResult {
    /// Tool executed successfully, contains output text
    Success(String),
    /// Tool execution failed, contains error message
    Error(String),
    /// Tool is asking the user a question, agent should pause
    AskUser(String),
    /// Agent is signaling task completion
    Complete(String),
    /// Tool needs user approval before executing (contains description of action)
    NeedsApproval(PendingAction),
}

/// An action that requires user approval before execution
#[derive(Debug, Clone)]
pub struct PendingAction {
    pub tool_name: String,
    pub description: String,
    pub preview: Option<String>,
    pub arguments: Value,
}

/// Trait that all tools must implement
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name of the tool
    fn name(&self) -> &str;

    /// Human-readable description of what the tool does
    fn description(&self) -> &str;

    /// JSON schema for tool parameters
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with the given arguments
    async fn execute(&self, arguments: Value) -> ToolResult;

    /// Whether this tool requires user approval before execution
    fn requires_approval(&self) -> bool {
        false
    }

    /// Get the Ollama tool definition
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(self.name(), self.description(), self.parameters_schema())
    }
}
