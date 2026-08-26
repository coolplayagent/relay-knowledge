# relay-knowledge 控制面与 Web 操作 API

[中文](api-control-web.md) | 英文版尚未提供

> 文档版本: 1.0
> 编制日期: 2026-06-06
> 文档定位：第 23 章 [HTTP API 参考](../23-api-reference.md)的控制面与 Web 操作专题；本页沿用主章迁移前的小节编号，不单独占用章节号。

## 4. 控制面 API

### 4.1 GET /api/project/status

获取项目元信息和运行时状态。

**响应 200（节选字段）**：

```json
{
  "project_name": "relay-knowledge",
  "metadata": { "_omitted": true },
  "runtime": {
    "config_dir": "/home/user/.config/relay-knowledge",
    "data_dir": "/home/user/.local/share/relay-knowledge",
    "storage_topology": "single_sqlite",
    "http_bind": "127.0.0.1:8791",
    "http_request_timeout_ms": 30000,
    "http_graceful_shutdown_timeout_ms": 10000,
    "http_max_request_body_bytes": 1048576,
    "qos_max_connections": 1024,
    "qos_max_in_flight_requests": 256,
    "qos_max_queue_depth": 512,
    "qos_current_connections": 3,
    "qos_current_in_flight_requests": 2,
    "qos_current_queued_requests": 0,
    "qos_admitted_total": 1024,
    "qos_queued_total": 128,
    "qos_rejected_total": 7,
    "qos_timed_out_total": 2,
    "qos_cancelled_total": 1,
    "qos_dropped_total": 0,
    "silent_updates_enabled": true,
    "semantic_backend_mode": "local",
    "vector_backend_mode": "local"
  }
}
```

### 4.2 GET /api/health

全面健康检查，包括存储、图、索引和运行时状态。

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
  "healthy": true,
  "storage": { "topology": "single_sqlite", "active_shard_count": 0, "_omitted": true },
  "graph": { "graph_version": 47, "entity_count": 1234, "relation_count": 5678, "_omitted": true },
  "repository_code_totals": { "repository_count": 3, "indexed_file_count": 15234, "_omitted": true },
  "indexes": [{ "kind": "bm25", "stale": false, "_omitted": true }],
  "index_cursors": [{ "_omitted": true }],
  "index_refresh": { "_omitted": true },
  "file_index": { "_omitted": true },
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
  "metadata": { "_omitted": true },
  "service_name": "relay-knowledge",
  "mode": "enabled",
  "background_enabled": true,
  "silent_updates_enabled": true,
  "service_definition_path": "/home/user/.local/share/relay-knowledge/service/relay-knowledge.service",
  "storage": { "topology": "single_sqlite", "active_shard_count": 0, "_omitted": true },
  "index_refresh": { "_omitted": true },
  "file_index": { "_omitted": true },
  "agent_protocols": {
    "mcp_streamable_http_enabled": true,
    "mcp_endpoint": "/mcp",
    "metrics_endpoint": "/mcp/metrics",
    "_omitted": true
  },
  "operator": { "state": "enabled", "_omitted": true },
  "workers": [{ "kind": "embedding", "backend_state": "fallback", "_omitted": true }],
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
  "audit_sink": { "durable": true, "event_count": 47 },
  "watcher": { "enabled": true, "commit_reconcile_interval_ms": 5000, "_omitted": true }
}
```

### 4.6 GET /api/v1/control/service/status

只读服务状态。**响应 200**：与 `/api/service/status` 同结构。

### 4.7 GET /api/v1/control/storage/topology

存储拓扑诊断，包括 shard catalog 和分区详情。

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
  "storage": {
    "topology": "partitioned_sqlite",
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
  "metadata": { "_omitted": true },
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
| `files.content` | 查询授权本地文件内容片段 | `query`, `limit`；可选 `source_scope`, `root_id`, `freshness` |
| `worker.status` | worker 状态 | 可选 `kind` |
| `worker.run-once` | 单次执行 worker | 可选 `kind` |
| `proposal.list` | 列出提案 | `limit`；可选 `state` |
| `proposal.show` | 查看提案详情 | `proposal_id` |
| `proposal.accept` | 接受提案 | `proposal_id`, `actor`；可选 `reason` |
| `proposal.reject` | 拒绝提案 | `proposal_id`, `actor`；可选 `reason` |
| `proposal.supersede` | 废弃提案 | `proposal_id`, `actor`；可选 `reason` |
| `audit.query` | 查询审计记录 | `limit`；可选 `filter_operation` |
| `code.repo.register` | 注册代码仓库 | `root_path`；可选 `alias`, `path_filters`, `language_filters` |
| `code.repo.list` | 列出至少有一个已完成索引 scope 的仓库 | 无 |
| `code.repo.index` | 全量索引 | `alias`；可选 `ref`, `path_filters`, `language_filters` |
| `code.repo.update` | 增量索引 | `alias`；可选 `base_ref`, `head_ref` |
| `code.repo.query` / `code.repo.context` | 查询代码仓库 / 打包 one-call codegraph context | `alias`, `query`, `kind` 或 context budget, `freshness`, `limit` |
| `code.repo.business` | 读取 authored 业务术语与技术映射 | `alias`, `kind`, `freshness`, `limit`；可选 `query`, `domain`, `ref` |
| `code.repo.view` | 从已索引代码图派生有界代码库理解视图 | `alias`, `kind`；可选 `ref`, `freshness`, `limit`, `changed_paths` |
| `code.repo.feature_flags` | 特性标志查询 | `alias`, `freshness`, `limit`；可选 `query` |
| `code.repo.impact` | 变更影响分析 | `alias`, `base_ref`, `head_ref`, `limit` |
| `code.repo.software` | 软件全局投影 | `alias`, `kind`, `freshness`, `limit` |
| `code.repo.status` | 仓库索引状态 | `alias` |
| `code.repo_set.create` | 创建仓库集 | `set_alias`；可选 `description`, `default_ref_policy_json` |
| `code.repo_set.add` | 添加仓库成员 | `set_alias`, `repository_alias`, `ref`；可选 `path_filters`, `language_filters`, `priority` |
| `code.repo_set.remove` | 移除仓库成员 | `set_alias`, `repository_alias` |
| `code.repo_set.query` | 跨仓库查询 | `set_alias`, `query`, `kind`, `freshness`, `limit` |
| `code.repo_set.status` | 仓库集状态 | `set_alias` |
| `code.repo_set.refresh` | 先进入有界持久队列；默认同步仅在精确 task 可定向 claim 时 drain，否则返回 queued | `set_alias`；可选 `async` |
| `service.doctor` | 服务诊断 | 无 |
| `service.run.streamable_http` | 兼容的服务状态快照；不会从 Web request 启动常驻进程 | 无 |
| `provider.embedding.probe` | 嵌入提供者探测 | 无 |

**freshness 枚举值**：`allow-stale`、`wait-until-fresh`、`graph-only`
**code query kind 枚举值**：`hybrid`、`symbol`、`definition`、`references`、`callers`、`callees`、`imports`、`sbom`
**index kind 枚举值**：`bm25`、`semantic`、`vector`
**software kind 枚举值**：`dependencies`、`sdks`、`files`、`topics`、`relationships`、`build`、`iac`、`design`、`all`
**files.content 契约**：该 operation 只查询已授权 root 内的文本内容 read model。请求字段为 `query`、`limit`，以及可选的 `source_scope`、`root_id`、`freshness`。响应包含 `freshness` 诊断、`truncated`、`duration_ms`、可选 `degraded_reason`，以及 `results[]`。每个命中必须携带 `scope_id`、`root_id`、`path`、`relative_path`、`chunk_id`、`excerpt`、`span`、`fingerprint`、`content_hash`、`graph_version`、`indexed_graph_version`、`freshness_cursor`、`rank`、`score`、`ranking_signals` 和 `fact_candidates`。`content_role` 固定表示非可信来源内容（当前为 `user_source`）；CLI、Web 和 agent adapter 只能把 `excerpt` 当作带 provenance 的引用数据，不能拼接为 system/developer 指令。

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
  "operation": "retrieve.context",
  "name": "retrieve-context",
  "command": "检索上下文",
  "result": {
    "metadata": { "_omitted": true },
    "context_pack": { "_omitted": true },
    "retrieval_mode": "hybrid",
    "freshness": "allow-stale",
    "results": [{ "_omitted": true }],
    "fusion": { "_omitted": true },
    "rerank": { "_omitted": true },
    "backend_statuses": [],
    "truncated": false,
    "budget_used": { "_omitted": true },
    "indexes": [{ "kind": "bm25", "stale": false }],
    "index_cursors": [],
    "index_refresh": { "_omitted": true }
  }
}
```

### 5.3 curl 示例

```bash
# 混合检索
curl -s http://127.0.0.1:8791/api/web/operations/execute \
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
curl -s http://127.0.0.1:8791/api/web/operations/execute \
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

导航：上一专题：[HTTP API 端点速查表](api-http-endpoints.md) | 下一专题：[代码仓库 API](api-code-repositories.md) | 返回：[23. HTTP API 参考](../23-api-reference.md) | 上级：[API 专题索引](README.md)
