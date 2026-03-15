# cocoindex-code-rs

Rust implementation of a local MCP code search server for Claude Code, Codex CLI, and other MCP clients.

This version is API-only for embeddings:

- it does not bundle a local embedding model
- it expects an embedding API provider configured by the user
- tool/runtime is free, but embedding API costs depend on the provider

## What It Does

- Exposes an MCP server over stdio with:
  - `search_code`
  - `index_project`
  - `project_status`
- Builds a local SQLite + `sqlite-vec` index per project
- Uses syntax-aware chunking with tree-sitter when available
- Auto-indexes on first use
- Performs incremental indexing on later searches
- Starts a local daemon automatically
- Watches project files in the background after the first successful index

## Binary Name

The installed CLI command is:

```bash
cocoindex-code-rs
```

After installation, it should be available globally from `PATH`.

## Build

```bash
cd cocoindex-rs
cargo build --release
```

Binary output:

```bash
./target/release/cocoindex-code-rs
```

## Basic CLI

```bash
cocoindex-code-rs init
cocoindex-code-rs index /path/to/project
cocoindex-code-rs search "authentication logic" --project-root /path/to/project
cocoindex-code-rs status --project-root /path/to/project
cocoindex-code-rs mcp --project-root /path/to/project
```

## Settings

User settings file:

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

Environment variable equivalents:

```bash
OPENAI_API_KEY
OPENAI_API_BASE
EMBEDDING_MODEL
EMBEDDING_DIM
COCOINDEX_CODE_DIR
```

This project is pure API mode for embeddings. Configure one of:

- OpenAI-compatible embedding API
- other compatible hosted embedding API endpoints

It does not run local embedding inference by itself.

## MCP Registration

If `cocoindex-code-rs` is installed globally, register it like this:

### Claude Code

```bash
claude mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

### Codex CLI

```bash
codex mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

After registration:

- opening Claude Code or Codex CLI makes the MCP available
- first search auto-builds the project index if missing
- later searches auto-refresh changed files incrementally

## Behavior

Search behavior is designed for automatic use by coding agents:

- no index yet: build it automatically
- existing index: refresh changed files incrementally
- after first successful indexing: daemon starts background file watching for that project

## Notes

- path filtering currently falls back to a full SQL distance scan
- language filtering can use vec partition-aware search
- the daemon is local-only and is started automatically by the CLI/MCP flow

## More Docs

- [INSTALL.md](./INSTALL.md)
- [MCP_SETUP.md](./MCP_SETUP.md)
- [USAGE_BILINGUAL.md](./USAGE_BILINGUAL.md)
