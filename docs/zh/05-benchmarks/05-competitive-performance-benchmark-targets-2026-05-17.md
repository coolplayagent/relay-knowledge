# 竞争力与高性能基准目标 2026-05-17

[中文](../../zh/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md) | [English](../../en/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md)

本文把竞争力和高性能研究转化为后续 benchmark 应跟踪的指标。它不是一次实测记录，而是设计回归门禁和优化实验时的目标清单。

## 1. 检索质量指标

| 场景 | 指标 |
| --- | --- |
| 混合图谱问答 | Recall@k、MRR、negative rejection、stale rejection、graph path coverage、context pack token budget。 |
| Hierarchical graph BM25 | Flat-versus-routed Recall@k、selected-document fraction、route admission/fallback rate、同 v4 公共文档 bitwise score parity、single-FTS route-intersection plan conformance、routed/flat p50/p95。 |
| 代码检索 | exact symbol rank、caller/callee rank、import/reference resolution rate、source fallback recall/provenance、false positive count、impact precision、query p50/p95/p99。 |
| 本机文件检索 | filename/path query p50/p95/p99、content query p50/p95/p99、permission-filter cost、candidate window size、stale/degraded rate。 |

## 2. 索引性能指标

| 场景 | 指标 |
| --- | --- |
| Cold graph/code/file index | indexed item count、elapsed、peak RSS、write batch count、parse/extract throughput、index size。 |
| Incremental update | changed item count、affected item count、refresh elapsed、cursor lag、missed event count、fallback rescan count。 |
| Hierarchical BM25 派生重建 | authoritative/active-FTS/shadow-FTS/route-document/grouped/state/semantic/vector counts、durable owner/expiry 与 takeover state、phase/cursor 和 semantic/vector-plan recovery、按 128 篇文档/4 MiB 估算 source bytes/8,192 个 labels/8,192 个 links 设界的 transaction、oversize-document isolation 与 bounded-warning count、stale-row keyset cleanup、旧 flat reader availability、activation/retired-cleanup duration、peak RSS、WAL 与临时磁盘 high-water mark、最终 graph/route/schema-marker state。 |
| No-op refresh | elapsed、blob/file reads、SQLite writes、queue tasks created、freshness state。 |
| Background worker | queue depth、lease recovery count、dead-letter count、retry count、worker saturation、timeout count。 |

## 3. 本机文件检索基准集

后续应准备三个 fixture 层级：

- Small: 1K-10K 文件，覆盖常见文档、源码、隐藏目录、ignore 规则和权限过滤。
- Medium: 100K-500K 文件，覆盖多 root、深目录、重复文件名、二进制和大文件跳过。
- Stress: 1M+ 文件或生成式路径列表，重点测 path/trigram/posting list、metadata filter、watcher lag 和 bounded rescan。

每个 fixture 至少包含：

- 文件名精确查询、模糊路径查询、扩展名查询、目录限定查询。
- 内容词项查询、短语查询、大小/mtime/mime 组合过滤。
- 删除、rename、move、permission change、watcher overflow 或 cursor invalidation 的恢复场景。

## 4. 高性能算法观测字段

检索 trace 和 benchmark 输出应记录：

- retriever family、candidate count、post-filter count、RRF rank contribution、rerank score、truncation reason。
- scope、authorization root、index cursor、graph/file/code version、stale lag、degraded reason。
- code source fallback trigger reason、candidate file count、materialized bytes、`text_fallback` hit count、candidate/budget degraded reason。
- query latency breakdown: normalize、filter、candidate recall、scoring、graph expansion、context packing、storage IO。
- worker latency breakdown: enqueue、lease wait、scan/parse/extract、write batch、cursor commit、reconcile。
- Hierarchical BM25: `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4` algorithm version、aggregate signal name、scope64/group-token plan shape、selected/matching group count、selected/population document count、selected-document fraction、approximation state、逐词 persisted/observed DF、reserved validation postings、高频/budget fallback reason、fuzzy label/input-byte/gram/posting work，以及 harness 是否观察到 flat retry。
- Hierarchical BM25 rebuild: lease owner/expiry/state、phase/cursor checkpoint、固定 semantic/vector plan、active/shadow/retired generation、各阶段 document/source-byte/label/link/transaction count、oversize-document isolation 与 bounded-warning count、writer-fence rejection、暂停的 companion read、stale-row cleanup cursor、swap duration，以及一次 search snapshot 是否只观察到一个 active generation。

## 5. Hierarchical BM25 确定性门禁

Hierarchical-BM25 suite 使用固定生成 fixture、固定 source scope、确定性插入顺序与 checked-in expectation。其 production-write/query-path fixture 当前只有一条 synthetic query 和一个 flat `graph_bm25` oracle；它没有提供 checked-in 自然词表 query list，也没有固定 runtime SQLite version。测试输出必须区分已实现 unit invariant 与 corpus measurement；不得把论文结果复制成 relay-knowledge 的结果行。

| 门禁 | 确定性规则 | 当前证据状态 |
| --- | --- | --- |
| 同 v4 score parity | 对比范围内每个公共文档在同一个 schema-v4 `graph_bm25` table 上的 routed 与 flat 执行必须产生相同 BM25 rank bits。该门禁不比较 v4 与 v3 schema 的分数，因为 indexed、zero-weight 的 `routing_key` 会改变 FTS5 length statistics。 | `bm25_hierarchy_suite_preserves_global_score_in_single_fts_intersection` 已覆盖一个代表性公共文档；corpus harness 必须扩展到全部 routed/flat 公共 hit。 |
| Single-FTS route intersection、hidden rank 与 candidate-domain reduction | 在同一个 `graph_bm25 MATCH` 中执行 business-column query、zero-weight scope64 token 与入选的 scope-qualified group token；独立 SQL scope predicate 仍是强制授权。在 CI 覆盖的每个受支持 SQLite build 上，`EXPLAIN QUERY PLAN` 必须只包含一个 `graph_bm25` virtual-table plan node。Plan 为有界 identity window 计算 hidden rank，再通过 route-document 的 `fts_rowid NOT NULL UNIQUE` sidecar 按 rowid hydrate，不能引入第二个 route FTS。固定 three-document fixture 必须保留两个已授权 flat candidate 和一个 routed candidate。4,096-document synthetic production-write/query-path fixture 的 planned-MATCH result domain 必须小于 flat result domain（当前 448 对 768）。 | `bm25_hierarchy_suite_preserves_global_score_in_single_fts_intersection`、`bm25_hierarchy_suite_partitions_scoped_common_terms_and_keeps_sql_authority` 与 `bm25_hierarchy_suite_production_routes_preserve_recall_and_reduce_candidate_domain` 覆盖 plan shape、result-domain reduction、partition 与 authorization；result-domain row count 不是 FTS posting、VM step 或 latency measurement，equal-score cutoff membership 也不确定。 |
| 持久 DF 准入与探测预算 | 每个 query term 的持久 global DF 必须等于最多读取 `df + 1` 行的 business-column `MATCH` count；任一 term 超过 global document 的 20% 都强制 flat fallback，所有探测合计预留不得超过 65,536 个 postings。 | `bm25_hierarchy_suite_activates_only_for_complete_current_routes` 覆盖 selective/common 混合 term fallback，`bm25_hierarchy_suite_bounds_term_validation_postings` 覆盖总量上限。自然 query harness 还必须报告实际 probe work 与 mismatch fallback rate。 |
| Sidecar、historical probe 与 fuzzy-label bound | `graph_bm25_route_documents` 必须保留 `fts_rowid NOT NULL UNIQUE`、`created_graph_version` 与可观察 `label_gram_state`。Historical unscoped authorized-corpus、label-state 与 `label_lower` probe 必须选择 version-leading global index；scoped probe 保留专用 index。Fuzzy 工作对每篇文档最多处理 256 个 labels、每个 label 1,024 UTF-8 bytes 与 8,192 个不同 grams；8,192-posting query budget exhaustion 必须可观察。 | Schema-marker、migration-EQP、unscoped-fallback-EQP、lifecycle 与 label-trigram 具名测试覆盖 exact index、hydrate identity、限制和 exhaustion signal；这些是 bounded-work/plan-shape invariant，不是自然语料质量测量。 |
| Shadow rebuild、fence 与 snapshot activation | 发布带 owner/expiry、phase/cursor checkpoint 和固定 semantic/vector plan 的 durable `building` lease，保持旧 flat FTS 可读，并允许过期 attempt 接管续跑。每个 transaction 最多接纳 128 篇文档、4 MiB 估算权威 source、8,192 个 labels 与 8,192 个 links。超过任一工作预算的单篇 document 独占 transaction 并发出 identity 受界 warning；这保证前进，但不是单文档绝对 byte bound。通过 `IMMEDIATE` fence 拒绝当前 evidence/code writer，暂停 semantic/vector/fuzzy read，并在一个事务内提交 active-to-retired/shadow-to-active rename、route `fresh` 与 schema marker。 | 具名测试覆盖 checkpoint takeover、全部累计工作预算、oversize-document isolation/warning bound、writer fence、companion read 暂停、complete-reader activation 与 swap rollback。Contended latency、WAL/磁盘 high-water mark 与完整 search snapshot 行为仍是 benchmark 输出，不是已声称的生产测量。 |
| Selected-document fraction | 每个准入 routed plan 都不得超过产品 25% 上限。固定的 5,000-document/100-group synthetic regression 在 40 个 group 命中时必须继续选择 500 个文档（10%）。 | 已由 `bm25_hierarchy_suite_bounds_selected_document_fraction` 覆盖；这是 synthetic budget result，不是生产语料实测。 |
| Coarse-boundary separation | Matching groups 必须超出 budget；最后一个入选 group 必须比第一个被拒 group 匹配更多 query terms，或 coarse score 至少领先 5%。全部 matching groups 都被选中和 equal-score dispersed-singleton 两种情况都必须走 flat fallback。 | 已由 `bm25_hierarchy_suite_skips_routes_that_cannot_reduce_candidates` 与 `bm25_hierarchy_suite_falls_back_when_coarse_scores_do_not_separate` 覆盖；这些是安全准入 invariant，不是 recall proof。 |
| Recall@k | 每个固定 query 和声明的 `k` 都要比较 routed IDs 与 flat top-k oracle，报告 `|routed_top_k intersect flat_top_k| / k`、aggregate 与 worst-query 值。生成式 4,096-document deterministic fixture 要求 Recall@10 >= 0.9。声称自然词表 corpus quality 可接受前，必须把 release floor 写入其 fixture manifest；equal-score cutoff membership 不要求确定。 | 具名 suite 只覆盖 synthetic Recall@10 floor；自然语料 recall/performance 仍未测，因此不声称 production floor 或 speedup。 |
| Query p50/p95 | 在一个记录完整的 hardware/storage/SQLite profile 上，对 routed/flat 使用同一版本化 warmup、repetition count、query order 与 cache-state protocol，报告绝对 latency 和 ratio。只有首个可复现 baseline 写入仓库后才能接受 ceiling。 | 尚未测量；论文 latency/throughput 不能作为本项目 baseline。 |
| Fallback rate | 分别统计 route-eligible attempts、routed completions、flat FTS retries 与后续 lexical-fallback outcomes，并报告 `flat_retries / route_eligible_attempts`。成功 flat FTS row 按稳定 hierarchy reason 分类；FTS-empty 后的 LIKE/trigram outcome 由 harness control flow 分类，不能要求这些 hit 携带 hierarchy explanation。再对 checked-in fixture expectation 设门禁。 | 尚未测量；flat-FTS explanation plumbing 已有覆盖，但本文不声称 corpus fallback rate。 |

确定性 unit gate 作为具名 `bm25_hierarchy_suite` product gate 进入 self-iteration 的 `fast`、`full` 与 `exhaustive` profile。它保护同 schema score parity、one-FTS hidden-rank/rowid-hydrate plan shape、hard scope authorization、候选减少、逐词 DF 准入、65,536-posting 校验上限、带四类累计工作预算的 durable resumable shadow rebuild、companion read 暂停、version-leading historical index、fuzzy-label limit、set-based bounded aggregate maintenance、selection budget 与一个 synthetic Recall@10 floor。Retriever-source disable 应抑制全部 disabled source family，而不是只覆盖部分 source。这些 gate 不证明 equal-rank cutoff 的确定 membership、rank safety、自然语料 recall/performance、低竞争 migration 或 speedup；其 hierarchical-BM25 bound 也不代表完整 hybrid pipeline 的全部既有层都达到 end-to-end bounded。只有版本化自然词表 Recall@k、p50/p95、selected-document-fraction、fallback-rate、rebuild-time 与临时资源结果行连同命令、corpus identity 和环境 metadata 一起出现，才能把该优化描述为 performance-complete。

## 6. 回归原则

- 不通过枚举 benchmark query、path、symbol 或 fixture 名称解决质量问题。
- 性能优化必须能解释通用机制，例如候选下推、索引结构、批处理、缓存、增量更新或并发边界。
- source fallback 只能作为有界 exact-text recovery；候选查询失败或预算耗尽时必须记录 degraded reason，不能绕过结构化排序和 scope 授权。
- 文件名查询和内容查询分开设预算；内容索引失败不得拖累文件定位。
- 所有指标必须能在 CLI、Web 或 benchmark harness 中复现，并记录命令、环境变量和数据版本。

## 7. 关联文档

- [竞争力、高性能与本机文件检索研究 2026](../04-research/08-competitive-performance-research-2026.md)
- [派生索引与新鲜度](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [混合检索与 Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [分层 BM25 算法分析 2026](../04-research/12-hierarchical-bm25-analysis-2026.md)
- [代码检索排序与影响分析](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)
- [C/C++ 语法型自迭代测评集 2026-05-20](06-c-cpp-syntax-self-iteration-evaluation.md)
- [多语言语法型自迭代测评集 2026-05-20](07-multilingual-syntax-self-iteration-evaluation.md)
