use async_trait::async_trait;
use ignore::WalkBuilder;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::tools::types::{Tool, ToolResult};

pub struct ListDirectoryTool {
    working_dir: PathBuf,
}

impl ListDirectoryTool {
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
impl Tool for ListDirectoryTool {
    fn name(&self) -> &str {
        "list_directory"
    }

    fn description(&self) -> &str {
        "List the contents of a directory. Respects .gitignore. Results show files and directories with '/' suffix for directories."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (relative to project root or absolute). Defaults to project root."
                }
            }
        })
    }

    async fn execute(&self, arguments: Value) -> ToolResult {
        let path = arguments
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let resolved = self.resolve_path(path);

        if !resolved.is_dir() {
            return ToolResult::Error(format!("'{}' is not a directory", path));
        }

        // Security: prevent path traversal
        if let Ok(wd_canonical) = self.working_dir.canonicalize() {
            if let Ok(canonical) = resolved.canonicalize() {
                if !canonical.starts_with(&wd_canonical) {
                    return ToolResult::Error(format!(
                        "Access denied: path '{}' is outside the project directory",
                        path
                    ));
                }
            }
        }

        let mut entries = Vec::new();

        let walker = WalkBuilder::new(&resolved)
            .max_depth(Some(1))
            .hidden(false)
            .build();

        for entry in walker.flatten() {
            let entry_path = entry.path();
            if entry_path == resolved {
                continue; // Skip the root directory itself
            }

            let name = match entry_path.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };

            if entry_path.is_dir() {
                entries.push(format!("{}/", name));
            } else {
                entries.push(name);
            }
        }

        entries.sort();

        if entries.is_empty() {
            ToolResult::Success(format!("Directory '{}' is empty", path))
        } else {
            ToolResult::Success(entries.join("\n"))
        }
    }
}
