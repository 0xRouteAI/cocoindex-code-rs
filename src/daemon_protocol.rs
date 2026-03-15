use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Handshake { version: String },
    Index { project_root: String, refresh: bool },
    Search {
        project_root: String,
        query: String,
        languages: Option<Vec<String>>,
        paths: Option<Vec<String>>,
        limit: usize,
        offset: usize,
        refresh: bool,
    },
    ProjectStatus { project_root: String },
    DaemonStatus,
    Stop,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResultPayload {
    pub file_path: String,
    pub language: Option<String>,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub score: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonProjectInfo {
    pub project_root: String,
    pub indexing: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Handshake { ok: bool, daemon_version: String },
    Index { success: bool, message: Option<String> },
    Search {
        success: bool,
        results: Vec<SearchResultPayload>,
        total_returned: usize,
        offset: usize,
        message: Option<String>,
    },
    ProjectStatus {
        indexing: bool,
        total_chunks: usize,
        total_files: usize,
        languages: HashMap<String, usize>,
    },
    DaemonStatus {
        version: String,
        projects: Vec<DaemonProjectInfo>,
    },
    Stop { ok: bool },
    Error { message: String },
}
