# 自迭代采纳优化记录归档 20260531

本文件归档 2026-05-31 附近的已采纳 self-iteration 记录，避免主记录 `04-self-iteration-accepted-optimizations.md` 超过 1000 行硬上限。原始 patch、报告和渐进式记忆仍保留在 `.git/relay-knowledge-self-iteration/`。

## run-1780213983-streaming-chunk-row-scoring

- 算法/架构：code retrieval chunk FTS 查询保持现有有界 candidate SQL、path/language 下推、BM25 ordering、score bonuses、dedupe/top-k 和 Hybrid graph-expansion gates，但 `search_chunks_with_fts_query` 不再先把 `rusqlite` rows 全量收集成中间 `Vec<ChunkRow>`；它从 SQLite iterator 逐行执行 selector 过滤、score 计算和 hit materialization，只保留最终返回给上层合并的 hits。
- 不变量/预期影响/风险：不改变 parser facts、SQLite schema、candidate limits、ranking math、source `text_fallback` 语义、repo-set overlay、semantic/vector read model、freshness、env/paths/net、任务 lease/checkpoint 或 CLI/API；预期降低 dense Hybrid、Definition fallback、References fallback 和多仓 fanout 中每个成员 chunk candidate window 的临时分配与 row-copy 峰值，风险是 row 迭代错误必须继续按原 StorageError 路径返回，受现有 code query 单测与 self-iteration performance cases 覆盖。
- 策略关联：建立在已采纳的 structured/dense Hybrid chunk-first planning、bounded candidate window 和 run-1780212377 repository-set overlay evidence index 之上；避免最近 rejected cluster 的 no-candidate-diff 模式，也避免扩大 grep/source fallback、枚举 case 字符串或削弱 semantic/vector/stability 保护层。
- patch: `/opt/workspace/relay-knowledge-spec/.git/relay-knowledge-self-iteration/patches-v2/run-1780213983.patch`
- score: 0.905188 (foundational=0.930723, competitive=0.788026, accuracy=0.859374, semantic_vector=1.000000, research_judge=n/a, performance=0.817016, stability=1.000000)
- cases: 269/308 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`
- key improvements: case:leveldb_cpp_inheritance_filter_policy_override_challenge false->true; metric:cargo_fmt_check_ms 3165.0->2623.0; metric:cargo_build_release_ms 179351.0->121964.0; metric:self_iteration_cargo_build_release_ms 29858.0->141.0; metric:cargo_clippy_ms 705.0->504.0; metric:cargo_test_ms 29117.0->23918.0; metric:self_iteration_cargo_clippy_ms 10793.0->383.0; metric:self_iteration_cargo_test_ms 6275.0->242.0
- known degradations: score_component:score 0.911695->0.9051875873239658; score_component:performance 0.854546->0.817016308687621; case_score:leveldb_cpp_inheritance_filter_policy_override 1.0->0.5; case_rank:leveldb_cpp_inheritance_filter_policy_override 1->2; metric:typescript_syntax_fixture_query_p95_ms 202.0->242.0; metric:relay_teams_query_p95_ms 2599.0->3526.0; metric:python_syntax_fixture_index_ms 2639.0->4131.0; metric:python_syntax_fixture_register_index_ms 2720.0->4172.0
- latency metrics: cargo_fmt_check_ms=2623ms; self_iteration_cargo_fmt_check_ms=323ms; linux_glibc_compatibility_policy_ms=142ms; cargo_build_release_ms=121964ms; self_iteration_cargo_build_release_ms=141ms; cargo_clippy_ms=504ms; cargo_test_ms=23918ms; self_iteration_cargo_clippy_ms=383ms
- adopted optimization notes: Rust self-iteration v2 accepted this candidate through the independent tools/self_iteration harness. The candidate is expected to improve the general retrieval, indexing, evaluation, or harness behavior described by the changed paths and recorded metrics.
