# Chapter 8: Advanced Configuration Reference

[English](../../en/user-guide/08-advanced-configuration.md) | [中文](../../zh/user-guide/08-advanced-configuration.md)

This is the English documentation page for `user-guide/08-advanced-configuration.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

本章只面向需要隔离运行时目录、调试网络预算、开放 MCP 服务、接入外部 embedding worker 或复现 CI 问题的用户。普通本地使用不需要设置这些变量。

## 8.1 配置分层

`relay-knowledge` 的默认使用路径是零配置:

- 本地 SQLite 存储。
- 平台默认运行时目录。
- 本地 deterministic semantic/vector read models。
- 本机 HTTP 监听和保守 QoS 默认值。
- MCP 写入和 index refresh 默认关闭。

高级配置按用途分层:

| 层级 | 用途 | 示例 |
| --- | --- | --- |
| Basic | 日常 CLI 参数 | `--source`、`--limit`、`--freshness`、`--format` |
| Advanced | 检索、网络、QoS、MCP policy | embedding backend、request timeout、scope allow-list |
| Deployment | 安装、service manager、远程访问 | systemd、Windows Service、launchd、service dir |
| Diagnostic | CI、故障复现、临时隔离 | one-off home dir、browser test paths |

## 8.2 运行时目录

优先使用默认目录。需要隔离一次性实验时，只设置一个根目录:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-demo \
  relay-knowledge status --format json
```

需要完全接管目录布局时再分别覆盖:

```text
RELAY_KNOWLEDGE_CONFIG_DIR
RELAY_KNOWLEDGE_DATA_DIR
RELAY_KNOWLEDGE_STATE_DIR
RELAY_KNOWLEDGE_CACHE_DIR
RELAY_KNOWLEDGE_LOG_DIR
RELAY_KNOWLEDGE_TEMP_DIR
RELAY_KNOWLEDGE_RUNTIME_DIR
RELAY_KNOWLEDGE_SERVICE_DIR
```

所有覆盖路径必须是绝对路径，且不能包含 `..`。

## 8.3 检索后端

默认值是本地 deterministic read models。只有外部 worker 已经按同一 metadata contract 写入派生 read model 时，才启用 external backend metadata:

```bash
RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external \
RELAY_KNOWLEDGE_VECTOR_BACKEND=external \
RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL=text-embed-3-small \
RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL=clip-vit-b32 \
RELAY_KNOWLEDGE_EMBEDDING_DIMENSION=1536 \
relay-knowledge index refresh --kind semantic --kind vector --format json
```

可选 provider worker 调优:

```text
RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE
RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS
RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY
```

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` 和 `RELAY_KNOWLEDGE_VECTOR_BACKEND` 接受 `local`、`external` 或 `disabled`。`disabled` 会跳过对应 retriever 和 refresh scheduling。外部 provider 配置只描述 metadata 和 worker contract；查询热路径不会同步调用外部 embedding 服务。

## 8.4 网络与 QoS

常驻服务和 MCP Streamable HTTP 使用 `net::http` 和 `net::qos` 统一处理网络能力:

```text
RELAY_KNOWLEDGE_HTTP_BIND
RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS
RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS
RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES
RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS
RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS
RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH
```

代理和证书验证继承 `HTTPS_PROXY`、`HTTP_PROXY`、`ALL_PROXY`、`NO_PROXY` 和 `SSL_VERIFY`。业务模块不直接读取进程环境。

非 loopback HTTP bind 应同时配置 MCP remote-client policy 和 origin/scope 限制。QoS budget 是 admission control，不是安全认证；它用于限制连接数、in-flight 请求、队列深度、超时和 overload 行为。

## 8.5 MCP Policy

本地 agent 工具服务通常只需要指定允许 scope:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --mcp streamable-http
```

完整 MCP policy 变量:

```text
RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED
RELAY_KNOWLEDGE_MCP_ENDPOINT
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES
RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE
RELAY_KNOWLEDGE_MCP_MAX_LIMIT
RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES
RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS
```

默认 policy 是只读且本机优先。远程监听和 unspecified scope 都需要显式开启。MCP 不提供 index refresh 或 repository indexing；这些操作必须通过 CLI/Web 的显式 workflow 触发。

## 8.6 OTLP Telemetry

常驻服务可把 traces 和 metrics 发送到 OpenTelemetry Collector 的 OTLP HTTP endpoint:

```text
RELAY_OTEL_ENDPOINT
RELAY_OTEL_TRACES
RELAY_OTEL_METRICS
RELAY_OTEL_EXPORT_TIMEOUT_MS
RELAY_OTEL_SERVICE_ENVIRONMENT
```

默认 endpoint 是 `http://127.0.0.1:4318`。启用 traces 后使用 `/v1/traces`，启用 metrics 后使用 `/v1/metrics`；当 endpoint 已经包含其中一个 signal path，另一个 signal 会改写到同级 path。`RELAY_OTEL_EXPORT_TIMEOUT_MS` 默认 5000，并用于服务停止时 flush OTLP providers。建议先让 Collector 在本机监听，再开启:

```bash
RELAY_OTEL_ENDPOINT=http://127.0.0.1:4318 \
RELAY_OTEL_TRACES=true \
RELAY_OTEL_METRICS=true \
relay-knowledge service run --web --mcp streamable-http
```

Exporter 初始化或导出失败不会阻止服务启动；错误会作为 telemetry diagnostics 暴露。单个 signal 失败不会阻断另一个 signal，trace exporter 失败时仍保留本地 tracing fallback。

## 8.7 Worker、Silent Updates 与 Audit

后台 worker 和 silent-update operator 使用这些变量:

```text
RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT
RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT
RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH
```

未设置 worker endpoint 时，`worker run-once` 使用 deterministic fallback 生成 proposal。开启 audit sink 后，agent audit JSONL 写入 `paths` 管理的 log 目录；队列深度在运行时 capped 到 65536，队列满时持久镜像可以丢弃事件，内存 audit log 仍保留最近事件。

## 8.8 Setup Interfaces

高级配置不需要从文档手工拼接。当前 CLI 提供两个只读 setup 入口:

```bash
relay-knowledge setup doctor
relay-knowledge setup profile local
relay-knowledge setup profile agent-readonly
relay-knowledge setup profile service
relay-knowledge setup profile external-embedding
```

`setup doctor` 会检查运行时目录、network/QoS budget、retrieval backend
metadata、MCP policy、service directory 和 worker budget，并在 JSON 响应中返回
`configuration_ready`、`live_health_checked=false`、`live_health_commands` 和
`recommended_actions`。它不打开 SQLite，不迁移 schema，也不刷新索引；需要检查
graph version、storage health、index freshness 或 worker/service live health 时继续运行
`health` 或 `service doctor`。如果旧本地数据库缺少早期 index refresh task 时间戳列，
启动时 schema migration 会补齐 `created_at_ms` 和 `updated_at_ms`，并用迁移时刻
回填旧任务，避免健康诊断因旧队列表结构失败或显示接近 epoch 的队列年龄。

`setup profile` 输出推荐环境变量、命令和安全提示，不写 `.env`，不修改 shell
profile，也不执行 service manager 安装。支持的 profile:

| Profile | 用途 |
| --- | --- |
| `local` | 零配置本地 CLI/Web 诊断循环，可选隔离 `RELAY_KNOWLEDGE_HOME`。 |
| `agent-readonly` | 本机 MCP Streamable HTTP 只读 agent 接入，要求显式 scope。 |
| `service` | 平台 service manager plan、definition write 和 operator 检查。 |
| `external-embedding` | 外部 OpenAI-compatible embedding provider metadata、probe 和 refresh 验证。 |
