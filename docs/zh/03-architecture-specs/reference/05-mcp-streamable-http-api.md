# relay-knowledge MCP Streamable HTTP API

[中文](05-mcp-streamable-http-api.md) | 英文版尚未提供

> 文档版本: 1.0
> 编制日期: 2026-06-06
> 文档定位：第 23 章 [HTTP API 参考](../23-api-reference.md)的 MCP Streamable HTTP 专题；本页沿用主章迁移前的小节编号，不单独占用章节号。

## 7. MCP Streamable HTTP 接口

relay-knowledge 内嵌 MCP (Model Context Protocol) Server，通过 Streamable HTTP 传输协议暴露工具、资源和提示词。

### 7.1 端点

MCP 端点路径默认为 `/mcp`，可通过 `RELAY_KNOWLEDGE_MCP_ENDPOINT` 环境变量配置。启用/禁用由 `RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED` 控制。

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| POST | `{mcp_endpoint}` | JSON-RPC 请求和通知 |
| DELETE | `{mcp_endpoint}` | 终止 MCP 会话 |
| GET | `{mcp_endpoint}/metrics` | MCP 协议指标（Prometheus 格式） |

### 7.2 协议版本

MCP 协议版本：`2025-11-25`

`initialize` 请求可以省略 `mcp-protocol-version` HTTP 头，但必须在 JSON-RPC params 中声明协议版本；后续 POST 和 DELETE 请求必须携带该头。

### 7.3 会话管理

MCP Streamable HTTP 使用 HTTP 头 `mcp-session-id` 进行会话跟踪。

**会话生命周期**：

1. 客户端发送 `initialize` 请求（不带 `mcp-session-id`）
2. 服务端创建会话并返回 `mcp-session-id` 头
3. 客户端在后续请求中携带该 Session ID
4. 客户端发送 `DELETE` 终止会话

### 7.4 JSON-RPC 方法

| 方法 | 类型 | 说明 |
| --- | --- | --- |
| `initialize` | 请求 | 初始化 MCP 会话，交换协议版本和能力 |
| `notifications/initialized` | 通知 | 客户端确认初始化完成 |
| `ping` | 请求 | 心跳探测 |
| `tools/list` | 请求 | 列出可用工具 |
| `tools/call` | 请求 | 调用指定工具 |
| `resources/list` | 请求 | 列出可用资源 |
| `resources/read` | 请求 | 读取指定资源 |
| `prompts/list` | 请求 | 列出可用提示词 |
| `prompts/get` | 请求 | 获取指定提示词 |
| `notifications/cancelled` | 通知 | 取消进行中的请求 |

### 7.5 初始化握手

**请求**：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2025-11-25",
    "capabilities": {},
    "clientInfo": {
      "name": "my-mcp-client",
      "version": "1.0.0"
    }
  }
}
```

**响应**：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2025-11-25",
    "serverInfo": {
      "name": "relay-knowledge",
      "version": "1.1.13"
    },
    "capabilities": {
      "tools": {},
      "resources": {},
      "prompts": {}
    }
  }
}
```

HTTP 响应头包含 `mcp-session-id: <uuid>`。

### 7.6 可用工具

| 工具名 | 说明 |
| --- | --- |
| `relay_retrieve_context` | 混合检索图谱上下文 |
| `relay_inspect_graph` | 检查图谱元数据和聚合计数 |
| `relay_health` | 返回 health 和 freshness 状态 |
| `relay_service_status` | 返回常驻服务状态 |
| `relay_index_status` | 返回派生索引状态 |
| `relay_code_query` | 代码仓库检索 |
| `relay_business_query` | 查询 authored 业务术语与技术映射 |
| `relay_codegraph_context` | 构建 one-call codegraph context pack |
| `relay_repository_graph` | 读取有界 OKF repository graph neighborhood |
| `relay_code_feature_flags` | 代码特性标志查询 |
| `relay_code_impact` | 代码变更影响分析 |
| `relay_code_repository_set_query` | 跨仓库集查询 |
| `relay_software_query` | 软件全局模型投影 |
| `relay_codebase_view` | 读取证据支持的代码库理解视图 |

### 7.7 工具定义详述

#### relay_retrieve_context

```json
{
  "name": "relay_retrieve_context",
  "description": "Retrieve grounded graph context for a query.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": { "type": "string", "minLength": 1 },
      "source_scope": { "type": "string" },
      "limit": { "type": "integer", "minimum": 1 },
      "freshness": {
        "type": "string",
        "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
      }
    },
    "required": ["query"]
  }
}
```

#### relay_code_query

代码仓库检索工具。参数包括 `query`、`alias`、`kind`、`freshness`、`limit`、`path_filters`、`language_filters`。

#### relay_code_impact

变更影响分析工具。参数包括 `alias`、`base_ref`、`head_ref`、`limit`。

#### relay_business_query

只读业务知识投影工具。参数包括 `alias`、可选 `ref`、`domain`、`query`，以及 `kind`（`terms`、`mappings`、`all`）、`freshness` 和 `limit`。跨 domain 同名词返回 `ambiguous`；未解析技术目标保留 `target_hint`，不会触发查询时仓库扫描。

#### relay_software_query

软件全域 ontology 与兼容投影工具。参数包括 `repository`、`kind`、`freshness`、`limit`、`ref_selector`、`path_filters`、`language_filters` 和可选 `export_profile`。Kind 接受 `dependencies`、`sdks`、`files`、`topics`、`relationships`、`build`、`iac`、`design`、`systems`、`apis`、`resources`、`tests`、`deployments`、`releases`、`statements`、`conflicts` 和 `all`。`export_profile` 接受 `spdx-3`、`cyclonedx-1.7` 或 `prov-o`；设置时返回带 metadata、scope、ontology status、profile、media type 和标准 `document` 的 envelope，并忽略 kind。

### 7.8 错误响应

MCP JSON-RPC 错误码遵循 MCP 规范：

| 错误码 | 含义 |
| --- | --- |
| `-32700` | JSON 解析错误 |
| `-32600` | 无效请求 |
| `-32601` | 方法未找到 |
| `-32602` | 无效参数 |
| `-32603` | 内部错误 |
| `-32000` | 自定义服务端错误 |
| `-32002` | 会话未初始化 |

每个错误响应包含 `code`、`message` 和 `data`（可选，含 `kind` 字段）。

### 7.9 QoS 限制

MCP 端点受 QoS (Quality of Service) 策略控制。当连接数或并发请求数超限时，返回 `429 Too Many Requests`。工具调用被 QoS 拒绝时，返回 `tools/call` 的成功响应，但结果包含错误信息。

### 7.10 curl 示例

```bash
# 初始化会话（-v 可查看返回的 mcp-session-id 头）
curl -s -X POST http://127.0.0.1:8791/mcp \
  -H "Content-Type: application/json" \
  -H "mcp-protocol-version: 2025-11-25" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"curl","version":"1.0"}}}' -v

# 列出工具
curl -s http://127.0.0.1:8791/mcp -H "Content-Type: application/json" \
  -H "mcp-protocol-version: 2025-11-25" \
  -H "mcp-session-id: <session-id>" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'

# 调用检索工具
curl -s http://127.0.0.1:8791/mcp -H "Content-Type: application/json" \
  -H "mcp-protocol-version: 2025-11-25" \
  -H "mcp-session-id: <session-id>" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"relay_retrieve_context","arguments":{"query":"graph database","freshness":"allow-stale","limit":5}}}'

# 终止会话
curl -s -X DELETE http://127.0.0.1:8791/mcp \
  -H "mcp-protocol-version: 2025-11-25" \
  -H "mcp-session-id: <session-id>"
```

---

导航：上一专题：[代码库视图 API](04-codebase-view-api.md) | 下一专题：[模型配置 API](06-model-configuration-api.md) | 返回：[23. HTTP API 参考](../23-api-reference.md) | 上级：[API 专题索引](README.md)
