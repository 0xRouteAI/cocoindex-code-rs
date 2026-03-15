use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    #[serde(default)]
    pub language_overrides: HashMap<String, String>,
}

impl ProjectSettings {
    pub fn load(project_root: &Path) -> Result<Self> {
        let path = project_root.join(".cocoindex_code/settings.yml");

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&content)?)
    }

    pub fn save(&self, project_root: &Path) -> Result<()> {
        let path = project_root.join(".cocoindex_code/settings.yml");
        std::fs::create_dir_all(path.parent().unwrap())?;
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            include_patterns: vec![
                // Programming languages
                "**/*.py", "**/*.js", "**/*.mjs", "**/*.cjs",
                "**/*.ts", "**/*.tsx", "**/*.jsx",
                "**/*.rs", "**/*.go", "**/*.java",
                "**/*.c", "**/*.h", "**/*.cpp", "**/*.cc", "**/*.cxx", "**/*.hpp",
                "**/*.cs", "**/*.rb", "**/*.php",
                "**/*.swift", "**/*.kt", "**/*.scala", "**/*.sql",
                "**/*.sh", "**/*.bash",
                // Markup and config
                "**/*.md", "**/*.html", "**/*.htm", "**/*.css",
                "**/*.json", "**/*.yaml", "**/*.yml", "**/*.toml", "**/*.xml",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            exclude_patterns: vec![
                "**/.*", "**/__pycache__", "**/node_modules",
                "**/target", "**/dist", "**/build", "**/.cocoindex_code",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            language_overrides: HashMap::new(),
        }
    }
}
