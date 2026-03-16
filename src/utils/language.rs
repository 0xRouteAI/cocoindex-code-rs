use std::collections::HashMap;
use std::path::Path;

pub fn detect_language(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;

    match ext {
        "py" => Some("python"),
        "js" | "mjs" | "cjs" => Some("javascript"),
        "ts" => Some("typescript"),
        "tsx" => Some("tsx"),
        "jsx" => Some("jsx"),
        "rs" => Some("rust"),
        "go" => Some("go"),
        "java" => Some("java"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("cpp"),
        "cs" => Some("csharp"),
        "rb" => Some("ruby"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        "kt" => Some("kotlin"),
        "scala" => Some("scala"),
        "sql" => Some("sql"),
        "sh" | "bash" => Some("bash"),
        "md" => Some("markdown"),
        "html" | "htm" => Some("html"),
        "css" => Some("css"),
        "json" => Some("json"),
        "yaml" | "yml" => Some("yaml"),
        "toml" => Some("toml"),
        "xml" => Some("xml"),
        _ => None,
    }
    .map(String::from)
}

pub fn detect_language_with_overrides(
    path: &Path,
    overrides: &HashMap<String, String>,
) -> Option<String> {
    let ext = path.extension()?.to_str()?;

    // Check overrides first
    if let Some(lang) = overrides.get(ext) {
        return Some(lang.clone());
    }

    // Fall back to default detection
    detect_language(path)
}
