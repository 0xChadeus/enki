use anyhow::Result;
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::llm::types::*;

/// Process a streaming response from Ollama, emitting StreamEvents
pub async fn process_stream(
    response: reqwest::Response,
    tx: &mpsc::UnboundedSender<StreamEvent>,
) -> Result<()> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        let text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&text);

        // Process complete JSON lines from the buffer
        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim().to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<ChatStreamChunk>(&line) {
                Ok(chunk) => {
                    if let Err(_) = process_chunk(&chunk, tx) {
                        return Ok(()); // Receiver dropped
                    }
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(format!(
                        "Failed to parse stream chunk: {e}"
                    )));
                }
            }
        }
    }

    // Process any remaining data in the buffer
    let remaining = buffer.trim().to_string();
    if !remaining.is_empty() {
        if let Ok(chunk) = serde_json::from_str::<ChatStreamChunk>(&remaining) {
            let _ = process_chunk(&chunk, tx);
        }
    }

    Ok(())
}

fn process_chunk(
    chunk: &ChatStreamChunk,
    tx: &mpsc::UnboundedSender<StreamEvent>,
) -> Result<(), ()> {
    if chunk.done {
        let stats = UsageStats {
            prompt_tokens: chunk.prompt_eval_count.unwrap_or(0),
            completion_tokens: chunk.eval_count.unwrap_or(0),
            total_duration_ns: chunk.total_duration.unwrap_or(0),
            eval_duration_ns: chunk.eval_duration.unwrap_or(0),
        };
        tx.send(StreamEvent::Done(stats)).map_err(|_| ())?;
        return Ok(());
    }

    if let Some(ref message) = chunk.message {
        // Check for tool calls (native path)
        if let Some(ref tool_calls) = message.tool_calls {
            for tc in tool_calls {
                let tool_call = ToolCall {
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                };
                tx.send(StreamEvent::ToolCall(tool_call)).map_err(|_| ())?;
            }
        }

        // Emit text content
        if !message.content.is_empty() {
            tx.send(StreamEvent::TextDelta(message.content.clone()))
                .map_err(|_| ())?;
        }
    }

    Ok(())
}
