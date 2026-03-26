pub mod types;
pub mod file_read;
pub mod file_write;
pub mod file_edit;
pub mod directory;
pub mod search;
pub mod bash;
pub mod completion;

use std::collections::HashMap;
use crate::tools::types::{Tool, ToolResult};

/// Registry of all available tools. Provides tool definitions for the LLM
/// and dispatches tool calls to the correct implementation.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool in the registry
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Register all default tools
    pub fn register_defaults(&mut self, working_dir: std::path::PathBuf) {
        self.register(Box::new(file_read::ReadFileTool::new(working_dir.clone())));
        self.register(Box::new(file_write::WriteFileTool::new(working_dir.clone())));
        self.register(Box::new(file_edit::EditFileTool::new(working_dir.clone())));
        self.register(Box::new(directory::ListDirectoryTool::new(working_dir.clone())));
        self.register(Box::new(search::SearchTextTool::new(working_dir.clone())));
        self.register(Box::new(search::SearchFilesTool::new(working_dir.clone())));
        self.register(Box::new(bash::BashTool::new(working_dir.clone())));
        self.register(Box::new(completion::AttemptCompletionTool));
        self.register(Box::new(completion::AskUserTool));
    }

    /// Get Ollama tool definitions for all registered tools
    pub fn tool_definitions(&self) -> Vec<crate::llm::types::ToolDefinition> {
        self.tools
            .values()
            .map(|t| t.definition())
            .collect()
    }

    /// Get a formatted description of all tools (for JSON fallback system prompt)
    pub fn tool_descriptions(&self) -> String {
        let mut desc = String::from("Available tools:\n\n");
        for tool in self.tools.values() {
            desc.push_str(&format!("## {}\n", tool.name()));
            desc.push_str(&format!("{}\n", tool.description()));
            desc.push_str(&format!("Parameters: {}\n\n", tool.parameters_schema()));
        }
        desc
    }

    /// Execute a tool by name with the given arguments
    pub async fn execute(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> ToolResult {
        match self.tools.get(name) {
            Some(tool) => tool.execute(arguments).await,
            None => ToolResult::Error(format!("Unknown tool: {name}")),
        }
    }

    /// Check if a tool requires user approval before execution
    pub fn requires_approval(&self, name: &str) -> bool {
        self.tools
            .get(name)
            .map(|t| t.requires_approval())
            .unwrap_or(true)
    }

    /// Get tool names
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }
}
