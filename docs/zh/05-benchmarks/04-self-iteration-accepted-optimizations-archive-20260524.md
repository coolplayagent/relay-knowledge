# 自迭代采纳优化归档：20260524 后期详细记录

本归档保存 `04-self-iteration-accepted-optimizations.md` 中 20260524-20260526 的后期详细条目，避免主记录文件超过 1000 行。主文件继续保留候选算法说明和摘要；完整 patch、报告和渐进式 memory 仍保留在 `.git/relay-knowledge-self-iteration/`。

## run-1779620755

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches-v2/run-1779620755.patch`
- score: 0.980082 (foundational=1.000000, competitive=1.000000, accuracy=1.000000, semantic_vector=1.000000, research_judge=n/a, performance=0.889347, stability=1.000000)
- cases: 51/51 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query_symbol_ranking_tests.rs`, `src/relay_knowledge/storage/sqlite/code_query_symbols.rs`
- key improvements: score_component:competitive_capability 0.988636->1.0; case_score:typescript_syntax_hybrid_typed_arrow_projector 0.625->1.0; case_rank:typescript_syntax_hybrid_typed_arrow_projector 4->1; metric:cargo_fmt_check_ms 1693.0->1615.0; metric:self_iteration_cargo_fmt_check_ms 303.0->263.0; metric:c_syntax_fixture_index_ms 202.0->122.0; metric:c_syntax_fixture_register_index_ms 243.0->183.0; metric:c_syntax_fixture_query_p50_ms 247.0->181.0
- known degradations: metric:temporal_sdk_go_index_ms 503.0->565.0; metric:temporal_sdk_go_register_index_ms 564.0->646.0; metric:leveldb_cpp_index_ms 121.0->262.0; metric:leveldb_cpp_register_index_ms 187.0->303.0; metric:typescript_syntax_fixture_query_p95_ms 1616.0->1696.0
- latency metrics: cargo_fmt_check_ms=1615ms; self_iteration_cargo_fmt_check_ms=263ms; cargo_build_debug_ms=263ms; self_iteration_cargo_check_ms=102ms; temporal_samples_go_index_ms=144ms; temporal_samples_go_register_index_ms=205ms; temporal_sdk_go_index_ms=565ms; temporal_sdk_go_register_index_ms=646ms

## run-1779626871

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches-v2/run-1779626871.patch`
- score: 0.981447 (foundational=1.000000, competitive=1.000000, accuracy=1.000000, semantic_vector=1.000000, research_judge=n/a, performance=0.896925, stability=1.000000)
- cases: 51/51 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/application/code_query_source_fallback.rs`, `src/relay_knowledge/application/code_query_source_fallback_tests.rs`, `src/relay_knowledge/application/code_query_source_surface.rs`, `src/relay_knowledge/application/code_repository_set_service.rs`, `src/relay_knowledge/application/mod.rs`, `src/relay_knowledge/code/parser/manual.rs`, `src/relay_knowledge/code/parser/records.rs`, `src/relay_knowledge/code/parser_exported_value_tests.rs`
- key improvements: score_component:score 0.972908->0.9814465778455568; score_component:performance 0.849491->0.8969254324753153; metric:cargo_build_debug_ms 15782.0->223.0; metric:cpp_syntax_fixture_index_ms 181.0->40.0; metric:cpp_syntax_fixture_register_index_ms 242.0->101.0; metric:cpp_syntax_fixture_query_p95_ms 465.0->303.0; metric:relay_teams_query_p50_ms 202.0->167.0; metric:c_syntax_fixture_index_ms 222.0->124.0
- known degradations: metric:temporal_sdk_go_index_ms 428.0->686.0; metric:temporal_sdk_go_register_index_ms 489.0->752.0; metric:relay_teams_index_ms 687.0->727.0; metric:relay_teams_register_index_ms 749.0->794.0; metric:relay_teams_query_p95_ms 586.0->629.0; metric:c_syntax_fixture_query_p50_ms 161.0->242.0; metric:leveldb_cpp_register_index_ms 263.0->303.0; metric:leveldb_cpp_query_p95_ms 203.0->304.0
- latency metrics: cargo_fmt_check_ms=1574ms; self_iteration_cargo_fmt_check_ms=303ms; cargo_build_debug_ms=223ms; self_iteration_cargo_check_ms=101ms; temporal_sdk_go_index_ms=686ms; temporal_sdk_go_register_index_ms=752ms; temporal_samples_go_index_ms=121ms; temporal_samples_go_register_index_ms=186ms

## run-1779644421

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches-v2/run-1779644421.patch`
- score: 0.884094 (foundational=0.908784, competitive=0.753324, accuracy=0.831054, semantic_vector=1.000000, research_judge=0.860000, performance=0.815574, stability=1.000000)
- cases: 214/251 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite.rs`, `src/relay_knowledge/storage/sqlite/code_batch.rs`, `src/relay_knowledge/storage/sqlite/retry.rs`
- key improvements: score_component:score 0.866459->0.884094314763636; score_component:foundational_capability 0.894531->0.9087837837837838; score_component:performance 0.788902->0.8155736174577234; score_component:stability 0.989362->1.0; score_component:research_judge 0.82->0.86; gate:typescript_syntax_fixture_index false->true; gate:javascript_syntax_fixture_index false->true; gate:kotlin_syntax_fixture_index false->true
- known degradations: metric:python_syntax_fixture_index_ms 1310.0->1913.0; metric:python_syntax_fixture_register_index_ms 1492.0->1955.0; metric:ruby_syntax_fixture_index_ms 1512.0->2035.0; metric:ruby_syntax_fixture_register_index_ms 1594.0->2076.0; metric:php_syntax_fixture_index_ms 1473.0->1554.0; metric:opencode_typescript_query_p50_ms 524.0->704.0; metric:leveldb_cpp_query_p50_ms 404.0->727.0; metric:leveldb_cpp_query_p95_ms 4532.0->17776.0
- latency metrics: cargo_fmt_check_ms=1634ms; self_iteration_cargo_fmt_check_ms=262ms; cargo_build_release_ms=85712ms; self_iteration_cargo_build_release_ms=142ms; cargo_clippy_ms=15450ms; cargo_test_ms=34380ms; self_iteration_cargo_clippy_ms=283ms; self_iteration_cargo_test_ms=202ms

## run-1779652179

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches-v2/run-1779652179.patch`
- score: 0.884525 (foundational=0.922297, competitive=0.759104, accuracy=0.840701, semantic_vector=1.000000, research_judge=0.860000, performance=0.796582, stability=1.000000)
- cases: 216/251 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/code/identity/mod.rs`, `src/relay_knowledge/code/identity/script.rs`, `src/relay_knowledge/code/parser/imports.rs`, `src/relay_knowledge/code/parser_import_resolution_tests.rs`, `src/relay_knowledge/storage/sqlite/code_query_import_ranking_tests.rs`
- key improvements: score_component:score 0.692099->0.8845254920334789; score_component:foundational_capability 0.908784->0.9222972972972973; score_component:competitive_capability 0.751879->0.7591040462427746; score_component:research_judge 0.0->0.86; gate:research_judge false->true; case:ruby_syntax_imports_require_relative_extensions false->true; case_rank:leveldb_cpp_inheritance_filter_policy_override_challenge null->5; case_score:leveldb_hybrid_recovery_manifest_full_scope 0.5->1.0
- known degradations: score_component:performance 0.801698->0.7965817575444437; case_score:opencode_ts_implementation_data_migration_service_challenge 1.0->0.75; case_rank:opencode_ts_implementation_data_migration_service_challenge 1->2; case_rank:otel_go_repo_set_receiver_factory_create_logs 5->10; case_rank:otel_go_repo_set_filelog_component_type 6->null; metric:temporal_samples_go_index_ms 1634.0->3394.0; metric:temporal_samples_go_register_index_ms 1675.0->3435.0; metric:relay_teams_query_p50_ms 402.0->504.0
- latency metrics: cargo_fmt_check_ms=1676ms; self_iteration_cargo_fmt_check_ms=282ms; cargo_build_release_ms=86811ms; self_iteration_cargo_build_release_ms=243ms; cargo_clippy_ms=363ms; cargo_test_ms=14775ms; self_iteration_cargo_clippy_ms=242ms; self_iteration_cargo_test_ms=161ms

## run-1779662884

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches-v2/run-1779662884.patch`
- score: 0.888382 (foundational=0.922297, competitive=0.759104, accuracy=0.840701, semantic_vector=1.000000, research_judge=0.860000, performance=0.822289, stability=1.000000)
- cases: 216/251 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_set.rs`, `src/relay_knowledge/storage/sqlite/code_set_tests.rs`
- key improvements: score_component:score 0.872638->0.8883815451024761; score_component:performance 0.775995->0.822288778004426; score_component:research_judge 0.82->0.86; case_rank:otel_go_repo_set_filelog_component_type null->7; case_score:research_judge 0.82->0.86; metric:cargo_fmt_check_ms 1776.0->1676.0; metric:temporal_samples_go_index_ms 1637.0->1575.0; metric:scala_syntax_fixture_index_ms 9032.0->564.0
- known degradations: metric:self_iteration_cargo_build_release_ms 122.0->222.0; metric:self_iteration_cargo_clippy_ms 202.0->262.0; metric:swift_syntax_fixture_index_ms 887.0->928.0; metric:swift_syntax_fixture_register_index_ms 928.0->968.0; metric:relay_teams_query_p50_ms 282.0->465.0; metric:otel_collector_contrib_index_ms 370144.0->399267.0; metric:otel_collector_contrib_register_index_ms 370185.0->399348.0; metric:opencode_typescript_index_ms 37265.0->47417.0
- latency metrics: cargo_fmt_check_ms=1676ms; self_iteration_cargo_fmt_check_ms=283ms; cargo_build_release_ms=87456ms; self_iteration_cargo_build_release_ms=222ms; cargo_clippy_ms=383ms; cargo_test_ms=14969ms; self_iteration_cargo_clippy_ms=262ms; self_iteration_cargo_test_ms=161ms

## run-1779664778

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches-v2/run-1779664778.patch`
- score: 0.894857 (foundational=0.922297, competitive=0.759104, accuracy=0.840701, semantic_vector=1.000000, research_judge=n/a, performance=0.805269, stability=1.000000)
- cases: 215/250 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_set.rs`, `src/relay_knowledge/storage/sqlite/code_set/manifest.rs`, `src/relay_knowledge/storage/sqlite/code_set_tests.rs`
- key improvements: none recorded
- known degradations: none recorded
- latency metrics: cargo_fmt_check_ms=1877ms; self_iteration_cargo_fmt_check_ms=303ms; cargo_build_release_ms=94238ms; self_iteration_cargo_build_release_ms=13032ms; cargo_clippy_ms=383ms; cargo_test_ms=15590ms; self_iteration_cargo_clippy_ms=283ms; self_iteration_cargo_test_ms=181ms

Adopted optimization notes:

Rust self-iteration v2 accepted this candidate through the independent tools/self_iteration harness. The candidate is expected to improve the general retrieval, indexing, evaluation, or harness behavior described by the changed paths and recorded metrics.

## run-1779694672-to-run-1779697627 compacted

- summary: accepted ambiguous overlay priority and strict API-dense Hybrid chunk pass records are compacted here to keep this primary benchmark log under the hard line cap. Latest accepted `run-1779694672` scored 0.982871 with foundational, competitive, accuracy, semantic_vector, and stability floors at 1.0; full patches, metrics, changed paths, reports, and progressive memory remain under `.git/relay-knowledge-self-iteration/patches-v2/`, `reports-v2/`, and memory summaries.

## run-1779720535

- summary: accepted SBOM dependency inventory query scored 0.980875 with foundational, competitive, accuracy, semantic_vector, and stability floors at 1.0; `fast` passed 146/146 gates and 68/68 cases, including Cargo, npm, Go, Python, Maven BOM, Gradle, and Conan manifest/lockfile guardrails. Full patch, changed paths, metrics, and accepted notes remain in `.git/relay-knowledge-self-iteration/patches-v2/run-1779720535.patch`, `.git/relay-knowledge-self-iteration/reports-v2/run-1779720535.json`, and progressive memory.

## manual-glibc-release-guard-2026-05-25

- changed paths: `.github/workflows/release.yml`, `.github/workflows/pr-checks.yml`, `tools/release/check_linux_glibc_compat.py`, `tools/self_iteration/src/evaluator_tail.rs`, release and skill docs
- optimization: lock Linux GNU release and skill asset binaries to a glibc 2.31 ABI ceiling, and add a fast self-iteration policy gate that self-tests the checker and verifies release workflow coverage.
- invariant: no retrieval scoring, parser, storage schema, runtime state, CLI JSON, or network behavior changes; the guard fails only release/packaging policy when a Linux GNU binary or workflow can reintroduce a higher GLIBC dependency.
- expected impact: issue #156-style Ubuntu 20.04/GLIBC_2.31 startup regressions are caught before release or skill packaging, while fast self-iteration stays lightweight because it validates policy rather than building release artifacts.

## manual-code-index-lease-recovery-2026-05-26

- changed paths: `src/relay_knowledge/application/code_service.rs`, `src/relay_knowledge/storage/sqlite/code_tasks.rs`, `tools/self_iteration/src/evaluator_tail.rs`, recovery docs
- optimization: make code-index task leases attempt-scoped and recoverable, renew active leases after each checkpoint batch, expose checkpoint `updated_at_ms`, and add fast `code_index_recovery_cases`.
- invariant: no parser facts, ranking, FTS documents, source scopes, repo-set overlay semantics, release artifacts, or query hot-path behavior change; recovery only changes background task lifecycle and diagnostics.
- expected impact: issue #161-style stuck large-repository indexing now retries or dead-letters instead of holding an expired running lease, while active long indexes keep their lease alive as checkpoints advance.

## run-1779722194 compacted

- summary: accepted internal exact-text source fallback, query-aware candidate recovery, external dependency source diagnostics, and self-iteration guardrails. Score 0.972946 with 68/68 cases passing; key gains included competitive capability 0.979592->0.994898, stability 0.993506->1.0, and the late comment source-fallback budget guardrail passing. Known tradeoffs were lower performance score 0.894786->0.855934 and one TypeScript import case rank drop, with full run details retained in the self-iteration patch/report artifacts.
