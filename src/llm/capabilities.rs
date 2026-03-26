use anyhow::Result;

use crate::llm::client::OllamaClient;
use crate::llm::types::ShowModelResponse;

/// Detected capabilities and metadata for a specific model
#[derive(Debug, Clone)]
pub struct ModelCapabilities {
    pub model_name: String,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub context_length: u32,
    pub parameter_size: Option<String>,
    pub family: Option<String>,
    pub quantization: Option<String>,
}

impl ModelCapabilities {
    /// Probe a model via `POST /api/show` and build its capabilities profile
    pub async fn detect(client: &OllamaClient, model: &str) -> Result<Self> {
        let info = client.show(model).await?;
        Ok(Self::from_show_response(model, &info))
    }

    fn from_show_response(model: &str, info: &ShowModelResponse) -> Self {
        let caps = info.capabilities.as_deref().unwrap_or(&[]);
        let supports_tools = caps.iter().any(|c| c == "tools");
        let supports_vision = caps.iter().any(|c| c == "vision");

        // Extract context length from model_info if available
        let context_length = info
            .model_info
            .as_ref()
            .and_then(|mi| {
                // Try common keys for context length
                mi.get("llama.context_length")
                    .or_else(|| mi.get("qwen2.context_length"))
                    .or_else(|| mi.get("gemma.context_length"))
                    .or_else(|| mi.get("context_length"))
                    .and_then(|v| v.as_u64())
            })
            .unwrap_or(4096) as u32;

        let (parameter_size, family, quantization) = match &info.details {
            Some(d) => (
                d.parameter_size.clone(),
                d.family.clone(),
                d.quantization_level.clone(),
            ),
            None => (None, None, None),
        };

        Self {
            model_name: model.to_string(),
            supports_tools,
            supports_vision,
            context_length,
            parameter_size,
            family,
            quantization,
        }
    }

    /// Suggested context budget for replies based on model size
    pub fn reply_token_budget(&self) -> u32 {
        if self.context_length >= 32768 {
            8192
        } else if self.context_length >= 8192 {
            4096
        } else {
            2048
        }
    }

    /// Available tokens for conversation history (context_length - reply_budget)
    pub fn history_budget(&self) -> u32 {
        self.context_length.saturating_sub(self.reply_token_budget())
    }

    /// Create a fallback capabilities struct when model detection fails
    pub fn fallback(model: &str) -> Self {
        Self {
            model_name: model.to_string(),
            supports_tools: false,
            supports_vision: false,
            context_length: 4096,
            parameter_size: None,
            family: None,
            quantization: None,
        }
    }
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            model_name: "unknown".to_string(),
            supports_tools: false,
            supports_vision: false,
            context_length: 4096,
            parameter_size: None,
            family: None,
            quantization: None,
        }
    }
}
