use anyhow::Context;
use std::path::{Path, PathBuf};

const PROJECT_MARKER_DIR: &str = ".cocoindex_code";
const PROJECT_SETTINGS_FILE: &str = "settings.yml";
const PROJECT_DB_FILE: &str = "target_sqlite.db";
const PROJECT_LOCK_FILE: &str = "index.lock";

pub fn project_settings_path(project_root: &Path) -> PathBuf {
    project_root.join(PROJECT_MARKER_DIR).join(PROJECT_SETTINGS_FILE)
}

pub fn project_db_path(project_root: &Path) -> PathBuf {
    project_root.join(PROJECT_MARKER_DIR).join(PROJECT_DB_FILE)
}

pub fn project_lock_path(project_root: &Path) -> PathBuf {
    project_root.join(PROJECT_MARKER_DIR).join(PROJECT_LOCK_FILE)
}

fn legacy_project_cache_dir(project_root: &Path) -> PathBuf {
    let key = project_cache_key(project_root);
    let name = project_root
        .file_name()
        .and_then(|part| part.to_str())
        .filter(|part| !part.is_empty())
        .unwrap_or("project");

    project_root
        .join(PROJECT_MARKER_DIR)
        .join("cache")
        .join("projects")
        .join(format!("{name}-{key}"))
}

fn project_cache_key(project_root: &Path) -> String {
    let digest = md5::compute(project_root.to_string_lossy().as_bytes());
    format!("{digest:x}")
}

pub fn ensure_project_cache_layout(project_root: &Path) -> anyhow::Result<()> {
    let local_dir = project_root.join(PROJECT_MARKER_DIR);
    std::fs::create_dir_all(&local_dir)
        .with_context(|| format!("failed to create project dir {}", local_dir.display()))?;

    let cached_dir = legacy_project_cache_dir(project_root);

    let cached_db = cached_dir.join(PROJECT_DB_FILE);
    let local_db = local_dir.join(PROJECT_DB_FILE);
    if cached_db.exists() && !local_db.exists() {
        std::fs::rename(&cached_db, &local_db).or_else(|_| {
            std::fs::copy(&cached_db, &local_db)?;
            std::fs::remove_file(&cached_db)
        }).with_context(|| {
            format!(
                "failed to migrate cached database from {} to {}",
                cached_db.display(),
                local_db.display()
            )
        })?;
    }

    let cached_lock = cached_dir.join(PROJECT_LOCK_FILE);
    let local_lock = local_dir.join(PROJECT_LOCK_FILE);
    if cached_lock.exists() && !local_lock.exists() {
        std::fs::rename(&cached_lock, &local_lock).or_else(|_| {
            std::fs::copy(&cached_lock, &local_lock)?;
            std::fs::remove_file(&cached_lock)
        }).with_context(|| {
            format!(
                "failed to migrate cached lock from {} to {}",
                cached_lock.display(),
                local_lock.display()
            )
        })?;
    }

    Ok(())
}

fn has_project_marker(path: &Path) -> bool {
    project_settings_path(path).exists()
        || path.join(".git").exists()
        || path.join("Cargo.toml").exists()
        || path.join("package.json").exists()
        || path.join("pyproject.toml").exists()
        || path.join("go.mod").exists()
}

pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };

    loop {
        if has_project_marker(&current) {
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
