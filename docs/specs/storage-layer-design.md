# 存储层架构设计

> 文档版本: 1.0
> 编制日期: 2026-05-11
> 适用范围: `relay-knowledge` v1 存储层、索引状态、图版本和后续数据库适配
> 默认路线: SQLite 优先，接口预留 SurrealDB / Neo4j / NebulaGraph 等后端

---

## 1. 设计结论

`relay-knowledge` 的存储层不应只是把节点和边写进数据库。它需要成为一个可持续演化的图谱状态机: 所有事实可追溯，所有图变更可重放，所有索引能说明自己对应哪个图版本，所有查询都能在高吞吐和可测试之间保持清晰边界。

v1 建议采用 **SQLite-first**:

1. **本地零依赖**: 适合作为 CLI、桌面开发、测试和单机知识库的默认运行模式。
2. **性能足够强**: WAL、事务批量写入、prepared statements、组合索引、FTS5 和递归 CTE 能支撑中等规模图谱。
3. **测试成本低**: 临时文件数据库、内存数据库和 deterministic fixture 都容易接入 CI。
4. **可替换**: 所有业务逻辑通过 Rust trait 访问存储，不暴露 SQLite SQL 到 domain、retrieval 或 interface 层。

当前实现已经落地 SQLite-first 的最小生产路径:

- `storage` 模块定义 `GraphStore`、`MutationLogStore`、`IndexStore`、`CodeGraphStore` 和 `KnowledgeStore` contract。
- `SqliteGraphStore` 负责 evidence、entity、mutation log、graph version 和 index metadata。
- `SqliteGraphStore` 同时承接 tree-sitter 解析输出的代码文件、符号、引用、chunk 和 parse status 诊断。
- `indexing` 模块负责 index refresh plan 和 index family 去重选择。
- `retrieval` 模块负责 query 文本、limit 和 freshness policy 的检索计划校验。
- application service 默认在 `paths.data_dir/relay-knowledge.sqlite` 打开 SQLite 数据库。
- SQLite 阻塞调用通过 `tokio::task::spawn_blocking` 隔离，不在 async executor 上运行。
- CLI ingest/query/graph/index/health 命令只调用 application service，不直接访问 SQLite。

核心判断:

- **GraphStore 是事实真源，IndexStore 是派生读模型**。BM25、向量、语义摘要、社区摘要都不能覆盖图事实本身。
- **Mutation log 是系统脊柱**。图写入成功后追加变更日志，索引器只消费日志刷新 read model。
- **图版本和业务时间分离**。`graph_version` 表示系统状态递增版本，`valid_from` / `valid_to` 表示事实在领域内的有效时间。
- **异步 API 不等于异步数据库驱动**。SQLite 操作应被隔离在专用 worker 或连接池边界，避免阻塞 async runtime。
- **可测试性来自接口和确定性**。domain 层使用内存假实现，存储层使用同一套 contract tests 验证 SQLite 和未来后端。

## 2. 架构边界

存储层位于 domain 和 retrieval / indexing 之间，只负责持久化、事务、版本、查询原语和索引元数据。

```text
CLI / Web / API / MCP
        |
Core Services
        |
+-------------------+      +-------------------+
| Retrieval Service |      | Ingestion Service |
+-------------------+      +-------------------+
        |                          |
        v                          v
+------------------------------------------------+
| Storage Facade                                 |
| - GraphStore                                  |
| - MutationLogStore                            |
| - IndexStore                                  |
| - StorageHealth                               |
+------------------------------------------------+
        |
+-------------------+    +-----------------------+
| SQLite Adapter    |    | Future Adapters       |
| WAL + FTS5 + CTE  |    | SurrealDB / Neo4j ... |
+-------------------+    +-----------------------+
```

### 2.1 分层职责

| 层 | 职责 | 禁止事项 |
| --- | --- | --- |
| `domain` | 实体、关系、证据、Claim、版本、错误类型 | 不依赖数据库、SQL、连接池 |
| `storage` | 事务、图写入、图查询、mutation log、索引元数据 | 不做 LLM 抽取、rerank、UI 展示 |
| `indexing` | 消费 mutation log，刷新 BM25 / vector / summary | 不直接修改图事实 |
| `retrieval` | 多路召回、融合、图扩展、上下文组织 | 不绕过 `GraphStore` 访问数据库 |
| `interfaces` | CLI / Web / MCP 参数解析和展示 | 不复制核心业务逻辑 |

### 2.2 写入路径

```text
Extracted facts
   -> validate domain model
   -> begin graph transaction
   -> upsert entities / relations / claims / evidence
   -> append graph_mutations
   -> bump graph_versions
   -> commit
   -> publish IndexRefreshRequested(graph_version)
```

设计要求:

- 图写入和 mutation log 追加必须在同一个事务内完成。
- 索引刷新事件只能在提交成功后发布。
- 索引失败不能回滚图写入，但必须记录 stale 状态和失败原因。
- 写入 API 接收批次，避免每条事实单独开事务。

当前最小实现中，evidence 写入、entity upsert、mutation log 追加和 graph version bump
在同一个 SQLite transaction 内完成。提交会把全部 index metadata 标记为 stale；
`refresh_indexes` 和 `ingest` 后置刷新会把指定 index family 标记到当前 graph version。

### 2.3 读取路径

```text
Query request
   -> choose graph_version / as_of / freshness policy
   -> read graph facts
   -> optionally read BM25 / vector / summary index state
   -> return data + graph_version + index_versions + stale flags
```

设计要求:

- 所有查询结果都携带 `graph_version`。
- 存储查询必须按请求中的 `graph_version` 快照过滤事实，不能在同一个响应中混入更新版本写入的数据。
- 经过索引的查询同时携带对应 `index_version` 和 `indexed_graph_version`。
- 当索引落后于图版本时，调用方可选择 `allow_stale`、`wait_until_fresh` 或 `graph_only` 降级。
- 遍历查询必须有 hop、节点数、边数和耗时预算。

当前 SQLite 搜索实现会读取指定 graph snapshot 内的所有 evidence 候选并在评分后按
请求 limit 截断，避免先按“最新 N 条”裁剪导致旧但相关的证据被跳过。持久化 entity
ID 使用固定哈希算法从规范化 label 生成，mutation receipt 中的 entity count 记录本次
事务实际链接到的唯一 entity ID 数。
Index metadata 必须保持单调: 较旧的 refresh completion 不能覆盖更新的
`indexed_graph_version`。缺失或未知的 `index_status` kind 属于存储元数据损坏，
未知的 state 也必须作为 storage error 暴露，而不是映射成默认 BM25 或普通 stale
状态。更新 evidence 的实体链接后必须清理不再被任何 evidence 引用的 orphan entity。
检索读取 entity label 时不得依赖分隔符拼接/拆分；label 可以包含控制字符，存储层
必须按行读取并保持原值。相同 score 的检索结果必须使用稳定 tie-breaker，避免同一
输入在不同执行计划下返回不同截断子集。

## 3. 核心接口

接口需要先表达能力边界，不应泄漏 SQLite 表结构。以下为设计草案，后续实现可按 Rust async trait 细化。

```rust
pub trait GraphStore {
    async fn commit_mutation_batch(
        &self,
        batch: GraphMutationBatch,
        options: CommitOptions,
    ) -> Result<CommitReceipt, StorageError>;

    async fn get_entity(
        &self,
        id: &EntityId,
        version: VersionSelector,
    ) -> Result<Option<Entity>, StorageError>;

    async fn query_neighbors(
        &self,
        seed: EntityId,
        policy: TraversalPolicy,
    ) -> Result<NeighborPage, StorageError>;

    async fn find_paths(
        &self,
        source: EntityId,
        target: EntityId,
        policy: PathPolicy,
    ) -> Result<PathPage, StorageError>;
}

pub trait MutationLogStore {
    async fn read_after(
        &self,
        graph_version: GraphVersion,
        limit: usize,
    ) -> Result<Vec<GraphMutation>, StorageError>;
}

pub trait IndexStore {
    async fn get_index_status(
        &self,
        index_kind: IndexKind,
    ) -> Result<IndexStatus, StorageError>;

    async fn mark_refresh_complete(
        &self,
        receipt: IndexRefreshReceipt,
    ) -> Result<(), StorageError>;
}
```

### 3.1 关键类型

| 类型 | 含义 |
| --- | --- |
| `GraphVersion` | 单调递增系统版本，每次提交图变更生成一个新版本 |
| `VersionSelector` | `Latest`、`AtGraphVersion(n)`、`AsOfTime(t)` |
| `GraphMutationBatch` | 一次事务内提交的节点、边、证据、Claim 和删除/失效操作 |
| `CommitReceipt` | 提交后的 `graph_version`、变更数量、受影响实体、trace id |
| `TraversalPolicy` | hop、方向、边类型、版本、预算、分页参数 |
| `IndexStatus` | 索引类型、索引版本、已处理图版本、是否 stale、最近错误 |
| `StorageHealth` | 数据库 schema 版本、WAL 状态、checkpoint、连接池、索引滞后 |

## 4. 数据模型

内部模型采用 Labeled Property Graph。节点、边、Claim 和 Evidence 都有稳定 ID、属性、版本和来源信息。

### 4.1 事实表

```sql
CREATE TABLE entities (
    id TEXT PRIMARY KEY,
    entity_kind TEXT NOT NULL,
    label TEXT NOT NULL,
    aliases_json TEXT NOT NULL DEFAULT '[]',
    properties_json TEXT NOT NULL DEFAULT '{}',
    confidence REAL NOT NULL DEFAULT 1.0,
    status TEXT NOT NULL DEFAULT 'accepted',
    valid_from_version INTEGER NOT NULL,
    valid_to_version INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE relations (
    id TEXT PRIMARY KEY,
    relation_kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    properties_json TEXT NOT NULL DEFAULT '{}',
    confidence REAL NOT NULL DEFAULT 1.0,
    status TEXT NOT NULL DEFAULT 'accepted',
    valid_from_version INTEGER NOT NULL,
    valid_to_version INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (source_id) REFERENCES entities(id),
    FOREIGN KEY (target_id) REFERENCES entities(id)
);

CREATE TABLE claims (
    id TEXT PRIMARY KEY,
    claim_kind TEXT NOT NULL,
    subject_id TEXT,
    statement TEXT NOT NULL,
    properties_json TEXT NOT NULL DEFAULT '{}',
    confidence REAL NOT NULL DEFAULT 1.0,
    status TEXT NOT NULL DEFAULT 'proposed',
    valid_from_version INTEGER NOT NULL,
    valid_to_version INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE evidence (
    id TEXT PRIMARY KEY,
    source_uri TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    span_start INTEGER,
    span_end INTEGER,
    excerpt_hash TEXT,
    extractor TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    properties_json TEXT NOT NULL DEFAULT '{}'
);
```

`claims` 用于表达复杂事实、事件或候选断言，避免把 n 元事实硬塞进二元边。参与者、时间、地点、条件等信息可以用 relation 连接到 Claim，也可以放入 typed properties。

### 4.2 证据关联

```sql
CREATE TABLE fact_evidence (
    fact_id TEXT NOT NULL,
    fact_table TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'supporting',
    created_at TEXT NOT NULL,
    PRIMARY KEY (fact_id, fact_table, evidence_id),
    FOREIGN KEY (evidence_id) REFERENCES evidence(id)
);
```

说明:

- `fact_table` 只允许 `entities`、`relations`、`claims`。
- `role` 支持 `supporting`、`contradicting`、`source`、`derived_from`。
- 后续如需更强约束，可拆成三张关联表；v1 优先保持写入路径简单。

### 4.2.1 当前代码图 v1 表

当前 SQLite 实现已为 tree-sitter 输出增加专用表，而不是把代码结构塞入通用 evidence:

```sql
CREATE TABLE code_files (
    source_scope TEXT NOT NULL,
    path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    language_id TEXT NOT NULL,
    parse_status TEXT NOT NULL,
    diagnostic TEXT,
    created_graph_version INTEGER NOT NULL,
    PRIMARY KEY (source_scope, path)
);

CREATE TABLE code_symbols (...);
CREATE TABLE code_references (...);
CREATE TABLE code_chunks (...);
CREATE TABLE code_chunk_symbols (...);
```

这些表记录版本化的 repository-relative path、syntax-level symbol/reference/chunk、extractor metadata 和 parse status。对同一 `source_scope + path` 的新解析结果在事务内替换旧代码事实，并推进 graph version、追加 mutation log、标记派生索引 stale。启动时如发现早期实验性 `code_*` 表缺少 v1 必需列，SQLite adapter 会先把整组旧表重命名为 `*_legacy_N`，再创建当前 v1 表，避免旧本地状态阻塞服务启动。

### 4.3 版本和变更日志

```sql
CREATE TABLE graph_versions (
    version INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_version INTEGER,
    mutation_count INTEGER NOT NULL,
    trace_id TEXT NOT NULL,
    committed_at TEXT NOT NULL,
    committed_by TEXT NOT NULL,
    summary TEXT
);

CREATE TABLE graph_mutations (
    id TEXT PRIMARY KEY,
    graph_version INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    mutation_kind TEXT NOT NULL,
    entity_id TEXT,
    relation_id TEXT,
    claim_id TEXT,
    payload_json TEXT NOT NULL,
    affected_entity_ids_json TEXT NOT NULL DEFAULT '[]',
    source_hashes_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    FOREIGN KEY (graph_version) REFERENCES graph_versions(version)
);
```

mutation log 的职责:

- 支持索引器从任意 `graph_version` 后续消费变更。
- 支持测试中重放固定 fixture。
- 支持未来审计、回滚、差分查询和跨仓库同步。
- 支持局部索引失效，不必每次全量重建。

### 4.4 索引状态

```sql
CREATE TABLE index_versions (
    id TEXT PRIMARY KEY,
    index_kind TEXT NOT NULL,
    index_name TEXT NOT NULL,
    index_version INTEGER NOT NULL,
    indexed_graph_version INTEGER NOT NULL,
    embedding_model TEXT,
    embedding_dimension INTEGER,
    source_hash TEXT,
    status TEXT NOT NULL,
    refreshed_at TEXT NOT NULL,
    last_error TEXT
);

CREATE VIRTUAL TABLE entity_fts USING fts5(
    entity_id UNINDEXED,
    label,
    aliases,
    properties,
    evidence_text,
    tokenize = 'unicode61'
);

CREATE TABLE embedding_records (
    id TEXT PRIMARY KEY,
    owner_kind TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    embedding_model TEXT NOT NULL,
    embedding_dimension INTEGER NOT NULL,
    vector_blob BLOB NOT NULL,
    content_hash TEXT NOT NULL,
    indexed_graph_version INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
```

v1 向量记录可以先存在 SQLite 中，ANN 检索由后续可插拔组件实现。关键是先固定元数据: 模型、维度、内容 hash 和图版本。

## 5. SQLite 适配设计

### 5.1 连接和运行模式

默认数据库路径:

```text
.relay-knowledge/graph.db
```

推荐启动配置:

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
```

运行原则:

- 写入使用单写事务队列，减少 writer contention。
- 读取使用独立只读连接池，允许 WAL 模式下读写并发。
- 大批量导入使用 chunked transaction，例如每 500 到 2000 条事实一批，具体阈值由基准测试确定。
- checkpoint 由维护任务触发，避免在用户查询热路径上意外阻塞。

### 5.2 索引设计

```sql
CREATE INDEX idx_entities_kind_current
    ON entities(entity_kind, id)
    WHERE valid_to_version IS NULL;

CREATE INDEX idx_relations_source_kind_current
    ON relations(source_id, relation_kind, target_id)
    WHERE valid_to_version IS NULL;

CREATE INDEX idx_relations_target_kind_current
    ON relations(target_id, relation_kind, source_id)
    WHERE valid_to_version IS NULL;

CREATE INDEX idx_relations_kind_current
    ON relations(relation_kind, source_id, target_id)
    WHERE valid_to_version IS NULL;

CREATE INDEX idx_mutations_graph_version_sequence
    ON graph_mutations(graph_version, sequence);

CREATE INDEX idx_index_versions_kind_name
    ON index_versions(index_kind, index_name);
```

查询约束:

- 邻域查询只走 `source_id` / `target_id` 组合索引，不扫描全表。
- `Latest` 查询默认使用 `valid_to_version IS NULL` partial index。
- 历史查询使用版本范围条件: `valid_from_version <= ? AND (valid_to_version IS NULL OR valid_to_version > ?)`。
- 所有列表接口必须分页，禁止无界返回。

### 5.3 图遍历

SQLite v1 用递归 CTE 支持基础遍历:

```sql
WITH RECURSIVE walk(depth, entity_id, path) AS (
    SELECT 0, :seed_id, json_array(:seed_id)
    UNION ALL
    SELECT
        walk.depth + 1,
        relations.target_id,
        json_insert(walk.path, '$[#]', relations.target_id)
    FROM walk
    JOIN relations ON relations.source_id = walk.entity_id
    WHERE walk.depth < :max_depth
      AND relations.valid_to_version IS NULL
      AND json_array_length(walk.path) <= :max_nodes
)
SELECT * FROM walk
LIMIT :limit;
```

实现时还需要在应用层强制:

- `max_depth`
- `max_nodes`
- `max_edges`
- `timeout`
- `allowed_relation_kinds`
- `direction`
- `page_token`

对于高频路径查询，后续可增加 materialized read model，例如 `entity_adjacency` 或 `relation_closure_cache`。这些 read model 必须从 mutation log 派生，不能成为事实真源。

## 6. 高性能策略

### 6.1 写入性能

- **批量提交**: ingestion 产物按批提交，单事务包含事实表更新、证据关联、mutation log 和 graph version。
- **幂等 upsert**: 实体、边、证据使用稳定 ID，重复导入只更新版本和属性，不生成重复事实。
- **append-only 优先**: 删除和覆盖默认写成 `valid_to_version` 失效，保留历史和审计能力。
- **prepared statements**: 热路径 SQL 预编译，减少解析开销。
- **背压**: 写入队列有界，超出队列长度时 ingestion 降速或返回 retryable error。

### 6.2 读取性能

- **读写分离连接**: 写队列单独串行化，读连接并发服务查询。
- **预算化遍历**: 所有 k-hop、path、impact 查询都有硬预算，避免图爆炸。
- **索引读模型**: BM25、向量、社区摘要只作为派生 read model，查询走更适合的结构。
- **分页和截断标记**: 返回 `next_page_token` 和 `truncated`，不让接口输出无限增长。
- **缓存最小化**: 优先缓存 schema、prepared statements、热点 entity metadata；避免缓存完整大子图。

### 6.3 索引刷新性能

- **增量消费 mutation log**: 索引器读取上次处理版本之后的变更。
- **局部失效**: 根据 `affected_entity_ids_json` 和 `source_hashes_json` 更新局部索引。
- **多索引独立进度**: BM25、vector、summary、community 各自记录 indexed graph version。
- **失败隔离**: 向量模型失败不影响 BM25；社区摘要失败不影响基础图查询。
- **stale 可见**: 查询结果明确返回 stale 状态，避免用户误以为索引完全新鲜。

## 7. 可测试设计

### 7.1 测试分层

| 测试类型 | 目标 | 依赖 |
| --- | --- | --- |
| domain unit tests | 验证 ID、版本、状态机、校验规则 | 无数据库 |
| storage contract tests | 验证 `GraphStore` 语义 | 内存假实现 + SQLite 实现 |
| integration tests | 验证事务、FTS、递归 CTE、索引状态 | 临时 SQLite 文件 |
| regression fixtures | 验证固定图查询、路径、stale 行为 | deterministic fixture |
| failure tests | 验证中断、重复提交、索引失败、超时 | fault injection |

### 7.2 Contract tests

同一套 contract tests 应覆盖所有后端:

- `commit_batch_creates_new_graph_version`
- `upsert_is_idempotent_for_same_stable_id`
- `latest_query_excludes_invalidated_facts`
- `version_query_returns_historical_fact`
- `mutation_log_replays_in_sequence`
- `neighbor_query_respects_depth_and_limit`
- `index_status_reports_stale_when_graph_advances`
- `failed_index_refresh_keeps_graph_committed`

这些测试定义的是业务语义，不是 SQLite 细节。未来 SurrealDB 或 Neo4j 适配器必须通过同一套测试。

### 7.3 Fixture 设计

建议维护一个最小图:

- 20 个实体，覆盖文档、代码符号、概念、事件。
- 50 条边，覆盖 `MENTIONS`、`DEPENDS_ON`、`DERIVED_FROM`、`CONTRADICTS`。
- 5 条 Claim，至少包含 proposed、accepted、rejected、superseded 状态。
- 10 条 Evidence，覆盖相同事实的支持和反驳证据。
- 3 个图版本，用于测试历史查询和索引滞后。

fixture 必须小、确定、可人工检查，避免把集成测试变成性能测试。

## 8. 迁移和兼容

v1 需要内置 schema migration 表:

```sql
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL,
    checksum TEXT NOT NULL
);
```

迁移规则:

- 所有迁移必须幂等或在事务内失败回滚。
- CI 中运行空库迁移和旧 fixture 库升级。
- schema 变更必须同步更新 contract tests。
- 不允许接口层依赖表名或列名，迁移影响只应停留在 adapter 内部。

## 9. 可观测性和运维

存储层应暴露 `StorageHealth`:

| 指标 | 含义 |
| --- | --- |
| `latest_graph_version` | 当前图版本 |
| `pending_mutations` | 尚未被索引器消费的变更数 |
| `index_lag_by_kind` | 各索引落后图版本多少 |
| `wal_size_bytes` | WAL 文件大小 |
| `checkpoint_age` | 距离上次 checkpoint 的时间 |
| `query_p95_ms` | 主要查询 p95 延迟 |
| `write_batch_p95_ms` | 批量提交 p95 延迟 |
| `storage_errors_total` | 按错误类型统计 |

日志和 trace 要包含:

- `trace_id`
- `graph_version`
- `index_kind`
- `indexed_graph_version`
- `affected_entity_count`
- `query_budget`
- `truncated`
- `stale`

## 10. 失败模式

| 场景 | 处理策略 |
| --- | --- |
| 写事务失败 | 整批回滚，不发布索引刷新事件 |
| 写事务成功但事件发布失败 | 后台 reconciler 根据 graph version 补发刷新任务 |
| 索引刷新失败 | 标记 `index_versions.status = failed`，保留最新图状态 |
| 查询超预算 | 返回部分结果、`truncated = true` 和明确错误/警告 |
| 数据库 busy | 使用 busy timeout，超过后返回 retryable error |
| WAL 过大 | 维护任务执行 checkpoint，不在查询热路径同步执行 |
| schema 版本不兼容 | 启动失败并提示迁移，不做隐式破坏性升级 |

## 11. 后端演进

SQLite 是 v1 默认，不是永久绑定。

### 11.1 SurrealDB 适配

适用场景:

- 需要统一图、文档、多模型查询。
- 希望从嵌入式模式平滑迁移到远程服务。
- 希望减少自维护 read model 数量。

要求:

- 必须通过 `GraphStore` contract tests。
- 不允许 retrieval 层直接写 SurrealQL。
- graph version、mutation log 和索引 freshness 语义必须保持一致。

### 11.2 Neo4j / NebulaGraph / Memgraph 适配

适用场景:

- 图规模超过 SQLite 递归 CTE 的舒适区。
- 需要原生图算法、集群能力、复杂路径查询或企业运维能力。
- Web 服务多租户和并发压力显著增长。

要求:

- 专用图数据库只替换存储实现，不改变 domain 模型。
- BM25 / vector 是否迁移到数据库内置能力由 adapter 决定，但 `IndexStore` 语义不变。
- 导入导出必须保留 evidence、version 和 mutation log 信息。

## 12. 实施顺序

1. 定义 domain 类型: `Entity`、`Relation`、`Claim`、`Evidence`、`GraphVersion`、`GraphMutation`。
2. 定义 storage traits 和错误类型，不实现具体数据库逻辑。
3. 实现内存假存储，支撑 domain 和 service 单测。
4. 实现 SQLite migration、连接管理、事务批量写入和 mutation log。
5. 实现基础图查询: get entity、neighbors、version query、path query。
6. 实现 index status 和 FTS5 BM25 read model。
7. 增加 contract tests、fixture tests 和索引 stale 场景。
8. 在 CLI / Web 共用服务层接入 storage facade。

## 参考资料

- SQLite WAL: <https://www.sqlite.org/wal.html>
- SQLite FTS5: <https://sqlite.org/fts5.html>
- SQLite Query Planner: <https://www.sqlite.org/queryplanner.html>
- SurrealDB Rust SDK: <https://surrealdb.com/docs/sdk/rust>
- SurrealDB Concepts: <https://surrealdb.com/docs/surrealdb>
- Neo4j Semantic Indexes: <https://neo4j.com/docs/cypher-manual/current/indexes/semantic-indexes/>
