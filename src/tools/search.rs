use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::tools::types::{Tool, ToolResult};

// ── SearchTextTool (grep) ───────────────────────────────────────────────────

pub struct SearchTextTool {
    working_dir: PathBuf,
}

impl SearchTextTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for SearchTextTool {
    fn name(&self) -> &str {
        "search_text"
    }

    fn description(&self) -> &str {
        "Search for text matching a regex pattern across files in the project. Respects .gitignore. Returns matching lines with file paths and line numbers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for (case-insensitive by default)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in (relative to project root). Defaults to project root."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return. Defaults to 50."
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, arguments: Value) -> ToolResult {
        let pattern = match arguments.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::Error("Missing required parameter: pattern".to_string()),
        };

        let regex = match Regex::new(&format!("(?i){}", pattern)) {
            Ok(r) => r,
            Err(e) => return ToolResult::Error(format!("Invalid regex pattern: {}", e)),
        };

        let search_path = arguments
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let resolved = if PathBuf::from(search_path).is_absolute() {
            PathBuf::from(search_path)
        } else {
            self.working_dir.join(search_path)
        };

        let max_results = arguments
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;

        let mut results = Vec::new();

        let walker = WalkBuilder::new(&resolved)
            .hidden(false)
            .build();

        for entry in walker.flatten() {
            if results.len() >= max_results {
                break;
            }

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Skip binary files
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let rel_path = path
                .strip_prefix(&self.working_dir)
                .unwrap_or(path)
                .display()
                .to_string();

            for (line_num, line) in content.lines().enumerate() {
                if results.len() >= max_results {
                    break;
                }
                if regex.is_match(line) {
                    results.push(format!("{}:{}: {}", rel_path, line_num + 1, line.trim()));
                }
            }
        }

        if results.is_empty() {
            ToolResult::Success(format!("No matches found for pattern '{}'", pattern))
        } else {
            let header = format!("Found {} matches:\n", results.len());
            ToolResult::Success(header + &results.join("\n"))
        }
    }
}

// ── SearchFilesTool (glob) ──────────────────────────────────────────────────

pub struct SearchFilesTool {
    working_dir: PathBuf,
}

impl SearchFilesTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search for files matching a glob pattern. Respects .gitignore. Returns matching file paths."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match file names (e.g., '**/*.rs', 'src/**/*.ts')"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, arguments: Value) -> ToolResult {
        let pattern = match arguments.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::Error("Missing required parameter: pattern".to_string()),
        };

        let glob_pattern = if pattern.contains('/') || pattern.contains('*') {
            self.working_dir.join(pattern).display().to_string()
        } else {
            self.working_dir
                .join(format!("**/{}", pattern))
                .display()
                .to_string()
        };

        let entries: Vec<String> = match glob::glob(&glob_pattern) {
            Ok(paths) => paths
                .flatten()
                .take(100)
                .map(|p| {
                    p.strip_prefix(&self.working_dir)
                        .unwrap_or(&p)
                        .display()
                        .to_string()
                })
                .collect(),
            Err(e) => return ToolResult::Error(format!("Invalid glob pattern: {}", e)),
        };

        if entries.is_empty() {
            ToolResult::Success(format!("No files matching '{}'", pattern))
        } else {
            ToolResult::Success(format!("{} files found:\n{}", entries.len(), entries.join("\n")))
        }
    }
}
