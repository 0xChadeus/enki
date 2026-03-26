use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::tools::types::{PendingAction, Tool, ToolResult};

pub struct BashTool {
    working_dir: PathBuf,
}

impl BashTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

/// Patterns that are always denied for safety
const DENY_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "rm -rf $HOME",
    "mkfs.",
    "> /dev/sd",
    "dd if=/dev/zero",
    ":(){ :|:& };:",
    "chmod -R 777 /",
];

fn is_command_denied(command: &str) -> bool {
    let cmd_lower = command.to_lowercase();
    DENY_PATTERNS.iter().any(|p| cmd_lower.contains(p))
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the project directory. The command runs in bash. Requires user approval before execution. Use this for running tests, installing packages, git operations, building projects, etc."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 120)"
                }
            },
            "required": ["command"]
        })
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, arguments: Value) -> ToolResult {
        let command = match arguments.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::Error("Missing required parameter: command".to_string()),
        };

        // Safety check
        if is_command_denied(command) {
            return ToolResult::Error(format!("Command denied for safety: {}", command));
        }

        ToolResult::NeedsApproval(PendingAction {
            tool_name: "bash".to_string(),
            description: format!("Execute: {}", command),
            preview: None,
            arguments: arguments.clone(),
        })
    }
}

/// Actually run the command (called after approval)
pub async fn execute_bash(working_dir: &PathBuf, arguments: &Value) -> ToolResult {
    let command = match arguments.get("command").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::Error("Missing command".to_string()),
    };

    let timeout_secs = arguments
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(120);

    let mut child = match Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return ToolResult::Error(format!("Failed to spawn command: {}", e)),
    };

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    let mut output = String::new();
    let mut err_output = String::new();

    let timeout = tokio::time::Duration::from_secs(timeout_secs);

    let result = tokio::time::timeout(timeout, async {
        loop {
            tokio::select! {
                line = stdout_reader.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            output.push_str(&l);
                            output.push('\n');
                        }
                        Ok(None) => break,
                        Err(e) => {
                            err_output.push_str(&format!("stdout read error: {}\n", e));
                            break;
                        }
                    }
                }
                line = stderr_reader.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            err_output.push_str(&l);
                            err_output.push('\n');
                        }
                        Ok(None) => {}
                        Err(e) => {
                            err_output.push_str(&format!("stderr read error: {}\n", e));
                        }
                    }
                }
            }
        }
        child.wait().await
    })
    .await;

    match result {
        Ok(Ok(status)) => {
            let mut result = String::new();
            if !output.is_empty() {
                result.push_str(&output);
            }
            if !err_output.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str("STDERR:\n");
                result.push_str(&err_output);
            }
            if result.is_empty() {
                result = "(no output)".to_string();
            }

            // Truncate very long output
            if result.len() > 50_000 {
                let truncated_len = result.len();
                result.truncate(25_000);
                result.push_str(&format!(
                    "\n... (output truncated, {} total bytes) ...",
                    truncated_len
                ));
            }

            result.push_str(&format!("\nExit code: {}", status.code().unwrap_or(-1)));
            ToolResult::Success(result)
        }
        Ok(Err(e)) => ToolResult::Error(format!("Command failed: {}", e)),
        Err(_) => {
            let _ = child.kill().await;
            ToolResult::Error(format!(
                "Command timed out after {} seconds",
                timeout_secs
            ))
        }
    }
}
