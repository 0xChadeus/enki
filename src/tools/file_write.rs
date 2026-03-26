use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::tools::types::{PendingAction, Tool, ToolResult};

pub struct WriteFileTool {
    working_dir: PathBuf,
}

impl WriteFileTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }

    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = PathBuf::from(path);
        if p.is_absolute() {
            p
        } else {
            self.working_dir.join(p)
        }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create a new file or overwrite an existing file with the given content. The directory will be created if it doesn't exist. Requires user approval."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to write to (relative to project root or absolute)"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, arguments: Value) -> ToolResult {
        let path = match arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::Error("Missing required parameter: path".to_string()),
        };

        let content = match arguments.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::Error("Missing required parameter: content".to_string()),
        };

        let resolved = self.resolve_path(path);

        // Security: prevent path traversal outside working dir
        // For new files, check parent dir
        let check_path = if resolved.exists() {
            resolved.clone()
        } else {
            resolved.parent().unwrap_or(&self.working_dir).to_path_buf()
        };

        if let Ok(wd_canonical) = self.working_dir.canonicalize() {
            if let Ok(canonical) = check_path.canonicalize() {
                if !canonical.starts_with(&wd_canonical) {
                    return ToolResult::Error(format!(
                        "Access denied: path '{}' is outside the project directory",
                        path
                    ));
                }
            }
        }

        // Build a preview for approval
        let preview = if resolved.exists() {
            let existing = std::fs::read_to_string(&resolved).unwrap_or_default();
            format_diff(&existing, content)
        } else {
            format!("New file: {}\n{}", path, content_preview(content))
        };

        // Return needs-approval with preview
        ToolResult::NeedsApproval(PendingAction {
            tool_name: "write_file".to_string(),
            description: format!("Write to file: {}", path),
            preview: Some(preview),
            arguments: arguments.clone(),
        })
    }
}

/// Actually perform the write (called after approval)
pub fn execute_write(working_dir: &PathBuf, arguments: &Value) -> ToolResult {
    let path = match arguments.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return ToolResult::Error("Missing path".to_string()),
    };

    let content = match arguments.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::Error("Missing content".to_string()),
    };

    let resolved = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        working_dir.join(path)
    };

    // Create parent directories
    if let Some(parent) = resolved.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ToolResult::Error(format!("Failed to create directory: {}", e));
        }
    }

    match std::fs::write(&resolved, content) {
        Ok(()) => ToolResult::Success(format!("Successfully wrote to {}", path)),
        Err(e) => ToolResult::Error(format!("Failed to write '{}': {}", path, e)),
    }
}

fn format_diff(old: &str, new: &str) -> String {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        output.push_str(&format!("{}{}", sign, change));
    }

    output
}

fn content_preview(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= 20 {
        content.to_string()
    } else {
        let mut preview = String::new();
        for line in &lines[..10] {
            preview.push_str(line);
            preview.push('\n');
        }
        preview.push_str(&format!("... ({} more lines) ...\n", lines.len() - 20));
        for line in &lines[lines.len() - 10..] {
            preview.push_str(line);
            preview.push('\n');
        }
        preview
    }
}
