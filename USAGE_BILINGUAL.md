# cocoindex-code-rs Usage / 使用说明

## English

### What is this

`cocoindex-code-rs` is a local MCP server and CLI for semantic code search.

It is designed for:

- Claude Code
- Codex CLI
- other MCP clients

### Install and use

After installing the binary globally, you can run:

```bash
cocoindex-code-rs --help
```

Register it with your agent:

```bash
claude mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
codex mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

### Automatic behavior

After registration:

- the MCP server starts automatically when the client uses it
- the local daemon starts automatically if needed
- if the current project has no index, the first search builds it
- later searches update only changed files
- after the first successful index, the daemon watches file changes in the background

### Common commands

```bash
cocoindex-code-rs init
cocoindex-code-rs index /path/to/project
cocoindex-code-rs search "user authentication" --project-root /path/to/project
cocoindex-code-rs status --project-root /path/to/project
```

### Settings

User config file:

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

## 中文

### 这是什么

`cocoindex-code-rs` 是一个本地运行的 MCP 代码语义搜索服务和 CLI。

主要给这些客户端使用：

- Claude Code
- Codex CLI
- 其他支持 MCP 的客户端

### 安装后怎么用

当二进制已经全局安装并进入 `PATH` 后，可以直接运行：

```bash
cocoindex-code-rs --help
```

注册到你的 agent：

```bash
claude mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
codex mcp add cocoindex-code-rs -- cocoindex-code-rs mcp
```

### 自动化行为

注册完成后：

- 当客户端需要使用它时，会自动拉起 MCP 服务
- 如果本地 daemon 没启动，会自动启动
- 如果当前项目还没有索引，第一次搜索会自动建索引
- 后续搜索只会增量更新变化过的文件
- 首次索引成功后，daemon 会在后台监听文件变化并自动更新索引

### 常用命令

```bash
cocoindex-code-rs init
cocoindex-code-rs index /path/to/project
cocoindex-code-rs search "user authentication" --project-root /path/to/project
cocoindex-code-rs status --project-root /path/to/project
```

### 配置文件

用户配置文件位置：

```bash
~/.cocoindex_code/settings.yml
```

示例：

```yaml
api_key: sk-your-api-key
api_base: https://api.openai.com/v1
model: text-embedding-3-small
embedding_dim: 1536
```

### 适合什么时候用

适合：

- 不知道精确函数名，只知道功能描述
- 想按语义找代码
- 想找相似实现
- 想快速理解陌生代码库

不适合完全替代：

- `rg` 精确文本搜索
- 符号引用追踪
- 已知函数名/类名时的精确定位
