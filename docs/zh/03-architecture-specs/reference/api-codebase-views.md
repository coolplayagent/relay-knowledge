# relay-knowledge 代码库视图 API

[中文](api-codebase-views.md) | 英文版尚未提供

> 文档版本: 1.0
> 编制日期: 2026-06-15
> 适用范围: `/api/v1/code/repositories/{alias}/views`

> 文档定位: [代码仓库 API](api-code-repositories.md)的视图字段专题，也是第 23 章 [HTTP API 参考](../23-api-reference.md)的一部分；本页不单独占用章节号。

## 1. POST /api/v1/code/repositories/{alias}/views

返回从版本化代码图事实派生的代码库理解视图；自然语言叙述只引用 `evidence`，不能写回事实表。

**请求体**：

```json
{
  "repository": { "repository": "my-project", "ref_selector": "HEAD", "path_filters": [], "language_filters": [] },
  "view_kind": "dependency_tour",
  "freshness_policy": "allow_stale",
  "limit": 20,
  "changed_paths": []
}
```

`view_kind` 枚举：`architecture_layers`、`business_domains`、`dependency_tour`、`process_flow`、`affected_scope`。`repository.path_filters`、`repository.language_filters` 可缩小已索引范围；`affected_scope` 必须提供 `changed_paths`。

**响应 200**：

```json
{
  "metadata": { "..." },
  "scope": { "..." },
  "freshness": { "state": "fresh", "..." },
  "request": { "view_kind": "dependency_tour", "..." },
  "graph_version": 42,
  "nodes": [{ "id": "module:api", "..." }],
  "edges": [{ "edge_kind": "depends_on", "..." }],
  "sections": [{ "evidence_ids": ["evidence:1"], "..." }],
  "evidence": [{ "path": "Cargo.toml", "..." }],
  "budget": {
    "requested_limit": 20,
    "snapshot_row_limit": 400,
    "snapshot_truncated": false,
    "nodes_truncated": false,
    "edges_truncated": false,
    "sections_truncated": false,
    "evidence_truncated": false
  }
}
```

## 2. 预算字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `requested_limit` | usize | 调用方请求的节点、边、section 返回上限 |
| `snapshot_row_limit` | usize | 构建视图前读取代码图快照时使用的每类行上限 |
| `snapshot_truncated` | bool | 快照读取是否发现超过 `snapshot_row_limit` 的候选事实 |
| `nodes_truncated` | bool | 返回节点是否因预算被截断 |
| `edges_truncated` | bool | 返回边是否因预算被截断 |
| `sections_truncated` | bool | 返回 section 是否因预算被截断 |
| `evidence_truncated` | bool | 返回 evidence 是否因预算被截断 |

---

导航：上一专题：[代码仓库 API](api-code-repositories.md) | 下一专题：[MCP Streamable HTTP API](api-mcp-streamable-http.md) | 返回：[23. HTTP API 参考](../23-api-reference.md) | 上级：[API 专题索引](README.md)
