# relay-knowledge HTTP API 参考

[中文](23-api-reference.md) | 英文版尚未提供

> 文档版本: 1.0
> 编制日期: 2026-06-06
> 适用范围: relay-knowledge API 完整参考

## 1. 概述

relay-knowledge 通过统一的 HTTP API 层暴露知识图谱能力，包含控制面诊断、代码仓库索引与检索、知识图谱操作以及 MCP Streamable HTTP 代理协议。

### 1.1 Base URL

```text
http://127.0.0.1:8791
```

服务默认只监听 loopback `127.0.0.1:8791`。通过 `RELAY_KNOWLEDGE_HTTP_BIND` 可以修改完整的 `host:port`；非 loopback 绑定还必须满足远端客户端、scope/origin、QoS 与审计策略，详见[安全配置指南](../01-user-guide/16-security-configuration-guide.md)。

### 1.2 API 版本
稳定的代码仓库与控制面路由使用 `/api/v1/` 前缀。same-origin Web 操作、模型配置、兼容诊断和 MCP 路由目前仍使用 `/api/web/`、`/api/configs/`、`/api/` 与可配置的 MCP endpoint；不能仅凭是否含 `/v1/` 判断接口是否存在。

### 1.3 认证

控制面 API 和 Web 操作 API 为同源（same-origin）设计，当前无需认证头。代码仓库 API 支持通过 HTTP 头传播请求追踪标识。

### 1.4 请求追踪

通过共享 application service 返回的业务成功响应包含 `metadata` 字段，内含：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `trace_id` | string | 分布式追踪 ID |
| `request_id` | string | 请求级 ID |
| `graph_version` | u64 | 响应时的图版本号 |
| `index_version` | u64 \| null | 派生索引版本号 |
| `indexed_graph_version` | u64 \| null | 索引对应的图版本 |
| `stale` | bool | 数据是否可能过期 |

代码仓库 API 支持通过请求头传递追踪 ID：

```http
X-Relay-Request-Id: my-request-001
X-Relay-Trace-Id: trace-abc123
```

### 1.5 内容类型

业务 API 请求和响应使用 `application/json`。静态资源按文件类型返回，`GET {mcp_endpoint}/metrics` 返回 Prometheus text exposition，MCP session DELETE 也不承诺 JSON body。

## 2. 错误响应

控制面、代码仓库与模型配置路由的业务错误使用统一格式：

```json
{
  "error_kind": "invalid_argument",
  "message": "描述具体原因的字符串"
}
```

**错误类型 (ErrorKind)**：

| 值 | HTTP 状态码 | 含义 |
| --- | --- | --- |
| `invalid_argument` | 400 BAD_REQUEST | 请求参数无效 |
| `storage_unavailable` | 503 SERVICE_UNAVAILABLE | 存储层不可用 |
| `qos_rejected` | 429 TOO_MANY_REQUESTS | QoS 准入预算耗尽 |
| `timeout` | 408 REQUEST_TIMEOUT | 直接 JSON API 路由的 application 操作或 HTTP middleware 超时 |
| `internal` | 500 INTERNAL_SERVER_ERROR | 内部错误 |

`/api/web/operations/execute` 的 adapter 错误使用 `{"error":"..."}` envelope，其 timeout 映射为 `504 Gateway Timeout`；MCP 使用 JSON-RPC 错误。

### curl 示例

```bash
curl -s http://127.0.0.1:8791/api/v1/code/repositories/unknown/status | jq .
# {"error_kind":"invalid_argument","message":"repository not found: unknown"}
```

## 3. 成功响应通用字段
共享业务 API 的成功响应包含 `metadata`（ApiMetadata 结构）；静态资源、MCP JSON-RPC envelope 和 metrics text 不使用该 envelope：

```json
{
  "metadata": {
    "trace_id": "trace-1717632000000000001",
    "request_id": "req-1717632000000000001",
    "graph_version": 47,
    "index_version": 3,
    "indexed_graph_version": 42,
    "stale": false
  }
}
```

## 4. 端点总览与专题导航

详细字段、约束和示例按 API 表面拆分到以下专题页；跨端点的响应 envelope、
错误映射和运行时约定仍以本章为准。

| API 表面 | 主要路径 | 详细参考 |
| --- | --- | --- |
| 控制面诊断 | `/api/project/status`、`/api/health`、`/api/service/status`、`/api/v1/control/**` | [控制面与 Web 操作 API](reference/02-control-and-web-api.md#4-控制面-api) |
| Web 操作 | `/api/web/graph/canvas`、`/api/web/operations/execute` | [控制面与 Web 操作 API](reference/02-control-and-web-api.md#5-web-操作-api) |
| 代码仓库 | `/api/v1/code/repositories`、`/api/v1/code/repositories/{alias}/**` | [代码仓库 API](reference/03-code-repository-api.md#6-代码仓库-api) |
| 代码库视图 | `/api/v1/code/repositories/{alias}/views` | [代码库视图 API](reference/04-codebase-view-api.md) |
| MCP Streamable HTTP | `{mcp_endpoint}`、`{mcp_endpoint}/metrics` | [MCP Streamable HTTP API](reference/05-mcp-streamable-http-api.md#7-mcp-streamable-http-接口) |
| 模型配置 | `/api/configs/model/**`、`/api/configs/model-*` | [模型配置 API](reference/06-model-configuration-api.md#8-模型配置-api) |

完整的方法、路径和用途清单见 [HTTP API 端点速查表](reference/01-http-endpoints.md)。
所有专题页都是本章的组成部分，不另行定义或放宽产品合同。

## 5. 静态资源

根路径 `/` 返回 `index.html`，其他路径（不以 `api/` 开头）作为静态资源提供。资源文件位于 `web/dist/` 目录。

支持的 Content-Type：

| 扩展名 | Content-Type |
| --- | --- |
| `.css` | `text/css; charset=utf-8` |
| `.html` | `text/html; charset=utf-8` |
| `.js` | `text/javascript; charset=utf-8` |
| `.json` | `application/json` |
| `.svg` | `image/svg+xml` |
| `.wasm` | `application/wasm` |

SPA 路由：所有未匹配的非 API 路径返回 `index.html`。

## 6. 通用约定

### 6.1 速率限制

QoS 层在所有端点上生效。超限时返回 `429 Too Many Requests`。可通过 `/api/project/status` 中的 `qos_*` 字段查看当前配置。

### 6.2 请求体大小限制

默认最大请求体为 1 MiB，可通过 `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` 配置。超限返回 `413 Payload Too Large`。

### 6.3 超时

HTTP 请求默认超时 30 秒，可通过 `RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS` 配置。直接 JSON API route 把 application timeout 映射为 `408 Request Timeout`；Web operation adapter 映射为 `504 Gateway Timeout`。

### 6.4 优雅关闭

服务收到 SIGTERM 后执行优雅关闭，默认等待 10 秒让进行中的请求完成。超时可通过 `RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS` 配置。

### 6.5 Serde 字段命名

所有 JSON 字段使用 `snake_case` 命名。枚举值由对应 contract 的 serde 注解决定；当前公开的代码查询、freshness、software、view 与 Web operation 枚举均使用文中列出的 `snake_case` 值，调用方不应把 Rust variant 名直接当作 wire value。

## 附录 A: 端点速查表

完整的方法、路径和用途索引见[端点速查表](reference/01-http-endpoints.md)。详细请求、响应和边界由[专题索引](reference/README.md)链接的各 API 页面承接；跨端点约定仍以本章为准。

---

导航：上一章：[22. 服务化部署、控制面与数据面分离](22-service-deployment-control-data-plane.md) | 下一章：[24. 代码地图驱动的 Knowledge 开发闭环](24-code-map-backed-knowledge-development-loop.md) | 专题：[API 专题索引](reference/README.md) | 返回：[架构规格](README.md)
