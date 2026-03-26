use anyhow::{Context, Result};
use reqwest::Client;
use tokio::sync::mpsc;

use crate::llm::types::*;

/// HTTP client for the Ollama API
#[derive(Clone)]
pub struct OllamaClient {
    client: Client,
    base_url: String,
}

impl OllamaClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Send a streaming chat request. Returns a receiver that yields StreamEvents.
    pub async fn chat_stream(
        &self,
        request: &ChatRequest,
    ) -> Result<mpsc::UnboundedReceiver<StreamEvent>> {
        let url = format!("{}/api/chat", self.base_url);

        // Ensure streaming is enabled
        let mut req = request.clone();
        req.stream = Some(true);

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .context("Failed to connect to Ollama server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API error ({}): {}", status, body);
        }

        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            if let Err(e) = super::stream::process_stream(response, &tx).await {
                let _ = tx.send(StreamEvent::Error(e.to_string()));
            }
        });

        Ok(rx)
    }

    /// Send a non-streaming chat request
    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatStreamChunk> {
        let url = format!("{}/api/chat", self.base_url);

        let mut req = request.clone();
        req.stream = Some(false);

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .context("Failed to connect to Ollama server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API error ({}): {}", status, body);
        }

        response
            .json::<ChatStreamChunk>()
            .await
            .context("Failed to parse Ollama response")
    }

    /// Get model information (capabilities, context length, etc.)
    pub async fn show(&self, model: &str) -> Result<ShowModelResponse> {
        let url = format!("{}/api/show", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "model": model }))
            .send()
            .await
            .context("Failed to connect to Ollama server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API error ({}): {}", status, body);
        }

        response
            .json::<ShowModelResponse>()
            .await
            .context("Failed to parse model info")
    }

    /// List locally available models
    pub async fn list_models(&self) -> Result<ListModelsResponse> {
        let url = format!("{}/api/tags", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to Ollama server")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API error ({}): {}", status, body);
        }

        response
            .json::<ListModelsResponse>()
            .await
            .context("Failed to parse model list")
    }

    /// Check if the Ollama server is reachable
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/api/version", self.base_url);
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}
