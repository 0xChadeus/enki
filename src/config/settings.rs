use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Application settings, loaded from config files
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Ollama server URL
    pub ollama_url: String,
    /// Default model to use
    pub default_model: String,
    /// Max agent loop iterations per turn
    pub max_iterations: u32,
    /// Auto-approve read-only tool calls (read_file, list_directory, search)
    pub auto_approve_reads: bool,
    /// Token count reserved for model's reply
    pub context_reserve_tokens: u32,
    /// Bash command execution timeout in seconds
    pub bash_timeout_secs: u64,
    /// Bash commands/patterns to always deny
    pub bash_deny_patterns: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ollama_url: "http://127.0.0.1:11434".to_string(),
            default_model: "qwen3.5:27b".to_string(),
            max_iterations: 25,
            auto_approve_reads: true,
            context_reserve_tokens: 4096,
            bash_timeout_secs: 120,
            bash_deny_patterns: vec![
                "rm -rf /".to_string(),
                "rm -rf ~".to_string(),
                "mkfs".to_string(),
                "> /dev/sd".to_string(),
                "dd if=".to_string(),
            ],
        }
    }
}

impl Settings {
    /// Load settings with precedence: project > global > defaults
    pub fn load(project_dir: &Path) -> Result<Self> {
        let mut settings = Self::default();

        // Load global config
        if let Some(global_path) = Self::global_config_path() {
            if global_path.exists() {
                let content = std::fs::read_to_string(&global_path)?;
                let global: Settings = toml::from_str(&content)?;
                settings.merge(global);
            }
        }

        // Load project config (overrides global)
        let project_config = project_dir.join("enki.toml");
        if project_config.exists() {
            let content = std::fs::read_to_string(&project_config)?;
            let project: Settings = toml::from_str(&content)?;
            settings.merge(project);
        }

        Ok(settings)
    }

    /// Get the global config file path (~/.config/enki/config.toml)
    pub fn global_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("enki").join("config.toml"))
    }

    /// Get the data directory path (~/.local/share/enki/)
    pub fn data_dir() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("enki"))
    }

    /// Merge another settings instance (non-default values override)
    fn merge(&mut self, other: Settings) {
        let defaults = Settings::default();
        if other.ollama_url != defaults.ollama_url {
            self.ollama_url = other.ollama_url;
        }
        if other.default_model != defaults.default_model {
            self.default_model = other.default_model;
        }
        if other.max_iterations != defaults.max_iterations {
            self.max_iterations = other.max_iterations;
        }
        if other.auto_approve_reads != defaults.auto_approve_reads {
            self.auto_approve_reads = other.auto_approve_reads;
        }
        if other.context_reserve_tokens != defaults.context_reserve_tokens {
            self.context_reserve_tokens = other.context_reserve_tokens;
        }
        if other.bash_timeout_secs != defaults.bash_timeout_secs {
            self.bash_timeout_secs = other.bash_timeout_secs;
        }
        if other.bash_deny_patterns != defaults.bash_deny_patterns {
            self.bash_deny_patterns = other.bash_deny_patterns;
        }
    }
}
