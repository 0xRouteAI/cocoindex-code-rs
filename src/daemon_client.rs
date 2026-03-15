use crate::daemon::{daemon_log_path, daemon_pid_path, daemon_socket_path};
use crate::daemon_protocol::{Request, Response};
use crate::version::VERSION;
use anyhow::Context;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct DaemonClient {
    socket_path: std::path::PathBuf,
}

impl DaemonClient {
    pub fn connect() -> anyhow::Result<Self> {
        let socket_path = daemon_socket_path()?;
        if !socket_path.exists() {
            anyhow::bail!("Daemon socket not found: {}", socket_path.display());
        }
        Ok(Self { socket_path })
    }

    fn round_trip(&self, request: &Request) -> anyhow::Result<Response> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("Failed to connect to daemon at {}", self.socket_path.display()))?;
        let payload = serde_json::to_vec(request)?;
        stream.write_all(&payload)?;
        stream.write_all(b"\n")?;
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            anyhow::bail!("Daemon returned empty response");
        }
        let response: Response = serde_json::from_str(&line)?;
        Ok(response)
    }

    pub fn handshake(&self) -> anyhow::Result<Response> {
        self.round_trip(&Request::Handshake {
            version: VERSION.to_string(),
        })
    }

    pub fn request(&self, request: &Request) -> anyhow::Result<Response> {
        self.round_trip(request)
    }
}

pub fn start_daemon() -> anyhow::Result<()> {
    let current_exe = std::env::var("CARGO_BIN_EXE_cocoindex-code-rs")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::current_exe())?;
    let log_path = daemon_log_path()?;
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    Command::new(current_exe)
        .arg("run-daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file.try_clone()?))
        .stderr(Stdio::from(log_file))
        .spawn()
        .context("Failed to spawn daemon")?;

    wait_for_daemon(Duration::from_secs(10))
}

pub fn wait_for_daemon(timeout: Duration) -> anyhow::Result<()> {
    let socket_path = daemon_socket_path()?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if socket_path.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("Daemon did not start in time");
}

pub fn stop_daemon() -> anyhow::Result<()> {
    if let Ok(client) = DaemonClient::connect() {
        let _ = client.request(&Request::Stop);
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    let pid_path = daemon_pid_path()?;
    while Instant::now() < deadline && pid_path.exists() {
        std::thread::sleep(Duration::from_millis(100));
    }

    if pid_path.exists() {
        let _ = std::fs::remove_file(&pid_path);
    }
    if let Ok(socket_path) = daemon_socket_path() {
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }
    }
    Ok(())
}

pub fn ensure_daemon() -> anyhow::Result<DaemonClient> {
    if let Ok(client) = DaemonClient::connect() {
        match client.handshake()? {
            Response::Handshake { ok: true, .. } => return Ok(client),
            _ => {
                let _ = stop_daemon();
            }
        }
    }

    start_daemon()?;
    let client = DaemonClient::connect()?;
    match client.handshake()? {
        Response::Handshake { ok: true, .. } => Ok(client),
        Response::Handshake { daemon_version, .. } => {
            anyhow::bail!("Daemon version mismatch: {}", daemon_version)
        }
        response => anyhow::bail!("Unexpected handshake response: {:?}", response),
    }
}

pub fn daemon_socket_exists() -> anyhow::Result<bool> {
    Ok(Path::new(&daemon_socket_path()?).exists())
}
