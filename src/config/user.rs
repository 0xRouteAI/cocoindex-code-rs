use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    pub api_key: String,
    pub api_base: String,
    pub model: String,
    #[serde(default = "default_embedding_dim")]
    pub embedding_dim: usize,
    #[serde(default)]
    pub envs: HashMap<String, String>,
}

fn default_embedding_dim() -> usize {
    std::env::var("EMBEDDING_DIM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1536)
}

impl UserSettings {
    pub fn settings_dir() -> Result<PathBuf> {
        if let Ok(dir) = std::env::var("COCOINDEX_CODE_DIR") {
            return Ok(PathBuf::from(dir));
        }

        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
        Ok(home.join(".cocoindex_code"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::settings_path()?;

        if !path.exists() {
            anyhow::bail!("User settings file does not exist");
        }

        let content = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&content)?)
    }

    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::settings_path()?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    fn settings_path() -> Result<PathBuf> {
        Ok(Self::settings_dir()?.join("settings.yml"))
    }
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            api_base: std::env::var("OPENAI_API_BASE")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            model: std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-small".to_string()),
            embedding_dim: default_embedding_dim(),
            envs: HashMap::new(),
        }
    }
}
