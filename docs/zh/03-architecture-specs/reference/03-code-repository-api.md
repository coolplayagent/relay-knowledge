# relay-knowledge 代码仓库 API

[中文](03-code-repository-api.md) | 英文版尚未提供

> 文档版本: 1.0
> 编制日期: 2026-06-06
> 文档定位：第 23 章 [HTTP API 参考](../23-api-reference.md)的代码仓库专题；本页沿用主章迁移前的小节编号，不单独占用章节号。

## 6. 代码仓库 API

代码仓库 API 路径模板为 `/api/v1/code/repositories/{alias}/*`，alias 是仓库注册时指定的别名。

### 6.0 GET /api/v1/code/repositories

列出至少有一个已完成 indexed scope 的仓库。响应包含 `metadata` 与有界的 `repositories` 状态数组；从未完成索引的 registration 不出现在该列表。

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
  "repository": { "repository": "my-project", "ref_selector": "HEAD", "path_filters": [], "language_filters": [] },
  "mode": "full",
  "freshness_policy": "allow_stale"
}
```

**约束**：`repository.repository` 必须与路径 `{alias}` 一致；mode 仅接受 `full` 或 `worktree_overlay`。

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
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
  "status": { "repository_id": "repo_xyz", "stale": true, "_omitted": true },
  "task": {
    "task_id": "task-001",
    "state": "queued",
    "repository_id": "repo_xyz",
    "alias": "my-project",
    "mode": "full",
    "_omitted": true
  }
}
```

### 6.1.1 POST /api/v1/code/repositories/{alias}/update

把 Git 仓库的一个不可变 base-to-head delta 提交为 durable Incremental 索引任务。该 route 不在 HTTP request executor 中直接写图谱。

**请求体**：

```json
{ "repository": "my-project", "base_ref": "abc123", "head_ref": "def456" }
```

`base_ref` 和 `head_ref` 都可省略。省略 `base_ref` 时使用最近一次成功发布的 clean Git snapshot；active snapshot 是 worktree overlay 时会解包其 clean base。省略 `head_ref` 时使用 `HEAD`。服务在入队前把两者解析为不可变 commit 并固定 target tree。没有已发布 clean base 时返回 invalid argument，调用方必须先执行 full index。单次 delta 在应用注册 path filter 前按整个 commit pair 计算，最多 512 个 changed path；超过上限必须 full index。

**响应 200（排队态）**：

```json
{
  "metadata": { "...": "..." },
  "scope": { "scope_id": "scope-def", "resolved_commit_sha": "def456", "tree_hash": "tree456", "stale": true },
  "status": { "repository_id": "repo_xyz", "stale": true, "...": "..." },
  "task": {
    "task_id": "task-002",
    "state": "queued",
    "mode": { "incremental": { "base_ref": "abc123", "head_ref": "def456" } },
    "...": "..."
  }
}
```

本地 CLI 可能在一个有界 drain 后返回完成态；此时 response 的 `summary.base_resolved_commit_sha` 是实际 base，`summary.resolved_commit_sha` 是实际 head。远端调用可能保持 queued，由常驻 worker pool 消费；通过 `GET .../status` 观察 `active_task`、checkpoint、freshness 与 retention。Durable queue 每仓库最多接纳 32 个、全局最多 256 个 unfinished task；容量耗尽返回可重试的 `qos_rejected`（HTTP 429）。

成功发布会触发有界 scope/task history retention。旧 scope 会先原子标为 `retiring` 并退出查询，然后由 durable GC job 每个 maintenance transaction 推进一个 scope-GC phase，该 phase 在受影响的应用表之间合计最多删除 512 个物理行；`retention.maintenance_pending` 与 `retention.retiring_jobs[]` 暴露 phase、累计删除行数和最近错误。`retained_scopes` 与 `prunable_scopes` 各最多返回 64 项；`scope_listing_truncated=true` 表示这些数组和显示计数是有界诊断投影与可观察 lower bound 而非完整保护集合，调用方不得据此自行删除 scope，partitioned shard maintenance 也会在 control-plane pin 投影被截断时暂停。该清理只覆盖 code scope 的 facts、FTS/search row、software projection 与状态数据；不表示 generic Knowledge Graph 或独立 semantic/vector generation 已与该 code scope 原子切代。

### 6.2 POST /api/v1/code/repositories/{alias}/scope/preview

预览索引范围（不执行索引）。

**请求体**：与 index 端点同结构。

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
  "scope": { "scope_id": "scope-abc", "_omitted": true },
  "preview": {
    "selected_file_count": 15234,
    "selected_byte_count": 123456789,
    "language_distribution": [{ "language_id": "rust", "file_count": 892, "byte_count": 123456 }],
    "_omitted": true
  }
}
```

### 6.3 POST /api/v1/code/repositories/{alias}/query

查询代码仓库。

**请求体**：

```json
{
  "query": "handle_request",
  "repository": { "repository": "my-project", "ref_selector": "HEAD", "path_filters": [], "language_filters": [] },
  "code_query_kind": "hybrid",
  "limit": 10,
  "freshness_policy": "allow_stale"
}
```

### 6.3.1 POST /api/v1/code/repositories/{alias}/graph

返回一个 snapshot-bound、只读且有界的 OKF repository graph neighborhood。请求包含 `repository`、位于显式 path filter 内的 `focus_path`、`depth`（1–2）、`node_limit`（1–100）与 `edge_limit`（1–200）；当前只接受 Markdown scope，不读取 live worktree，也不触发索引。

### 6.3.2 POST /api/v1/code/repositories/{alias}/context

构建一次有界 codegraph context pack。请求包含 `repository`、`query`、`limit`（1–20）、`freshness_policy`、`max_context_bytes`（1024–262144）、`include_code` 与 `exclude_generated`；响应返回 `business_context`、entry points、related symbols、graph paths、impact hints、code excerpts、freshness 与实际 budget。业务术语和 mapping seed 与代码结果严格绑定同一 resolved commit/source scope。

### 6.3.3 POST /api/v1/code/repositories/{alias}/business

读取索引阶段写入的 authored business projection，不在请求路径读取 YAML。请求为共享的 `BusinessKnowledgeQueryRequest`：`repository` 选择器、可选 `domain`/`query`、`kind`（`terms`、`mappings` 或 `all`）、`freshness_policy` 与 `limit`（1–500）。响应返回 resolution（包含 `ambiguous`）、domain、canonical term、definitions、aliases、semantics、conflicts、technical mappings、`resolution_state`、`target_hint`、evidence 和 repository scope。

`code_query_kind` 枚举：`hybrid`、`symbol`、`definition`、`references`、`callers`、`callees`、`imports`、`sbom`、`impact`

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
  "scope": { "scope_id": "scope-abc", "_omitted": true },
  "freshness": { "state": "fresh", "_omitted": true },
  "request": { "query": "handle_request", "_omitted": true },
  "results": [
    {
      "symbol_snapshot_id": "symbol-001",
      "excerpt": "pub fn handle_request(...) {...}",
      "path": "src/server/mod.rs",
      "line_range": { "start": 42, "end": 67 },
      "score": 0.95,
      "_omitted": true
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
  "repository": { "repository": "my-project", "ref_selector": "HEAD", "path_filters": [], "language_filters": [] },
  "limit": 20,
  "freshness_policy": "allow_stale"
}
```

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
  "scope": { "_omitted": true },
  "freshness": { "_omitted": true },
  "request": { "_omitted": true },
  "flags": [
    {
      "name": "FEATURE_NEW_PARSER",
      "usages": [{ "path": "src/main.rs", "_omitted": true }],
      "_omitted": true
    }
  ]
}
```

### 6.5 POST /api/v1/code/repositories/{alias}/impact

变更影响分析。

**请求体**：

```json
{
  "repository": { "repository": "my-project", "ref_selector": "HEAD", "path_filters": [], "language_filters": [] },
  "base_ref": "main",
  "head_ref": "feature/new-parser",
  "limit": 20
}
```

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
  "scope": { "_omitted": true },
  "request": { "base_ref": "main", "head_ref": "feature/new-parser" },
  "path_groups": {
    "in_scope_changed_paths": ["src/parser/mod.rs"],
    "out_of_scope_changed_paths": [],
    "_omitted": true
  },
  "results": [{ "_omitted": true }]
}
```

### 6.6 GET /api/v1/code/repositories/{alias}/report

获取仓库索引报告。

**查询参数**：无

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
  "scope": { "scope_id": "scope-abc", "_omitted": true },
  "report": {
    "annotation_counts": { "deprecated": 12, "todo": 45 },
    "language_stats": { "Rust": 892 },
    "_omitted": true
  }
}
```

### 6.7 POST /api/v1/code/repositories/{alias}/software

软件全域模型查询，按指定 kind 返回兼容投影、ontology entity、provenance statement 或冲突诊断。CLI、Web 和 MCP 复用同一个 application service。

**请求体**：

```json
{
  "repository": { "repository": "my-project", "ref_selector": "HEAD", "path_filters": [], "language_filters": [] },
  "kind": "dependencies",
  "freshness_policy": "allow_stale",
  "limit": 50
}
```

`kind` 枚举：`dependencies`、`sdks`、`files`、`topics`、`relationships`、`build`、`iac`、`design`、`systems`、`apis`、`resources`、`tests`、`deployments`、`releases`、`statements`、`conflicts`、`all`

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
  "scope": { "_omitted": true },
  "request": { "kind": "dependencies", "_omitted": true },
  "status": {
    "ontology_version": "1.0.0",
    "projection_schema_version": 7,
    "source_coverage": {
      "source_kinds": ["manifest"],
      "source_path_count": 1,
      "evidence_ref_count": 1
    },
    "completeness_basis_points": 10000,
    "freshness": "fresh",
    "conflict_count": 0,
    "_omitted": true
  },
  "components": [],
  "dependency_usages": [
    { "package_name": "serde", "module": "serde", "_omitted": true }
  ],
  "sdk_usages": [],
  "files": [],
  "topics": [],
  "relationships": [],
  "build_targets": [],
  "iac_resources": [],
  "design_elements": [],
  "entities": [],
  "statements": [],
  "diagnostics": []
}
```

兼容 kind 保留既有数组；类型化 kind 使用 `entities`，`statements` 使用 `statements`，`conflicts` 可同时返回非 active statement 和 `diagnostics`。稳定 `entity_key` 不包含 commit/source scope，`occurrence_id` 绑定本次 snapshot evidence。普通 README heading 不会成为 software system，CI job 不会成为 IaC resource，Dockerfile 归入 build definition。

### 6.7.1 POST /api/v1/code/repositories/{alias}/software/export/{profile}

从同一 snapshot-bound statement 视图生成标准导出。`profile` 只接受 `spdx-3`、`cyclonedx-1.7` 或 `prov-o`。请求体与 6.7 相同，但服务会忽略 `kind` 并读取 `statements` 视图；`limit` 仍为 1–500 的有界单切片限制。

响应 envelope 包含 `metadata`、`scope`、上述 `status`、`profile`、`media_type` 和 `document`。`document` 分别是 SPDX 3.0.1 JSON-LD、CycloneDX 1.7 JSON 或 PROV-O JSON-LD；Web 保留 envelope，CLI `repo software export` 只输出其中的原始 `document`。该端点不读取 live worktree、不调用云 API，也不根据目标标准补造无证据字段。

### 6.8 POST /api/v1/code/repositories/{alias}/views

返回从版本化代码图事实派生的代码库理解视图。请求、响应和预算字段见[代码库视图 API 专题](04-codebase-view-api.md)。

### 6.9 GET /api/v1/code/repositories/{alias}/status

获取仓库索引状态。

**查询参数**：

| 参数 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `ref` | string | 否 | 指定 Git ref，默认 `HEAD` |

**响应 200**：

```json
{
  "metadata": { "_omitted": true },
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
    "_omitted": true
  },
  "active_task": null,
  "checkpoint": { "_omitted": true },
  "retention": { "retained_scope_count": 1, "prunable_scope_count": 0, "pruned_scope_count": 0, "scope_listing_truncated": false, "maintenance_pending": false, "retiring_job_count": 0, "retained_scopes": ["scope-abc"], "prunable_scopes": [], "pruned_scopes": [], "retiring_jobs": [] }
}
```

---

导航：上一专题：[控制面与 Web 操作 API](02-control-and-web-api.md) | 下一专题：[代码库视图 API](04-codebase-view-api.md) | 返回：[23. HTTP API 参考](../23-api-reference.md) | 上级：[API 专题索引](README.md)
