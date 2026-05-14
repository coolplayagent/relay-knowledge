# Source Scope 与多模态摄取规格

[中文](../../zh/03-architecture-specs/source-scope-and-multimodal-ingestion.md) | [英文](../../en/03-architecture-specs/source-scope-and-multimodal-ingestion.md)

> 文档版本: 1.0
> 编制日期: 2026-05-11
> 适用范围: Git 代码仓库检索隔离、rebase 场景、文档文字/图片摄取和多模态索引
> 默认路线: Git 快照作用域优先，文档 evidence 多模态统一建模

## 1. 设计结论

`relay-knowledge` 的检索精度不能只依赖查询文本和排序算法。代码仓库、分支、rebase、文档集合和媒体类型都会改变“应该被搜索的事实集合”。因此 v1 设计必须先固定两个边界:

1. **代码仓库检索按快照隔离**: 查询绑定到 `repository_id + resolved_commit_sha + tree_hash`。branch、tag、worktree、PR 和 rebase 只是解析到快照或变更集的输入，不作为事实真源。
2. **文档摄取按多模态 evidence 统一建模**: 文本、图片、OCR 文本、图注、表格和页面区域都进入同一套来源、hash、抽取器版本和图版本机制。

这两个边界要进入 storage、indexing、retrieval 和 API contract。否则同一符号在不同分支、同一图片的 OCR 与视觉描述、同一文档的多个版本都会互相污染检索结果。

## 2. Source Scope 模型

`SourceScope` 表示一次查询或索引可见的数据边界。所有 ingest、graph mutation、index refresh 和 retrieval request 都必须能追溯到一个或多个 scope。

核心字段:

| 字段 | 含义 |
| --- | --- |
| `scope_id` | 稳定 ID，建议由 scope kind、source ID、版本指纹和过滤条件 hash 得到 |
| `scope_kind` | `git_snapshot`、`git_changeset`、`document_collection`、`global` |
| `source_id` | 仓库、文档集合或外部数据源 ID |
| `resolved_version` | 对 Git 是 commit SHA；对文档是 collection version 或 source revision |
| `content_fingerprint` | 对 Git 是 tree hash；对文档集合是 manifest hash |
| `path_filters` | 可选路径前缀、glob 或文档子集过滤 |
| `metadata` | branch、tag、worktree、base/head ref、rebase session 等审计信息 |

规则:

- `global` scope 只能显式请求，不能作为代码仓库搜索默认值。
- 用户传入 branch 名称时，adapter 必须先解析为 commit SHA 和 tree hash，再构造 `git_snapshot` scope。
- API 响应必须返回实际使用的 `scope_id` 和 `resolved_version`，避免用户误以为结果来自仍在移动的 branch 名称。
- 索引状态、检索结果、证据和 graph mutations 都要记录 scope，至少记录到可过滤、可审计的粒度。

## 3. Git 仓库隔离

### 3.1 快照优先

Git 默认 scope 为:

```text
GitSnapshotScope {
  repository_id,
  resolved_commit_sha,
  tree_hash,
  path_filters,
}
```

branch、tag、HEAD、worktree 和 remote ref 只参与解析:

```text
user selector -> git rev-parse -> commit sha -> tree hash -> scope_id
```

同一 branch 在 rebase 前后会解析到不同 `resolved_commit_sha` 和 `tree_hash`，因此必须产生不同 scope。旧 scope 可保留用于审计、diff、回归比较和历史回答，但不能参与默认 latest 搜索。

### 3.2 符号和事实 ID

代码符号需要区分“逻辑身份”和“快照实例”:

- `canonical_symbol_id`: 可选的跨快照逻辑符号，例如 `repo://core/src/relay_knowledge/lib.rs::GraphStore`。
- `symbol_snapshot_id`: 必须存在的快照实例，例如 hash(`repository_id`, `tree_hash`, `qualified_name`, `blob_hash`)。

图事实、边、evidence 和索引记录默认绑定到 `symbol_snapshot_id`。跨分支或跨 rebase 合并同名符号只能通过显式关系表达，例如 `SAME_LOGICAL_SYMBOL_AS`，不能靠裸 `qualified_name` 自动合并。

### 3.3 Rebase 和变更集

rebase 场景需要同时支持两类查询:

| 查询类型 | Scope | 用途 |
| --- | --- | --- |
| 快照查询 | `git_snapshot(head_after_rebase)` | 默认搜索和问答，结果只来自 rebase 后 head |
| 变更集查询 | `git_changeset(base, head)` | code review、影响分析、只看 diff 和受影响邻域 |

`git_changeset` 不是事实真源。它是从两个或多个快照派生出的检索视图，适合提升 changed files、affected symbols、test impact 和依赖邻域的权重。返回结果仍要标记其所属快照。

### 3.4 索引分区

BM25、semantic、vector、summary 和 community 索引都必须按 scope 分区或按 scope 可过滤:

```text
index partition key =
  index_kind + source_id + scope_id + modality + model_or_strategy
```

实现要求:

- 查询默认带 `scope_id IN (...)` 过滤，禁止无 scope 的代码仓库搜索。
- `index_versions` 未来需要记录 `scope_id`、`source_id`、`content_fingerprint` 和 `indexed_graph_version`。
- 相同 tree hash 可以复用索引分区，即使来自不同 branch 名称。
- rebase 后的新 scope 触发局部或全量索引刷新；旧索引分区保留 TTL 或手动清理策略。
- stale 判断必须在 scope 内计算，不能用仓库全局 index version 代表某个快照已经新鲜。

## 4. 多模态文档摄取

### 4.1 Evidence 统一模型

文档不是只有纯文本。PDF、网页、Markdown、Office 文档和知识库页面都可能包含正文、图片、图表、表格、截图和扫描页。v1 evidence 需要能表达这些媒体单元:

| Evidence 类型 | 示例 | 最小元数据 |
| --- | --- | --- |
| `text_span` | 段落、标题、列表项 | source URI、span、text hash、语言 |
| `image_asset` | 图片、截图、扫描页、图表 | media hash、MIME、尺寸、页码/区域 |
| `ocr_text` | 图片或扫描页识别出的文字 | parent image、OCR 引擎、置信度、文本 span |
| `caption` | 图片标题、alt text、附近说明 | parent image、来源位置、text hash |
| `table` | 表格结构和单元格文本 | 行列范围、结构 hash、抽取器版本 |
| `layout_region` | 页面区域、栏、图文块 | 坐标、页码、parent document |

所有 evidence 都必须记录:

- `source_uri`
- `source_hash` 或 `media_hash`
- `extractor`
- `extractor_version`
- `observed_at`
- `scope_id`
- 可选 `parent_evidence_id`

`parent_evidence_id` 用于表达图片 -> OCR 文本、页面 -> 区域、区域 -> 图注等层级关系。

### 4.2 抽取流水线

推荐流水线:

```text
document source
  -> manifest and source scope resolution
  -> text/layout extraction
  -> image asset extraction
  -> OCR and caption extraction
  -> optional vision description
  -> candidate entities/claims/evidence links
  -> graph mutation batch
  -> scoped index refresh
```

要求:

- 文档集合先生成 manifest hash，再形成 `document_collection` scope。
- 图片抽取失败不能阻塞文本摄取；失败写入 extractor diagnostics 和降级状态。
- OCR、caption、vision description 是派生 evidence，不能覆盖原始图片 evidence。
- 文档重新摄取时，如果 source hash 和 media hash 未变化，应复用 evidence 和索引记录。

### 4.3 多模态索引

检索层从 v1 起预留这些 modality:

| Modality | 检索方式 | 用途 |
| --- | --- | --- |
| `text` | BM25 + text embedding | 正文、标题、图注、OCR 文本 |
| `image` | image embedding | 图片、截图、图表语义相似 |
| `layout` | layout-aware rerank | PDF 页面区域、表格和图文邻近关系 |
| `table` | structured extraction + text search | 单元格事实、指标、配置矩阵 |

融合策略:

- 默认仍按三层检索: BM25、semantic、vector。
- 图片 evidence 可以通过 image embedding 召回，也可以通过 OCR/caption 文本召回。
- RRF 融合后，organizer 必须保留 modality、parent evidence、source region 和版本信息。
- 视觉模型或 OCR 不可用时，返回降级状态，但 text/BM25 检索继续可用。

## 5. API 和存储落点

当前 Rust API 的 evidence ingest 已支持可选 `extraction` 元数据，字段覆盖
`modality`、`source_uri`、`source_hash`、`media_hash`、`extractor`、
`extractor_version`、`observed_at`、`parent_evidence_id`、`layout_region`、
`embedding_model`、`embedding_dimension` 和 extractor diagnostic。未提供时默认写入
`text_span` evidence。

检索请求后续可继续扩展为显式 modality selector:

```rust
pub struct RetrievalRequest {
    pub query: String,
    pub source_scope: SourceScopeSelector,
    pub modalities: Vec<Modality>,
    pub freshness_policy: FreshnessPolicy,
}
```

响应 metadata 应扩展:

```rust
pub struct RetrievalMetadata {
    pub graph_version: u64,
    pub scope_id: String,
    pub resolved_version: String,
    pub index_versions: Vec<ScopedIndexVersion>,
    pub stale: bool,
}
```

存储层未来需要支持:

- `source_scopes`: 记录 scope kind、source ID、resolved version、fingerprint 和 metadata。
- `evidence.scope_id`: 所有 evidence 可按 scope 过滤；当前实现使用 `evidence.source_scope` 并在 BM25、semantic、vector、path、temporal 和 community retrieval 中做 scope post-filter。
- `graph_mutations.scope_id`: 索引器可以局部消费和刷新。
- `index_versions.scope_id`: stale 判断按 scope 和 modality 计算。
- `embedding_records.modality`: 区分 text/image/layout/table embedding；当前实现使用 `graph_semantic_documents` 和 `graph_vector_documents` 存储 modality、parent evidence、model、dimension、source hash 和 graph version。

当前 parent grouping 规则:

- `ocr_text`、`caption`、`image_embedding` 和 `layout_region` 必须引用
  `parent_evidence_id`。
- retrieval 使用 parent evidence id 作为 RRF merge key，避免同一图片的 OCR 和
  caption 重复出现在 context pack 中。
- `image_asset` 必须提供 `media_hash` 或 `source_hash`，便于后续 extractor 和
  embedding worker 做幂等刷新。
- 后台或 maintenance extractor 通过 `commit_multimodal_extraction` 提交派生
  evidence；该边界校验 parent evidence、派生 modality 和 extractor identity，
  然后复用普通 ingest、bounded index refresh 和 cursor metadata 路径。查询热路径
  只读取已提交 evidence/read model，不运行 OCR、caption、table/layout 或 vision
  抽取。

## 6. 可观测性

日志、trace 和 metrics 至少包含:

- `scope_id`
- `scope_kind`
- `source_id`
- `resolved_version`
- `content_fingerprint`
- `modality`
- `index_kind`
- `indexed_graph_version`
- `stale`
- `degraded_reason`

关键指标:

| 指标 | 含义 |
| --- | --- |
| `relay_scope_index_lag_versions` | 某 scope 下各索引落后图版本数 |
| `relay_retrieval_scope_miss_total` | 查询未指定或解析不到 scope 的次数 |
| `relay_multimodal_extraction_failures_total` | 按 extractor 和 modality 统计抽取失败 |
| `relay_retrieval_degraded_total` | OCR、视觉 embedding 或索引不可用导致的降级次数 |

## 7. 测试场景

当前实现和测试覆盖:

- Git branch fixture 覆盖同一路径在 branch A/B 内容不同；两个 branch 先后索引后，显式查询 branch A 不返回 branch B 的符号或文本。
- branch force-move/rebase fixture 覆盖同一 branch 名称解析到新 commit/tree 后必须形成新 scope；新 head 未索引时查询失败，索引后默认只返回新 head。
- 相同 tree hash fixture 覆盖多个 branch 指向同一 commit/tree 时复用同一 `scope_id`，同时响应 `requested_ref` 保留用户请求的 branch 审计信息。
- `repo impact` fixture 覆盖 PR/rebase range 类 changeset 视图；changed files、deleted symbol names、callers 和 importers 会提升影响结果，命中项仍带 head snapshot 的 `scope_id`、commit 和 tree hash。
- 多模态 application/storage fixture 覆盖同一文档 scope 下 text、image asset、OCR text 和 caption evidence 的提交、检索和 scope 过滤。
- OCR 失败 diagnostic fixture 覆盖原始 image evidence 入库不阻塞，同 scope 文本检索继续可用。
- OCR 与 caption 同时命中同一图片的 fixture 覆盖 organizer 按 parent evidence 合并，context pack 不重复展示同一图片。

## 8. 实施顺序

当前实施状态:

1. `SourceScope` 已作为规范化 domain 类型落地；多模态 `EvidenceModality`、extractor metadata 和 scoped index cursor 已落地。结构化 `SourceScopeSelector` 保留为后续公开 API 兼容扩展。
2. storage 已记录 evidence scope、code snapshot scope、index cursor scope 和 modality；代码仓库新增 `code_repository_scopes` 清单，scope id 由 `repository_id + tree_hash + path/language filters` 稳定生成。
3. Git adapter 已按 branch/ref -> commit -> tree hash -> scope 的顺序索引；branch 名只作为请求和审计输入，不作为事实真源。
4. 代码检索强制解析到已索引 scope；未索引的新 branch head 或 rebase head 返回 invalid argument，不回退到旧 branch 内容。
5. 文档摄取已支持 `text_span`、`image_asset`、`ocr_text`、`caption`、`image_embedding`、`table` 和 `layout_region` metadata；真实 extractor 仍通过 worker/maintenance 边界提交。
6. BM25、semantic 和 vector read model 已携带 scope、modality、parent evidence、model、dimension、source hash 和 graph version；index cursor 按 kind/scope/modality 跟踪 freshness。
7. Git 分支/rebase fixture、多模态 parent grouping fixture 和 OCR failure fixture 已加入测试集。
