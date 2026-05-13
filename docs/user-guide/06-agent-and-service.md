# 第 6 章 Agent 与常驻服务

## 6.1 前台常驻服务

启动前台服务并启用 MCP Streamable HTTP:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --mcp streamable-http
```

默认监听:

```text
http://127.0.0.1:8791/mcp
```

`service run` 启动时会先执行 startup index reconciler，尽量在接受 resident adapter 请求前恢复落后的索引任务。没有启用 MCP 时，命令仍会作为前台服务等待 shutdown signal。

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

## 6.3 MCP 会话流程

客户端需要按 MCP Streamable HTTP 会话顺序调用:

1. 调用 `initialize`，并提供受支持的 `MCP-Protocol-Version`。
2. 保存服务端返回的 `Mcp-Session-Id`。
3. 发送 `notifications/initialized`。
4. 后续请求携带 `Mcp-Session-Id` 和 `MCP-Protocol-Version`。

缺失 session header 会返回 HTTP 400。未知或已淘汰 session id 会返回 HTTP 404。工具请求、`ping` 和 `notifications/cancelled` 都绑定到服务端签发的 session。

## 6.4 Tool 面

MCP tool surface 当前包括:

- graph retrieval
- graph inspection
- health
- service status
- index status
- authorized code graph query
- authorized code impact analysis
- permission-gated index refresh

Agent 请求会写入 bounded in-process audit events，包含 runtime identity、scope、freshness、QoS decision、budget、truncation、result count 和 status。

## 6.5 ACP 本地 adapter

本地 ACP session adapter 暴露相同的检索 contract，支持 progress updates、cancellation 和 context artifact。ACP 适合 agent-client 会话入口，MCP 更适合作为其它 agent runtime 的工具服务入口。两者都复用统一 API 和核心服务，不复制检索逻辑。
