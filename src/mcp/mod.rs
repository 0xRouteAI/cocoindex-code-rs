use crate::daemon_client::DaemonClient;
use crate::daemon_protocol::{Request as DaemonRequest, Response as DaemonResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::task;

#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Option<serde_json::Value>,
    id: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
    id: Option<serde_json::Value>,
}

pub async fn run(client: DaemonClient, default_project_root: PathBuf) -> anyhow::Result<()> {
    let warmup_client = client.clone();
    let warmup_project_root = default_project_root.clone();
    task::spawn_blocking(move || {
        let _ = warmup_client.request(&DaemonRequest::Index {
            project_root: warmup_project_root.display().to_string(),
            refresh: false,
        });
    });

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut reader = BufReader::new(stdin);

    loop {
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await? == 0 {
                return Ok(());
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            headers.push_str(&line);
        }

        let content_length = headers
            .lines()
            .find(|line| line.to_lowercase().starts_with("content-length:"))
            .and_then(|line| line.split(':').nth(1))
            .and_then(|value| value.trim().parse::<usize>().ok());

        let Some(content_length) = content_length else {
            continue;
        };

        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body).await?;

        let request: JsonRpcRequest = match serde_json::from_slice(&body) {
            Ok(request) => request,
            Err(error) => {
                write_response(
                    &mut stdout,
                    &JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: None,
                        error: Some(json!({
                            "code": -32700,
                            "message": format!("Parse error: {}", error),
                        })),
                        id: None,
                    },
                )
                .await?;
                continue;
            }
        };

        let response = handle_request(request, &client, &default_project_root);
        write_response(&mut stdout, &response).await?;
    }
}

async fn write_response(stdout: &mut io::Stdout, response: &JsonRpcResponse) -> anyhow::Result<()> {
    let json = serde_json::to_string(response)?;
    stdout
        .write_all(format!("Content-Length: {}\r\n\r\n", json.len()).as_bytes())
        .await?;
    stdout.write_all(json.as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}

fn tool_error(message: impl Into<String>) -> serde_json::Value {
    json!({
        "isError": true,
        "content": [{
            "type": "text",
            "text": message.into()
        }]
    })
}

fn tool_text(message: impl Into<String>) -> serde_json::Value {
    json!({
        "content": [{
            "type": "text",
            "text": message.into()
        }]
    })
}

fn handle_request(
    req: JsonRpcRequest,
    client: &DaemonClient,
    default_project_root: &std::path::Path,
) -> JsonRpcResponse {
    let result = match req.method.as_str() {
        "initialize" => Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {
                    "listChanged": false
                }
            },
            "serverInfo": {
                "name": "cocoindex-rs",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        "initialized" => Some(json!({})),
        "shutdown" => Some(json!(null)),
        "tools/list" => Some(json!({
            "tools": [
                {
                    "name": "index_project",
                    "description": "Index a project directory for code search. Supports incremental updates based on file hashes.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to the project directory to index" },
                            "refresh_index": { "type": "boolean", "default": false }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "search_code",
                    "description": "Search code snippets using semantic similarity. Automatically performs incremental indexing before search by default.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "project_root": { "type": "string" },
                            "limit": { "type": "integer", "default": 10, "minimum": 1, "maximum": 100 },
                            "offset": { "type": "integer", "default": 0, "minimum": 0 },
                            "refresh_index": { "type": "boolean", "default": true },
                            "languages": { "type": "array", "items": { "type": "string" } },
                            "paths": { "type": "array", "items": { "type": "string" } }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "project_status",
                    "description": "Return indexing statistics for a project.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "project_root": { "type": "string" }
                        }
                    }
                }
            ]
        })),
        "tools/call" => {
            let Some(params) = req.params.as_ref() else {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: None,
                    error: Some(json!({ "code": -32602, "message": "Invalid params: missing params" })),
                    id: req.id,
                };
            };
            let Some(name) = params.get("name").and_then(|value| value.as_str()) else {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: None,
                    error: Some(json!({ "code": -32602, "message": "Invalid params: missing tool name" })),
                    id: req.id,
                };
            };
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

            match name {
                "index_project" => {
                    let project_root = args
                        .get("path")
                        .and_then(|value| value.as_str())
                        .map(PathBuf::from)
                        .unwrap_or_else(|| default_project_root.to_path_buf());
                    let refresh = args
                        .get("refresh_index")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);

                    match client.request(&DaemonRequest::Index {
                        project_root: project_root.display().to_string(),
                        refresh,
                    }) {
                        Ok(DaemonResponse::Index { success: true, message }) => {
                            Some(tool_text(message.unwrap_or_else(|| format!("Indexed {}", project_root.display()))))
                        }
                        Ok(DaemonResponse::Error { message }) => Some(tool_error(message)),
                        Ok(other) => Some(tool_error(format!("Unexpected daemon response: {:?}", other))),
                        Err(error) => Some(tool_error(format!("Index error: {}", error))),
                    }
                }
                "search_code" => {
                    let query = args.get("query").and_then(|value| value.as_str()).unwrap_or_default();
                    if query.is_empty() {
                        Some(tool_error("Error: Query cannot be empty"))
                    } else {
                        let limit = args.get("limit").and_then(|value| value.as_u64()).unwrap_or(10) as usize;
                        let offset = args.get("offset").and_then(|value| value.as_u64()).unwrap_or(0) as usize;
                        let refresh = args
                            .get("refresh_index")
                            .and_then(|value| value.as_bool())
                            .unwrap_or(true);
                        let languages = args.get("languages").and_then(|value| value.as_array()).map(|items| {
                            items.iter().filter_map(|item| item.as_str().map(String::from)).collect::<Vec<_>>()
                        });
                        let paths = args.get("paths").and_then(|value| value.as_array()).map(|items| {
                            items.iter().filter_map(|item| item.as_str().map(String::from)).collect::<Vec<_>>()
                        });
                        let project_root = args
                            .get("project_root")
                            .and_then(|value| value.as_str())
                            .map(PathBuf::from)
                            .unwrap_or_else(|| default_project_root.to_path_buf());

                        match client.request(&DaemonRequest::Search {
                            project_root: project_root.display().to_string(),
                            query: query.to_string(),
                            languages,
                            paths,
                            limit,
                            offset,
                            refresh,
                        }) {
                            Ok(DaemonResponse::Search { results, .. }) => {
                                if results.is_empty() {
                                    Some(tool_text("No results found. Try a different query or check if the project is indexed."))
                                } else {
                                    let mut output = format!("Found {} result(s):\n\n", results.len());
                                    for (index, result) in results.iter().enumerate() {
                                        output.push_str(&format!(
                                            "{}. {} (Lines {}-{}, Score: {:.3})\n",
                                            index + 1 + offset,
                                            result.file_path,
                                            result.start_line,
                                            result.end_line,
                                            result.score,
                                        ));
                                        if let Some(language) = &result.language {
                                            output.push_str(&format!("   Language: {}\n", language));
                                        }
                                        output.push_str(&format!("```\n{}\n```\n\n", result.content));
                                    }
                                    Some(tool_text(output))
                                }
                            }
                            Ok(DaemonResponse::Error { message }) => Some(tool_error(message)),
                            Ok(other) => Some(tool_error(format!("Unexpected daemon response: {:?}", other))),
                            Err(error) => Some(tool_error(format!("Search error: {}", error))),
                        }
                    }
                }
                "project_status" => {
                    let project_root = args
                        .get("project_root")
                        .and_then(|value| value.as_str())
                        .map(PathBuf::from)
                        .unwrap_or_else(|| default_project_root.to_path_buf());
                    match client.request(&DaemonRequest::ProjectStatus {
                        project_root: project_root.display().to_string(),
                    }) {
                        Ok(DaemonResponse::ProjectStatus { indexing, total_chunks, total_files, languages }) => {
                            Some(tool_text(format!(
                                "Project: {}\nIndexing: {}\nIndexed files: {}\nIndexed chunks: {}\nLanguages: {}",
                                project_root.display(),
                                indexing,
                                total_files,
                                total_chunks,
                                serde_json::to_string(&languages).unwrap_or_else(|_| "{}".to_string()),
                            )))
                        }
                        Ok(DaemonResponse::Error { message }) => Some(tool_error(message)),
                        Ok(other) => Some(tool_error(format!("Unexpected daemon response: {:?}", other))),
                        Err(error) => Some(tool_error(format!("Status error: {}", error))),
                    }
                }
                _ => {
                    return JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: None,
                        error: Some(json!({
                            "code": -32601,
                            "message": format!("Unknown tool: {}", name)
                        })),
                        id: req.id,
                    }
                }
            }
        }
        _ => {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(json!({
                    "code": -32601,
                    "message": format!("Method not found: {}", req.method)
                })),
                id: req.id,
            }
        }
    };

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        result,
        error: None,
        id: req.id,
    }
}
