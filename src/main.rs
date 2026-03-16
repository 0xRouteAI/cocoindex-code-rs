use clap::{Parser, Subcommand};
use coco_rs::config::{Config, ProjectSettings, UserSettings};
use coco_rs::project::{default_path_filter, ensure_project_cache_layout, project_db_path, resolve_project_root};
use coco_rs::service::ProjectService;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "cocoindex-code-rs")]
#[command(about = "CocoIndex-Code Rust implementation", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(short = 'k', long, env = "OPENAI_API_KEY")]
    api_key: Option<String>,

    #[arg(short = 'b', long, env = "OPENAI_API_BASE")]
    api_base: Option<String>,

    #[arg(short = 'm', long, env = "EMBEDDING_MODEL")]
    model: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start as MCP server
    Serve {
        #[arg(long, value_name = "PATH")]
        project_root: Option<PathBuf>,
    },
    /// Start as MCP server
    Mcp {
        #[arg(long, value_name = "PATH")]
        project_root: Option<PathBuf>,
    },
    /// Index a project directory
    Index {
        #[arg(value_name = "PATH", default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value_t = false)]
        refresh: bool,
    },
    /// Search code in the index
    Search {
        #[arg(value_name = "QUERY")]
        query: String,
        #[arg(long, default_value = "5")]
        limit: usize,
        #[arg(long, default_value = "0")]
        offset: usize,
        #[arg(long)]
        languages: Option<Vec<String>>,
        #[arg(long)]
        paths: Option<Vec<String>>,
        #[arg(long, value_name = "PATH")]
        project_root: Option<PathBuf>,
        #[arg(long, default_value_t = true)]
        refresh: bool,
    },
    /// Initialize project settings
    Init {
        #[arg(value_name = "PATH", default_value = ".")]
        path: PathBuf,
    },
    /// Show project status
    Status {
        #[arg(long, value_name = "PATH")]
        project_root: Option<PathBuf>,
    },
}

fn build_config(cli: &Cli, user_settings: &UserSettings, project_root: &Path) -> Config {
    Config {
        api_key: cli
            .api_key
            .clone()
            .unwrap_or_else(|| user_settings.api_key.clone()),
        api_base: cli
            .api_base
            .clone()
            .unwrap_or_else(|| user_settings.api_base.clone()),
        model: cli
            .model
            .clone()
            .unwrap_or_else(|| user_settings.model.clone()),
        embedding_dim: user_settings.embedding_dim,
        db_path: project_db_path(project_root).to_string_lossy().to_string(),
    }
}

async fn open_service(
    cli: &Cli,
    user_settings: &UserSettings,
    project_root: PathBuf,
) -> anyhow::Result<ProjectService> {
    ensure_project_cache_layout(&project_root)?;
    let config = build_config(cli, user_settings, &project_root);
    ProjectService::open(project_root, config).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    if !matches!(
        cli.command,
        None | Some(Commands::Serve { .. }) | Some(Commands::Mcp { .. })
    ) {
        tracing_subscriber::fmt::init();
    }

    let user_settings = UserSettings::load_or_default();

    match &cli.command {
        Some(Commands::Init { path }) => {
            let settings = ProjectSettings::default();
            settings.save(path)?;
            println!(
                "Initialized project settings at {}/.cocoindex_code/settings.yml",
                path.display()
            );

            if UserSettings::load().is_err() {
                let user_settings = UserSettings::default();
                user_settings.save()?;
                println!("Created user settings at ~/.cocoindex_code/settings.yml");
                println!("Please update with your API key.");
            }
        }
        Some(Commands::Status { project_root }) => {
            let root = resolve_project_root(project_root.as_deref())?;
            ensure_project_cache_layout(&root)?;
            let db_path = project_db_path(&root);
            if !db_path.exists() {
                println!("Project: {}", root.display());
                println!("Database: {}", db_path.display());
                println!("Indexing: false");
                println!("Chunks: 0");
                println!("Files: 0");
                println!("Languages: {{}}");
                return Ok(());
            }
            let service = open_service(&cli, &user_settings, root.clone()).await?;
            let status = service.stats().await?;
            println!("Project: {}", root.display());
            println!("Database: {}", status.db_path.display());
            println!("Indexing: {}", status.indexing);
            println!("Chunks: {}", status.stats.total_chunks);
            println!("Files: {}", status.stats.total_files);
            println!("Languages: {}", serde_json::to_string(&status.stats.languages)?);
        }
        Some(Commands::Index { path, refresh }) => {
            let root = resolve_project_root(Some(path.as_path()))?;
            let service = open_service(&cli, &user_settings, root).await?;
            let result = service.index(*refresh).await?;
            println!("{}", result.message);
        }
        Some(Commands::Search {
            query,
            limit,
            offset,
            languages,
            paths,
            project_root,
            refresh,
        }) => {
            let root = resolve_project_root(project_root.as_deref())?;
            let effective_paths = if paths.is_some() {
                paths.clone()
            } else {
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| default_path_filter(&root, &cwd))
                    .map(|path| vec![path])
            };
            let service = open_service(&cli, &user_settings, root).await?;
            let results = service
                .search(
                    query,
                    *limit,
                    *offset,
                    languages.clone(),
                    effective_paths,
                    *refresh,
                )
                .await?;

            for (i, result) in results.iter().enumerate() {
                println!(
                    "{}. {} (Lines: {}-{}, Score: {:.4})",
                    i + 1,
                    result.file_path,
                    result.start_line,
                    result.end_line,
                    result.score
                );
                println!("---\n{}\n---", result.content);
            }
        }
        Some(Commands::Serve { project_root }) | Some(Commands::Mcp { project_root }) => {
            let root = resolve_project_root(project_root.as_deref())?;
            let service = open_service(&cli, &user_settings, root.clone()).await?;
            coco_rs::mcp::run(service).await?;
        }
        None => {
            let root = resolve_project_root(None)?;
            let service = open_service(&cli, &user_settings, root.clone()).await?;
            coco_rs::mcp::run(service).await?;
        }
    }

    Ok(())
}
