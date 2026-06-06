# relay-knowledge HTTP API 参考

> 文档版本: 1.0
> 编制日期: 2026-06-06
> 适用范围: relay-knowledge API 完整参考

## 1. 概述

relay-knowledge 通过统一的 HTTP API 层暴露知识图谱能力，包含控制面诊断、代码仓库索引与检索、知识图谱操作以及 MCP Streamable HTTP 代理协议。

### 1.1 Base URL

```
http://localhost:8080
```

服务启动时默认监听 `0.0.0.0:8080`，实际地址由 `RELAY_KNOWLEDGE_HTTP_BIND` 环境变量和 `RELAY_KNOWLEDGE_HTTP_PORT` 配置决定。

### 1.2 API 版本

当前所有 API 路径均以 `/api/v1/` 为前缀（部分旧端点使用 `/api/` 无版本前缀）。版本策略为路径版本化，未来不兼容变更将通过 `/api/v2/` 暴露。

### 1.3 认证

控制面 API 和 Web 操作 API 为同源（same-origin）设计，当前无需认证头。代码仓库 API 支持通过 HTTP 头传播请求追踪标识。

### 1.4 请求追踪

所有响应均包含 `metadata` 字段，内含：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `trace_id` | string | 分布式追踪 ID |
| `request_id` | string | 请求级 ID |
| `graph_version` | u64 | 响应时的图版本号 |
| `index_version` | u64 \| null | 派生索引版本号 |
| `indexed_graph_version` | u64 \| null | 索引对应的图版本 |
| `stale` | bool | 数据是否可能过期 |

代码仓库 API 支持通过请求头传递追踪 ID：

```
X-Relay-Request-Id: my-request-001
X-Relay-Trace-Id: trace-abc123
```

### 1.5 内容类型

所有 API 端点接受和返回 `application/json`。

---

## 2. 错误响应

所有 API 错误遵循统一格式：

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
| `timeout` | 504 GATEWAY_TIMEOUT | 操作超时 |
| `internal` | 500 INTERNAL_SERVER_ERROR | 内部错误 |

### curl 示例

```bash
curl -s http://localhost:8080/api/v1/code/repositories/unknown/status | jq .
# {"error_kind":"invalid_argument","message":"repository not found: unknown"}
```

---

## 3. 成功响应通用字段

所有成功响应均包含 `metadata`（ApiMetadata 结构）：

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

---

## 4. 控制面 API

### 4.1 GET /api/project/status

获取项目元信息和运行时状态。

**响应 200**：

```json
{
  "project_name": "relay-knowledge",
  "metadata": { "..." },
  "runtime": {
    "config_dir": "/home/user/.config/relay-knowledge",
    "data_dir": "/home/user/.local/share/relay-knowledge",
    "storage_topology": "single",
    "http_bind": "0.0.0.0:8080",
    "http_request_timeout_ms": 30000,
    "http_max_request_body_bytes": 10485760,
    "qos_max_connections": 512,
    "qos_max_in_flight_requests": 64,
    "qos_max_queue_depth": 128,
    "silent_updates_enabled": true,
    "semantic_backend_mode": "embedded",
    "vector_backend_mode": "embedded",
    "embedding_provider": "openai",
    "text_embedding_model": "text-embedding-3-small",
    "embedding_dimension": 1536
  }
}
```

### 4.2 GET /api/health

全面健康检查，包括存储、图、索引和运行时状态。

**响应 200**：

```json
{
  "metadata": { "..." },
  "healthy": true,
  "storage": { "topology": "single", "active_shard_count": 1, "..." },
  "graph": { "node_count": 1234, "edge_count": 5678, "..." },
  "repository_code_totals": { "repository_count": 3, "total_files": 15234, "..." },
  "indexes": [{ "kind": "bm25", "stale": false, "..." }],
  "index_cursors": [{ "..." }],
  "index_refresh": { "..." },
  "file_index": { "..." },
  "runtime": { "config_dir": "..." }
}
```

### 4.3 GET /api/v1/control/status

运行时诊断，始终返回。**响应 200**：与 `/api/project/status` 同结构。

### 4.4 GET /api/v1/control/health

只读健康检查。**响应 200**：与 `/api/health` 同结构。

### 4.5 GET /api/service/status

完整服务状态，包含 service manager、operator、worker、audit 等。

**响应 200**：

```json
{
  "metadata": { "..." },
  "service_name": "relay-knowledge",
  "mode": "standalone",
  "background_enabled": true,
  "silent_updates_enabled": true,
  "service_definition_path": "/home/user/.local/share/relay-knowledge/service/relay-knowledge.service",
  "storage": { "topology": "single", "active_shard_count": 1, "..." },
  "index_refresh": { "..." },
  "file_index": { "..." },
  "agent_protocols": {
    "mcp": { "enabled": true, "endpoint": "/mcp" },
    "acp_local": { "enabled": false }
  },
  "operator": { "state": "idle", "..." },
  "workers": [{ "kind": "index_maintenance", "state": "idle", "..." }],
  "code_index_workers": {
    "configured_worker_count": 2,
    "active_worker_slots": 1,
    "queue_depth": 3,
    "queued_task_count": 2,
    "running_task_count": 1,
    "retrying_task_count": 0,
    "dead_letter_task_count": 0,
    "running_lease_count": 1
  },
  "proposal_backlog": 0,
  "audit_sink": { "durable": true, "event_count": 47 }
}
```

### 4.6 GET /api/v1/control/service/status

只读服务状态。**响应 200**：与 `/api/service/status` 同结构。

### 4.7 GET /api/v1/control/storage/topology

存储拓扑诊断，包括 shard catalog 和分区详情。

**响应 200**：

```json
{
  "metadata": { "..." },
  "storage": {
    "topology": "partitioned",
    "control_database_path": "/home/user/.local/share/relay-knowledge/relay-knowledge.sqlite",
    "repository_shards_dir": "/home/user/.local/share/relay-knowledge/stores/repositories",
    "shard_catalog_active": true,
    "active_shard_count": 3,
    "staged_shard_count": 0,
    "missing_shard_count": 0,
    "runtime_state_paths": ["..."],
    "shards": [
      {
        "repository_id": "repo_abc",
        "state": "active",
        "shard_locator": "repositories/repo_abc",
        "resolved_path": "/home/user/.local/share/relay-knowledge/stores/repositories/repo_abc/code.sqlite",
        "source_scope_count": 12,
        "exists": true,
        "updated_at_ms": 1717632000000
      }
    ]
  }
}
```

---

## 5. Web 操作 API

### 5.1 GET /api/web/graph/canvas

获取图可视化画布数据。

**查询参数**：

| 参数 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `kind` | string | 否 | 画布类型：`knowledge`（默认）、`code`、`mixed` |
| `scope` | string | 否 | 按 source_scope 过滤 |
| `query` | string | 否 | 按标签/ID 搜索过滤 |
| `limit` | integer | 否 | 返回节点上限，默认 250，最大 1000 |

**响应 200**：

```json
{
  "metadata": { "..." },
  "nodes": [
    {
      "id": "node-001",
      "kind": "entity",
      "label": "relay-knowledge",
      "subtitle": "rust graph database project",
      "source_scope": "default",
      "graph_version": 47,
      "weight": 100,
      "status": "active",
      "details": { "type": "project" }
    }
  ],
  "edges": [
    {
      "id": "edge-001",
      "kind": "depends_on",
      "source": "node-001",
      "target": "node-002",
      "label": "depends on",
      "graph_version": 47,
      "confidence_basis_points": 9500,
      "evidence_count": 3
    }
  ],
  "summary": {
    "kind": "knowledge",
    "node_count": 234,
    "edge_count": 567,
    "truncated": false,
    "available_kinds": ["entity", "concept", "fact"]
  }
}
```

### 5.2 POST /api/web/operations/execute

统一操作执行端点，通过 `operation` 字段分发到不同业务逻辑。

**请求体**：

```json
{
  "snapshot": {
    "name": "retrieve-context",
    "command": "检索上下文",
    "payload": {
      "operation": "retrieve.context",
      "query": "knowledge graph architecture",
      "freshness": "allow-stale",
      "limit": 10
    }
  }
}
```

**支持的 operation 值**：

| operation | 说明 | payload 必填字段 |
| --- | --- | --- |
| `retrieve.context` | 混合检索图谱上下文 | `query`, `freshness`, `limit`；可选 `source_scope` |
| `graph.ingest` | 图谱摄取 | `source_scope`, `content`；可选 `entity_labels` |
| `graph.inspect` | 图谱检查 | 可选 `source_scope` |
| `index.refresh` | 刷新派生索引 | `kinds`（字符串数组，如 `["bm25","semantic"]`） |
| `files.index` | 索引本地文件 | 可选 `source_scope`, `roots` |
| `files.query` | 查询索引文件 | `query`, `limit`；可选 `source_scope`, `root_id`, `freshness` |
| `worker.status` | worker 状态 | 可选 `kind` |
| `worker.run-once` | 单次执行 worker | 可选 `kind` |
| `proposal.list` | 列出提案 | `limit`；可选 `state` |
| `proposal.show` | 查看提案详情 | `proposal_id` |
| `proposal.accept` | 接受提案 | `proposal_id`, `actor`；可选 `reason` |
| `proposal.reject` | 拒绝提案 | `proposal_id`, `actor`；可选 `reason` |
| `proposal.supersede` | 废弃提案 | `proposal_id`, `actor`；可选 `reason` |
| `audit.query` | 查询审计记录 | `limit`；可选 `filter_operation` |
| `code.repo.register` | 注册代码仓库 | `root_path`；可选 `alias`, `path_filters`, `language_filters` |
| `code.repo.index` | 全量索引 | `alias`；可选 `ref`, `path_filters`, `language_filters` |
| `code.repo.update` | 增量索引 | `alias`, `base_ref`, `head_ref` |
| `code.repo.query` | 查询代码仓库 | `alias`, `query`, `kind`, `freshness`, `limit` |
| `code.repo.feature_flags` | 特性标志查询 | `alias`, `freshness`, `limit`；可选 `query` |
| `code.repo.impact` | 变更影响分析 | `alias`, `base_ref`, `head_ref`, `limit` |
| `code.repo.software` | 软件全局投影 | `alias`, `kind`, `freshness`, `limit` |
| `code.repo.status` | 仓库索引状态 | `alias` |
| `code.repo_set.create` | 创建仓库集 | `set_alias`；可选 `description`, `default_ref_policy_json` |
| `code.repo_set.add` | 添加仓库成员 | `set_alias`, `repository_alias`, `ref`；可选 `path_filters`, `language_filters`, `priority` |
| `code.repo_set.remove` | 移除仓库成员 | `set_alias`, `repository_alias` |
| `code.repo_set.query` | 跨仓库查询 | `set_alias`, `query`, `kind`, `freshness`, `limit` |
| `code.repo_set.status` | 仓库集状态 | `set_alias` |
| `code.repo_set.refresh` | 刷新仓库集索引 | `set_alias`；可选 `async` |
| `service.doctor` | 服务诊断 | 无 |
| `service.run.streamable_http` | 服务状态 | 无 |
| `provider.embedding.probe` | 嵌入提供者探测 | 无 |

**freshness 枚举值**：`allow-stale`、`wait-until-fresh`、`graph-only`

**code query kind 枚举值**：`hybrid`、`symbol`、`definition`、`references`、`callers`、`callees`、`imports`、`sbom`

**index kind 枚举值**：`bm25`、`semantic`、`vector`

**software kind 枚举值**：`dependencies`、`sdks`、`files`、`topics`、`relationships`、`build`、`iac`、`design`、`all`

**响应 200**：

```json
{
  "metadata": { "..." },
  "operation": "retrieve.context",
  "name": "retrieve-context",
  "command": "检索上下文",
  "result": {
    "metadata": { "..." },
    "context_pack": { "..." },
    "retrieval_mode": "hybrid",
    "freshness": "allow-stale",
    "results": [{ "..." }],
    "fusion": { "..." },
    "rerank": { "..." },
    "backend_statuses": [],
    "truncated": false,
    "budget_used": { "..." },
    "indexes": [{ "kind": "bm25", "stale": false }],
    "index_cursors": [],
    "index_refresh": { "..." }
  }
}
```

### curl 示例

```bash
# 混合检索
curl -s http://localhost:8080/api/web/operations/execute \
  -H "Content-Type: application/json" \
  -d '{
    "snapshot": {
      "name": "retrieve",
      "command": "检索",
      "payload": {
        "operation": "retrieve.context",
        "query": "graph database",
        "freshness": "allow-stale",
        "limit": 5
      }
    }
  }' | jq .

# 仓库状态
curl -s http://localhost:8080/api/web/operations/execute \
  -H "Content-Type: application/json" \
  -d '{
    "snapshot": {
      "name": "repo-status",
      "command": "仓库状态",
      "payload": {
        "operation": "code.repo.status",
        "alias": "my-project"
      }
    }
  }' | jq .
```

---

## 6. 代码仓库 API

代码仓库 API 路径模板为 `/api/v1/code/repositories/{alias}/*`，alias 是仓库注册时指定的别名。

### 请求头

代码仓库 API 支持以下可选 HTTP 头：

| 头名称 | 说明 |
| --- | --- |
| `X-Relay-Request-Id` | 自定义请求 ID |
| `X-Relay-Trace-Id` | 自定义追踪 ID |

### 6.1 POST /api/v1/code/repositories/{alias}/index

启动全量索引任务。

**请求体**：

```json
{
  "repository": "my-project",
  "mode": "Full",
  "freshness_policy": "AllowStale"
}
```

**约束**：`repository` 必须与路径 `{alias}` 一致；mode 仅接受 `Full`。

**响应 200**：

```json
{
  "metadata": { "..." },
  "scope": {
    "scope_id": "scope-abc",
    "repository_id": "repo_xyz",
    "alias": "my-project",
    "requested_ref": "HEAD",
    "resolved_commit_sha": "abc123def456",
    "tree_hash": "hash123",
    "path_filters": [],
    "language_filters": [],
    "index_versions": ["code:scope-abc:hash123"],
    "stale": true
  },
  "status": { "repository_id": "repo_xyz", "stale": true, "..." },
  "task": {
    "task_id": "task-001",
    "state": "queued",
    "repository_id": "repo_xyz",
    "alias": "my-project",
    "mode": "Full",
    "..."
  }
}
```

### 6.2 POST /api/v1/code/repositories/{alias}/scope/preview

预览索引范围（不执行索引）。

**请求体**：与 index 端点同结构。

**响应 200**：

```json
{
  "metadata": { "..." },
  "scope": { "scope_id": "scope-abc", "..." },
  "preview": {
    "total_files": 15234,
    "total_bytes": 123456789,
    "language_counts": { "Rust": 892, "C": 234, "TypeScript": 156 },
    "..."
  }
}
```

### 6.3 POST /api/v1/code/repositories/{alias}/query

查询代码仓库。

**请求体**：

```json
{
  "query": "handle_request",
  "repository": "my-project",
  "code_query_kind": "Hybrid",
  "limit": 10,
  "freshness_policy": "AllowStale"
}
```

`code_query_kind` 枚举：`Hybrid`、`Symbol`、`Definition`、`References`、`Callers`、`Callees`、`Imports`、`Sbom`

**响应 200**：

```json
{
  "metadata": { "..." },
  "scope": { "scope_id": "scope-abc", "..." },
  "freshness": { "state": "fresh", "..." },
  "request": { "query": "handle_request", "..." },
  "results": [
    {
      "hit_id": "hit-001",
      "symbol_name": "handle_request",
      "file_path": "src/server/mod.rs",
      "range": { "start_line": 42, "end_line": 67 },
      "score": 0.95,
      "..."
    }
  ],
  "degraded_reason": null
}
```

### 6.4 POST /api/v1/code/repositories/{alias}/feature-flags

查询代码仓库特性标志引用。

**请求体**：

```json
{
  "repository": "my-project",
  "limit": 20,
  "freshness_policy": "AllowStale"
}
```

**响应 200**：

```json
{
  "metadata": { "..." },
  "scope": { "..." },
  "freshness": { "..." },
  "request": { "..." },
  "flags": [
    {
      "flag_name": "FEATURE_NEW_PARSER",
      "file_paths": ["src/main.rs", "src/parser/mod.rs"],
      "..."
    }
  ]
}
```

### 6.5 POST /api/v1/code/repositories/{alias}/impact

变更影响分析。

**请求体**：

```json
{
  "repository": "my-project",
  "base_ref": "main",
  "head_ref": "feature/new-parser",
  "limit": 20
}
```

**响应 200**：

```json
{
  "metadata": { "..." },
  "scope": { "..." },
  "request": { "base_ref": "main", "head_ref": "feature/new-parser" },
  "path_groups": {
    "changed_files": ["src/parser/mod.rs"],
    "affected_symbols": [{ "..." }],
    "..."
  },
  "results": [{ "..." }]
}
```

### 6.6 GET /api/v1/code/repositories/{alias}/report

获取仓库索引报告。

**查询参数**：无

**响应 200**：

```json
{
  "metadata": { "..." },
  "scope": { "scope_id": "scope-abc", "..." },
  "report": {
    "annotation_counts": { "deprecated": 12, "todo": 45 },
    "language_stats": { "Rust": 892 },
    "..."
  }
}
```

### 6.7 POST /api/v1/code/repositories/{alias}/software

软件全局模型投影，按指定 kind 返回组件、依赖、SDK 等。

**请求体**：

```json
{
  "repository": "my-project",
  "kind": "Dependencies",
  "freshness_policy": "AllowStale",
  "limit": 50
}
```

`kind` 枚举：`Dependencies`、`Sdks`、`Files`、`Topics`、`Relationships`、`Build`、`Iac`、`Design`、`All`

**响应 200**：

```json
{
  "metadata": { "..." },
  "scope": { "..." },
  "request": { "kind": "Dependencies", "..." },
  "status": { "..." },
  "components": [],
  "dependency_usages": [
    { "package_name": "serde", "version": "1.0", "..." }
  ],
  "sdk_usages": [],
  "files": [],
  "topics": [],
  "relationships": [],
  "build_targets": [],
  "iac_resources": [],
  "design_elements": []
}
```

### 6.8 GET /api/v1/code/repositories/{alias}/status

获取仓库索引状态。

**查询参数**：

| 参数 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `ref` | string | 否 | 指定 Git ref，默认 `HEAD` |

**响应 200**：

```json
{
  "metadata": { "..." },
  "status": {
    "repository_id": "repo_xyz",
    "alias": "my-project",
    "root_path": "/path/to/repo",
    "stale": false,
    "last_indexed_commit": "abc123def456",
    "last_indexed_scope_id": "scope-abc",
    "tree_hash": "hash123",
    "path_filters": [],
    "language_filters": [],
    "..."
  },
  "active_task": null,
  "checkpoint": { "..." },
  "retention": { "..." }
}
```

### curl 示例

```bash
# 注册代码仓库 (通过 Web operations)
curl -s http://localhost:8080/api/web/operations/execute \
  -H "Content-Type: application/json" \
  -d '{
    "snapshot": {
      "name": "register",
      "command": "注册仓库",
      "payload": {
        "operation": "code.repo.register",
        "root_path": "/home/user/my-project",
        "alias": "my-project"
      }
    }
  }' | jq .

# 代码仓库索引
curl -s http://localhost:8080/api/v1/code/repositories/my-project/index \
  -H "Content-Type: application/json" \
  -H "X-Relay-Request-Id: my-req-001" \
  -d '{"repository": "my-project", "mode": "Full", "freshness_policy": "AllowStale"}' | jq .

# 代码仓库查询
curl -s http://localhost:8080/api/v1/code/repositories/my-project/query \
  -H "Content-Type: application/json" \
  -d '{
    "query": "handle_request",
    "repository": "my-project",
    "code_query_kind": "Hybrid",
    "limit": 10,
    "freshness_policy": "AllowStale"
  }' | jq .

# 仓库状态
curl -s "http://localhost:8080/api/v1/code/repositories/my-project/status?ref=HEAD" | jq .
```

---

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

所有请求必须通过 `mcp-protocol-version` HTTP 头声明协议版本。

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
      "version": "0.1.0"
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
| `relay_code_feature_flags` | 代码特性标志查询 |
| `relay_code_impact` | 代码变更影响分析 |
| `relay_code_repository_set_query` | 跨仓库集查询 |
| `relay_software_query` | 软件全局模型投影 |

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

#### relay_software_query

软件全局投影工具。参数包括 `alias`、`kind`、`freshness`、`limit`。

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

### curl 示例

```bash
# 初始化会话（-v 可查看返回的 mcp-session-id 头）
curl -s -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "mcp-protocol-version: 2025-11-25" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"curl","version":"1.0"}}}' -v

# 列出工具
curl -s http://localhost:8080/mcp -H "Content-Type: application/json" \
  -H "mcp-protocol-version: 2025-11-25" \
  -H "mcp-session-id: <session-id>" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'

# 调用检索工具
curl -s http://localhost:8080/mcp -H "Content-Type: application/json" \
  -H "mcp-protocol-version: 2025-11-25" \
  -H "mcp-session-id: <session-id>" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"relay_retrieve_context","arguments":{"query":"graph database","freshness":"allow-stale","limit":5}}}'

# 终止会话
curl -s -X DELETE http://localhost:8080/mcp \
  -H "mcp-protocol-version: 2025-11-25" \
  -H "mcp-session-id: <session-id>"
```

---

## 8. 模型配置 API

模型配置端点（`/api/web/model/config/*`）用于管理模型提供者 profile 和 fallback 策略，包括 `POST /upload`（上传 profile）、`POST /apply`（应用变更）、`GET /cache`（查看缓存状态）。

---

## 9. 静态资源

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

---

## 10. 通用约定

### 10.1 速率限制

QoS 层在所有端点上生效。超限时返回 `429 Too Many Requests`。可通过 `/api/project/status` 中的 `qos_*` 字段查看当前配置。

### 10.2 请求体大小限制

默认最大请求体为 10 MiB，可通过 `RELAY_KNOWLEDGE_HTTP_MAX_REQUEST_BODY_BYTES` 配置。超限返回 `413 Payload Too Large`。

### 10.3 超时

HTTP 请求默认超时 30 秒，可通过 `RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS` 配置。超时返回 `504 Gateway Timeout`。

### 10.4 优雅关闭

服务收到 SIGTERM 后执行优雅关闭，默认等待 5 秒让进行中的请求完成。超时可通过 `RELAY_KNOWLEDGE_HTTP_GRACEFUL_SHUTDOWN_TIMEOUT_MS` 配置。

### 10.5 Serde 字段命名

所有 JSON 字段使用 `snake_case` 命名（与 Rust serde 默认行为一致）。枚举值在 JSON 中使用 `PascalCase`（代码仓库 API）或 `snake_case`（Web 操作 API），具体取决于各类型的 serde 注解。

---

## 附录 A: 端点速查表

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/project/status` | 项目状态和运行时 |
| GET | `/api/health` | 全面健康检查 |
| GET | `/api/service/status` | 完整服务状态 |
| GET | `/api/v1/control/status` | 运行时诊断（只读） |
| GET | `/api/v1/control/health` | 健康检查（只读） |
| GET | `/api/v1/control/service/status` | 服务状态（只读） |
| GET | `/api/v1/control/storage/topology` | 存储拓扑 |
| GET | `/api/web/graph/canvas` | 图可视化画布 |
| POST | `/api/web/operations/execute` | 统一操作执行 |
| POST | `/api/v1/code/repositories/{alias}/index` | 仓库全量索引 |
| POST | `/api/v1/code/repositories/{alias}/scope/preview` | 索引范围预览 |
| POST | `/api/v1/code/repositories/{alias}/query` | 仓库代码查询 |
| POST | `/api/v1/code/repositories/{alias}/feature-flags` | 特性标志查询 |
| POST | `/api/v1/code/repositories/{alias}/impact` | 变更影响分析 |
| GET | `/api/v1/code/repositories/{alias}/report` | 仓库索引报告 |
| POST | `/api/v1/code/repositories/{alias}/software` | 软件全局投影 |
| GET | `/api/v1/code/repositories/{alias}/status` | 仓库索引状态 |
| POST | `/api/web/model/config/upload` | 模型配置上传 |
| POST | `/api/web/model/config/apply` | 模型配置应用 |
| GET | `/api/web/model/config/cache` | 模型缓存状态 |
| POST | `/mcp` | MCP JSON-RPC |
| DELETE | `/mcp` | MCP 会话终止 |
| GET | `/mcp/metrics` | MCP 指标 |
