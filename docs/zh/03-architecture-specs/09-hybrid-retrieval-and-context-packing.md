# 混合检索与 Context Packing

[中文](../../zh/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md) | [English](../../en/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)

> 文档版本: 2.3
> 编制日期: 2026-08-11
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

混合检索是系统的算法核心。普通向量检索擅长相似内容，普通 BM25 擅长精确词项；`relay-knowledge` 需要同时回答术语、概念、多跳关系、时间事实、代码符号和影响分析，因此必须把多路召回、结构扩展、融合、rerank 和 context packing 作为一个算法整体。

## 2. 查询流程

```text
normalize query
  -> resolve source scope and freshness policy
  -> plan retriever families
  -> lexical (flat or single-FTS routed) / semantic / vector / graph / code / local file recall
  -> candidate normalization and dedup
  -> weighted reciprocal-rank fusion
  -> graph expansion and local rerank
  -> context pack budgeting
  -> response with provenance and freshness metadata
```

任何 retriever 都不能绕过 scope filter、authorization policy 或 freshness policy。Request-level `disabled_retriever_sources` 必须在融合前应用到 BM25/code-graph rows、graph evidence、semantic、vector、graph path、temporal 和 community-summary source。

Query planner 需要先识别查询意图：exact term、conceptual、multi-hop、temporal、code symbol、impact、file path、file content 或 mixed agent context。不同意图选择不同 retriever family 和预算；例如文件名/路径查询优先 `local_file_path` 和 metadata，内容问题才进入 `local_file_content`、BM25 或 semantic/vector 路径。

代码意图的召回顺序是 tree-sitter code graph、SQLite FTS/BM25、语义/向量补充，最后才是有界内部 exact-text source fallback。产品运行时兜底必须继承 source scope、path/language filter、authorization 和 freshness policy，并搜索已索引 commit 的物化候选内容而不是脏工作树；它只能产生 source span evidence，不能声明新的图边或覆盖 edge confidence。Agent 或维护者检查源码时，可以使用有界 `rg` 或 `grep -RIn` 搜索，但这只是开发/排障手段，不是产品查询热路径替代品。

## 3. 融合模型

基础融合使用加权 RRF：

```text
score(candidate) = sum(weight_i / (k + rank_i)) + structural_bonus - penalty
```

`structural_bonus` 来自 source authority、direct graph path、accepted lifecycle、exact symbol match、exact file path/basename match、fresh index 和 evidence confidence。`penalty` 来自 stale lag、degraded backend、ambiguous entity、low confidence、unauthorized candidate rejection 或 duplicate parent evidence。

RRF 之后允许多阶段 rerank，但 rerank 必须只处理有界候选窗口，并保留每个 retriever 的 rank contribution。BM25、向量、图路径、代码边和文件路径分数不可在未归一化时直接相加。

## 4. Hierarchical BM25 路由边界

Graph BM25 只拥有一个同时负责评分与路由的 FTS5 read model：

```text
bounded topical terms (path / labels / aliases / content)
  -> scope64 partition token + scope-qualified SimHash10 group + aggregate route metadata
  -> one graph_bm25 MATCH:
     {business columns}:(query)
     AND routing_key:(scope token)
     AND routing_key:(selected groups, when admitted)
  -> hidden-rank identity window -> rowid hydrate
```

`graph_bm25` 是唯一 FTS corpus。其 `routing_key` 为 indexed column，让 SQLite 在一次 virtual-table `MATCH` 中对 business query、scoped request 的 scope64 partition token 与入选 group token 求交；显式 business-column scope 防止 routing token 满足用户文本。`routing_key` 的固定 BM25 column weight 为零，因此没有直接 term-score contribution；但它仍进入 FTS5 document length 与 corpus average document length，所以 v4 score 可能不同于 pre-v4 baseline，而同一个 v4 table 上 routed 与 flat 执行的公共文档必须具有 bitwise-identical score。普通 SQL `source_scope` predicate 始终是硬授权 filter，不能信任 hash token 替代授权。

粗选 owner 对 path、label、alias 与 content 执行有界 10-bit SimHash。`source_scope` 进入 indexed inventory、稳定 64-bit partition token 与 scope-qualified group identity，但不进入 topical SimHash。版本化指纹是 `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4`：每个 document inventory 最多 256 个不同 safe-ASCII term，每个最多 128 bytes；可路由 query 最多 32 个这样的 term。Selector 使用的是仅 aggregate、类 `A` 的工程适配 `global_idf * log2(1 + group_collection_frequency)^2`，而不是论文的精确 `A`；它不实现 `B(c,Q)`，也不声称具备 document-level query-term co-occurrence。该工程适配不是论文的 balanced topical LDA，不能继承论文性能结果。

只有 graph/route version 与 algorithm identity 为 current、startup-reconciled source/global/sidecar population 一致，且 query 位于有界 ASCII-term 限制内时，才继续判断 routing。对每个 query term，从 `graph_bm25_route_term_totals` 读取的持久 global document frequency 必须等于仅限定 business column、最多探测 `df + 1` 行的 `graph_bm25 MATCH` 观测值；每个 term 都必须不超过 global corpus 的 20%，所有探测合计最多预留 65,536 个 postings。Population 还必须通过 minimum-size、group-count、per-group-size、skew 与 selected-document-fraction gate。Matching groups 必须超出 selection budget；最后一个入选 group 还必须比第一个被拒 group 匹配更多 query terms，或 coarse score 至少领先 5%。全部 matching groups 都被选中或边界不清晰时走 fallback。其余准入失败会禁用 hierarchy，并继续既有 flat/fallback lexical path。非 ASCII query 会跳过只接受 ASCII 的 routing-term extractor，并继续它既有的 flat FTS/fallback 行为。Routed query 没有返回行，或 approximate selection 下 distinct candidate 少于请求 limit 时，会重试 flat BM25。瞬时 SQLite query error 有界重试后仍失败时，本次 search 会把 BM25 source 视为暂时不可用；非瞬时 planning 或 query error 则显式上报。

近似边界必须显式：要求同 v4 公共文档 score parity，但 selected-group recall 不是 rank-safe。FTS5 hidden `rank` column 使用固定 `bm25(...)` weights 排序有界 identity window，第二个有界 query 再按 rowid hydrate；identity-window `LIMIT` 两侧完全同分时不承诺确定 membership。Routed hit 的 ranking explanation 记录完整 algorithm fingerprint、`aggregate_tf_idf`、selected/matching group counts、selected/population document counts 与 approximation state。Flat FTS 成功返回的 row 保留 planner 稳定的 `hierarchical_bm25 fallback=<reason>` explanation，其中包括 `no_candidate_reduction`、`coarse_score_margin` 与 route 后的 `routed_candidate_retry`。如果 FTS 没有 row，最终由后续 LIKE/trigram fallback 提供 hit，则该 hit 保留后级既有 explanation 行为。

SQLite schema marker v4 负责单一 global `graph_bm25`、route state/documents/groups、每组 collection-frequency terms 与持久 global route-term document frequency。Route document 保存 document identity/kind/scope/path、`created_graph_version`、可观测 `label_gram_state`、group token、有界 term-count JSON，以及与 `document_id` 配对用于精确 mutation/verification 的 `fts_rowid NOT NULL UNIQUE` sidecar。Document-write transaction 维护 route-state document count，并通过 set-based JSON 操作更新 aggregate statistics。Fresh-open reconciliation 比较 authoritative、active-global、route-document、grouped、semantic、vector 与持久 state population，以及 route algorithm/version/freshness 和 semantic/vector generation marker；派生状态缺失、不兼容或 count 不一致时，从权威 evidence/code document 重建。Canonical identity 与逐行 tokenizer consistency 只在其他 stale/schema/count gate 已触发重建后，于 plan/finalize 阶段校验；仅有 equal-count per-row drift 不会触发 fresh-open rebuild。Query hot path 读取持久 version/count/DF，不运行 full-table `COUNT` 或 `SUM`，随后执行上文的有界 business-column probe。

重建使用 shadow FTS generation。带 owner/expiry 的 durable lease 会连同 phase/cursor checkpoint 和固定 semantic/vector rebuild plan 发布 `building`，因此过期 attempt 可以被接管并续跑。每个 transaction 最多接纳 128 篇文档、4 MiB 估算权威 source bytes、8,192 个 labels 和 8,192 个 links；若第一篇文档本身越过一个或多个工作预算，则独占 transaction，并发出 identity fields 受界的 warning，该前进例外不代表单文档绝对 byte bound。旧的 flat `graph_bm25` 保持可读，semantic、vector 与 fuzzy lexical fallback 暂停，避免读取跨 generation companion；stale label/semantic/vector row 随后只用有界 rowid keyset cleanup 删除。当前 evidence/code writer 取得 `IMMEDIATE` transaction 后会在 lease 为 building 时拒绝写入。完整性校验通过后，一个短事务把 active 改名为 `graph_bm25_retired`、把 shadow 改名为 active、把 route state 发布为 `fresh`，并把 schema marker 写为 current；retired cleanup 在提交后执行。一次 graph search 会让所有已启用 source 共用一个 deferred read transaction，因此并发切代不能拆分其 SQLite snapshot。

Fuzzy label 另有独立界限：每篇文档最多 256 个 labels、每个 label 1,024 UTF-8 bytes、8,192 个不同 grams，每个 query 最多探测 8,192 个不同 document-label postings。同一规范化 document label 命中多个 query grams 只消耗一个 posting；label-gram 主键与 64-query-gram 上限因此同时给每个 posting 背后的 join rows 设定固定上界。Limit skip 通过 `label_gram_state` 持久化，posting exhaustion 是可观测的 fuzzy-only degraded。Historical unscoped fallback 的 semantic authorized-corpus probe、route label-state probe 和 `label_lower` hydrate 使用 version-leading global indexes；scoped request 保留 scope-leading index。这些界限只覆盖 BM25 与 lexical-fallback owner，不能据此声称既有 graph-evidence、path、temporal、community 或其他 hybrid layer 已端到端受界；它们仍需单独的 query-plan 证据。

因为 v4 改变 global derived FTS schema，只回滚二进制并不能精确回滚旧数值评分基线。Pre-v4 binary 可以提供既有 flat 行为，但既不维护 v4 route metadata，也不遵守当前应用的 rebuild fence；旧二进制有写入后还会恢复旧 schema marker。下次 v4 open 会利用该 marker transition 把表面兼容的 route state 也判为 invalid，并在 routing 前强制从权威数据重建。若要求精确旧 schema score，必须还原 pre-v4 database checkpoint。Upgrade/rebuild 必须独占 database，新旧 writer 不得并发操作。

## 5. 图扩展

Graph expansion 从高置信候选出发，只在预算内扩展：

- entity neighborhood。
- direct relation/claim/event path。
- schema-guided path。
- temporal predecessor/successor。
- code symbol reference/call/import edge。
- local file path/content evidence relation。

扩展结果必须带 path provenance，不能只返回“相关上下文”。

## 6. Context Pack

Context pack 是 agent 和 UI 的稳定证据包。它包含：query metadata、retriever sources、rank explanations、context items、source spans、graph paths、structured facts、code artifacts、local file artifacts、freshness、degraded state、budget、truncation reason 和 traversal provenance trace。`provenance_trace` 是 query-time 的有界解释对象，不持久化为后台任务；它必须在授权 scope 内记录 graph version、routed intent、visited nodes/edges、cited evidence、visited-but-uncited context、ranking contributions、stale/degraded 状态和 redaction/truncation 摘要。Storage search outcome 返回前必须先应用 request-level trace budget；application/agent adapter 在 rerank 和 citation marking 之后再应用最终 context budget，确保 cited evidence 仍可审计。Response-level truncation flag 必须包含 trace budget truncation，不能只反映 result count truncation。

Context packing 优先保证多样性和可引用性：同一父 evidence、同一 symbol、同一 source span 的重复命中会合并；低置信扩展不能挤掉直接 evidence。

Codegraph context pack 是面向 coding agent 的 one-call 编排。它执行有界 hybrid、definition 和 symbol 入口查询，围绕 top seed 通过 references、callers、callees 和 imports 展开，再按文件、符号、edge 与 line span 去重，并执行 `max_context_bytes` 预算。响应拆分 entry points、related symbols、graph paths、impact hints 和 code excerpts，每项都带 retrieval layer、score、line range 和 provenance。它复用既有 code graph 读模型和 freshness policy；不新增存储 schema，不启动后台 refresh，也不替代基于 diff 的 impact analysis。

## 7. 验收标准

- 精确术语、概念相似、多跳关系、时间事实和代码符号查询都有对应 retriever 信号。
- 文件名/路径和文件内容查询能区分 path、metadata、content 和 change cursor 的 freshness。
- 返回结果能解释每个 item 的来源、rank 贡献和 freshness。
- 具名 `bm25_hierarchy_suite` self-iteration gate 必须证明同 v4 公共文档 bitwise score parity、hidden-rank single-FTS plan 与 rowid hydrate、scope/business/group intersection、fixture candidate reduction、每个 term 的 20% DF 准入、65,536-posting 校验上限、current/complete route admission、historical global-index 与 fuzzy-limit guard、有界 selected-document fraction、durable takeover/checkpoint/work-budget/swap 不变量，以及一个生成式 4,096-document flat-oracle fixture 上 synthetic Recall@10 >= 0.9。它不承诺同分边界 membership、自然语料质量或所有 hybrid layer 的端到端界限；任何性能结论前，版本化自然词表 fixture 必须报告 Recall@k、p50/p95 与 fallback rate。
- Hierarchical selection 不能绕过 source scope 或 graph-version policy，任一 admission gate 失败都必须保留既有 non-hierarchical lexical path。
- 代码 exact-text fallback 命中必须保留 `text_fallback` provenance，并在候选路径或预算耗尽时返回 degraded reason；人工/agent 检查的 `rg`/`grep -RIn` fallback 路径需要单独记录。
- 宽 scope 的代码 exact-text fallback 必须先用 indexed FTS read model 根据 query、path filter 和 language filter 收敛候选路径，只有没有 query candidate 时才退回有界 scope 枚举。
- 任一 backend degraded 时仍能以可解释方式降级，而不是静默缺失。

## 8. 查询标识符智能提取

查询预处理（`retrieval/terms.rs`）从自然语言查询文本中识别和提取代码标识符模式，提升 FTS/BM25 召回率：

| 模式 | 示例 | 提取结果 |
| --- | --- | --- |
| PascalCase / CamelCase | `UserService`, `signInWithGoogle` | 原词 + 分词 |
| snake_case | `user_service`, `max_retries` | 原词 |
| SCREAMING_SNAKE_CASE | `MAX_RETRIES`, `API_KEY` | 原词 |
| dot.notation | `app.isPackaged` | 拆分各段 |
| ALL_CAPS 缩写 | `REST`, `HTTP`, `LRU` | 原词 |
| 小写标识符 (3+ chars) | `render`, `parse`, `undo` | 原词 |

停用词过滤覆盖至少 80 个常见英文词（the, and, for, with, from, how, what 等），在标识符提取阶段排除不可能对应代码符号的词。词干变体扩展对英文动词/名词变化生成匹配候选（connecting → connect, connected; renderer → render），扩大匹配面。提取的 PascalCase/CamelCase 标识符在 BM25/FTS 查询中获得 1.5x 权重，snake_case/SCREAMING_SNAKE 获得 1.3x 权重，小写标识符使用 0.8x 基础权重。

---

导航: 上一章: [8. 派生索引与新鲜度](08-derived-indexes-and-freshness.md) | 下一章: [10. Semantic/Vector Provider 架构](10-semantic-vector-provider-architecture.md)
