use crate::config::Config;
use crate::indexer::Indexer;
use crate::project::{ensure_project_cache_layout, project_db_path, project_lock_path};
use crate::provider::Provider;
use crate::store::{SearchResult, Store, StoreStats};
use anyhow::Context;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub struct ProjectService {
    project_root: PathBuf,
    config: Config,
    provider: Provider,
    store: Store,
}

impl ProjectService {
    pub async fn open(project_root: PathBuf, config: Config) -> anyhow::Result<Self> {
        ensure_project_cache_layout(&project_root)?;
        let provider = Provider::new(&config);
        let store = Store::new(&config).await?;
        Ok(Self {
            project_root,
            config,
            provider,
            store,
        })
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub async fn open_related(&self, project_root: PathBuf) -> anyhow::Result<Self> {
        let mut config = self.config.clone();
        config.db_path = project_db_path(&project_root).to_string_lossy().to_string();
        Self::open(project_root, config).await
    }

    pub async fn index(&self, full_refresh: bool) -> anyhow::Result<IndexResult> {
        let Some(_lock) = ProjectLock::try_acquire(&self.project_root)? else {
            return Ok(IndexResult {
                indexing: true,
                message: "Index update already in progress for this project.".to_string(),
            });
        };

        let indexer = Indexer::new(
            self.store.clone_internal(),
            self.provider.clone_internal(),
            &self.project_root,
        )?;
        indexer
            .index_directory_with_refresh(&self.project_root, full_refresh)
            .await?;

        Ok(IndexResult {
            indexing: false,
            message: format!("Indexed {}", self.project_root.display()),
        })
    }

    pub async fn refresh_if_idle(&self) -> anyhow::Result<bool> {
        let Some(_lock) = ProjectLock::try_acquire(&self.project_root)? else {
            return Ok(false);
        };

        let indexer = Indexer::new(
            self.store.clone_internal(),
            self.provider.clone_internal(),
            &self.project_root,
        )?;
        indexer.index_directory(&self.project_root).await?;
        Ok(true)
    }

    pub async fn start_watcher(&self) -> anyhow::Result<JoinHandle<()>> {
        let service = self.open_related(self.project_root.clone()).await?;
        let project_root = self.project_root.clone();

        Ok(tokio::spawn(async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<()>();
            let mut watcher = match RecommendedWatcher::new(
                move |result: notify::Result<notify::Event>| {
                    if result.is_ok() {
                        let _ = tx.send(());
                    }
                },
                notify::Config::default(),
            ) {
                Ok(watcher) => watcher,
                Err(_) => return,
            };

            if watcher.watch(&project_root, RecursiveMode::Recursive).is_err() {
                return;
            }

            while rx.recv().await.is_some() {
                let debounce = tokio::time::sleep(Duration::from_millis(500));
                tokio::pin!(debounce);

                loop {
                    tokio::select! {
                        _ = &mut debounce => {
                            let _ = service.refresh_if_idle().await;
                            break;
                        }
                        item = rx.recv() => {
                            if item.is_none() {
                                return;
                            }
                        }
                    }
                }
            }
        }))
    }

    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
        languages: Option<Vec<String>>,
        paths: Option<Vec<String>>,
        refresh: bool,
    ) -> anyhow::Result<Vec<SearchResult>> {
        if refresh {
            let _ = self.refresh_if_idle().await?;
        }

        let embeddings = self.provider.get_embeddings(vec![query.to_string()]).await?;
        let query_embedding = embeddings
            .into_iter()
            .next()
            .context("Embedding API returned no query embedding")?;

        let fetch_limit = ((limit + offset).max(10) * 5).min(100);
        let results = self.store
            .search(
                &query_embedding,
                fetch_limit,
                0,
                languages.as_deref(),
                paths.as_deref(),
            )
            .await?;

        let mut ranked = rerank_results(query, results, offset, limit);
        self.hydrate_result_content(&mut ranked)?;
        Ok(ranked)
    }

    pub async fn stats(&self) -> anyhow::Result<ProjectStatus> {
        let stats = self.store.get_stats().await?;
        Ok(ProjectStatus {
            indexing: project_lock_path(&self.project_root).exists(),
            stats,
            db_path: project_db_path(&self.project_root),
        })
    }

    fn hydrate_result_content(&self, results: &mut [SearchResult]) -> anyhow::Result<()> {
        for result in results {
            result.content = read_snippet(
                &self.project_root.join(&result.file_path),
                result.start_line,
                result.end_line,
            )?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct IndexResult {
    pub indexing: bool,
    pub message: String,
}

#[derive(Debug)]
pub struct ProjectStatus {
    pub indexing: bool,
    pub stats: StoreStats,
    pub db_path: PathBuf,
}

struct ProjectLock {
    path: PathBuf,
}

impl ProjectLock {
    fn try_acquire(project_root: &Path) -> anyhow::Result<Option<Self>> {
        let path = project_lock_path(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                let _ = writeln!(file, "{}", std::process::id());
                Ok(Some(Self { path }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn read_snippet(path: &Path, start_line: usize, end_line: usize) -> anyhow::Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read snippet from {}", path.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Ok(String::new());
    }

    let start = start_line.saturating_sub(1).min(lines.len());
    let end = end_line.min(lines.len());
    if start >= end {
        return Ok(String::new());
    }

    let mut snippet = lines[start..end].join("\n");
    if content.ends_with('\n') {
        snippet.push('\n');
    }
    Ok(snippet)
}

fn rerank_results(
    query: &str,
    mut results: Vec<SearchResult>,
    offset: usize,
    limit: usize,
) -> Vec<SearchResult> {
    let intent = QueryIntent::from_query(query);

    for result in &mut results {
        result.score += path_weight(&result.file_path, &intent);
        result.score += path_token_weight(&result.file_path, &intent);
    }

    results.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut seen = std::collections::HashMap::<String, usize>::new();
    results.retain(|result| {
        let count = seen.entry(result.file_path.clone()).or_insert(0);
        if *count >= 2 {
            return false;
        }
        *count += 1;
        true
    });

    results.into_iter().skip(offset).take(limit).collect()
}

#[derive(Debug, Default)]
struct QueryIntent {
    implementation_bias: bool,
    docs_bias: bool,
    examples_bias: bool,
    tests_bias: bool,
    config_bias: bool,
    preferred_extensions: Vec<&'static str>,
    query_tokens: Vec<String>,
}

impl QueryIntent {
    fn from_query(query: &str) -> Self {
        let normalized = query.to_ascii_lowercase();
        let query_tokens = tokenize_query(&normalized);
        let mut intent = Self {
            implementation_bias: contains_any(
                &normalized,
                &[
                    "implementation",
                    "implement",
                    "logic",
                    "function",
                    "method",
                    "class",
                    "struct",
                    "handler",
                    "server",
                    "client",
                    "where is",
                    "find the code",
                    "show the code",
                ],
            ),
            docs_bias: contains_any(
                &normalized,
                &[
                    "doc",
                    "docs",
                    "documentation",
                    "guide",
                    "tutorial",
                    "readme",
                    "manual",
                    "navbar",
                    "sidebar",
                ],
            ),
            examples_bias: contains_any(
                &normalized,
                &["example", "examples", "demo", "sample", "playground"],
            ),
            tests_bias: contains_any(
                &normalized,
                &["test", "tests", "testing", "assert", "coverage", "fixture", "spec"],
            ),
            config_bias: contains_any(
                &normalized,
                &[
                    "config",
                    "configuration",
                    "setting",
                    "settings",
                    "env",
                    "environment",
                    "yaml",
                    "yml",
                    "json",
                    "toml",
                ],
            ),
            preferred_extensions: Vec::new(),
            query_tokens,
        };

        for (needles, exts) in [
            (&["python", "py"][..], &[".py"][..]),
            (&["rust", "rs", "cargo"][..], &[".rs"][..]),
            (&["golang", "go version", "go implementation"][..], &[".go"][..]),
            (&["typescript", "ts"][..], &[".ts", ".tsx"][..]),
            (&["javascript", "js", "node"][..], &[".js", ".jsx", ".mjs", ".cjs"][..]),
            (&["java"][..], &[".java"][..]),
            (&["csharp", "c#", "dotnet"][..], &[".cs"][..]),
            (&["cpp", "c++"][..], &[".cpp", ".cc", ".cxx", ".hpp", ".hh", ".h"][..]),
            (&["c language", "ansi c"][..], &[".c", ".h"][..]),
            (&["bash", "shell", "zsh"][..], &[".sh", ".bash", ".zsh"][..]),
            (&["markdown", "md"][..], &[".md", ".mdx"][..]),
            (&["yaml", "yml"][..], &[".yaml", ".yml"][..]),
            (&["json"][..], &[".json"][..]),
            (&["toml"][..], &[".toml"][..]),
            (&["sql", "sqlite", "postgres"][..], &[".sql"][..]),
            (&["html"][..], &[".html", ".htm"][..]),
            (&["css", "scss"][..], &[".css", ".scss"][..]),
        ] {
            if contains_any(&normalized, needles) {
                intent.preferred_extensions.extend_from_slice(exts);
            }
        }

        intent
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn tokenize_query(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            if current.len() >= 3 {
                tokens.push(current.clone());
            }
            current.clear();
        }
    }

    if !current.is_empty() && current.len() >= 3 {
        tokens.push(current);
    }

    tokens.retain(|token| {
        !matches!(
            token.as_str(),
            "where"
                | "what"
                | "when"
                | "which"
                | "with"
                | "does"
                | "this"
                | "that"
                | "from"
                | "into"
                | "used"
                | "using"
                | "code"
                | "file"
                | "files"
                | "path"
                | "paths"
                | "logic"
        )
    });
    tokens.sort();
    tokens.dedup();
    tokens
}

fn path_weight(file_path: &str, intent: &QueryIntent) -> f32 {
    let normalized = file_path.replace('\\', "/").to_ascii_lowercase();
    let mut weight: f32 = 0.0;
    let is_source = ["/src/", "/app/", "/lib/", "/pkg/", "/internal/", "/core/", "/server/", "/client/"]
        .iter()
        .any(|good| normalized.contains(good));
    let is_test = ["/tests/", "/test/", "/__tests__/", "/spec/", "/fixtures/"]
        .iter()
        .any(|penalized| normalized.contains(penalized));
    let is_docs = ["/docs/", "/doc/", "/guides/", "/tutorials/"]
        .iter()
        .any(|dir| normalized.contains(dir));
    let is_examples = ["/examples/", "/example/", "/samples/", "/demos/"]
        .iter()
        .any(|dir| normalized.contains(dir));
    let is_readme = normalized.ends_with("/readme.md") || normalized.ends_with("readme.md");
    let is_config = normalized.ends_with(".toml")
        || normalized.ends_with(".yaml")
        || normalized.ends_with(".yml")
        || normalized.ends_with(".json")
        || normalized.ends_with(".ini")
        || normalized.ends_with(".env");

    if is_source {
        weight += 0.035;
    }
    if is_test {
        weight -= 0.06;
    }
    if is_docs {
        weight -= 0.04;
    }
    if is_examples {
        weight -= 0.02;
    }

    if normalized.ends_with(".md") || normalized.ends_with(".txt") {
        weight -= 0.04;
    }
    if normalized.ends_with(".sh") || normalized.ends_with(".bash") {
        weight -= 0.03;
    }
    if is_readme {
        weight -= 0.05;
    }

    if intent.implementation_bias {
        if is_source {
            weight += 0.05;
        }
        if is_test {
            weight -= 0.04;
        }
        if is_docs || is_readme {
            weight -= 0.05;
        }
    }

    if intent.docs_bias {
        if is_docs || is_readme {
            weight += 0.16;
        }
        if is_source {
            weight -= 0.04;
        }
        if is_test {
            weight -= 0.04;
        }
        if is_examples {
            weight -= 0.02;
        }
    }

    if intent.examples_bias {
        if is_examples {
            weight += 0.14;
        }
        if is_docs {
            weight += 0.02;
        }
    }

    if intent.tests_bias {
        if is_test {
            weight += 0.10;
        }
        if is_source {
            weight -= 0.02;
        }
    }

    if intent.config_bias {
        if is_config {
            weight += 0.10;
        }
        if is_test {
            weight -= 0.02;
        }
    }

    if intent
        .preferred_extensions
        .iter()
        .any(|ext| normalized.ends_with(ext))
    {
        weight += 0.14;
    }

    weight
}

fn path_token_weight(file_path: &str, intent: &QueryIntent) -> f32 {
    if intent.query_tokens.is_empty() {
        return 0.0;
    }

    let normalized = file_path.replace('\\', "/").to_ascii_lowercase();
    let file_name = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    let stem = file_name
        .split('.')
        .next()
        .unwrap_or(file_name);

    let mut weight: f32 = 0.0;
    for token in &intent.query_tokens {
        if file_name.contains(token) {
            weight += 0.08;
            continue;
        }
        if stem.contains(token) {
            weight += 0.06;
            continue;
        }
        if normalized.contains(token) {
            weight += 0.03;
        }
    }

    weight.min(0.24)
}
