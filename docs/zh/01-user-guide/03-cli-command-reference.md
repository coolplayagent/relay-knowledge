# 第 3 章 CLI 命令参考

[中文](../../zh/01-user-guide/03-cli-command-reference.md) | [English](../../en/01-user-guide/03-cli-command-reference.md)

本章提供可执行命令索引。工作流说明分散在后续章节；本章用于快速找到入口和诊断命令。

## 3.1 常用状态命令

项目状态:

```bash
relay-knowledge status --format json
```

健康检查:

```bash
relay-knowledge health --format json
```

服务诊断:

```bash
relay-knowledge service status --format json
relay-knowledge service doctor --format json
```

`service status` 和 `service doctor` 当前复用统一 API 输出，报告 service mode、后台更新状态、service definition path、agent protocol status 和 refresh queue diagnostics。

版本检查:

```bash
relay-knowledge version
relay-knowledge version check --format json
```

`version` 只打印当前二进制版本，不加载 runtime configuration，也不联网。`version check`
通过 `net::http` 按配置查询 GitHub Releases 和 crates.io，结果缓存到 runtime cache
目录；普通交互式 text/markdown CLI 命令只会在发现稳定新版时向 stderr 输出短提示，且会先输出主命令
stdout，不会自动替换二进制。

## 3.2 Provider 诊断

```bash
relay-knowledge provider probe --format json
```

`provider probe` 读取环境边界解析出的 remote embedding provider 配置，并执行一次轻量探测。JSON 响应包含 `ok`、`provider`、`model`、`dimension`、可选 `latency_ms`，失败时还包含 `error_code`、`error_message` 和 `retryable`。HTTP 429、HTTP 402 以及带 quota/backpressure 诊断的 HTTP 400 或 HTTP 403 响应表示 endpoint、认证边界和模型路由已经可达，因此 `ok=true`，同时保留 `error_code=rate_limited` 与 `retryable=true` 作为可观测降级诊断；普通认证、endpoint、model、timeout 和 malformed-response 失败仍返回 `ok=false`。它不会输出 API key 原文，也不会绕过 `env` 模块直接读取环境变量。

OpenAI-compatible embedding base URL 可以配置为 host root、版本化 API root（如 `/v1`、`/v4`）或完整 `/embeddings` endpoint；非版本路径前缀继续按 `<prefix>/v1/embeddings` 解析，query 或 fragment 后缀不参与 endpoint 构造。

endpoint host、batch、timeout、并发和 cursor metadata 属于 `status`、`health` 或 Web Providers 面板的运行时诊断。

## 3.3 Setup 诊断与配置画像

`setup doctor` 是 storage-free 的只读诊断命令:

```bash
relay-knowledge setup doctor --format json
```

它只读取已解析 runtime configuration，不打开或迁移 SQLite，也不刷新索引。`configuration_ready=true` 只表示配置检查通过；`live_health_checked=false` 表示 graph storage、index freshness 和 worker/service live health 仍需通过 `health` 或 `service doctor` 检查。

`setup profile` 不写文件、不安装服务，只输出推荐环境变量、命令和注意事项:

```bash
relay-knowledge setup profile local --format json
relay-knowledge setup profile agent-readonly --format json
relay-knowledge setup profile service --format json
relay-knowledge setup profile external-embedding --format json
```

这些 profile 分别覆盖零配置本地循环、只读 MCP agent 接入、平台 service manager 预览和外部 embedding provider metadata。需要把建议固化到 shell、service manager 或部署工具时，由调用方显式写入自己的配置面。

## 3.4 命令总览

```bash
relay-knowledge status
relay-knowledge help [command...] [--format text|json]
relay-knowledge ingest --source <scope> --content <text> [--entity <label>]
relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness allow-stale|wait-until-fresh|graph-only]
relay-knowledge repo register <path> --alias <name> [--path <filter>] [--language <id>]
relay-knowledge repo index <alias> [--ref <ref>] [--dry-run]
relay-knowledge repo scope preview <alias> [--ref <ref>]
relay-knowledge repo update <alias> --base <ref> --head <ref>
relay-knowledge repo query <alias> --query <text> [--kind hybrid|symbol|definition|references|callers|callees|imports]
relay-knowledge repo impact <alias> --base <ref> --head <ref>
relay-knowledge repo report <alias> [--format markdown|json]
relay-knowledge repo status <alias>
relay-knowledge graph inspect
relay-knowledge index refresh [--kind bm25|semantic|vector]
relay-knowledge worker status|run-once [--kind embedding|ocr|vision|extractor]
relay-knowledge proposal list [--state proposed|accepted|rejected|superseded] [--limit <n>]
relay-knowledge proposal show <proposal-id>
relay-knowledge proposal accept|reject|supersede <proposal-id> --by <actor> [--reason <text>]
relay-knowledge audit query [--operation <name>] [--limit <n>]
relay-knowledge provider probe
relay-knowledge health
relay-knowledge service status
relay-knowledge service doctor
relay-knowledge service plan install|uninstall
relay-knowledge service definition write
relay-knowledge service operator status|pause|resume
relay-knowledge service run [--web] [--mcp streamable-http]
relay-knowledge setup doctor
relay-knowledge setup profile local|agent-readonly|service|external-embedding
relay-knowledge version
relay-knowledge version check
```

冷启动 full `repo index` 会立即返回持久化任务 handle，并由 CLI 进程启动有界后台 worker。`service run` 会消费同一个 code-index 队列，用于已安装服务或前台服务模式。cold repository index 运行中可用 `repo status --format json` 查看 `active_task`、checkpoint 计数和 scope retention。

## 3.5 读写影响

状态、健康、帮助、setup doctor/profile、provider probe、version check、report 和 audit query 是诊断入口，不应修改图谱事实。`version check` 只可能刷新 runtime cache 下的版本检查缓存。`ingest`、`repo index`、`repo update`、`index refresh`、`worker run-once`、proposal 状态变更和 service definition write 会写入运行时状态、派生索引、proposal/audit 或 service definition。

自动化调用方应优先读取 `help --format json` 中的 operation 和 read/write 说明，再决定是否在 CI、agent 或 Web 操作面中开放命令。
