# 分层 BM25 算法分析 2026

[中文](../../zh/04-research/12-hierarchical-bm25-analysis-2026.md) | [English](../../en/04-research/12-hierarchical-bm25-analysis-2026.md)

> 文档版本: 1.2
> 编制日期: 2026-08-11
> 适用范围: Issue #350、论文证据、实现边界与评估计划

## 1. 来源与结论

本文基于 Umesh Deshpande 与 Swaminathan Sundararaman 的 *Hierarchical BM25: Lexical Search at Billion-Document Scale*，一手来源为 [arXiv 摘要](https://arxiv.org/abs/2608.00229) 与 [v1 全文](https://arxiv.org/html/2608.00229v1)。下文“论文结果”中的数字只属于论文的语料、硬件和查询分布，不是 relay-knowledge 的实测数据。

可借鉴的核心是：把近似限制在粗粒度路由决策中，进入精排的每个文档仍使用全语料 BM25 统计。relay-knowledge 采用这一分层边界，但没有照搬论文完整的聚类、存储或双信号选择器。

## 2. 论文证明了什么

论文从十亿文档、约 400 GB 的平铺 BM25 索引出发，报告 disk-backed 查询需要 4–12 秒。其两级设计常驻约 1,000 个主题化、大小平衡 cluster 的粗索引，选择有界 cluster 后，只在入选的细粒度索引内穷举，并统一使用全局 `N`、document frequency 与 average document length 评分，避免各 cluster 使用 local IDF 带来的跨 cluster 偏差。

Cluster 选择组合两个信号：

```text
A(c, Q) = sum(idf(t) * (log2(max(f_c(t), 1)))^2)
Score(c, Q) = A(c, Q) + lambda * B(c, Q)
```

`A` 表示 query term 在 cluster 内的聚合集中度。`B` 为一组有区分力但分散的词项保留 document-level postings，记录每个 cluster 内最强的同文档共现证据。论文以 `lambda = 1` 作为起点。带 capacity cap 和局部拆分的 balanced topical LDA 避免热门主题形成超大 cluster，从而破坏查询预算。

论文报告约 4.4 GB 常驻内存，十亿文档上的 16-term query 约 300 ms，相对 flat multi-threaded baseline 的吞吐提升 4.7–5.6 倍，warm cache 约 32 QPS，而 flat index 低于 3 QPS。另一组 50 万文档实验中，访问 500 个 cluster 的 5–10% 可恢复 exhaustive result score 的 0.83–0.92。论文明确将十亿规模 recall、自然词表验证、`B` 信号贡献的独立测量、带 relevance judgment 的 nDCG，以及与 document-reordered BlockMax-WAND 的直接对比留作开放问题。

这些结果说明的是成本与质量交换，不是 rank safety。未入选 cluster 中仍可能存在真实 global top-k 文档。

## 3. relay-knowledge 实现了什么

| 关注点 | 论文 | relay-knowledge 实现 |
| --- | --- | --- |
| 粗粒度分组 | 约 1K 个 balanced topical LDA cluster | 每个 source scope 内按内容生成 10-bit SimHash group，去掉空桶前每个 scope 最多 1,024 个 hash bucket |
| 分组输入 | 从选择后的词汇特征生成 topic vector | path、label、alias 与 content 的有界 topical inventory；source scope 具有 zero-weight 64-bit partition token 并进入 indexed statistics，但不作为 topical SimHash feature |
| 粗粒度元数据 | 常驻 Level-1 路由结构 | SQLite route state/document/group/term 表、精确 FTS-rowid sidecar 和持久 global route-term document frequency |
| 选择信号 | `A + lambda B` | 仅 aggregate 的、类 `A` 工程适配：`global_idf * log2(1 + group_collection_frequency)^2`；它不是论文的精确 `A`，也不实现 `B` 或同文档共现索引 |
| 候选与细索引 | 各 cluster 的 Level-2 索引使用 global statistics | 单一全局 `graph_bm25` FTS5 带 indexed `routing_key`；scoped query 在同一个 `MATCH` 中对 business term、scope64 partition token 和入选 group token 求交 |
| 近似范围 | 仅 cluster selection | 仅 route selection；在同一个 v4 index 内，routed 与 flat 结果的公共文档得到 bitwise-identical BM25 分数 |
| 存储模型 | 固定 cache 加 NVMe cluster index | 既有本机 SQLite runtime database；不能继承论文的内存和延迟界限 |

实现指纹是 `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4`。`topical4` 表示 path、label、alias 与 content 决定 10-bit SimHash group；`scope64-partition` 表示每篇文档的 indexed `routing_key` 同时保存稳定的 zero-weight 64-bit scope token 和 scope-qualified group token。Scoped query 会在同一个 `graph_bm25 MATCH` 中对显式 business-column query、scope token 和入选 group token 求交；unscoped query 不加入 scope token。普通 SQL `source_scope` predicate 始终是硬授权边界，不能被 hash token 替代。`ascii-subset128b-256t` 记录 routing inventory 使用的有界安全 ASCII 子集，`docidlen1` 记录当前 FTS identity-length 约定。

`bm25(graph_bm25, ...)` 把 `routing_key` 的固定 weight 设为零，因此 scope/group token 没有直接 term-score contribution；但它们仍进入 FTS5 document length 与 corpus average document length。因此 schema v4 的数值基线会不同于 pre-v4 index；score parity 承诺严格限定在同一个 v4 table 上的 routed 与 flat 执行。候选读取使用 FTS5 hidden `rank` column 和固定 `bm25(...)` weights 执行 `ORDER BY rank`，先读取有界 identity window，再按 FTS rowid hydrate 入选行。`graph_bm25_route_documents.fts_rowid` 为 `NOT NULL UNIQUE`，mutation 与 rebuild verification 都把它和 `document_id` 配对校验。

选择器从 `graph_bm25_route_term_totals` 读取持久 global document frequency，从 `graph_bm25_route_terms` 读取每组 collection frequency。对每个 query term，它使用仅限定 business column 的 `graph_bm25 MATCH` 校验持久频率，探测上限是预期频率加一行；整条 query 为这些探测预留的 posting 总和最多为 65,536。每个 query term 的 global document frequency 都必须不超过全语料的 20%；任一 term 超过 20%、探测预算溢出，或持久值与观测值不一致，都会选择 flat search。Document inventory 仍最多保留 256 个不同 safe-ASCII term，每个最多 128 bytes；query routing 最多接受 32 个受相同 byte bound 约束的 term。这些结构以 set-based aggregate 方式维护，并不是 document-level co-occurrence index。

派生重建使用 shadow generation。Route state 在填充 `graph_bm25_rebuild` 前持久化 owner/expiry、phase/cursor 与 semantic/vector rebuild plan；旧 attempt 过期后可以接管并从 checkpoint 续跑，不会静默改变该 plan。每个 transaction 最多接纳 128 篇文档、4 MiB 估算权威 source bytes、8,192 个 labels 和 8,192 个 links。单篇文档若超过一个或多个工作预算，会独占一个 transaction，并发出 identity fields 受界的 warning；该例外用于保证前进，不代表单文档具有绝对 byte bound。旧的 flat `graph_bm25` 在此期间保持可读；state 为 `building` 时 semantic、vector 与 fuzzy lexical fallback 暂停，之后通过有界 rowid keyset cleanup 删除 stale row。当前 evidence/code writer 会先取得 `IMMEDIATE` transaction，并在 rebuild 活跃时拒绝写入。Identity、count 与 tokenizer 校验通过后，一个短事务把 active `graph_bm25` 改名为 `graph_bm25_retired`、把 shadow 提升为 active、把 route state 发布为 `fresh`，并写入 schema marker；只有提交后才删除 retired table。每次完整 graph search 都运行在同一个 deferred read transaction 内，因此切代前后的各 retrieval layer 使用同一个 SQLite snapshot。旧二进制不理解这个应用层 fence，所以可能触发 v4 重建的升级必须独占 database，并先停止所有旧 service 与 CLI writer。

Route-document sidecar 还保存 `created_graph_version` 和可观测的 `label_gram_state`。Fuzzy label indexing 对每篇文档最多接受 256 个 labels、每个 label 1,024 UTF-8 bytes、8,192 个不同 grams；越限 skip 和每次 query 的 8,192 个不同 document-label posting 预算耗尽只降级 fuzzy fallback，并保持可观测。同一规范化 document label 命中多个 query grams 只消耗一个 posting。Historical unscoped fallback 的 semantic authorized-corpus probe、route label-state probe 和 `label_lower` hydrate 使用 version-leading global indexes；scoped query 继续使用 scope-leading 专用索引。

## 4. 准入门禁与 Flat Fallback

只有全部满足以下条件时才启用 hierarchical routing：

- requested graph version、current graph version、route-state version 与 `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4` algorithm version 一致，且 route state 为 `fresh`。
- Fresh-open reconciliation 已确认 authoritative source、active global BM25、route-document、grouped-document、持久 route-state、semantic 与 vector document count 一致，且 route algorithm/version/freshness 与持久 semantic/vector generation marker 为 current。写事务维护持久 count 与 global document-frequency total。Startup 有意不做无界 identity、逐行 tokenizer 或 aggregate 深扫。Canonical identity 与 tokenizer consistency 只在其他 stale/schema/count 条件已触发重建后检查，因此仅有 equal-count per-row drift 不会触发 fresh-open rebuild。Query hot path 不运行 full-table `COUNT` 或 `SUM` reconciliation；每个实际 query term 的持久 global frequency 必须等于其有界 business-column `MATCH` 观测值。
- 目标 population 至少有 4,096 个文档，并含 8–2,048 个非空 group。
- 每个 group 最多 512 个文档，且不超过 `max(2 * ceil(mean group size), 64)`。
- 这些结构门禁把可准入 scoped population 上限设为 524,288 个文档，unscoped population 上限设为 1,048,576 个文档；更大 population 使用 flat search。这不是十亿文档产品规模的证据。
- 每个 query term 的 global document frequency 都不超过全语料的 20%，且所有逐词校验探测合计预留不超过 65,536 个 postings。
- Group budget 为 `ceil(group_count / 10)` 并限制在 4–32，入选文档不超过 population 的 25%。
- Matching groups 必须多于 budget，让 routing 形成 approximate cut；最后一个入选 group 还必须比第一个被拒 group 匹配更多 query terms，或 coarse score 至少领先 5%。全部 matching groups 都被选中，或出现 dispersed singleton/equal-score 这类模糊边界时，会禁用 hierarchy。

非 ASCII 或超预算 routing query、stale/historical version、统计不完整、population 太小或倾斜、任一 query term 超过 20% document-frequency 上限、校验探测预算耗尽、入选范围过大，或 routing state 暂时不可用时，会禁用 hierarchy 并继续既有 flat/fallback lexical path。非 ASCII query 会跳过 safe-ASCII routing-term extractor，并继续它既有的 flat FTS/fallback 行为。Routed query 已经尝试后，如果结果为空，或 distinct candidate 少于请求 limit，则先重试 flat BM25，再进入后续 fallback level。瞬时 SQLite query error 有界重试后仍失败时，本次 search 会把 BM25 视为暂时不可用；非瞬时 planning 或 query error 会显式上报，不会被隐藏成 fallback。因此 routing 准入失败只降级优化，database failure 则保留既有 error semantics。Request-level `disabled_retriever_sources` 对 BM25/code-graph rows、graph evidence、semantic、vector、graph path、temporal 和 community-summary source 全部生效，merge 或 fallback 编排不得重新引入已禁用来源。

Routed hit 的 explanation 会记录 algorithm version、`aggregate_tf_idf` signal、selected/matching group counts、selected/population document counts 与 approximate 状态。Flat FTS 成功返回 rows 时，这些 row 会携带稳定的 `hierarchical_bm25 fallback=<reason>` explanation，覆盖 query 不可路由、generation stale、route index/statistics 不完整、population guard、低选择性、无 candidate reduction、coarse-score margin 不足、candidate budget、route state 不可用或 routed-candidate retry。如果 FTS 自身没有 row，最终由 LIKE/trigram 或其他 lexical fallback 提供 hit，则后级保留其既有 explanation 行为，不保证携带 hierarchy reason。

## 5. 正确性边界与风险

- **同 schema score parity 小于 result parity。** 同一个 v4 FTS table 会给 routed 与 flat 结果中的公共文档 bitwise-identical 分数，但 approximate route 可能漏掉分数更高的文档。
- **同分 cutoff membership 不承诺确定性。** Hidden-rank SQL window 先按 BM25 rank 排序，受界内存 window 再按 `(rank, visible evidence ID, document ID)` 排序后执行 parent collapse 与 hydrate。若完全同分文档跨越 SQL `LIMIT` 边界，实现仍不承诺哪篇 tied document 进入该受界 window。
- **v4 是 scoring-schema migration。** 零 weight 的 `routing_key` 仍会改变 FTS5 document-length statistics，因此不承诺 v4 score 等于旧 schema 的 flat baseline。
- **SimHash 不是 balanced topical LDA。** Group-size 与 population gate 会拒绝不安全布局，但不会让已准入 group 自动成为语义最优分区。
- **选择器仅使用 aggregate，只是类 `A` 适配，不是论文的精确 `A`。** 它的 collection-frequency 变换分数无法区分多个有区分力的词出现在同一文档，还是分散在多篇文档；boundary-separation gate 会拒绝模糊 cutoff，但不会复现论文的 `B(c,Q)`，本实现明确不声称或模拟该机制。
- **Fallback 是安全阀，不是 recall proof。** Routed candidate 数量达到请求 limit，并不能证明真实 flat top-k 全部在内。
- **论文性能不能迁移。** SQLite、语料规模、词表、存储、硬件、查询长度和 cache 行为都不同；relay-knowledge 必须独立测量 Recall@k、selected-document fraction、fallback rate 和 p50/p95。
- **授权边界保持独立。** `source_scope` 是硬 SQL filter，也是 scope-specific route identity 的组成部分，不能把它当成主题平衡的证据。
- **应用写 fence 只约束当前版本。** 当前 evidence/code writer 会与 rebuild 串行化，并在 durable lease 生效时拒绝写入；旧二进制不会执行该检查。跨版本升级安全必须依赖独占访问，不能只依赖应用 fence。
- **这些界限只属于 BM25。** 它们不能证明既有 graph-evidence、path、temporal、community 或其他 hybrid layer 的端到端界限；这些层仍需各自的 query-plan 与 corpus measurement。

## 6. 验收与后续证据

具名 `bm25_hierarchy_suite` self-iteration gate 会运行确定性回归，证明 v4 内公共文档 bitwise score parity、hidden-rank single-FTS plan 与 rowid hydrate、scope-partition/business/group intersection、固定 fixture 候选减少、任一高频 term 触发 fallback、65,536-posting 校验上限、current/complete route admission、historical global-index plan、label/posting limit、模糊 cutoff guard 和 selected-document budget。它还覆盖 durable rebuild takeover、phase/cursor 与 semantic/vector plan 持久化、writer fence、document/source-byte/label/link transaction budget、超大文档隔离、reader 完整可见的 shadow swap、swap rollback 与有界 route-term persistence。一个生成式 4,096-document fixture 要求相对 flat oracle 的 routed Recall@10 至少为 0.9。这些是结构与 synthetic deterministic gate，不是自然语料质量证据或生产提速证据。仍必须用固定、版本化的自然词表 fixture 报告 Recall@k、selected-document fraction、query p50/p95 与 fallback rate；在这些结果行出现前，不声称 relay-knowledge 的自然语料 recall、latency、throughput、fallback 数值、同分边界成员确定性或整个 hybrid 的端到端界限。

如果以后加入 `B(c,Q)`，必须设计有界 document-level postings、明确资源预算、独立 effectiveness study，并证明 recall 收益足以覆盖存储与查询成本。不能用 cluster-level term table 冒充 `B`，因为该表无法观察同文档共现。

## 7. 关联文档

- [混合检索竞争力](../02-capabilities/05-hybrid-retrieval-advantage.md)
- [混合检索与 Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [安装、发布与升级](../03-architecture-specs/19-installation-release-and-upgrade.md)
- [竞争力与高性能基准目标](../05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md)
