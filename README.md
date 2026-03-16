# cocoindex-code-rs

Rust implementation of a local MCP code search server for Claude Code, Codex CLI, and other MCP clients.

This version is pure API mode for embeddings:

- it does not bundle a local embedding model
- it expects a hosted embedding API configured by the user
- the tool/runtime can be free, but embedding API costs depend on the provider

## Install

Install the latest release binary with one command:

```bash
curl -fsSL https://raw.githubusercontent.com/0xRouteAI/cocoindex-code-rs/main/install.sh | bash
```

Prebuilt binaries currently support Linux and macOS only.
Windows native binaries are not supported yet.
On Windows, use WSL and run the Linux install command inside WSL.

This installs the prebuilt release binary to:

```bash
~/.local/bin/cocoindex-code-rs
```

The installed global CLI command is:

```bash
cocoindex-code-rs
```

## Register MCP

Claude Code:

```bash
claude mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

Codex CLI:

```bash
codex mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

After registration:

- Claude Code or Codex CLI starts `cocoindex-code-rs mcp` when needed
- the MCP process ensures the local daemon is running
- first search auto-builds the project index if missing
- later searches auto-refresh changed files incrementally
- after the first successful index, the daemon watches file changes in the background

## Agent Prompt

Add this to your project's `AGENTS.md`:

```md
Use the `cocoindex-code-rs` MCP server automatically for semantic code search when:
- the user asks by behavior, intent, or meaning rather than exact text
- the codebase area is unfamiliar
- similar implementations or related patterns are needed
- grep, filename search, or symbol lookup is noisy or inconclusive

Prefer normal text search first when exact names, symbols, routes, config keys, or error strings are known.

When using `cocoindex-code-rs`:
- use it to identify candidate files and code chunks
- then verify results by reading files or using local text search
- avoid repeated semantic searches if one search already narrowed the area
```

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

This project does not run local embedding inference by itself.

## Build From Source

```bash
cargo build --release
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

## Notes

- path filtering currently falls back to a full SQL distance scan
- language filtering can use vec partition-aware search
- the daemon is local-only and is started automatically by the CLI/MCP flow

## More Docs

- [INSTALL.md](./INSTALL.md)
- [MCP_SETUP.md](./MCP_SETUP.md)
- [USAGE_BILINGUAL.md](./USAGE_BILINGUAL.md)
