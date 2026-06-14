# 第 7 章 MCP Agent 接入

[中文](../../zh/01-user-guide/07-mcp-agent-access.md) | [English](../../en/01-user-guide/07-mcp-agent-access.md)

MCP Streamable HTTP 用于把本地图检索能力暴露给外部 agent runtime。它复用统一 API、QoS、scope policy 和 audit，不直接暴露存储或索引实现。

## 7.1 启动 MCP 服务

启动前先生成只读 agent 配置画像:

```bash
relay-knowledge setup profile agent-readonly --format json
```

本机最小启动方式:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --mcp streamable-http
```

默认 endpoint:

```text
http://127.0.0.1:8791/mcp
```

同时启用 Web/API/MCP 时参考 [第 9 章 常驻服务](09-resident-service.md)。

## 7.2 权限变量

常用 MCP policy 变量:

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

默认 policy 要求配置允许 scope。未设置 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` 时，graph tools 会拒绝 unspecified scope，除非显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true`，或者请求的 scope 已经是当前运行时注册过的 code repository alias。

已注册仓库 alias 会在首次 MCP 访问时补入进程内动态白名单。Repository-set alias 不会按这种方式缓存：`relay_code_repository_set_query` 每次调用都会重新校验当前 set 成员。只有当 set alias 被显式允许且没有与已注册 repository alias 冲突，或者当前每个成员 repository alias / 成员 `source_scope` 已被静态 policy 或运行时仓库授权允许时，repository-set alias 才会通过授权。

未知 scope 仍会被拒绝，并返回缺失 scope 与 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=<scope>` 修复提示。远程 bind 默认被拒绝，非本机监听需要显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`。

允许远程客户端前，同时确认 HTTP bind、origin allow-list、scope allow-list、QoS budget 和审计策略。不要把远程 bind 加 unspecified scope 作为默认配置。

## 7.3 会话流程

客户端需要按 MCP Streamable HTTP 会话顺序调用:

1. 调用 `initialize`，并提供受支持的 `MCP-Protocol-Version`。
2. 保存服务端返回的 `Mcp-Session-Id`。
3. 发送 `notifications/initialized`。
4. 后续请求携带 `Mcp-Session-Id` 和 `MCP-Protocol-Version`。

缺失 session header 会返回 HTTP 400。未知或已淘汰 session id 会返回 HTTP 404。工具请求、`ping` 和 `notifications/cancelled` 都绑定到服务端签发的 session。

当 Web/API/MCP 共用同一个 HTTP service 时，`notifications/cancelled` 会在已配置的 MCP endpoint 上通过协议层优先路径处理，并先完成 header、协议版本和已 initialized session 校验。即使普通 in-flight request budget 已满，客户端也能取消正在运行的工具调用，包括未携带 `Content-Length` header 的小型 JSON notification。QoS cancellation 计数只在 notification 命中活跃请求时增加。

`initialize` 到 `tools/list` 的发现路径保持 storage-cold：MCP 只注册静态 tool schema 并返回探索提示，不打开 SQLite，也不执行 schema migration。第一次需要存储的 tool call 才会延迟打开存储；多个并发首次调用共享 service 侧的存储初始化保护。每个 session 的首次 `tools/list` 会把 initialize-to-tools-list cold-start 样本记录到 agent protocol metrics 和 `/mcp/metrics`。

## 7.4 Tools、Resources 和 Prompts

MCP tool surface 当前包括:

- 图检索。
- 图检查。
- 健康状态。
- 服务状态。
- 索引状态。
- 已授权代码图谱查询。
- 已授权软件全域模型查询。
- 已授权 repository-set 代码图谱查询。
- 已授权代码库理解视图。
- 已授权代码影响分析。

Agent kind 选择复用现有产品 kind，而不是新增一套 MCP taxonomy。`relay_code_query` 接受 `hybrid`、`symbol`、`definition`、`references`、`callers`、`callees`、`imports` 和 `sbom`。`relay_software_query` 接受 `dependencies`、`sdks`、`files`、`topics`、`relationships`、`build`、`iac`、`design` 和 `all`。`relay_codebase_view` 接受 `architecture_layers`、`business_domains`、`dependency_tour`、`process_flow` 和 `affected_scope`，并接受与 CLI 一致的短横线别名。为方便 agent 调用，singular alias 会被接受；`configuration` 映射到软件 `relationships`，`model` 或 `models` 映射到软件 `design`；配置驱动 feature flag 仍通过 `relay_code_feature_flags` 查询。

`relay_retrieve_context` 返回带 `indexes`、`index_cursors`、`index_refresh` 和 `context_pack.provenance_trace` 诊断的 GraphRAG context，便于 agent 在信任派生 context 前检查 BM25、semantic、vector、scoped cursor lag、cited evidence、visited-but-uncited context、ranking contributions、stale/degraded 状态和授权裁剪。

`relay_code_query`、`relay_code_feature_flags` 和 `relay_codebase_view` 返回与 CLI 和 Web 相同的代码图谱 freshness 对象，包括 `freshness.state`、`freshness.index_lag`、`freshness.pending`、`freshness.cursor` 和 `freshness.direct_source_read_required`。当响应要求直接读取源码时，agent 必须遵循 `freshness.agent_instructions`，并在使用 stale 图谱证据处理变化文件前验证 `freshness.direct_source_read_paths`。代码库理解视图返回 `nodes`、`edges`、`sections`、`evidence` 和预算截断元数据；section narrative 只是由 evidence id 支撑的派生说明，不会写回图谱事实。

代码查询响应会根据已索引仓库规模返回 `explore_budget`。小于 500 个已索引文件时，预算为 1 次探索调用、15,000 输出字符和 5 个返回文件；500-4,999 个文件为 2 次、30,000 字符和 10 个文件；5,000-14,999 个文件为 3 次、45,000 字符和 15 个文件；更大仓库为 5 次、75,000 字符和 25 个文件。MCP 会把文件上限应用到 `relay_code_query` 与 `relay_code_repository_set_query` 的结果，并在 `agent_output` 中报告截断状态。

所有 MCP 自由文本查询上限为 10,000 字符，path filter 条目上限为 4,096 字符。`relay_code_query` 和 `relay_code_repository_set_query` 也接受 `include_code=true`；class、struct、interface、enum、trait 等容器命中会返回紧凑的签名与行号大纲，而不是大段源码正文。

MCP resource surface 当前包括:

- `relay://service/status`
- `relay://service/health`
- `relay://indexes/status`
- `relay://graph/summary`，仅在 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true` 时暴露。
- `relay://metrics/prometheus`

MCP prompt surface 当前包括:

- `relay_retrieve_context_prompt`
- `relay_code_impact_prompt`

Resources 和 prompts 只提供只读诊断、上下文和调用模板，不能绕过 access policy，也不会开启 mutation、index refresh 或 repository indexing 权限。

## 7.5 写权限边界

MCP 不暴露 index refresh 或 repository indexing。仓库索引需要用户主动运行 `relay-knowledge repo index` 或 `relay-knowledge repo update`；derived index refresh 需要通过 CLI/Web 的显式运维 workflow 触发。

Agent 请求会写入 bounded in-process audit events，包含 runtime identity、scope、freshness、QoS decision、budget、truncation、result count 和 status。发现流程或只读诊断不会为了 durable audit 打开冷存储；当某个 storage-backed tool 已经打开存储后，audit event 才会镜像写入 durable store。Repository-set query 的审计条目会把 `request.set_alias` 记录为 scope，使多仓读取在审计链路中可见。开启持久 audit sink 的方法见 [第 10 章](10-workers-proposals-audit.md)。

## 7.6 Metrics Endpoint

`GET /mcp/metrics` 返回 Prometheus text 格式快照，覆盖当前 graph version、index refresh queue depth、dead-letter count、QoS in-flight/queued request count、MCP cold-start 样本数量与耗时总量，以及每个 index 的 stale 状态。MCP tool call、resource read、metrics read 和本地 ACP prompt 触发 runtime budget 超时时，会推进 `relay_knowledge_qos_timed_out_total`。该 endpoint 仍通过 MCP router 和 QoS admission 进入服务。

MCP 客户端只使用 Streamable HTTP `/mcp`。`/mcp/sse` 和 `/mcp/message` 不再作为兼容入口提供。
