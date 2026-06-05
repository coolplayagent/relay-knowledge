# 第 12 章 高级配置参考

[中文](../../zh/01-user-guide/12-advanced-configuration.md) | [English](../../en/01-user-guide/12-advanced-configuration.md)

本章是环境变量和配置层级参考。普通本地使用不需要设置这些变量；需要隔离运行时目录、调试网络预算、开放 MCP 服务、接入外部 embedding worker 或复现 CI 问题时再查本章。

## 12.1 配置分层

`relay-knowledge` 的默认使用路径是零配置:

- 本地 SQLite 存储。
- 平台默认运行时目录。
- 本地 deterministic semantic/vector read models。
- 本机 HTTP 监听和保守 QoS 默认值。
- MCP 写入、远程监听和 silent updates 默认关闭。

高级配置按用途分层:

| 层级 | 用途 | 示例 |
| --- | --- | --- |
| Basic | 日常 CLI 参数 | `--source`、`--limit`、`--freshness`、`--format`、`--remote` |
| Advanced | 检索、网络、QoS、MCP policy | embedding backend、request timeout、scope allow-list |
| Deployment | 安装、service manager、远程访问 | systemd、Windows Service、launchd、service dir |
| Diagnostic | CI、故障复现、临时隔离 | one-off home dir、browser test paths |

## 12.2 运行时目录

远端服务访问可用一次性 `--remote http://host:8791`，或在 automation profile 中设置 `RELAY_KNOWLEDGE_REMOTE_BASE_URL=http://host:8791`。该变量影响支持的代码仓库 index、scope preview、status、query、feature-flags、impact、report 和 software projection 命令，并阻止 `repo index --reset` 与 `repo index-worker` 回落到本机；无关本地命令仍使用本机 runtime 目录解析。远端分发在联系服务前只校验远端 URL 和 outbound network 设置；完整本机 runtime、storage、retrieval 与路径校验会延迟到命令实际使用本机状态时。

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

## 12.3 存储拓扑

默认存储拓扑是 `single_sqlite`，所有运行时状态写入 runtime data 目录下的主
SQLite 数据库。需要把代码仓库事实隔离到每个注册仓库一个 SQLite 文件时，再启用分片拓扑:

```bash
RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite \
  relay-knowledge repo register /path/to/repository --format json
```

`partitioned_sqlite` 会把全局控制状态、持久任务、lease、审计和图事实保留在主数据库中。仓库文件、符号、引用、chunk、checkpoint 和按 scope 的代码查询使用 runtime data 目录下 `stores/repositories/` 中的 shard 文件。多仓 repository-set overlay refresh 在跨 shard import/export 聚合实现前仍要求 `single_sqlite`。

主数据库一旦包含 active 的分片目录，`single_sqlite` 会拒绝打开这份运行时状态。此后应继续设置 `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite`，或先执行显式 rollback，清理分片目录和 shard 文件后再回退。

分片目录记录是可迁移的：恢复时会根据 repository id 和当前 runtime data 目录重新计算 shard 路径，因此备份或移动时必须同时保留主数据库和 `stores/repositories/`。

## 12.4 检索后端

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

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` 和 `RELAY_KNOWLEDGE_VECTOR_BACKEND` 接受 `local`、`external` 或 `disabled`。外部 provider 配置只描述 metadata 和 worker contract；查询热路径不会同步调用外部 embedding 服务。

Rerank 默认启用本地确定性精选，不需要远端服务:

```text
RELAY_KNOWLEDGE_RERANK_BACKEND=local
RELAY_KNOWLEDGE_RERANK_MODEL=relay-local-deterministic-rerank-v1
RELAY_KNOWLEDGE_RERANK_CANDIDATE_MULTIPLIER=4
RELAY_KNOWLEDGE_RERANK_MAX_CANDIDATES=64
RELAY_KNOWLEDGE_RERANK_TIMEOUT_MS=100
```

`RELAY_KNOWLEDGE_RERANK_BACKEND` 接受 `local`、`external` 或 `disabled`。`external` 当前只保留 provider contract 并降级为本地 rerank；查询热路径不会同步调用远端 rerank 模型。

## 12.5 网络与 QoS

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

版本提示也使用 `net::http`，并将结果缓存到 runtime cache:

```text
RELAY_KNOWLEDGE_UPDATE_CHECK_ENABLED
RELAY_KNOWLEDGE_UPDATE_SOURCES
RELAY_KNOWLEDGE_UPDATE_CHECK_INTERVAL_MS
RELAY_KNOWLEDGE_UPDATE_GITHUB_REPO
```

默认启用 GitHub Releases 与 crates.io 双源稳定版本检查，缓存周期为 24 小时。关闭该能力只会停止提示，不影响
`relay-knowledge version` 打印本地版本。release metadata 响应体受
`RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` 限制；关闭更新检查时，检测源、仓库和间隔覆盖值都会被忽略，避免仅用于提示的坏配置阻塞 runtime loading。

非 loopback HTTP bind 应同时配置 MCP remote-client policy 和 origin/scope 限制。QoS budget 是 admission control，不是安全认证；它用于限制连接数、in-flight 请求、队列深度、超时和 overload 行为。

## 12.6 MCP Policy

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

默认 policy 是只读且本机优先。远程监听和 unspecified scope 都需要显式开启；已注册 code repository alias 可在首次 MCP 访问时按需进入进程内动态白名单，未知 scope 仍需要配置 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES`。

## 12.7 Worker、Audit 与 OTLP

后台 worker 和 agent audit:

```text
RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT
RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT
RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH
```

`RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` 控制 `service run` 中 code-index
worker pool 的并发度。默认值为 2，运行时会限制最大值，保证多个索引任务可以独立推进，
同时 SQLite 写入仍通过单 writer lane 保持一致性。

OTLP:

```text
RELAY_OTEL_ENDPOINT
RELAY_OTEL_TRACES
RELAY_OTEL_METRICS
RELAY_OTEL_EXPORT_TIMEOUT_MS
RELAY_OTEL_SERVICE_ENVIRONMENT
```

行为说明分别见 [第 10 章](10-workers-proposals-audit.md) 和 [第 11 章](11-observability-and-telemetry.md)。

## 12.8 Setup 接口

高级配置不需要从文档手工拼接。当前 CLI 提供两个只读 setup 入口:

```bash
relay-knowledge setup doctor
relay-knowledge setup profile local
relay-knowledge setup profile agent-readonly
relay-knowledge setup profile service
relay-knowledge setup profile external-embedding
```

`setup doctor` 会检查运行时目录、network/QoS budget、retrieval backend metadata、MCP policy、service directory 和 worker budget，并在 JSON 响应中返回 `configuration_ready`、`live_health_checked=false`、`live_health_commands` 和 `recommended_actions`。它不打开 SQLite，不迁移 schema，也不刷新索引。

`setup profile` 输出推荐环境变量、命令和安全提示，不写 `.env`，不修改 shell profile，也不执行 service manager 安装。
