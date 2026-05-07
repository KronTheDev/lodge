//! Clean Cabin configuration — persisted alongside Lodge's config directory.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Determines which AI backend is used for scoring suggestions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
pub enum AiMode {
    /// Try Ollama first, then cloud API keys, then heuristics-only.
    #[default]
    Auto,
    /// Force Ollama (local inference at 127.0.0.1:11434).
    Ollama,
    /// Force the configured cloud API key (LODGE_GEMINI_KEY or LODGE_CLAUDE_KEY).
    Api,
    /// Disable AI scoring entirely — heuristics only.
    None,
}

/// Persisted clean-cabin configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// How many days staged files are retained before auto-purge (default 30).
    pub retention_days: u32,
    /// AI scoring mode (default Auto).
    pub ai_mode: AiMode,
    /// Ollama URL used when ai_mode is Ollama (default "http://localhost:11434").
    pub ollama_url: String,
    /// Minimum age in days before a file is a candidate (default 90).
    pub scan_min_age_days: u32,
    /// Minimum size in MB before a large-old file is flagged (default 50).
    pub scan_large_file_mb: u64,
    /// Respect .gitignore files during walks (default true).
    pub scan_respect_gitignore: bool,
    /// Additional path prefixes / globs to exclude from scanning.
    pub scan_exclude: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            retention_days: 30,
            ai_mode: AiMode::Auto,
            ollama_url: "http://localhost:11434".into(),
            scan_min_age_days: 90,
            scan_large_file_mb: 50,
            scan_respect_gitignore: true,
            scan_exclude: Vec::new(),
        }
    }
}

impl Config {
    /// Load persisted config, returning defaults if the file is absent or unreadable.
    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist config to disk, creating directories as needed.
    pub fn save(&self) -> Result<()> {
        let path = config_path().context("could not determine config path")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("couldn't create {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self).context("couldn't serialise config")?;
        std::fs::write(&path, json)
            .with_context(|| format!("couldn't write {}", path.display()))?;
        Ok(())
    }

    /// Apply a `key = value` update, returning an error for unknown keys.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "retention_days" => {
                self.retention_days = value
                    .parse()
                    .with_context(|| format!("'{value}' is not a valid number"))?;
            }
            "scan_min_age_days" => {
                self.scan_min_age_days = value
                    .parse()
                    .with_context(|| format!("'{value}' is not a valid number"))?;
            }
            "scan_large_file_mb" => {
                self.scan_large_file_mb = value
                    .parse()
                    .with_context(|| format!("'{value}' is not a valid number"))?;
            }
            "scan_respect_gitignore" => {
                self.scan_respect_gitignore = value
                    .parse()
                    .with_context(|| format!("'{value}' is not a valid bool"))?;
            }
            "ollama_url" => {
                self.ollama_url = value.to_string();
            }
            "ai_mode" => {
                self.ai_mode = match value {
                    "auto" | "Auto" => AiMode::Auto,
                    "ollama" | "Ollama" => AiMode::Ollama,
                    "api" | "Api" => AiMode::Api,
                    "none" | "None" => AiMode::None,
                    other => anyhow::bail!("unknown ai_mode '{other}'"),
                };
            }
            other => anyhow::bail!("unknown config key '{other}'"),
        }
        Ok(())
    }
}

fn config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|b| PathBuf::from(b).join("lodge").join("clean-cabin.json"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join(".local")
                .join("share")
                .join("lodge")
                .join("clean-cabin.json")
        })
    }
}
