# 第 6 章 Agent 与常驻服务

## 6.1 前台常驻服务

启动前台服务并启用 MCP Streamable HTTP:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --mcp streamable-http
```

启动同端口 Web/API/MCP 服务:

```bash
./build.sh
./run.sh start --port 8791 --daemon
```

对应的底层命令是:

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
target/release/relay-knowledge service run --web --mcp streamable-http
```

默认监听:

```text
http://127.0.0.1:8791/mcp
```

`service run` 启动时会先执行 startup index reconciler，尽量在接受 resident adapter 请求前恢复落后的索引任务。没有启用 MCP 或 Web 时，命令仍会作为前台服务等待 shutdown signal。

## 6.2 MCP 权限变量

常用 MCP 配置:

```text
RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED
RELAY_KNOWLEDGE_MCP_ENDPOINT
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES
RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE
RELAY_KNOWLEDGE_MCP_MAX_LIMIT
RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES
RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH
RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS
```

默认 policy 要求配置允许 scope。未设置 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` 时，graph tools 会拒绝 unspecified scope，除非显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true`。远程 bind 默认被拒绝，非本机监听需要显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`。

`relay.refresh_indexes` 默认隐藏，只有设置 `RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH=true` 后才会出现在 tool list 中。

## 6.3 Worker、Proposal 与 Audit

多模态 evidence 写入后会进入持久 worker 队列。可配置外部 HTTP worker endpoint:

```text
RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT
RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT
RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED
```

常用命令:

```bash
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
relay-knowledge proposal show <proposal-id> --format json
relay-knowledge proposal accept <proposal-id> --by <actor> --reason "reviewed"
relay-knowledge audit query --limit 50 --format json
```

未配置外部 endpoint 时，worker run-once 使用 deterministic fallback 生成 proposal，不阻塞 BM25、graph retrieval 或 ingest。proposal 必须人工 accept 后才会通过 graph mutation pipeline 写入 accepted facts。

## 6.4 Service Manager 与 Silent Update Operator

service manager v1 生成平台定义和命令预览，不自动执行需要权限的安装命令:

```bash
relay-knowledge service plan install --format json
relay-knowledge service definition write --format json
relay-knowledge service operator status --format json
relay-knowledge service operator pause
relay-knowledge service operator resume
```

Linux 输出 systemd user service 计划，macOS 输出 launchd plist 计划，Windows 输出 service XML/PowerShell 计划。runtime state、graph database、indexes、audit 和 worker 队列仍使用 `paths` 解析后的 platform data/state/log/cache 目录，不写入 release extraction directory。

## 6.5 MCP 会话流程

客户端需要按 MCP Streamable HTTP 会话顺序调用:

1. 调用 `initialize`，并提供受支持的 `MCP-Protocol-Version`。
2. 保存服务端返回的 `Mcp-Session-Id`。
3. 发送 `notifications/initialized`。
4. 后续请求携带 `Mcp-Session-Id` 和 `MCP-Protocol-Version`。

缺失 session header 会返回 HTTP 400。未知或已淘汰 session id 会返回 HTTP 404。工具请求、`ping` 和 `notifications/cancelled` 都绑定到服务端签发的 session。

## 6.6 Tool 面

MCP tool surface 当前包括:

- graph retrieval
- graph inspection
- health
- service status
- index status
- authorized code graph query
- authorized code impact analysis
- permission-gated index refresh

Agent 请求会写入 bounded in-process audit events；CLI/Web/service operation 还写入持久 audit sink，可通过 `audit query` 检查最近操作。

## 6.7 ACP 本地 adapter

本地 ACP session adapter 暴露相同的检索 contract，支持 progress updates、cancellation 和 context artifact。ACP 适合 agent-client 会话入口，MCP 更适合作为其它 agent runtime 的工具服务入口。两者都复用统一 API 和核心服务，不复制检索逻辑。
