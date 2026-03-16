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
claude mcp add cocoindex-code-rs -- cocoindex-code-rs
```

Codex CLI:

```bash
codex mcp add cocoindex-code-rs -- cocoindex-code-rs
```

After registration:

- Claude Code or Codex CLI starts a single local MCP process when needed
- the MCP process keeps a project-local index under `.cocoindex_code/`
- first use auto-builds the project index if missing
- later updates are incremental and only changed files are re-indexed
- while the MCP session stays alive, the server watches accessed projects in the background

## Agent Prompt

Add this to your project's `AGENTS.md`:

```md
Use the `cocoindex-code-rs` MCP server for semantic code search when:
- searching by behavior or meaning instead of exact text
- exploring unfamiliar parts of the codebase
- looking for similar implementations
- grep or filename search is noisy or inconclusive

Prefer normal text search first when exact names, symbols, routes, config keys, or error strings are known.

When using semantic search:
- use it to find candidate files and code chunks
- then verify results with local code reading or text search
- avoid repeated semantic searches when one search already identified the area
```

## What It Does

- Exposes an MCP server over stdio with:
  - `search_code`
  - `index_project`
  - `project_status`
- Builds a local SQLite + `sqlite-vec` index per project
- Stores project data under each project's `.cocoindex_code/`
- Uses syntax-aware chunking with tree-sitter when available
- Auto-indexes on first use
- Performs incremental indexing on later searches
- Watches accessed project files in the background during the active MCP session

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
- the MCP server is single-process and does not require a separate local daemon
- each project keeps its own local index database

## Benchmarks

Real-world measurements on this repository with a hosted embedding API:

- Whole monorepo, persistent MCP process, 100 natural-language queries:
  - `Top1`: 39%
  - `Top5`: 62%
  - median latency: ~322 ms
- Rust subproject only (`cocoindex-rs`), persistent MCP process, 100 queries:
  - `Top1`: 59%
  - `Top5`: 89%
  - median latency: ~349 ms

Practical guidance:

- best results come from searching within the current project or language scope
- whole-monorepo semantic search is usable, but precision is lower across mixed languages and docs
- persistent MCP sessions are much faster than repeated CLI cold starts

## More Docs

- [INSTALL.md](./INSTALL.md)
- [MCP_SETUP.md](./MCP_SETUP.md)
- [USAGE_BILINGUAL.md](./USAGE_BILINGUAL.md)
