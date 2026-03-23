use crate::AppResult;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    Anthropic,
    OpenAI,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub openai_model: String,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: u64,
    #[serde(default)]
    pub exclude_projects: Vec<String>,
}

fn default_model() -> String {
    "claude-sonnet-4-20250514".to_string()
}

fn default_idle_timeout() -> u64 {
    300
}

fn default_openai_model() -> String {
    "gpt-4.1".to_string()
}

impl Default for HarnConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            openai_api_key: String::new(),
            model: default_model(),
            openai_model: default_openai_model(),
            idle_timeout: default_idle_timeout(),
            exclude_projects: Vec::new(),
        }
    }
}

impl HarnConfig {
    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }

        let contents = fs::read_to_string(path).unwrap_or_default();
        toml::from_str(&contents).unwrap_or_default()
    }

    pub fn save(&self) -> AppResult<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        fs::write(path, contents)?;
        Ok(())
    }

    pub fn path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".harn")
            .join("config.toml")
    }

    pub fn api_key(&self) -> Option<String> {
        std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                if self.api_key.trim().is_empty() {
                    None
                } else {
                    Some(self.api_key.clone())
                }
            })
    }

    pub fn openai_api_key(&self) -> Option<String> {
        std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                if self.openai_api_key.trim().is_empty() {
                    None
                } else {
                    Some(self.openai_api_key.clone())
                }
            })
    }

    pub fn resolve_provider(&self) -> Option<(LlmProvider, String, String)> {
        if let Some(key) = self.api_key() {
            return Some((LlmProvider::Anthropic, key, self.model_name()));
        }
        if let Some(key) = self.openai_api_key() {
            return Some((LlmProvider::OpenAI, key, self.openai_model_name()));
        }
        None
    }

    pub fn model_name(&self) -> String {
        std::env::var("HARN_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| self.model.clone())
    }

    pub fn openai_model_name(&self) -> String {
        std::env::var("HARN_OPENAI_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                if self.openai_model.trim().is_empty() {
                    default_openai_model()
                } else {
                    self.openai_model.clone()
                }
            })
    }

    pub fn is_excluded_project(&self, project_path: &str) -> bool {
        self.exclude_projects
            .iter()
            .any(|pattern| wildcard_match(pattern, project_path))
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }

    if pattern == "*" {
        return true;
    }

    let mut remaining = value;
    let parts = pattern.split('*').collect::<Vec<_>>();

    if !pattern.starts_with('*') {
        if let Some(prefix) = parts.first() {
            if !remaining.starts_with(prefix) {
                return false;
            }
            remaining = &remaining[prefix.len()..];
        }
    }

    for part in parts.iter().filter(|part| !part.is_empty()) {
        if let Some(index) = remaining.find(part) {
            remaining = &remaining[index + part.len()..];
        } else {
            return false;
        }
    }

    if !pattern.ends_with('*') {
        if let Some(last) = parts.last() {
            return value.ends_with(last);
        }
    }

    true
}
