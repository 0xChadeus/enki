use crate::llm::types::{Message, Role};

/// Manages the conversation history and provides token-aware truncation.
pub struct ContextManager {
    /// Maximum tokens available for conversation (model context - reply reserve)
    max_history_tokens: u32,
    /// Estimated tokens used by the system prompt
    system_prompt_tokens: u32,
}

impl ContextManager {
    pub fn new(max_history_tokens: u32, system_prompt_tokens: u32) -> Self {
        Self {
            max_history_tokens,
            system_prompt_tokens,
        }
    }

    /// Estimate the number of tokens in a string (rough heuristic: ~4 chars per token)
    pub fn estimate_tokens(text: &str) -> u32 {
        (text.len() as u32 + 3) / 4
    }

    /// Estimate total tokens used by a list of messages
    pub fn estimate_messages_tokens(messages: &[Message]) -> u32 {
        messages
            .iter()
            .map(|m| {
                let content_tokens = Self::estimate_tokens(&m.content);
                let overhead = 4; // role tokens + formatting
                content_tokens + overhead
            })
            .sum()
    }

    /// Available token budget for new messages
    pub fn available_tokens(&self, current_messages: &[Message]) -> u32 {
        let used = self.system_prompt_tokens + Self::estimate_messages_tokens(current_messages);
        self.max_history_tokens.saturating_sub(used)
    }

    /// Check if we're approaching the context limit (>80% used)
    pub fn needs_compaction(&self, current_messages: &[Message]) -> bool {
        let used = self.system_prompt_tokens + Self::estimate_messages_tokens(current_messages);
        let usage_pct = (used as f64) / (self.max_history_tokens as f64);
        usage_pct > 0.80
    }

    /// Get context usage as a percentage (0.0 - 1.0)
    pub fn usage_percentage(&self, current_messages: &[Message]) -> f64 {
        let used = self.system_prompt_tokens + Self::estimate_messages_tokens(current_messages);
        (used as f64) / (self.max_history_tokens as f64)
    }

    /// Truncate conversation history to fit within budget.
    /// Strategy: keep the first message (often important) and the most recent messages.
    pub fn truncate_history(&self, messages: &mut Vec<Message>) {
        if messages.len() <= 2 {
            return;
        }

        let target_tokens = self.max_history_tokens.saturating_sub(self.system_prompt_tokens);

        // If already within budget, nothing to do
        if Self::estimate_messages_tokens(messages) <= target_tokens {
            return;
        }

        // Keep first message and trim from the middle
        let first = messages[0].clone();

        // Count backwards to find how many recent messages fit
        let mut recent_tokens = Self::estimate_tokens(&first.content) + 4;
        let mut keep_from = messages.len();

        for i in (1..messages.len()).rev() {
            let msg_tokens = Self::estimate_tokens(&messages[i].content) + 4;
            if recent_tokens + msg_tokens > target_tokens {
                break;
            }
            recent_tokens += msg_tokens;
            keep_from = i;
        }

        // Build truncated history
        if keep_from > 1 {
            let removed_count = keep_from - 1;
            let mut truncated = vec![first];

            // Add a summary message about removed context
            truncated.push(Message {
                role: Role::System,
                content: format!(
                    "[{} earlier messages were removed to fit the context window. The conversation continues below.]",
                    removed_count
                ),
                tool_calls: None,
                tool_name: None,
            });

            truncated.extend_from_slice(&messages[keep_from..]);
            *messages = truncated;
        }
    }

    /// Truncate a single tool result if it's too long
    pub fn truncate_tool_result(result: &str, max_tokens: u32) -> String {
        let max_chars = (max_tokens * 4) as usize;
        if result.len() <= max_chars {
            return result.to_string();
        }

        let half = max_chars / 2;
        let lines: Vec<&str> = result.lines().collect();

        // Take first and last lines that fit
        let mut head = String::new();
        let mut head_len = 0;
        for line in &lines {
            if head_len + line.len() + 1 > half {
                break;
            }
            head.push_str(line);
            head.push('\n');
            head_len += line.len() + 1;
        }

        let mut tail = String::new();
        let mut tail_len = 0;
        for line in lines.iter().rev() {
            if tail_len + line.len() + 1 > half {
                break;
            }
            tail = format!("{}\n{}", line, tail);
            tail_len += line.len() + 1;
        }

        format!(
            "{}... (output truncated, {} total chars) ...\n{}",
            head,
            result.len(),
            tail
        )
    }
}
