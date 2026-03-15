use std::path::{Path, PathBuf};

const PROJECT_MARKER_DIR: &str = ".cocoindex_code";
const PROJECT_SETTINGS_FILE: &str = "settings.yml";
const PROJECT_DB_FILE: &str = "target_sqlite.db";

pub fn project_settings_path(project_root: &Path) -> PathBuf {
    project_root.join(PROJECT_MARKER_DIR).join(PROJECT_SETTINGS_FILE)
}

pub fn project_db_path(project_root: &Path) -> PathBuf {
    project_root.join(PROJECT_MARKER_DIR).join(PROJECT_DB_FILE)
}

pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };

    loop {
        if project_settings_path(&current).exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

pub fn resolve_project_root(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
    let candidate = match explicit {
        Some(path) => path.canonicalize()?,
        None => std::env::current_dir()?,
    };

    Ok(find_project_root(&candidate).unwrap_or(candidate))
}

pub fn scoped_chunk_id(project_root: &Path, rel_path: &str, chunk_index: usize) -> String {
    format!("{}::{}:{}", project_root.display(), rel_path, chunk_index)
}

pub fn default_path_filter(project_root: &Path, cwd: &Path) -> Option<String> {
    let rel = cwd.strip_prefix(project_root).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    Some(format!("{}/*", rel.to_string_lossy().replace('\\', "/")))
}
