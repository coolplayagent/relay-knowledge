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

## 历史 compacted summaries

本节保存从主记录 `04-self-iteration-accepted-optimizations.md` 迁出的 compacted summaries，目的是让主记录继续低于 1000 行硬上限，同时保留历史 run 范围、分数、patch/report 位置与策略摘要。

## 20260517T212719Z-to-20260517T234741Z compacted
- summary: accepted early retrieval/ranking, QoS test, scoped caller, TypeScript finalize, and parser manual extraction records were compacted to keep this primary benchmark log under the 1000-line hard cap. Scores ranged from 0.925314 to 0.964671 with 36/36 cases passed; raw patches and metrics remain in `.git/relay-knowledge-self-iteration/patches/` and historical reports.
## 20260518T014540Z-to-20260518T041031Z compacted
- summary: accepted retrieval token-signature simplification, code-query path/call ranking, and code-batch indexing records from this range were compacted to keep this primary benchmark log under the 1000-line hard cap. Full patch, score, metric, and raw adopted-note details remain in `.git/relay-knowledge-self-iteration/patches/20260518T014540Z.patch`, `20260518T035623Z.patch`, `20260518T041031Z.patch`, and historical reports.
## 20260518T051523Z-to-20260518T071744Z compacted
- summary: accepted code-query ranking, call-direction, code-batch search/indexing, and caller ranking records from this range were compacted to keep this primary benchmark log under the 1000-line hard cap. Full score, metric, patch, and raw adopted-note details remain in `.git/relay-knowledge-self-iteration/patches/20260518T051523Z.patch`, `20260518T052713Z.patch`, `20260518T062727Z.patch`, `20260518T071744Z.patch`, and historical reports.
## 20260518T093310Z-to-20260518T094852Z compacted
- summary: accepted code scope/indexing and path-index cleanup records were compacted to keep this primary log under the 1000-line hard cap; full metrics and raw patches remain in `.git/relay-knowledge-self-iteration/patches/20260518T093310Z.patch`, `.git/relay-knowledge-self-iteration/patches/20260518T094852Z.patch`, and historical reports.
## 20260518T114107Z compacted
- summary: accepted finalize reference-resolution SQL compaction with score 0.94048 and 45/45 cases passed; raw patch, metrics, and notes remain in `.git/relay-knowledge-self-iteration/patches/20260518T114107Z.patch` and historical reports.
## 20260518T114915Z compacted
- summary: accepted compound symbol FTS/query-token expansion with score 0.94613, 45/45 cases passed, and semantic_vector/stability floors at 1.0. Full patch/report details remain in `.git/relay-knowledge-self-iteration/patches/20260518T114915Z.patch` and historical reports.
## 20260518T164005Z-20260518T184904Z archived
- summary: older accepted parser, import ranking, path ranking, benchmark latency, and research-judge entries were compacted on 2026-05-20 to keep this primary log under the tracked-file line cap; full patch/report details remain in `.git/relay-knowledge-self-iteration/patches/`, reports, and progressive memory.
## 20260518T195201Z-to-20260518T202013Z compacted
- summary: accepted hybrid chunk ranking and import target-symbol ranking records were compacted to keep this primary log under the tracked-file line cap; full patch, score, metric, and raw excerpt details remain in `.git/relay-knowledge-self-iteration/patches/` and historical reports.
## 20260518T213007Z compacted
- summary: accepted C parser optimization with score 0.916663 and 87/87 cases passed; detailed metrics remain in `.git/relay-knowledge-self-iteration/patches/20260518T213007Z.patch` and historical reports.
## 20260518T214222Z
- summary: search-document content optimization accepted with score 0.923286; raw metrics and patch excerpt were compacted to keep this primary log under the 1000-line hard cap. Full details remain in the patch/report artifacts.
## 20260518T220331Z-20260519T-import-usage archived
- summary: older accepted finalize, call-site, type-relationship, function-flow, header-declaration, and import-usage ranking notes were compacted to keep this primary log under the 1000-line hard cap; raw details remain in their patch/report artifacts and the archive document.
## 20260519T050321Z compacted
- summary: parser manual extraction candidate scored 0.885665 with 98/104 cases passed; detailed metrics remain in `.git/relay-knowledge-self-iteration/patches/20260519T050321Z.patch` and historical reports.
## 20260519T-run-1779223761-to-run-1779240531 compacted
- summary: Rust v2 harness migration, bounded SQLite schema maintenance, compact high-coverage hybrid chunks, history synthesis, and assigned-result caller ranking were compacted on 20260520 to keep this primary benchmark log under the repository line cap. Raw patch/report details remain under `.git/relay-knowledge-self-iteration/patches-v2/`, `reports-v2/`, and progressive memory summaries.
## run-1779242800-to-run-1779266107 compacted
- summary: accepted overlay evidence, repository-set bridge support, caller ranking, streamed call-search finalize, hybrid chunk anchor FTS, symbol identity fast path, edge language marker, repository-set fanout retry, export-key index, caller/callee identity fast path, and dynamic category guardrails are compacted to keep this primary log under the line cap; full raw metrics remain in `.git/relay-knowledge-self-iteration/patches-v2/`, `reports-v2/`, and progressive memory.
## run-1779279305-to-run-1779289571171823700 compacted
- summary: returned overlay pruning, vector lexical coverage tie-break, derived cursor streaming, and exact-path symbol/chunk ranking accepted records were compacted on 2026-05-20 to keep this primary benchmark log under the repository line cap; strategy details remain in the candidate sections above and raw metrics remain in `.git/relay-knowledge-self-iteration/patches-v2/`, `reports-v2/`, and progressive memory summaries.
## run-1779290426265110061-to-run-1779291615794938640 compacted
- summary: derived token signature filter and derived scope/version pushdown validate records were compacted on 2026-05-21 to keep this primary benchmark log under the repository line cap; full details moved to the archive document and raw patch/report artifacts remain under `.git/relay-knowledge-self-iteration/`.
## run-1779317832-to-run-1779323184 compacted
- summary: reference identity fast path, reference/call source excerpts, and C-family usage reference facts accepted records were compacted on 2026-05-21 to keep this primary benchmark log under the repository line cap; detailed metrics remain in `.git/relay-knowledge-self-iteration/patches-v2/`, `reports-v2/`, and progressive memory, with strategy details preserved in the candidate sections above.
## run-1779324524 compacted
- summary: accepted callee sequence, source excerpt, and graph BM25 stability records were compacted on 2026-05-21 to keep this primary benchmark log under the repository line cap; full metrics remain in `.git/relay-knowledge-self-iteration/patches-v2/run-1779324524.patch`, reports, and progressive memory, with strategy details preserved above.
## run-1779331577-to-run-1779363937 compacted
- summary: bidirectional call identity lookup, compact API-sequence hybrid ranking, and indirect function-pointer caller recovery are compacted here to keep this primary log under the file-length cap; full algorithms, metrics, and risks remain in `.git/relay-knowledge-self-iteration/patches-v2/`, reports, and progressive memory.
## run-1779365756-to-run-1779366747 compacted
- summary: accepted repository-set member diversification plus hybrid chunk anchor/designated-initializer scoring records were compacted on 2026-05-21 to keep this primary benchmark log under the 1000-line hard cap; full metrics remain in `.git/relay-knowledge-self-iteration/patches-v2/run-1779365756.patch`, `.git/relay-knowledge-self-iteration/patches-v2/run-1779366747.patch`, reports, and progressive memory, with strategy details preserved above.
## run-1779368858-to-run-1779395376 compacted
- summary: accepted source fallback, hybrid body/API/proximity ranking, reference usage context, declaration/call/excerpt ranking, compact API-sequence ranking, operation-surface ranking, and pure Hybrid symbol-identity planning records were compacted on 2026-05-22 to keep this primary benchmark log under the 1000-line hard cap; detailed algorithms, metrics, risks, raw patches, reports, and progressive memory remain under `.git/relay-knowledge-self-iteration/patches-v2/`, `reports-v2/`, and memory summaries, with strategy details preserved in the candidate sections above.
## run-1779399365 compacted
- summary: accepted repository-set dependency symbol plan scored 0.968533 with foundational, competitive, semantic_vector, and stability floors at 1.0; full patch, metrics, and raw report remain in `.git/relay-knowledge-self-iteration/patches-v2/run-1779399365.patch`, reports, and progressive memory.
## run-1779433139-to-run-1779434421 compacted
- summary: accepted reusable search-document buffers and lazy borrowed score fields are compacted here to keep this primary benchmark log under the 1000-line hard cap; latest accepted score was 0.963817 with foundational, competitive, semantic_vector, and stability floors at 1.0, performance 0.798985, 52/52 cases passed, and full metrics preserved in `.git/relay-knowledge-self-iteration/patches-v2/run-1779433139.patch`, `.git/relay-knowledge-self-iteration/patches-v2/run-1779434421.patch`, reports, and progressive memory.
## run-1779435315 compacted
- summary: latest accepted checkpoint batch delete-elision scored 0.964516 with foundational, competitive, accuracy, semantic_vector, and stability floors at 1.0; performance was 0.802867 and 52/52 cases passed. Full metrics, patch, and adopted notes remain in `.git/relay-knowledge-self-iteration/patches-v2/run-1779435315.patch`, reports, and progressive memory; strategy details remain in the candidate section above.
## run-1779439043-to-run-1779463498 compacted
- summary: accepted code-search backfill marker, repository-set dependency API symbol query/direct plan, line-aware reference evidence, repository-set API identity coverage/declaration demotion, and bounded external import fallback records are compacted to keep this primary benchmark log under the 1000-line hard cap. Latest accepted in this range was `run-1779463498` with score 0.969160 and foundational, competitive, accuracy, semantic_vector, and stability floors at 1.0; full raw metrics, changed paths, adopted notes, patches, reports, and progressive memory remain under `.git/relay-knowledge-self-iteration/patches-v2/`, `reports-v2/`, and memory summaries, with strategy details preserved in candidate sections above.
## run-1779465105 compacted
- summary: accepted hybrid symbol elision with BM25 query retry scored 0.976355 with foundational, competitive, accuracy, semantic_vector, and stability floors at 1.0; full metrics, patch, changed paths, and adopted notes remain in `.git/relay-knowledge-self-iteration/patches-v2/run-1779465105.patch`, reports, and progressive memory, with strategy details preserved in the candidate section above.
## run-1779596127-to-run-1779596855 compacted
- summary: accepted internal source grep fallback and repository-set symbol fallback merge records are compacted here to keep this primary benchmark log under the 1000-line hard cap. Latest accepted `run-1779596855` scored 0.954442 with foundational=1.0, competitive=0.909091, semantic_vector=1.0, stability=1.0, and performance=0.858012; full raw metrics, changed paths, patches, reports, and progressive memory remain under `.git/relay-knowledge-self-iteration/patches-v2/`, `reports-v2/`, and memory summaries.
## run-1779602603 compacted
- summary: accepted TypeScript import-edge/source fallback recovery scored 0.970528 with foundational=1.0, competitive=0.988636, semantic_vector=1.0, stability=1.0, and performance=0.850156; full patch, changed paths, metrics, and report remain in `.git/relay-knowledge-self-iteration/patches-v2/run-1779602603.patch`, reports, and progressive memory.
## run-1779602969 compacted
- summary: accepted batched Git blob size/read source-grep optimization scored 0.971214 with foundational=1.0, competitive=0.988636, semantic_vector=1.0, stability=1.0, and performance=0.853969; full metrics, degradations, patch, and report remain in `.git/relay-knowledge-self-iteration/patches-v2/run-1779602969.patch`, reports, and progressive memory.
## run-1779604104-to-run-1779619958 compacted
- summary: accepted repository-set Hybrid gating, adaptive parser fanout, API-dense Hybrid chunk recall, and collective chunk gate records are compacted here to keep this primary benchmark log under the 1000-line hard cap. Scores ranged from 0.974107 to 0.977769 with foundational=1.0, semantic_vector=1.0, and stability=1.0; full patches, metrics, changed paths, and reports remain under `.git/relay-knowledge-self-iteration/patches-v2/`, `reports-v2/`, and progressive memory.
