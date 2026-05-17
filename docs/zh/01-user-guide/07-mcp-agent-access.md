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

已注册仓库 alias 会在首次 MCP 访问时补入进程内动态白名单；未知 scope 仍会被拒绝，并返回缺失 scope 与 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=<scope>` 修复提示。远程 bind 默认被拒绝，非本机监听需要显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`。

允许远程客户端前，同时确认 HTTP bind、origin allow-list、scope allow-list、QoS budget 和审计策略。不要把远程 bind 加 unspecified scope 作为默认配置。

## 7.3 会话流程

客户端需要按 MCP Streamable HTTP 会话顺序调用:

1. 调用 `initialize`，并提供受支持的 `MCP-Protocol-Version`。
2. 保存服务端返回的 `Mcp-Session-Id`。
3. 发送 `notifications/initialized`。
4. 后续请求携带 `Mcp-Session-Id` 和 `MCP-Protocol-Version`。

缺失 session header 会返回 HTTP 400。未知或已淘汰 session id 会返回 HTTP 404。工具请求、`ping` 和 `notifications/cancelled` 都绑定到服务端签发的 session。

## 7.4 Tools、Resources 和 Prompts

MCP tool surface 当前包括:

- 图检索。
- 图检查。
- 健康状态。
- 服务状态。
- 索引状态。
- 已授权代码图谱查询。
- 已授权代码影响分析。

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

Agent 请求会写入 bounded in-process audit events，包含 runtime identity、scope、freshness、QoS decision、budget、truncation、result count 和 status。开启持久 audit sink 的方法见 [第 10 章](10-workers-proposals-audit.md)。

## 7.6 Metrics Endpoint

`GET /mcp/metrics` 返回 Prometheus text 格式快照，覆盖当前 graph version、index refresh queue depth、dead-letter count、QoS in-flight/queued request count 和每个 index 的 stale 状态。该 endpoint 仍通过 MCP router 和 QoS admission 进入服务。

MCP 客户端只使用 Streamable HTTP `/mcp`。`/mcp/sse` 和 `/mcp/message` 不再作为兼容入口提供。
