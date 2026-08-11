# 混合检索竞争力

[中文](./05-hybrid-retrieval-advantage.md) | [English](../../en/02-capabilities/05-hybrid-retrieval-advantage.md)

> 文档版本: 2.3
> 编制日期: 2026-08-11
> 适用范围: 第二卷能力说明

## 能力定位

混合检索是第二卷最核心的竞争力能力。它同时使用 BM25、local semantic token read model、local hashed-vector ANN、可配置 external semantic/vector backend、graph evidence fallback、code graph documents、有界代码 exact-text source fallback、local file path/content read model、schema path、temporal event、community summary 和 RRF。

## 用户可见行为

- 查询结果带 retriever sources 和 ranking explanation。
- BM25 会索引 entity 和 code symbol 的生成式 lexical alias，但不把 alias 当 canonical label 返回。
- 满足门禁的大型、平衡 graph corpus 可使用有界 single-FTS hierarchical BM25；routed hit 会解释 algorithm、入选 group/document 以及 group selection 是否 approximate。
- Graph paths 保留节点标签、edge fact id、predicate、supporting evidence ids、confidence、status 和 version range。
- Temporal、community 和 code graph 信号可以与普通 evidence 一起进入 context pack。
- 代码 exact-text 兜底命中以 `lexical`/`text_fallback` provenance 进入结果，不伪装成 resolved graph edge。
- 本机文件结果区分 path、metadata、content 和 change cursor freshness；文件名/路径查询不依赖内容索引。

## 竞争力特性

普通全文搜索容易漏概念相似，普通向量搜索容易漏精确符号，普通图查询缺少自然语言召回，普通桌面文件搜索又常缺少图谱和 agent context。混合检索把这些信号融合后再做预算分配，能同时服务事实问答、代码定位、本机文件定位、多跳关系和 agent 上下文构造。

## 命令/API 入口

```bash
relay-knowledge query "retry policy graph path"   --freshness wait-until-fresh   --limit 10   --format json
```

## 降级与诊断

Semantic/vector backend disabled 或 cursor stale 时，BM25 和 graph evidence 仍可工作。响应的 `context_pack.backend_statuses` 会说明 configured backend、model、dimension、scope post-filter 和 indexed graph version。
Request-level `disabled_retriever_sources` 对所有 graph-search source 生效：BM25/code-graph rows、graph evidence、semantic、vector、graph path、temporal 与 community summary；merge 和 fallback 编排不会重新引入已禁用来源。
代码 source fallback 候选路径或预算耗尽时，只降级 exact-text 兜底层；已有 BM25、code graph edge 和 graph evidence 仍可进入 context pack。
本机文件 content cursor stale 时，path/metadata 仍可服务文件定位；响应需要说明 content stale、watcher lag 或 bounded rescan 状态。

### 单 FTS Hierarchical BM25

对于满足门禁的 current graph version，graph BM25 会把文档分配到 scope-qualified、content-driven 的 10-bit SimHash group。每个 indexed `routing_key` 同时包含 zero-weight scope64 partition token 与 scope-qualified group token。粗选仅使用 global IDF 和 group aggregate term frequency，不实现论文的同文档共现信号。Scoped request 会在同一个 `graph_bm25 MATCH` 中对显式 business-column query、scope token 和入选 group token 求交；SQL `source_scope` predicate 始终是硬授权 filter，不能信任 hash token 代替授权。

Indexed `routing_key` 的固定 BM25 weight 为零，因此在 schema v4 内，routed 与 flat 结果的公共文档具有 bitwise-identical score。但 token 仍影响 FTS5 document-length statistics，所以 v4 score 可能不同于 pre-v4 flat index。FTS plan 通过 hidden `rank` column 排序有界 identity window，再按 rowid hydrate 入选行；route sidecar 把 `fts_rowid NOT NULL UNIQUE` 与 document identity、graph version、label state 一起保存。Group selection 仍是 approximate 且不是 rank-safe；`LIMIT` 边界完全同分时不承诺确定 membership。

只有 current、complete、足够大且倾斜受控的 population 才能启用 routing；ASCII query term、group count、group size、selected-group count、最多 25% 的 selected-document fraction，以及有明确区分度的 coarse-score cutoff 均为门禁；全部 matching groups 都被选中或 cutoff 模糊时走 flat search。每个 query term 的持久 global document frequency 必须等于最多探测 `df + 1` 行的 business-column `MATCH`，每个 term 都必须不超过 corpus 的 20%，所有探测合计最多预留 65,536 个 postings。Stale version、统计不一致、不支持或非 ASCII 的 routing query、population 太小或倾斜、入选范围过大，或 routing state 暂时不可用时，会禁用 hierarchy 并继续既有 flat/fallback lexical path；非瞬时 planning error 会显式上报，不会被隐藏成 fallback。Routing-term extractor 只接受 ASCII，因此非 ASCII query 会跳过 hierarchy，并继续它既有的 flat FTS/fallback 行为。已经尝试的 routed query 如果为空，或 distinct candidate 少于请求 limit，则在后续 fallback 前重试 flat BM25。瞬时 SQLite query error 会有界重试；若仍失败，本次 search 会把 BM25 视为暂时不可用，非瞬时 query error 则显式上报。Routed hit 带 `hierarchical_bm25` selection fields；flat FTS 成功返回的 row 带 `hierarchical_bm25 fallback=<reason>`，只有后续 LIKE/trigram fallback 产生的 hit 保留该层既有 explanation 行为。

SQLite schema marker v4 会在旧 flat FTS 保持可读时重建 `graph_bm25_rebuild`。Durable owner/expiry、phase/cursor 与 semantic/vector-plan fields 让过期 attempt 可以被接管并续跑。每个 transaction 最多接纳 128 篇文档、4 MiB 估算 source bytes、8,192 个 labels 和 8,192 个 links；单篇超大文档会独占 transaction 并发出 identity 受界的 warning，以保证前进，这不是单文档绝对 byte bound。`building` 期间 semantic、vector 与 fuzzy lexical fallback 暂停，随后 shadow、route `fresh` 与 marker 原子激活。Historical unscoped fallback probe 使用 version-leading global indexes，scoped probe 继续使用 scope-leading index。一次 graph search 会让各 retrieval layer 共用一个 read transaction。Durable rebuild lease 为 `building` 时，当前 evidence/code writer 会被 fence，但旧 binary 不执行该应用检查。只回滚二进制并不会恢复旧数值评分基线，而且旧二进制写入会使 v4 metadata stale。若要求精确恢复旧 schema score，必须还原 pre-v4 database checkpoint；否则之后的 v4 startup 会 reconcile 并重建派生状态。Upgrade 必须独占 database，新旧 writer 不得并发写入。

论文中的十亿文档 latency、memory、throughput 和小规模 quality 数字只是研究证据，不是产品实测。确定性 suite 要求 synthetic Recall@10 >= 0.9，但自然词表 Recall@k、p50/p95 与 fallback rate 仍是独立、尚未测量的产品基准。这些 BM25-local bounds 也不能证明其他既有 hybrid graph layer 的端到端界限。

### BM25 多级降级策略

BM25 检索路径内部实现三级降级链，最大化召回率的同时保持排序质量：

```
FTS5 prefix match (BM25 评分)
  ↓ 结果为空且 query ≥ 2 字符
精确名匹配 (JSON-safe entity_labels LIKE / LOWER(content))
  ↓ 结果为空
LIKE 子串搜索 (content LIKE '%query%' ESCAPE '\')
  ↓ 结果为空且 query ≥ 3 字符
Levenshtein fuzzy search (edit distance ≤ 1..2)
```

**性能保底**：
- 精确名匹配使用 JSON 编码后的 `LIKE '%"target"%'` pattern 支持多标签实体和转义后的 label 字符
- LIKE fallback 在参数绑定前转义 `\`、`%` 和 `_`
- 所有 WHERE 子句将 OR 条件包裹在括号内，确保 scope 和 version 过滤对全部分支生效
- Levenshtein 使用维护在 SQLite 中的 `graph_bm25_label_grams` label gram 索引，按 query-specific gram overlap 和 label length bound 收集 scope/version 候选，避免扫描 graph documents 或截断任意 anchor rows
- label gram schema 和 backfill 由 SQLite schema marker version 保护，通过比较每个 document 的 expected grams 恢复未完成升级，并在构造 SQL bind 参数前限制 query grams 数量
- 每篇文档最多接纳 256 个 labels、每个 label 1,024 UTF-8 bytes、8,192 个不同 grams；skip 会持久化到 `label_gram_state`，8,192-posting fuzzy-query budget 耗尽会报告 degraded
- Historical unscoped authorized-corpus、label-state 与 `label_lower` hydrate probe 使用 version-leading global indexes；scoped query 保留专用 scope-leading indexes
- fuzzy 匹配先应用 gram-overlap 候选上限，再由 Rust Levenshtein 评分，并在 matched-name cap 前按 edit distance 排序
- fuzzy 结果通过 label-gram document ids 批量 join 已排序 name，保留该 name 的 edit-distance score 参与结果排序，避免 per-name leading-wildcard 扫描或单次跨 name SQL `LIMIT` 丢掉更近的匹配
- fallback SQL 会先限制 rows，再做确定性内存排序，避免 leading-wildcard LIKE 路径触发无界 SQL sort
- edit distance 上限随 query 长度自适应：≤ 4 字符 → max dist 1，> 4 字符 → max dist 2
- 降级为互斥瀑布式：前级有结果则跳过后续级，结果按 document_id 去重
- 所有 SQL 查询均使用 `graph_bm25.` 表前缀消除歧义

**适用场景**：
- 用户拼写错误（如 `getUssr` → `getUser`）
- 子串查询（如 `sign` → `signInWithGoogle`）
- 短词查询（FTS 前缀匹配噪音太大时）

## 关联架构章节

- [混合检索与 Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Semantic/Vector Provider 架构](../03-architecture-specs/10-semantic-vector-provider-architecture.md)
- [分层 BM25 算法分析 2026](../04-research/12-hierarchical-bm25-analysis-2026.md)

---

导航: 上一章: [4. 查询与 Context Pack 基础](04-query-and-context-pack-basics.md) | 下一章: [6. 新鲜度与索引恢复](06-freshness-and-index-recovery.md)
