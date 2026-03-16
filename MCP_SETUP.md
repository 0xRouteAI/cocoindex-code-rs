# MCP Setup for `cocoindex-code-rs`

This document describes how to register `cocoindex-code-rs` as a local MCP server.

## Prerequisite

Make sure the binary is installed and globally available:

```bash
cocoindex-code-rs --help
```

If you built from source:

```bash
cd cocoindex-rs
cargo build --release
./target/release/cocoindex-code-rs --help
```

## Claude Code

Register:

```bash
claude mcp add cocoindex-code-rs -- cocoindex-code-rs
```

## Codex CLI

Register:

```bash
codex mcp add cocoindex-code-rs -- cocoindex-code-rs
```

## Optional Environment

If needed, register with environment variables:

```bash
claude mcp add cocoindex-code-rs \
  -e OPENAI_API_KEY=your-key \
  -e OPENAI_API_BASE=https://api.openai.com/v1 \
  -e EMBEDDING_MODEL=text-embedding-3-small \
  -e EMBEDDING_DIM=1536 \
  -- cocoindex-code-rs
```

Equivalent settings can also be stored in:

```bash
~/.cocoindex_code/settings.yml
```

Example:

```yaml
api_key: sk-your-api-key
api_base: https://api.openai.com/v1
model: text-embedding-3-small
embedding_dim: 1536
```

## Runtime Model

When the MCP server is used:

1. the MCP process starts as a single local stdio server
2. if the project has no index, first use builds it automatically
3. later updates refresh only changed files
4. each project keeps its own `.cocoindex_code/target_sqlite.db`
5. during the active MCP session, accessed projects are watched in the background

## Available MCP Tools

### `search_code`

Semantic code search.

Arguments:

- `query`
- `project_root` optional
- `limit` optional
- `offset` optional
- `refresh_index` optional, default `false`
- `languages` optional
- `paths` optional

### `index_project`

Index a project manually.

Arguments:

- `path`
- `refresh_index` optional

### `project_status`

Show project indexing stats.

Arguments:

- `project_root` optional

## Recommended Agent Guidance

Use semantic search selectively:

- use `rg` and symbol lookup first when exact names are known
- use `search_code` when the user describes behavior or intent
- use `search_code` to find similar implementations or unfamiliar code areas

## Troubleshooting

If the command is not found:

```bash
which cocoindex-code-rs
```

If indexing does not update:

- verify the binary is executable
- check that the project contains a writable `.cocoindex_code/` directory
- use `index_project` or `cocoindex-code-rs index /path/to/project` to force a rebuild

If search is slow:

- avoid unnecessary path filters
- use a fast embedding provider
- let the initial index complete once before heavy usage
