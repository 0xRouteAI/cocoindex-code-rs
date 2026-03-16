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
                "**/.*", "**/.*/**",
                "**/__pycache__", "**/__pycache__/**",
                "**/node_modules", "**/node_modules/**",
                "**/.git", "**/.git/**",
                "**/.svn", "**/.svn/**",
                "**/.hg", "**/.hg/**",
                "**/.idea", "**/.idea/**",
                "**/.vscode", "**/.vscode/**",
                "**/target", "**/target/**",
                "**/dist", "**/dist/**",
                "**/build", "**/build/**",
                "**/coverage", "**/coverage/**",
                "**/.next", "**/.next/**",
                "**/.nuxt", "**/.nuxt/**",
                "**/out", "**/out/**",
                "**/tmp", "**/tmp/**",
                "**/temp", "**/temp/**",
                "**/vendor", "**/vendor/**",
                "**/.cocoindex_code", "**/.cocoindex_code/**",
                "**/*.min.js", "**/*.min.css", "**/*.map", "**/*.lock", "**/*.log",
                "**/*.sqlite", "**/*.db", "**/*.bin", "**/*.exe", "**/*.dll",
                "**/*.so", "**/*.dylib", "**/*.class", "**/*.jar", "**/*.war",
                "**/*.pyc", "**/*.pyo", "**/*.o", "**/*.a",
                "**/*.jpg", "**/*.jpeg", "**/*.png", "**/*.gif", "**/*.webp",
                "**/*.pdf", "**/*.zip", "**/*.tar", "**/*.gz",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            language_overrides: HashMap::new(),
        }
    }
}
