# 第 6 章 Agent 与常驻服务

## 6.1 前台常驻服务

启动前台服务并启用 MCP Streamable HTTP。默认只建议允许明确的 source scope:

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

Web 页面中的 service run 操作只通过 `/api/web/operations/execute` 返回当前 service runtime snapshot，用于检查即将运行的配置和 MCP 状态；实际常驻服务必须由 CLI、`run.sh` 或平台 service manager 启动。

## 6.2 MCP 权限变量

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
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH
```

默认 policy 要求配置允许 scope。未设置 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` 时，graph tools 会拒绝 unspecified scope，除非显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true`。远程 bind 默认被拒绝，非本机监听需要显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`。

`relay.refresh_indexes` 默认隐藏，只有设置 `RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH=true` 后才会出现在 tool list 中。完整 MCP policy 变量见 [第 8 章 高级配置参考](08-advanced-configuration.md)。

允许远程客户端前应同时确认 HTTP bind、origin allow-list、scope allow-list、QoS budget 和审计策略。不要用远程 bind 加 unspecified scope 作为默认配置。

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

worker endpoint 负责 CPU-heavy 或 I/O-heavy 工作，例如 embedding、OCR、视觉 caption、表格/layout 抽取。worker 结果先进入 proposal 或 multimodal extraction commit path，不在查询热路径里同步调用外部服务。

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

## 6.6 Tools、Resources 和 Prompts

MCP tool surface 当前包括:

- graph retrieval
- graph inspection
- health
- service status
- index status
- authorized code graph query
- authorized code impact analysis
- permission-gated index refresh

MCP resource surface 当前包括:

- `relay://service/status`
- `relay://service/health`
- `relay://indexes/status`
- `relay://graph/summary`，仅在 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true` 时暴露
- `relay://metrics/prometheus`

MCP prompt surface 当前包括:

- `relay.retrieve-context`
- `relay.code-impact`

Resources 和 prompts 只提供只读诊断、上下文和调用模板，不能绕过 access policy，也不会开启 mutation 或 index refresh 权限。

工具调用的写权限边界由 tool 本身和 policy 共同控制。默认 graph retrieval、inspection、health、service status、index status、code query 和 code impact 是主要暴露面；index refresh 需要显式开启。mutation 类能力应通过受控 CLI/Web/API workflow 执行，并保留 audit。

## 6.7 Metrics、兼容端点和审计

`GET /mcp/metrics` 返回 Prometheus text 格式快照，覆盖当前 graph version、index refresh queue depth、dead-letter count、QoS in-flight/queued request count 和每个 index 的 stale 状态。该 endpoint 仍通过 MCP router 和 QoS admission 进入服务。

旧 HTTP+SSE 客户端可使用 `GET /mcp/sse` 获取 server-issued session id 和 message endpoint，并保持 SSE stream 打开以接收后续 `message` events；再向 `/mcp/message?sessionId=<id>` 发送 JSON-RPC。新集成应优先使用 Streamable HTTP `/mcp`。

Agent 请求会写入 bounded in-process audit events，包含 runtime identity、scope、freshness、QoS decision、budget、truncation、result count 和 status。设置 `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true` 后，MCP 和本地 ACP audit events 会通过有界 async queue 写入 `logs/agent-audit.jsonl`。`RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH` 控制持久 sink 的排队深度，并在运行时限制到最多 65536；队列满时持久镜像可丢弃事件，但内存 audit log 仍保留最近事件。

CLI/Web/service operation 还写入持久 audit sink，可通过 `audit query` 检查最近操作。

## 6.8 OTLP Telemetry

常驻服务支持真实 OTLP HTTP/protobuf traces 和 metrics export。示例:

```bash
RELAY_OTEL_ENDPOINT=http://127.0.0.1:4318 \
RELAY_OTEL_TRACES=true \
RELAY_OTEL_METRICS=true \
RELAY_OTEL_SERVICE_ENVIRONMENT=local \
relay-knowledge service run --web --mcp streamable-http
```

默认 endpoint 是 `http://127.0.0.1:4318`，traces 发送到 `/v1/traces`，metrics 发送到 `/v1/metrics`。如果配置了 signal-specific endpoint，另一个 signal 会自动使用同级路径。Collector 不可用时，检索和协议响应不会失败；错误会出现在 service diagnostics 的 telemetry 状态中。服务停止时会按 `RELAY_OTEL_EXPORT_TIMEOUT_MS` flush 已安装的 OTLP exporters。

## 6.9 ACP 本地 adapter

本地 ACP session adapter 暴露相同的检索 contract，支持 progress updates、cancellation 和 context artifact。ACP 适合 agent-client 会话入口，MCP 更适合作为其它 agent runtime 的工具服务入口。两者都复用统一 API 和核心服务，不复制检索逻辑。

## 6.9 服务运行建议

开发机临时验证优先使用前台命令或 `run.sh`:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
```

长期后台运行应使用 `service plan` 和 `service definition write` 生成平台 service manager 配置，再由用户或安装器执行需要权限的安装动作。不要用未受管 CLI 循环替代 systemd、Windows Service 或 launchd。运行时数据、日志、缓存、worker 队列和 dead-letter 数据必须留在 `paths` 管理的目录中，而不是 release 解压目录或仓库目录。
