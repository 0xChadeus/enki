use crate::llm::capabilities::ModelCapabilities;
use crate::tools::ToolRegistry;
use std::path::Path;

/// Builds the system prompt dynamically based on project context,
/// model capabilities, and available tools.
pub fn build_system_prompt(
    working_dir: &Path,
    capabilities: &ModelCapabilities,
    tools: &ToolRegistry,
    project_instructions: &Option<String>,
) -> String {
    let mut prompt = String::new();

    // ── Role definition ─────────────────────────────────────────────────
    prompt.push_str(
        "You are Enki, an expert AI coding assistant running locally. \
         You help users with programming tasks by reading files, writing code, \
         editing existing code, running commands, and searching codebases.\n\n",
    );

    // ── Working directory ───────────────────────────────────────────────
    prompt.push_str(&format!(
        "Working directory: {}\n\n",
        working_dir.display()
    ));

    // ── Tool usage instructions ─────────────────────────────────────────
    if capabilities.supports_tools {
        prompt.push_str(
            "You have access to tools that you can call to interact with the codebase. \
             Use tools to gather information before answering, and to make changes when asked.\n\n\
             RULES:\n\
             - Always read a file before editing it\n\
             - Use search_text or list_directory to explore unfamiliar codebases\n\
             - When editing files, include enough context in old_string for a unique match\n\
             - For multi-step tasks, work through them one tool call at a time\n\
             - Use attempt_completion when the task is fully done\n\
             - Use ask_user if you need clarification\n\
             - Never guess file contents — always read first\n\n",
        );
    } else {
        // JSON fallback mode instructions
        prompt.push_str(
            "You have access to tools to interact with the codebase. \
             To use a tool, respond with a JSON object containing:\n\
             - \"thinking\": your reasoning about what to do\n\
             - \"tool\": the tool name to call\n\
             - \"arguments\": an object with the tool's parameters\n\n\
             If you want to respond to the user without calling a tool, use:\n\
             - \"thinking\": your reasoning\n\
             - \"response\": your text response\n\n\
             RULES:\n\
             - Always read a file before editing it\n\
             - Use search_text or list_directory to explore unfamiliar codebases\n\
             - When editing, include enough context in old_string for a unique match\n\
             - For multi-step tasks, work through them one tool call at a time\n\
             - Call attempt_completion when the task is fully done\n\
             - Call ask_user if you need clarification\n\
             - Never guess file contents — always read first\n\n",
        );

        // Include tool descriptions in the prompt for the JSON fallback path
        prompt.push_str(&tools.tool_descriptions());
        prompt.push('\n');
    }

    // ── Safety guidelines ───────────────────────────────────────────────
    prompt.push_str(
        "SAFETY:\n\
         - Never execute destructive commands without the user's explicit intent\n\
         - Do not access files outside the project directory\n\
         - Do not generate or execute malicious code\n\
         - If unsure about a destructive operation, ask the user first\n\n",
    );

    // ── Project instructions (from enki.md) ─────────────────────────────
    if let Some(instructions) = project_instructions {
        prompt.push_str("PROJECT INSTRUCTIONS (from enki.md):\n");
        prompt.push_str(instructions);
        prompt.push_str("\n\n");
    }

    prompt
}

/// Load project instructions from enki.md in the working directory
pub fn load_project_instructions(working_dir: &Path) -> Option<String> {
    let enki_md = working_dir.join("enki.md");
    if enki_md.exists() {
        std::fs::read_to_string(&enki_md).ok()
    } else {
        None
    }
}
