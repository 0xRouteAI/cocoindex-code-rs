use clap::{Parser, Subcommand};
use coco_rs::{Provider, config::{Config, UserSettings, ProjectSettings}};
use coco_rs::daemon_client::{ensure_daemon, stop_daemon};
use coco_rs::daemon_protocol::{Request, Response};
use coco_rs::project::{default_path_filter, project_db_path, resolve_project_root};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

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
    /// Index a project directory
    Index {
        #[arg(value_name = "PATH", default_value = ".")]
        path: PathBuf,
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
        #[arg(long, default_value_t = false)]
        refresh: bool,
    },
    /// Start as MCP server
    Mcp {
        #[arg(long, value_name = "PATH")]
        project_root: Option<PathBuf>,
    },
    #[command(hide = true)]
    RunDaemon,
    DaemonStatus,
    StopDaemon,
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

fn prompt_with_default(label: &str, default: &str, secret: bool) -> anyhow::Result<String> {
    let mut stdout = io::stdout();
    if default.is_empty() {
        write!(stdout, "{}: ", label)?;
    } else {
        write!(stdout, "{} [{}]: ", label, default)?;
    }
    stdout.flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let value = input.trim().to_string();

    if !value.is_empty() {
        return Ok(value);
    }

    if !default.is_empty() {
        return Ok(default.to_string());
    }

    if secret {
        anyhow::bail!("{} cannot be empty", label);
    }

    Ok(String::new())
}

fn prompt_embedding_dim(default: usize) -> anyhow::Result<usize> {
    let mut stdout = io::stdout();
    loop {
        write!(stdout, "Embedding dimension [{}]: ", default)?;
        stdout.flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let value = input.trim();

        if value.is_empty() {
            return Ok(default);
        }

        if let Ok(parsed) = value.parse::<usize>() {
            return Ok(parsed);
        }

        writeln!(stdout, "Please enter a valid positive integer.")?;
    }
}

fn maybe_bootstrap_user_settings() -> anyhow::Result<UserSettings> {
    if let Ok(settings) = UserSettings::load() {
        return Ok(settings);
    }

    let defaults = UserSettings::default();

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Ok(defaults);
    }

    println!("No user settings found. Let's configure cocoindex-code-rs.");

    let settings = UserSettings {
        api_base: prompt_with_default("Embedding API base URL", &defaults.api_base, false)?,
        api_key: prompt_with_default("Embedding API key", &defaults.api_key, true)?,
        model: prompt_with_default("Embedding model", &defaults.model, false)?,
        embedding_dim: prompt_embedding_dim(defaults.embedding_dim)?,
        envs: defaults.envs,
    };

    settings.save()?;
    println!(
        "Saved user settings to {}",
        UserSettings::path()?.display()
    );

    Ok(settings)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    // Disable logging for MCP mode to not corrupt stdout
    if !matches!(cli.command, Some(Commands::Mcp { .. })) {
        tracing_subscriber::fmt::init();
    }

    let needs_interactive_settings = !matches!(
        cli.command,
        Some(Commands::Mcp { .. }) | Some(Commands::RunDaemon)
    );

    let user_settings = if needs_interactive_settings {
        maybe_bootstrap_user_settings()?
    } else {
        UserSettings::load_or_default()
    };

    // Build config from user settings and CLI args
    let config = Config {
        api_key: cli.api_key.unwrap_or(user_settings.api_key),
        api_base: cli.api_base.unwrap_or(user_settings.api_base),
        model: cli.model.unwrap_or(user_settings.model),
        embedding_dim: user_settings.embedding_dim,
        db_path: ".cocoindex_code/target_sqlite.db".to_string(),
    };

    match cli.command {
        Some(Commands::Init { path }) => {
            let settings = ProjectSettings::default();
            settings.save(&path)?;
            println!("Initialized project settings at {}/.cocoindex_code/settings.yml", path.display());

            // Also create user settings if they don't exist
            if !UserSettings::exists()? {
                let user_settings = maybe_bootstrap_user_settings()?;
                println!(
                    "Created user settings at {}",
                    UserSettings::path()?.display()
                );
                if user_settings.api_key.is_empty() {
                    println!("Please update the API key before indexing.");
                }
            }
        }
        Some(Commands::Status { project_root }) => {
            let root = resolve_project_root(project_root.as_deref())?;
            match ensure_daemon()?.request(&Request::ProjectStatus {
                project_root: root.display().to_string(),
            })? {
                Response::ProjectStatus { indexing, total_chunks, total_files, languages } => {
                    println!("Project: {}", root.display());
                    println!("Database: {}", project_db_path(&root).display());
                    println!("Indexing: {}", indexing);
                    println!("Chunks: {}", total_chunks);
                    println!("Files: {}", total_files);
                    println!("Languages: {}", serde_json::to_string(&languages)?);
                }
                Response::Error { message } => println!("{}", message),
                _ => println!("Unexpected daemon response"),
            }
        }
        Some(Commands::Index { path }) => {
            let abs_path = resolve_project_root(Some(path.as_path()))?;
            match ensure_daemon()?.request(&Request::Index {
                project_root: abs_path.display().to_string(),
                refresh: false,
            })? {
                Response::Index { success: true, message } => {
                    println!("{}", message.unwrap_or_else(|| "Indexing complete.".to_string()));
                }
                Response::Error { message } => println!("{}", message),
                _ => println!("Unexpected daemon response"),
            }
        }
        Some(Commands::Search { query, limit, offset, languages, paths, project_root, refresh }) => {
            let root = resolve_project_root(project_root.as_deref())?;
            let effective_paths = if paths.is_some() {
                paths
            } else {
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| default_path_filter(&root, &cwd))
                    .map(|path| vec![path])
            };
            match ensure_daemon()?.request(&Request::Search {
                project_root: root.display().to_string(),
                query,
                languages,
                paths: effective_paths,
                limit,
                offset,
                refresh,
            })? {
                Response::Search { results, .. } => {
                    for (i, result) in results.iter().enumerate() {
                        println!("{}. {} (Lines: {}-{}, Score: {:.4})",
                            i + 1, result.file_path, result.start_line, result.end_line, result.score);
                        println!("---\n{}\n---", result.content);
                    }
                }
                Response::Error { message } => println!("{}", message),
                _ => println!("Unexpected daemon response"),
            }
        }
        Some(Commands::Mcp { project_root }) => {
            let root = resolve_project_root(project_root.as_deref())?;
            let client = ensure_daemon()?;
            coco_rs::mcp::run(client, root).await?;
        }
        Some(Commands::RunDaemon) => {
            let provider = Provider::new(&config);
            coco_rs::daemon::run(config, provider).await?;
        }
        Some(Commands::DaemonStatus) => {
            match ensure_daemon()?.request(&Request::DaemonStatus)? {
                Response::DaemonStatus { version, projects } => {
                    println!("Version: {}", version);
                    for project in projects {
                        println!(
                            "{} [{}]",
                            project.project_root,
                            if project.indexing { "indexing" } else { "idle" }
                        );
                    }
                }
                Response::Error { message } => println!("{}", message),
                _ => println!("Unexpected daemon response"),
            }
        }
        Some(Commands::StopDaemon) => {
            stop_daemon()?;
            println!("Daemon stopped.");
        }
        None => {
            println!("No command provided. Use --help for usage.");
        }
    }

    Ok(())
}
