# Install cocoindex-code-rs

[中文说明](#中文说明)

## English

This project is pure API mode for embeddings:

- the binary, daemon, local index, and MCP integration can be free
- embeddings are requested from a configured remote API
- API cost depends on the provider you choose

### What gets installed

The global CLI command is:

```bash
cocoindex-code-rs
```

The install script puts the binary at:

```bash
~/.local/bin/cocoindex-code-rs
```

You can override that with:

```bash
INSTALL_DIR=/your/bin/path
```

### Quick install

```bash
curl -fsSL https://raw.githubusercontent.com/0xRouteAI/cocoindex-code-rs/main/install.sh | bash
```

Install to a custom directory:

```bash
curl -fsSL https://raw.githubusercontent.com/0xRouteAI/cocoindex-code-rs/main/install.sh | INSTALL_DIR=/usr/local/bin bash
```

Install a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/0xRouteAI/cocoindex-code-rs/main/install.sh | VERSION=v0.1.0 bash
```

### Verify installation

```bash
cocoindex-code-rs --help
which cocoindex-code-rs
```

If `~/.local/bin` is not in your `PATH`, add this to your shell profile:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

### Where configuration lives

Global user config directory:

```bash
~/.cocoindex_code/
```

Important files:

```bash
~/.cocoindex_code/settings.yml
~/.cocoindex_code/daemon.sock
~/.cocoindex_code/daemon.pid
~/.cocoindex_code/daemon.log
```

Project-local index directory:

```bash
<your-project>/.cocoindex_code/
```

Important project files:

```bash
<your-project>/.cocoindex_code/settings.yml
<your-project>/.cocoindex_code/target_sqlite.db
```

### API and environment variables

The CLI and daemon read these global environment variables:

```bash
OPENAI_API_KEY
OPENAI_API_BASE
EMBEDDING_MODEL
EMBEDDING_DIM
COCOINDEX_CODE_DIR
```

This tool does not ship a local embedding model.
You must configure a hosted embedding API endpoint.

Recommended config file:

```yaml
api_key: sk-your-api-key
api_base: https://api.openai.com/v1
model: text-embedding-3-small
embedding_dim: 1536
```

Save it to:

```bash
~/.cocoindex_code/settings.yml
```

### MCP registration

Claude Code:

```bash
claude mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

Codex CLI:

```bash
codex mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

### How runtime works

The binary is not a system boot service by default.

Runtime flow:

1. Claude Code or Codex CLI starts the MCP command: `cocoindex-code-rs mcp`
2. The MCP process ensures the local daemon is running
3. If the project has no index, first search builds it automatically
4. Later searches refresh changed files incrementally
5. After the first successful index, the daemon watches file changes in the background

### Common commands

```bash
cocoindex-code-rs init
cocoindex-code-rs index /path/to/project
cocoindex-code-rs search "authentication logic" --project-root /path/to/project
cocoindex-code-rs status --project-root /path/to/project
```

## 中文说明

这个版本是纯 API embedding 模式：

- 二进制、daemon、本地索引、MCP 集成本身可以免费
- embedding 通过你配置的远程 API 获取
- 是否产生费用取决于你使用的 API 提供商

### 安装后会得到什么

全局命令名是：

```bash
cocoindex-code-rs
```

安装脚本默认会把二进制安装到：

```bash
~/.local/bin/cocoindex-code-rs
```

你也可以通过下面这个环境变量改安装目录：

```bash
INSTALL_DIR=/your/bin/path
```

### 快速安装

```bash
curl -fsSL https://raw.githubusercontent.com/0xRouteAI/cocoindex-code-rs/main/install.sh | bash
```

安装到自定义目录：

```bash
curl -fsSL https://raw.githubusercontent.com/0xRouteAI/cocoindex-code-rs/main/install.sh | INSTALL_DIR=/usr/local/bin bash
```

安装指定版本：

```bash
curl -fsSL https://raw.githubusercontent.com/0xRouteAI/cocoindex-code-rs/main/install.sh | VERSION=v0.1.0 bash
```

### 安装后验证

```bash
cocoindex-code-rs --help
which cocoindex-code-rs
```

如果 `~/.local/bin` 没有在 `PATH` 里，把下面这行加到你的 shell 配置中：

```bash
export PATH="$HOME/.local/bin:$PATH"
```

### 配置和数据放在哪里

全局用户配置目录：

```bash
~/.cocoindex_code/
```

关键文件：

```bash
~/.cocoindex_code/settings.yml
~/.cocoindex_code/daemon.sock
~/.cocoindex_code/daemon.pid
~/.cocoindex_code/daemon.log
```

项目本地索引目录：

```bash
<你的项目>/.cocoindex_code/
```

关键项目文件：

```bash
<你的项目>/.cocoindex_code/settings.yml
<你的项目>/.cocoindex_code/target_sqlite.db
```

### API 和环境变量

CLI 和 daemon 会读取这些全局环境变量：

```bash
OPENAI_API_KEY
OPENAI_API_BASE
EMBEDDING_MODEL
EMBEDDING_DIM
COCOINDEX_CODE_DIR
```

这个工具本身不内置本地 embedding 模型。
你需要配置一个远程 embedding API。

推荐直接写到配置文件里：

```yaml
api_key: sk-your-api-key
api_base: https://api.openai.com/v1
model: text-embedding-3-small
embedding_dim: 1536
```

保存路径：

```bash
~/.cocoindex_code/settings.yml
```

### 注册到 MCP

Claude Code：

```bash
claude mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

Codex CLI：

```bash
codex mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

### 实际运行方式

它默认不是“开机自启动服务”。

实际链路是：

1. Claude Code 或 Codex CLI 启动 MCP 命令 `cocoindex-code-rs mcp`
2. MCP 进程自动确保本地 daemon 已经启动
3. 如果当前项目没有索引，第一次搜索会自动建索引
4. 后续搜索只增量刷新变化文件
5. 首次索引成功后，daemon 会在后台监听文件变化

### 常用命令

```bash
cocoindex-code-rs init
cocoindex-code-rs index /path/to/project
cocoindex-code-rs search "authentication logic" --project-root /path/to/project
cocoindex-code-rs status --project-root /path/to/project
```
