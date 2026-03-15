use crate::config::Config;
use crate::daemon_protocol::{DaemonProjectInfo, Request, Response, SearchResultPayload};
use crate::project::{project_db_path, resolve_project_root};
use crate::store::Store;
use crate::utils::PatternMatcher;
use crate::{Indexer, Provider};
use crate::version::VERSION;
use anyhow::Context;
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::time::{Duration, Instant};

#[derive(Default)]
struct DaemonState {
    loaded_projects: HashMap<String, ProjectEntry>,
    project_locks: HashMap<String, Arc<AsyncMutex<()>>>,
    project_stores: HashMap<String, Arc<Store>>,
    query_embedding_cache: QueryEmbeddingCache,
    project_watchers: HashMap<String, RecommendedWatcher>,
}

#[derive(Clone, Debug, Default)]
struct ProjectEntry {
    indexing: bool,
    initial_index_done: bool,
}

pub struct QueryEmbeddingCache {
    capacity: usize,
    entries: HashMap<String, Vec<f32>>,
    access_order: Vec<String>,
}

impl Default for QueryEmbeddingCache {
    fn default() -> Self {
        Self::new(256)
    }
}

impl QueryEmbeddingCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            access_order: Vec::new(),
        }
    }

    pub fn get(&mut self, query: &str) -> Option<Vec<f32>> {
        let embedding = self.entries.get(query).cloned()?;
        self.touch(query);
        Some(embedding)
    }

    pub fn insert(&mut self, query: String, embedding: Vec<f32>) {
        self.entries.insert(query.clone(), embedding);
        self.touch(&query);

        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.access_order.first().cloned() {
                self.access_order.remove(0);
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }

    fn touch(&mut self, query: &str) {
        self.access_order.retain(|existing| existing != query);
        self.access_order.push(query.to_string());
    }
}

fn user_dir() -> anyhow::Result<PathBuf> {
    if let Ok(dir) = std::env::var("COCOINDEX_CODE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    Ok(home.join(".cocoindex_code"))
}

pub fn daemon_socket_path() -> anyhow::Result<PathBuf> {
    Ok(user_dir()?.join("daemon.sock"))
}

pub fn daemon_pid_path() -> anyhow::Result<PathBuf> {
    Ok(user_dir()?.join("daemon.pid"))
}

pub fn daemon_log_path() -> anyhow::Result<PathBuf> {
    Ok(user_dir()?.join("daemon.log"))
}

fn config_for_project(base_config: &Config, project_root: &Path) -> Config {
    let mut config = base_config.clone();
    config.db_path = project_db_path(project_root).to_string_lossy().to_string();
    config
}

fn set_indexing(state: &Arc<Mutex<DaemonState>>, project_root: &Path, indexing: bool) {
    let mut guard = state.lock().unwrap();
    guard
        .loaded_projects
        .entry(project_root.display().to_string())
        .and_modify(|entry| entry.indexing = indexing)
        .or_insert(ProjectEntry {
            indexing,
            initial_index_done: false,
        });
}

fn project_lock(state: &Arc<Mutex<DaemonState>>, project_root: &Path) -> Arc<AsyncMutex<()>> {
    let mut guard = state.lock().unwrap();
    guard
        .project_locks
        .entry(project_root.display().to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

async fn get_or_create_project_store(
    state: &Arc<Mutex<DaemonState>>,
    base_config: &Config,
    project_root: &Path,
) -> anyhow::Result<Arc<Store>> {
    let project_key = project_root.display().to_string();

    if let Some(store) = state.lock().unwrap().project_stores.get(&project_key).cloned() {
        return Ok(store);
    }

    let config = config_for_project(base_config, project_root);
    let store = Arc::new(Store::new(&config).await?);

    let mut guard = state.lock().unwrap();
    Ok(guard
        .project_stores
        .entry(project_key)
        .or_insert_with(|| store.clone())
        .clone())
}

fn mark_index_complete(state: &Arc<Mutex<DaemonState>>, project_root: &Path) {
    let mut guard = state.lock().unwrap();
    guard
        .loaded_projects
        .entry(project_root.display().to_string())
        .and_modify(|entry| {
            entry.indexing = false;
            entry.initial_index_done = true;
        })
        .or_insert(ProjectEntry {
            indexing: false,
            initial_index_done: true,
        });
}

fn should_auto_index(state: &Arc<Mutex<DaemonState>>, project_root: &Path, refresh: bool) -> bool {
    if refresh || !project_db_path(project_root).exists() {
        return true;
    }

    let guard = state.lock().unwrap();
    match guard.loaded_projects.get(&project_root.display().to_string()) {
        Some(entry) => !entry.initial_index_done,
        None => true,
    }
}

fn event_requires_reindex(project_root: &Path, matcher: &PatternMatcher, event: &Event) -> bool {
    event.paths.iter().any(|path| {
        let Ok(rel_path) = path.strip_prefix(project_root) else {
            return false;
        };

        if rel_path.starts_with(".cocoindex_code") {
            return false;
        }

        matcher.matches(rel_path)
    })
}

async fn watcher_loop(
    mut rx: UnboundedReceiver<Event>,
    state: Arc<Mutex<DaemonState>>,
    base_config: Config,
    provider: Provider,
    project_root: PathBuf,
) {
    let settings = match crate::config::ProjectSettings::load(&project_root) {
        Ok(settings) => settings,
        Err(_) => return,
    };
    let matcher = match PatternMatcher::new(&settings.include_patterns, &settings.exclude_patterns) {
        Ok(matcher) => matcher,
        Err(_) => return,
    };

    while let Some(event) = rx.recv().await {
        if !event_requires_reindex(&project_root, &matcher, &event) {
            continue;
        }

        let debounce = tokio::time::sleep(Duration::from_millis(400));
        tokio::pin!(debounce);

        loop {
            tokio::select! {
                _ = &mut debounce => break,
                maybe_event = rx.recv() => {
                    let Some(next_event) = maybe_event else {
                        return;
                    };
                    if event_requires_reindex(&project_root, &matcher, &next_event) {
                        debounce.as_mut().reset(Instant::now() + Duration::from_millis(400));
                    }
                }
            }
        }

        let _ = auto_index_project(&state, &base_config, &provider, &project_root, false).await;
    }
}

fn ensure_project_watcher(
    state: &Arc<Mutex<DaemonState>>,
    base_config: &Config,
    provider: &Provider,
    project_root: &Path,
) -> anyhow::Result<()> {
    let project_key = project_root.display().to_string();
    if state.lock().unwrap().project_watchers.contains_key(&project_key) {
        return Ok(());
    }

    let (tx, rx) = unbounded_channel::<Event>();
    let mut watcher = RecommendedWatcher::new(
        move |result| {
            if let Ok(event) = result {
                let _ = tx.send(event);
            }
        },
        NotifyConfig::default(),
    )?;
    watcher.watch(project_root, RecursiveMode::Recursive)?;

    state
        .lock()
        .unwrap()
        .project_watchers
        .insert(project_key, watcher);

    tokio::spawn(watcher_loop(
        rx,
        state.clone(),
        base_config.clone(),
        provider.clone_internal(),
        project_root.to_path_buf(),
    ));

    Ok(())
}

async fn get_query_embedding(
    state: &Arc<Mutex<DaemonState>>,
    provider: &Provider,
    query: &str,
) -> anyhow::Result<Vec<f32>> {
    if let Some(embedding) = state.lock().unwrap().query_embedding_cache.get(query) {
        return Ok(embedding);
    }

    let embeddings = provider.get_embeddings(vec![query.to_string()]).await?;
    let Some(embedding) = embeddings.into_iter().next() else {
        anyhow::bail!("Embedding API returned empty data");
    };

    state
        .lock()
        .unwrap()
        .query_embedding_cache
        .insert(query.to_string(), embedding.clone());

    Ok(embedding)
}

async fn auto_index_project(
    state: &Arc<Mutex<DaemonState>>,
    base_config: &Config,
    provider: &Provider,
    project_root: &Path,
    full_refresh: bool,
) -> anyhow::Result<Arc<Store>> {
    let project_guard = project_lock(state, project_root);
    let _guard = project_guard.lock().await;

    set_indexing(state, project_root, true);
    let store = get_or_create_project_store(state, base_config, project_root).await?;
    let indexer = Indexer::new(store.clone_internal(), provider.clone_internal(), project_root)?;
    let result = indexer
        .index_directory_with_refresh(project_root, full_refresh)
        .await;
    set_indexing(state, project_root, false);
    result?;
    mark_index_complete(state, project_root);
    let _ = ensure_project_watcher(state, base_config, provider, project_root);
    Ok(store)
}

async fn handle_request(
    state: &Arc<Mutex<DaemonState>>,
    base_config: &Config,
    provider: &Provider,
    request: Request,
) -> Response {
    match request {
        Request::Handshake { version } => Response::Handshake {
            ok: version == VERSION,
            daemon_version: VERSION.to_string(),
        },
        Request::Index { project_root, refresh } => {
            let root = match resolve_project_root(Some(Path::new(&project_root))) {
                Ok(root) => root,
                Err(err) => {
                    return Response::Error {
                        message: format!("Index failed: {}", err),
                    }
                }
            };

            match auto_index_project(state, base_config, provider, &root, refresh).await {
                Ok(_) => Response::Index {
                    success: true,
                    message: Some(format!("Indexed {}", root.display())),
                },
                Err(err) => Response::Error {
                    message: format!("Index failed: {}", err),
                },
            }
        }
        Request::Search {
            project_root,
            query,
            languages,
            paths,
            limit,
            offset,
            refresh,
        } => {
            let root = match resolve_project_root(Some(Path::new(&project_root))) {
                Ok(root) => root,
                Err(err) => {
                    return Response::Error {
                        message: format!("Search failed: {}", err),
                    }
                }
            };

            let project_guard = project_lock(state, &root);
            let _guard = project_guard.lock().await;

            let store = if should_auto_index(state, &root, refresh) {
                set_indexing(state, &root, true);
                let store = match get_or_create_project_store(state, base_config, &root).await {
                    Ok(store) => store,
                    Err(err) => {
                        set_indexing(state, &root, false);
                        return Response::Error {
                            message: format!("Search failed: {}", err),
                        };
                    }
                };
                let indexer = match Indexer::new(store.clone_internal(), provider.clone_internal(), &root) {
                    Ok(indexer) => indexer,
                    Err(err) => {
                        set_indexing(state, &root, false);
                        return Response::Error {
                            message: format!("Search failed: {}", err),
                        };
                    }
                };
                if let Err(err) = indexer.index_directory_with_refresh(&root, false).await {
                    set_indexing(state, &root, false);
                    return Response::Error {
                        message: format!("Search failed: {}", err),
                    };
                }
                set_indexing(state, &root, false);
                mark_index_complete(state, &root);
                store
            } else {
                match get_or_create_project_store(state, base_config, &root).await {
                    Ok(store) => store,
                    Err(err) => {
                        return Response::Error {
                            message: format!("Search failed: {}", err),
                        }
                    }
                }
            };

            let embedding = match get_query_embedding(state, provider, &query).await {
                Ok(embedding) => embedding,
                Err(err) => {
                    return Response::Error {
                        message: format!("Embedding failed: {}", err),
                    }
                }
            };

            match store
                .search(&embedding, limit, offset, languages.as_deref(), paths.as_deref())
                .await
            {
                Ok(results) => Response::Search {
                    success: true,
                    total_returned: results.len(),
                    offset,
                    results: results
                        .into_iter()
                        .map(|r| SearchResultPayload {
                            file_path: r.file_path,
                            language: r.language,
                            content: r.content,
                            start_line: r.start_line,
                            end_line: r.end_line,
                            score: r.score,
                        })
                        .collect(),
                    message: None,
                },
                Err(err) => Response::Error {
                    message: format!("Search failed: {}", err),
                },
            }
        }
        Request::ProjectStatus { project_root } => {
            let root = match resolve_project_root(Some(Path::new(&project_root))) {
                Ok(root) => root,
                Err(err) => {
                    return Response::Error {
                        message: format!("Status failed: {}", err),
                    }
                }
            };
            let config = config_for_project(base_config, &root);
            let indexing = state
                .lock()
                .unwrap()
                .loaded_projects
                .get(&root.display().to_string())
                .map(|entry| entry.indexing)
                .unwrap_or(false);

            if !Path::new(&config.db_path).exists() {
                return Response::ProjectStatus {
                    indexing,
                    total_chunks: 0,
                    total_files: 0,
                    languages: HashMap::new(),
                };
            }

            match get_or_create_project_store(state, base_config, &root).await {
                Ok(store) => match store.get_stats().await {
                    Ok(stats) => Response::ProjectStatus {
                        indexing,
                        total_chunks: stats.total_chunks,
                        total_files: stats.total_files,
                        languages: stats.languages,
                    },
                    Err(err) => Response::Error {
                        message: format!("Status failed: {}", err),
                    },
                },
                Err(err) => Response::Error {
                    message: format!("Status failed: {}", err),
                },
            }
        }
        Request::DaemonStatus => {
            let projects = state
                .lock()
                .unwrap()
                .loaded_projects
                .iter()
                .map(|(project_root, entry)| DaemonProjectInfo {
                    project_root: project_root.clone(),
                    indexing: entry.indexing,
                })
                .collect();
            Response::DaemonStatus {
                version: VERSION.to_string(),
                projects,
            }
        }
        Request::Stop => Response::Stop { ok: true },
    }
}

fn write_response(stream: &mut UnixStream, response: &Response) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(response)?;
    stream.write_all(&payload)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

async fn serve_connection(
    mut stream: UnixStream,
    state: Arc<Mutex<DaemonState>>,
    base_config: Config,
    provider: Provider,
) -> anyhow::Result<bool> {
    let cloned = stream.try_clone()?;
    let mut reader = BufReader::new(cloned);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim().is_empty() {
        return Ok(false);
    }
    let request: Request = serde_json::from_str(&line)?;
    let should_stop = matches!(request, Request::Stop);
    let response = handle_request(&state, &base_config, &provider, request).await;
    write_response(&mut stream, &response)?;
    Ok(should_stop)
}

pub async fn run(base_config: Config, provider: Provider) -> anyhow::Result<()> {
    let socket_path = daemon_socket_path()?;
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    let pid_path = daemon_pid_path()?;
    std::fs::write(&pid_path, std::process::id().to_string())?;

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("Failed to bind daemon socket at {}", socket_path.display()))?;
    let state = Arc::new(Mutex::new(DaemonState::default()));

    loop {
        let (stream, _) = listener.accept()?;
        let should_stop = serve_connection(stream, state.clone(), base_config.clone(), provider.clone_internal()).await?;
        if should_stop {
            break;
        }
    }

    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&pid_path);
    Ok(())
}
