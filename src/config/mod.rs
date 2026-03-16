pub mod user;
pub mod project;

pub use user::UserSettings;
pub use project::ProjectSettings;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub api_key: String,
    pub api_base: String,
    pub model: String,
    pub embedding_dim: usize,
    pub db_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            api_base: std::env::var("OPENAI_API_BASE")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            model: std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-small".to_string()),
            embedding_dim: 1536,
            db_path: ".cocoindex_code/target_sqlite.db".to_string(),
        }
    }
}
