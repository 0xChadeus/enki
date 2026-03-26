use async_trait::async_trait;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use std::path::PathBuf;

use crate::tools::types::{PendingAction, Tool, ToolResult};

pub struct EditFileTool {
    working_dir: PathBuf,
}

impl EditFileTool {
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
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit an existing file by replacing an exact string with new content. The old_string must match exactly one location in the file (including whitespace and indentation). Include several lines of context to ensure a unique match."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to edit (relative to project root or absolute)"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact text to find and replace. Must match exactly once in the file."
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace old_string with"
                }
            },
            "required": ["path", "old_string", "new_string"]
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

        let old_string = match arguments.get("old_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::Error("Missing required parameter: old_string".to_string()),
        };

        let new_string = match arguments.get("new_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::Error("Missing required parameter: new_string".to_string()),
        };

        let resolved = self.resolve_path(path);

        // Security: prevent path traversal
        if let Ok(wd_canonical) = self.working_dir.canonicalize() {
            match resolved.canonicalize() {
                Ok(canonical) if !canonical.starts_with(&wd_canonical) => {
                    return ToolResult::Error(format!(
                        "Access denied: path '{}' is outside the project directory",
                        path
                    ));
                }
                Err(e) => return ToolResult::Error(format!("Cannot access '{}': {}", path, e)),
                _ => {}
            }
        }

        let content = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => return ToolResult::Error(format!("Failed to read '{}': {}", path, e)),
        };

        // Check for exact match count
        let match_count = content.matches(old_string).count();

        if match_count == 0 {
            return ToolResult::Error(format!(
                "old_string not found in '{}'. Make sure the string matches exactly including whitespace.",
                path
            ));
        }

        if match_count > 1 {
            return ToolResult::Error(format!(
                "old_string found {} times in '{}'. Include more context lines to make the match unique.",
                match_count, path
            ));
        }

        // Generate diff preview
        let new_content = content.replacen(old_string, new_string, 1);
        let diff = TextDiff::from_lines(&content, &new_content);
        let mut preview = String::new();
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            preview.push_str(&format!("{}{}", sign, change));
        }

        ToolResult::NeedsApproval(PendingAction {
            tool_name: "edit_file".to_string(),
            description: format!("Edit file: {}", path),
            preview: Some(preview),
            arguments: arguments.clone(),
        })
    }
}

/// Actually perform the edit (called after approval)
pub fn execute_edit(working_dir: &PathBuf, arguments: &Value) -> ToolResult {
    let path = match arguments.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return ToolResult::Error("Missing path".to_string()),
    };
    let old_string = match arguments.get("old_string").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return ToolResult::Error("Missing old_string".to_string()),
    };
    let new_string = match arguments.get("new_string").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return ToolResult::Error("Missing new_string".to_string()),
    };

    let resolved = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        working_dir.join(path)
    };

    let content = match std::fs::read_to_string(&resolved) {
        Ok(c) => c,
        Err(e) => return ToolResult::Error(format!("Failed to read '{}': {}", path, e)),
    };

    let new_content = content.replacen(old_string, new_string, 1);

    match std::fs::write(&resolved, &new_content) {
        Ok(()) => ToolResult::Success(format!("Successfully edited {}", path)),
        Err(e) => ToolResult::Error(format!("Failed to write '{}': {}", path, e)),
    }
}
