use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::types::{Tool, ToolResult};

// ── AttemptCompletion ───────────────────────────────────────────────────────

pub struct AttemptCompletionTool;

#[async_trait]
impl Tool for AttemptCompletionTool {
    fn name(&self) -> &str {
        "attempt_completion"
    }

    fn description(&self) -> &str {
        "Signal that the task is complete. Provide a result message summarizing what was done. The user will review and may provide feedback to continue."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "result": {
                    "type": "string",
                    "description": "A summary of what was accomplished"
                }
            },
            "required": ["result"]
        })
    }

    async fn execute(&self, arguments: Value) -> ToolResult {
        let result = arguments
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("Task completed.");

        ToolResult::Complete(result.to_string())
    }
}

// ── AskUser ─────────────────────────────────────────────────────────────────

pub struct AskUserTool;

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user a question when you need clarification or additional information to proceed. The agent will pause and wait for the user's response."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                }
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, arguments: Value) -> ToolResult {
        let question = arguments
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Could you provide more details?");

        ToolResult::AskUser(question.to_string())
    }
}
