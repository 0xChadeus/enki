use serde::{Deserialize, Serialize};

// ── Chat API types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ModelOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

// ── Chat response types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamChunk {
    pub model: String,
    pub created_at: String,
    pub message: Option<StreamMessage>,
    pub done: bool,
    #[serde(default)]
    pub done_reason: Option<String>,
    #[serde(default)]
    pub total_duration: Option<u64>,
    #[serde(default)]
    pub load_duration: Option<u64>,
    #[serde(default)]
    pub prompt_eval_count: Option<u32>,
    #[serde(default)]
    pub prompt_eval_duration: Option<u64>,
    #[serde(default)]
    pub eval_count: Option<u32>,
    #[serde(default)]
    pub eval_duration: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamMessage {
    pub role: Role,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallResponse>>,
}

// ── Tool-related types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: serde_json::Value,
}

impl ToolDefinition {
    pub fn new(name: &str, description: &str, parameters: serde_json::Value) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
            },
        }
    }
}

// ── Parsed tool call (unified from both paths) ─────────────────────────────

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

// ── Model info types (from /api/show) ───────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ShowModelResponse {
    #[serde(default)]
    pub modelfile: Option<String>,
    #[serde(default)]
    pub parameters: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub details: Option<ModelDetails>,
    #[serde(default)]
    pub model_info: Option<serde_json::Value>,
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelDetails {
    #[serde(default)]
    pub parent_model: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub families: Option<Vec<String>>,
    #[serde(default)]
    pub parameter_size: Option<String>,
    #[serde(default)]
    pub quantization_level: Option<String>,
}

// ── List models types (from /api/tags) ──────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ListModelsResponse {
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub name: String,
    pub model: String,
    #[serde(default)]
    pub modified_at: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub details: Option<ModelDetails>,
}

// ── JSON fallback schema for models without tool calling ────────────────────

/// Schema used with Ollama's `format` parameter for models that don't support
/// native tool calling. Forces structured JSON output.
pub fn tool_call_fallback_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "thinking": {
                "type": "string",
                "description": "Your reasoning about what to do next"
            },
            "response": {
                "type": "string",
                "description": "Your text response to the user (if no tool call needed)"
            },
            "tool": {
                "type": "string",
                "description": "The name of the tool to call (if a tool call is needed)"
            },
            "arguments": {
                "type": "object",
                "description": "The arguments for the tool call"
            }
        },
        "required": ["thinking"]
    })
}

/// Parse a tool call from the JSON fallback format
pub fn parse_fallback_tool_call(json_str: &str) -> anyhow::Result<FallbackResponse> {
    let val: serde_json::Value = serde_json::from_str(json_str)?;
    Ok(FallbackResponse {
        thinking: val.get("thinking").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        response: val.get("response").and_then(|v| v.as_str()).map(String::from),
        tool_call: match val.get("tool").and_then(|v| v.as_str()) {
            Some(name) if !name.is_empty() => Some(ToolCall {
                name: name.to_string(),
                arguments: val.get("arguments").cloned().unwrap_or(serde_json::Value::Null),
            }),
            _ => None,
        },
    })
}

#[derive(Debug, Clone)]
pub struct FallbackResponse {
    pub thinking: String,
    pub response: Option<String>,
    pub tool_call: Option<ToolCall>,
}

// ── Stream events (emitted by StreamProcessor) ──────────────────────────────

#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of text content from the assistant
    TextDelta(String),
    /// The assistant is making a tool call (native path)
    ToolCall(ToolCall),
    /// Stream is complete, includes usage stats
    Done(UsageStats),
    /// An error occurred during streaming
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct UsageStats {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_duration_ns: u64,
    pub eval_duration_ns: u64,
}
