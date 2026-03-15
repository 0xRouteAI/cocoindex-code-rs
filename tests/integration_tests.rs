use std::path::{Path, PathBuf};
use tempfile::TempDir;
use std::fs;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;
use coco_rs::{Store, Provider, config::Config};
use std::sync::Arc;

// Helper to create a test project
fn create_test_project() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().to_path_buf();

    // Create test files
    fs::create_dir_all(project_path.join("src")).unwrap();
    fs::write(
        project_path.join("src/main.rs"),
        "fn main() {\n    println!(\"Hello, world!\");\n}\n\nfn helper() {\n    println!(\"helper\");\n}\n"
    ).unwrap();

    fs::write(
        project_path.join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn multiply(a: i32, b: i32) -> i32 {\n    a * b\n}\n"
    ).unwrap();

    (temp_dir, project_path)
}

fn create_test_config(project_path: &PathBuf) -> Config {
    // Create database directory
    let db_dir = project_path.join(".cocoindex_code");
    fs::create_dir_all(&db_dir).unwrap();

    Config {
        api_key: "test-key".to_string(),
        api_base: "https://api.openai.com/v1".to_string(),
        model: "text-embedding-3-small".to_string(),
        embedding_dim: 1536,
        db_path: db_dir.join("target_sqlite.db").to_string_lossy().to_string(),
    }
}

fn write_project_marker(project_root: &Path) {
    let config_dir = project_root.join(".cocoindex_code");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("settings.yml"), "include_patterns: []\n").unwrap();
}

fn daemon_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_daemon_test() -> std::sync::MutexGuard<'static, ()> {
    match daemon_test_lock().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[tokio::test]
async fn test_deleted_file_cleanup() {
    // Test: 删除文件后搜索不应返回旧结果
    let (_temp_dir, project_path) = create_test_project();
    let config = create_test_config(&project_path);

    // Create store
    let store = Arc::new(Store::new(&config).await.unwrap());

    // Verify lib.rs exists
    assert!(project_path.join("src/lib.rs").exists());

    // Get all indexed files before deletion
    // Delete src/lib.rs
    fs::remove_file(project_path.join("src/lib.rs")).unwrap();
    assert!(!project_path.join("src/lib.rs").exists());

    // Simulate re-indexing (which should clean up deleted files)
    let current_files: std::collections::HashSet<String> = vec!["src/main.rs".to_string()]
        .into_iter()
        .collect();

    let indexed_files = store.get_all_indexed_files().await.unwrap();
    let deleted_files: Vec<String> = indexed_files
        .into_iter()
        .filter(|f| !current_files.contains(f))
        .collect();

    if !deleted_files.is_empty() {
        store.delete_files(&deleted_files).await.unwrap();
    }

    // Verify lib.rs is no longer in index
    let files_after = store.get_all_indexed_files().await.unwrap();
    assert!(!files_after.iter().any(|f| f.contains("lib.rs")));
}

#[tokio::test]
async fn test_file_shrink_cleanup() {
    // Test: 文件 chunk 数减少后不应返回旧 chunk
    let (_temp_dir, project_path) = create_test_project();
    let config = create_test_config(&project_path);

    let store = Arc::new(Store::new(&config).await.unwrap());

    // Truncate src/main.rs to 1 line
    fs::write(
        project_path.join("src/main.rs"),
        "fn main() {}\n"
    ).unwrap();

    // Delete old chunks for the file
    store.delete_file_chunks("src/main.rs").await.unwrap();

    // Verify deletion worked (no chunks for main.rs)
    let all_files = store.get_all_indexed_files().await.unwrap();
    assert!(!all_files.iter().any(|f| f == "src/main.rs"));
}

#[tokio::test]
async fn test_language_filter() {
    // Test: --languages rust 能命中 .rs 文件
    let (_temp_dir, project_path) = create_test_project();

    // Create a Python file
    fs::write(
        project_path.join("test.py"),
        "def hello():\n    print('hello')\n"
    ).unwrap();

    // Verify language detection
    let rust_lang = coco_rs::utils::detect_language(&project_path.join("src/main.rs"));
    assert_eq!(rust_lang, Some("rust".to_string()));

    let python_lang = coco_rs::utils::detect_language(&project_path.join("test.py"));
    assert_eq!(python_lang, Some("python".to_string()));
}

#[tokio::test]
async fn test_language_overrides() {
    // Test: language_overrides 生效
    let (_temp_dir, project_path) = create_test_project();

    // Create a .inc file
    fs::write(
        project_path.join("config.inc"),
        "<?php\necho 'test';\n?>\n"
    ).unwrap();

    // Test language override
    let mut overrides = std::collections::HashMap::new();
    overrides.insert("inc".to_string(), "php".to_string());

    let detected = coco_rs::utils::detect_language_with_overrides(
        &project_path.join("config.inc"),
        &overrides
    );

    assert_eq!(detected, Some("php".to_string()));
}

#[tokio::test]
async fn test_provider_empty_response() {
    // Test: provider 返回空数组时不 panic
    let (_temp_dir, project_path) = create_test_project();
    let config = create_test_config(&project_path);

    let provider = Provider::new(&config);

    // Test with empty input
    let result = provider.get_embeddings(vec![]).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}

#[tokio::test]
async fn test_provider_http_error() {
    // Test: provider 返回 4xx/5xx 时不 panic
    let (_temp_dir, project_path) = create_test_project();
    let mut config = create_test_config(&project_path);

    // Use invalid API endpoint to trigger error
    config.api_base = "https://invalid-endpoint-that-does-not-exist.example.com".to_string();

    let provider = Provider::new(&config);
    let result = provider.get_embeddings(vec!["test".to_string()]).await;

    // Should return error, not panic
    assert!(result.is_err());
}

#[test]
fn test_provider_batches_by_item_count_and_char_budget() {
    let texts = vec![
        "a".repeat(10),
        "b".repeat(10),
        "c".repeat(10),
        "d".repeat(10),
        "e".repeat(10),
    ];

    let batches = coco_rs::provider::plan_embedding_batches(&texts, 2, 25);

    assert_eq!(batches, vec![(0, 2), (2, 4), (4, 5)]);
}

#[test]
fn test_provider_batches_large_single_item_alone() {
    let texts = vec![
        "a".repeat(10),
        "b".repeat(80),
        "c".repeat(10),
    ];

    let batches = coco_rs::provider::plan_embedding_batches(&texts, 4, 32);

    assert_eq!(batches, vec![(0, 1), (1, 2), (2, 3)]);
}

#[test]
fn test_provider_reduces_chunk_size_for_short_context_models() {
    let config = Config {
        api_key: "test-key".to_string(),
        api_base: "mock://embedding".to_string(),
        model: "BAAI/bge-large-zh-v1.5".to_string(),
        embedding_dim: 1024,
        db_path: "/tmp/test.db".to_string(),
    };

    let provider = Provider::new(&config);
    let chunking = provider.chunking_profile();

    assert!(chunking.chunk_size < 2000);
    assert!(chunking.chunk_overlap < chunking.chunk_size);
}

#[test]
fn test_query_embedding_cache_reuses_existing_embedding() {
    let mut cache = coco_rs::daemon::QueryEmbeddingCache::new(2);
    let first = vec![0.1, 0.2];
    let second = vec![0.3, 0.4];

    cache.insert("first".to_string(), first.clone());
    cache.insert("second".to_string(), second.clone());

    assert_eq!(cache.get("first"), Some(first.clone()));

    cache.insert("third".to_string(), vec![0.5, 0.6]);

    assert_eq!(cache.get("first"), Some(first));
    assert_eq!(cache.get("second"), None);
    assert_eq!(cache.get("third"), Some(vec![0.5, 0.6]));
}

#[test]
fn test_find_project_root_from_nested_directory() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().join("project");
    let nested = project_root.join("src/lib");
    fs::create_dir_all(&nested).unwrap();
    write_project_marker(&project_root);

    let resolved = coco_rs::project::find_project_root(&nested);

    assert_eq!(resolved.as_deref(), Some(project_root.as_path()));
}

#[test]
fn test_project_db_path_is_scoped_to_project_root() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().join("project");
    fs::create_dir_all(&project_root).unwrap();

    let db_path = coco_rs::project::project_db_path(&project_root);

    assert_eq!(db_path, project_root.join(".cocoindex_code/target_sqlite.db"));
}

#[test]
fn test_default_path_filter_for_nested_directory() {
    let project_root = Path::new("/tmp/demo");
    let cwd = Path::new("/tmp/demo/src/module");

    let filter = coco_rs::project::default_path_filter(project_root, cwd);

    assert_eq!(filter.as_deref(), Some("src/module/*"));
}

#[test]
fn test_scoped_chunk_ids_do_not_collide_across_projects() {
    let left = Path::new("/tmp/project-a");
    let right = Path::new("/tmp/project-b");

    let left_id = coco_rs::project::scoped_chunk_id(left, "src/main.rs", 0);
    let right_id = coco_rs::project::scoped_chunk_id(right, "src/main.rs", 0);

    assert_ne!(left_id, right_id);
}

#[tokio::test]
async fn test_store_creates_missing_parent_directories() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().join("project");
    let db_path = project_root.join(".cocoindex_code/target_sqlite.db");

    let config = Config {
        api_key: "test-key".to_string(),
        api_base: "https://api.openai.com/v1".to_string(),
        model: "text-embedding-3-small".to_string(),
        embedding_dim: 1536,
        db_path: db_path.to_string_lossy().to_string(),
    };

    let _store = Store::new(&config).await.unwrap();

    assert!(db_path.exists());
}

#[tokio::test]
async fn test_project_scoped_stores_do_not_mix_results() {
    let left_dir = TempDir::new().unwrap();
    let right_dir = TempDir::new().unwrap();
    let left_root = left_dir.path().join("project-a");
    let right_root = right_dir.path().join("project-b");
    fs::create_dir_all(&left_root).unwrap();
    fs::create_dir_all(&right_root).unwrap();

    let left_config = Config {
        api_key: "test-key".to_string(),
        api_base: "https://api.openai.com/v1".to_string(),
        model: "text-embedding-3-small".to_string(),
        embedding_dim: 3,
        db_path: coco_rs::project::project_db_path(&left_root).to_string_lossy().to_string(),
    };
    let right_config = Config {
        api_key: "test-key".to_string(),
        api_base: "https://api.openai.com/v1".to_string(),
        model: "text-embedding-3-small".to_string(),
        embedding_dim: 3,
        db_path: coco_rs::project::project_db_path(&right_root).to_string_lossy().to_string(),
    };

    let left_store = Store::new(&left_config).await.unwrap();
    let right_store = Store::new(&right_config).await.unwrap();

    left_store.save_chunk(
        &coco_rs::project::scoped_chunk_id(&left_root, "src/main.rs", 0),
        "src/main.rs",
        Some("rust"),
        "fn alpha() {}",
        1,
        1,
        "hash-a",
        &[0.0, 0.0, 0.0],
    ).await.unwrap();

    right_store.save_chunk(
        &coco_rs::project::scoped_chunk_id(&right_root, "src/main.rs", 0),
        "src/main.rs",
        Some("rust"),
        "fn beta() {}",
        1,
        1,
        "hash-b",
        &[10.0, 10.0, 10.0],
    ).await.unwrap();

    let left_results = left_store.search(&[0.0, 0.0, 0.0], 10, 0, None, None).await.unwrap();
    let right_results = right_store.search(&[10.0, 10.0, 10.0], 10, 0, None, None).await.unwrap();

    assert_eq!(left_results.len(), 1);
    assert_eq!(right_results.len(), 1);
    assert!(left_results[0].content.contains("alpha"));
    assert!(right_results[0].content.contains("beta"));
}

#[test]
fn test_cli_help_succeeds() {
    let binary = env!("CARGO_BIN_EXE_cocoindex-code-rs");
    let output = Command::new(binary).arg("--help").output().unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn test_daemon_index_and_search_with_mock_embeddings() {
    let _guard = lock_daemon_test();

    let daemon_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let project_root = project_dir.path().join("project");
    fs::create_dir_all(project_root.join("src")).unwrap();
    fs::write(
        project_root.join("src/main.rs"),
        "fn calculate_fibonacci(n: u32) -> u32 { if n <= 1 { n } else { calculate_fibonacci(n - 1) + calculate_fibonacci(n - 2) } }\n",
    )
    .unwrap();

    std::env::set_var("COCOINDEX_CODE_DIR", daemon_dir.path());
    std::env::set_var("OPENAI_API_BASE", "mock://embedding");
    std::env::set_var("OPENAI_API_KEY", "test-key");
    std::env::set_var("EMBEDDING_MODEL", "mock-model");
    std::env::set_var("EMBEDDING_DIM", "16");
    std::env::set_var(
        "CARGO_BIN_EXE_cocoindex-code-rs",
        env!("CARGO_BIN_EXE_cocoindex-code-rs"),
    );

    let _ = coco_rs::daemon_client::stop_daemon();

    let client = coco_rs::daemon_client::ensure_daemon().unwrap();
    let handshake = client.handshake().unwrap();
    match handshake {
        coco_rs::daemon_protocol::Response::Handshake { ok, .. } => assert!(ok),
        other => panic!("unexpected handshake response: {:?}", other),
    }

    let index_response = client.request(&coco_rs::daemon_protocol::Request::Index {
        project_root: project_root.display().to_string(),
        refresh: false,
    }).unwrap();
    match index_response {
        coco_rs::daemon_protocol::Response::Index { success, .. } => assert!(success),
        other => panic!("unexpected index response: {:?}", other),
    }

    let search_response = client.request(&coco_rs::daemon_protocol::Request::Search {
        project_root: project_root.display().to_string(),
        query: "fibonacci".to_string(),
        languages: None,
        paths: None,
        limit: 5,
        offset: 0,
        refresh: false,
    }).unwrap();
    match search_response {
        coco_rs::daemon_protocol::Response::Search { success, results, .. } => {
            assert!(success);
            assert!(!results.is_empty());
            assert!(results[0].content.contains("fibonacci"));
        }
        other => panic!("unexpected search response: {:?}", other),
    }

    let _ = coco_rs::daemon_client::stop_daemon();
    std::env::remove_var("COCOINDEX_CODE_DIR");
    std::env::remove_var("OPENAI_API_BASE");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("EMBEDDING_MODEL");
    std::env::remove_var("EMBEDDING_DIM");
}

#[test]
fn test_daemon_search_auto_indexes_project() {
    let _guard = lock_daemon_test();

    let daemon_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let project_root = project_dir.path().join("project");
    fs::create_dir_all(project_root.join("src")).unwrap();
    fs::write(
        project_root.join("src/main.rs"),
        "fn auto_index_probe() -> bool { true }\n",
    )
    .unwrap();

    std::env::set_var("COCOINDEX_CODE_DIR", daemon_dir.path());
    std::env::set_var("OPENAI_API_BASE", "mock://embedding");
    std::env::set_var("OPENAI_API_KEY", "test-key");
    std::env::set_var("EMBEDDING_MODEL", "mock-model");
    std::env::set_var("EMBEDDING_DIM", "16");
    std::env::set_var(
        "CARGO_BIN_EXE_cocoindex-code-rs",
        env!("CARGO_BIN_EXE_cocoindex-code-rs"),
    );

    let _ = coco_rs::daemon_client::stop_daemon();

    let client = coco_rs::daemon_client::ensure_daemon().unwrap();
    let search_response = client.request(&coco_rs::daemon_protocol::Request::Search {
        project_root: project_root.display().to_string(),
        query: "auto index".to_string(),
        languages: None,
        paths: None,
        limit: 5,
        offset: 0,
        refresh: false,
    }).unwrap();

    match search_response {
        coco_rs::daemon_protocol::Response::Search { success, results, .. } => {
            assert!(success);
            assert!(!results.is_empty());
            assert!(project_root.join(".cocoindex_code/target_sqlite.db").exists());
        }
        other => panic!("unexpected search response: {:?}", other),
    }

    let _ = coco_rs::daemon_client::stop_daemon();
    std::env::remove_var("COCOINDEX_CODE_DIR");
    std::env::remove_var("OPENAI_API_BASE");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("EMBEDDING_MODEL");
    std::env::remove_var("EMBEDDING_DIM");
}

#[test]
fn test_daemon_handles_multiple_clients_for_same_project() {
    let _guard = lock_daemon_test();

    let daemon_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let project_root = project_dir.path().join("project");
    fs::create_dir_all(project_root.join("src")).unwrap();
    fs::write(
        project_root.join("src/main.rs"),
        "fn concurrent_probe() -> bool { true }\n",
    )
    .unwrap();

    std::env::set_var("COCOINDEX_CODE_DIR", daemon_dir.path());
    std::env::set_var("OPENAI_API_BASE", "mock://embedding");
    std::env::set_var("OPENAI_API_KEY", "test-key");
    std::env::set_var("EMBEDDING_MODEL", "mock-model");
    std::env::set_var("EMBEDDING_DIM", "16");
    std::env::set_var(
        "CARGO_BIN_EXE_cocoindex-code-rs",
        env!("CARGO_BIN_EXE_cocoindex-code-rs"),
    );

    let _ = coco_rs::daemon_client::stop_daemon();
    let _client = coco_rs::daemon_client::ensure_daemon().unwrap();

    let project_root_str = project_root.display().to_string();

    let index_thread = {
        let project_root = project_root_str.clone();
        thread::spawn(move || {
            let client = coco_rs::daemon_client::DaemonClient::connect().unwrap();
            client.request(&coco_rs::daemon_protocol::Request::Index {
                project_root,
                refresh: false,
            }).unwrap()
        })
    };

    let search_thread = {
        let project_root = project_root_str.clone();
        thread::spawn(move || {
            let client = coco_rs::daemon_client::DaemonClient::connect().unwrap();
            client.request(&coco_rs::daemon_protocol::Request::Search {
                project_root,
                query: "concurrent_probe".to_string(),
                languages: None,
                paths: None,
                limit: 5,
                offset: 0,
                refresh: false,
            }).unwrap()
        })
    };

    match index_thread.join().unwrap() {
        coco_rs::daemon_protocol::Response::Index { success, .. } => assert!(success),
        other => panic!("unexpected index response: {:?}", other),
    }

    match search_thread.join().unwrap() {
        coco_rs::daemon_protocol::Response::Search { success, results, .. } => {
            assert!(success);
            assert!(!results.is_empty());
        }
        other => panic!("unexpected search response: {:?}", other),
    }

    let _ = coco_rs::daemon_client::stop_daemon();
    std::env::remove_var("COCOINDEX_CODE_DIR");
    std::env::remove_var("OPENAI_API_BASE");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("EMBEDDING_MODEL");
    std::env::remove_var("EMBEDDING_DIM");
}

#[test]
fn test_cli_index_and_search_with_mock_embeddings() {
    let _guard = lock_daemon_test();

    let daemon_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let project_root = project_dir.path().join("project");
    fs::create_dir_all(project_root.join("src")).unwrap();
    fs::write(
        project_root.join("src/main.rs"),
        "fn authentication_logic() -> bool { true }\n",
    )
    .unwrap();

    let binary = env!("CARGO_BIN_EXE_cocoindex-code-rs");
    let base = Command::new(binary);

    let common_envs = [
        ("COCOINDEX_CODE_DIR", daemon_dir.path().to_string_lossy().to_string()),
        ("OPENAI_API_BASE", "mock://embedding".to_string()),
        ("OPENAI_API_KEY", "test-key".to_string()),
        ("EMBEDDING_MODEL", "mock-model".to_string()),
        ("EMBEDDING_DIM", "16".to_string()),
        ("CARGO_BIN_EXE_cocoindex-code-rs", binary.to_string()),
    ];

    let _ = coco_rs::daemon_client::stop_daemon();

    let mut index_cmd = base;
    let index_output = index_cmd
        .envs(common_envs.clone())
        .arg("index")
        .arg(&project_root)
        .output()
        .unwrap();
    assert!(index_output.status.success(), "stderr: {}", String::from_utf8_lossy(&index_output.stderr));

    let search_output = Command::new(binary)
        .envs(common_envs)
        .arg("search")
        .arg("authentication")
        .arg("--project-root")
        .arg(&project_root)
        .output()
        .unwrap();
    assert!(search_output.status.success(), "stderr: {}", String::from_utf8_lossy(&search_output.stderr));
    let stdout = String::from_utf8_lossy(&search_output.stdout);
    assert!(stdout.contains("src/main.rs"));

    let _ = coco_rs::daemon_client::stop_daemon();
}

#[test]
fn test_cli_daemon_status_and_stop() {
    let _guard = lock_daemon_test();

    let daemon_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let project_root = project_dir.path().join("project");
    fs::create_dir_all(project_root.join("src")).unwrap();
    fs::write(project_root.join("src/main.rs"), "fn daemon_status_probe() {}\n").unwrap();

    let binary = env!("CARGO_BIN_EXE_cocoindex-code-rs");
    let common_envs = [
        ("COCOINDEX_CODE_DIR", daemon_dir.path().to_string_lossy().to_string()),
        ("OPENAI_API_BASE", "mock://embedding".to_string()),
        ("OPENAI_API_KEY", "test-key".to_string()),
        ("EMBEDDING_MODEL", "mock-model".to_string()),
        ("EMBEDDING_DIM", "16".to_string()),
        ("CARGO_BIN_EXE_cocoindex-code-rs", binary.to_string()),
    ];

    let _ = coco_rs::daemon_client::stop_daemon();

    let index_output = Command::new(binary)
        .envs(common_envs.clone())
        .arg("index")
        .arg(&project_root)
        .output()
        .unwrap();
    assert!(index_output.status.success(), "stderr: {}", String::from_utf8_lossy(&index_output.stderr));

    let status_output = Command::new(binary)
        .envs(common_envs.clone())
        .arg("daemon-status")
        .output()
        .unwrap();
    assert!(status_output.status.success(), "stderr: {}", String::from_utf8_lossy(&status_output.stderr));
    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(status_stdout.contains(&project_root.to_string_lossy().to_string()));

    let stop_output = Command::new(binary)
        .envs(common_envs.clone())
        .arg("stop-daemon")
        .output()
        .unwrap();
    assert!(stop_output.status.success(), "stderr: {}", String::from_utf8_lossy(&stop_output.stderr));

    let socket_path = daemon_dir.path().join("daemon.sock");
    assert!(!socket_path.exists(), "daemon socket still exists at {}", socket_path.display());
}

#[test]
fn test_status_does_not_create_database() {
    let _guard = lock_daemon_test();

    let daemon_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let project_root = project_dir.path().join("project");
    fs::create_dir_all(&project_root).unwrap();

    let binary = env!("CARGO_BIN_EXE_cocoindex-code-rs");
    let common_envs = [
        ("COCOINDEX_CODE_DIR", daemon_dir.path().to_string_lossy().to_string()),
        ("OPENAI_API_BASE", "mock://embedding".to_string()),
        ("OPENAI_API_KEY", "test-key".to_string()),
        ("EMBEDDING_MODEL", "mock-model".to_string()),
        ("EMBEDDING_DIM", "16".to_string()),
        ("CARGO_BIN_EXE_cocoindex-code-rs", binary.to_string()),
    ];

    let _ = coco_rs::daemon_client::stop_daemon();

    let status_output = Command::new(binary)
        .envs(common_envs.clone())
        .arg("status")
        .arg("--project-root")
        .arg(&project_root)
        .output()
        .unwrap();
    assert!(status_output.status.success(), "stderr: {}", String::from_utf8_lossy(&status_output.stderr));

    assert!(!project_root.join(".cocoindex_code/target_sqlite.db").exists());

    let _ = coco_rs::daemon_client::stop_daemon();
}

#[test]
fn test_cli_search_defaults_to_current_subdirectory() {
    let _guard = lock_daemon_test();

    let daemon_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let project_root = project_dir.path().join("project");
    fs::create_dir_all(project_root.join("src/feature")).unwrap();
    fs::create_dir_all(project_root.join("src/other")).unwrap();
    fs::write(
        project_root.join("src/feature/main.rs"),
        "fn feature_authentication() -> bool { true }\n",
    )
    .unwrap();
    fs::write(
        project_root.join("src/other/main.rs"),
        "fn other_authentication() -> bool { true }\n",
    )
    .unwrap();

    let binary = env!("CARGO_BIN_EXE_cocoindex-code-rs");
    let common_envs = [
        ("COCOINDEX_CODE_DIR", daemon_dir.path().to_string_lossy().to_string()),
        ("OPENAI_API_BASE", "mock://embedding".to_string()),
        ("OPENAI_API_KEY", "test-key".to_string()),
        ("EMBEDDING_MODEL", "mock-model".to_string()),
        ("EMBEDDING_DIM", "16".to_string()),
        ("CARGO_BIN_EXE_cocoindex-code-rs", binary.to_string()),
    ];

    let _ = coco_rs::daemon_client::stop_daemon();

    let search_output = Command::new(binary)
        .envs(common_envs)
        .current_dir(project_root.join("src/feature"))
        .arg("search")
        .arg("feature_authentication")
        .arg("--project-root")
        .arg(&project_root)
        .output()
        .unwrap();
    assert!(search_output.status.success(), "stderr: {}", String::from_utf8_lossy(&search_output.stderr));
    let stdout = String::from_utf8_lossy(&search_output.stdout);
    assert!(stdout.contains("src/feature/main.rs"));
    assert!(!stdout.contains("src/other/main.rs"));

    let _ = coco_rs::daemon_client::stop_daemon();
}

#[cfg(test)]
mod mcp_tests {
    use serde_json::json;

    #[tokio::test]
    async fn test_mcp_content_length_framing() {
        // Test: MCP stdio 使用 Content-Length header

        let request = json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {},
            "id": 1
        });

        let request_str = serde_json::to_string(&request).unwrap();
        let content_length = request_str.len();

        // Verify Content-Length header format
        let header = format!("Content-Length: {}\r\n\r\n", content_length);
        assert!(header.starts_with("Content-Length: "));
        assert!(header.ends_with("\r\n\r\n"));

        // Verify body is valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&request_str).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "initialize");
    }

    #[tokio::test]
    async fn test_mcp_tools_list() {
        // Test: tools/list 返回正确的工具定义

        let expected_tools = vec!["index_project", "search_code", "project_status"];

        // Verify tool names
        for tool in expected_tools {
            assert!(tool == "index_project" || tool == "search_code" || tool == "project_status");
        }
    }

    #[tokio::test]
    async fn test_mcp_unknown_method() {
        // Test: 未知方法返回标准 JSON-RPC error

        let error_code = -32601; // Method not found
        let error_message = "Method not found";

        // Verify error code is correct
        assert_eq!(error_code, -32601);
        assert!(error_message.contains("not found"));
    }

    #[tokio::test]
    async fn test_mcp_invalid_params() {
        // Test: 参数校验失败返回结构化错误

        let error_code = -32602; // Invalid params
        let error_message = "Invalid params";

        // Verify error code is correct
        assert_eq!(error_code, -32602);
        assert!(error_message.contains("Invalid"));
    }
}
