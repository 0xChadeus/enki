use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::tools::types::{Tool, ToolResult};

pub struct ReadFileTool {
    working_dir: PathBuf,
}

impl ReadFileTool {
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
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. You can optionally specify a line range to read only a portion of the file. Line numbers are 1-indexed."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to read (relative to project root or absolute)"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Starting line number (1-indexed, inclusive). Optional."
                },
                "end_line": {
                    "type": "integer",
                    "description": "Ending line number (1-indexed, inclusive). Optional."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, arguments: Value) -> ToolResult {
        let path = match arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::Error("Missing required parameter: path".to_string()),
        };

        let resolved = self.resolve_path(path);

        // Security: prevent path traversal outside working dir
        match resolved.canonicalize() {
            Ok(canonical) => {
                if let Ok(wd_canonical) = self.working_dir.canonicalize() {
                    if !canonical.starts_with(&wd_canonical) {
                        return ToolResult::Error(format!(
                            "Access denied: path '{}' is outside the project directory",
                            path
                        ));
                    }
                }
            }
            Err(e) => return ToolResult::Error(format!("Cannot access '{}': {}", path, e)),
        }

        let content = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => return ToolResult::Error(format!("Failed to read '{}': {}", path, e)),
        };

        // Check file size limit (100KB)
        if content.len() > 100_000 {
            return ToolResult::Error(format!(
                "File '{}' is too large ({} bytes). Use start_line/end_line to read a portion.",
                path,
                content.len()
            ));
        }

        let lines: Vec<&str> = content.lines().collect();
        let start = arguments
            .get("start_line")
            .and_then(|v| v.as_u64())
            .map(|n| (n as usize).saturating_sub(1))
            .unwrap_or(0);
        let end = arguments
            .get("end_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(lines.len());

        let end = end.min(lines.len());
        if start >= lines.len() {
            return ToolResult::Error(format!(
                "start_line {} exceeds file length ({} lines)",
                start + 1,
                lines.len()
            ));
        }

        let mut output = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            output.push_str(&format!("{:>4} | {}\n", start + i + 1, line));
        }

        ToolResult::Success(output)
    }
}
