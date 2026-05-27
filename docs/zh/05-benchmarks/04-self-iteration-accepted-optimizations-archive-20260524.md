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

## run-1779847461

- patch: `/opt/workspace/relay-knowledge-spec/.git/relay-knowledge-self-iteration/patches-v2/run-1779847461.patch`
- score: 0.972124 (foundational=1.000000, competitive=0.996377, accuracy=0.998188, semantic_vector=1.000000, research_judge=n/a, performance=0.849559, stability=1.000000)
- cases: 92/92 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_fts.rs`, `src/relay_knowledge/storage/sqlite/code_query_hybrid_chunk_gate_tests.rs`
- key improvements: none recorded
- known degradations: none recorded
- latency metrics: cargo_fmt_check_ms=3586ms; self_iteration_cargo_fmt_check_ms=524ms; linux_glibc_compatibility_policy_ms=142ms; skill_metadata_policy_cases_ms=423ms; cargo_build_debug_ms=33465ms; self_iteration_cargo_check_ms=684ms; code_index_recovery_cases_ms=19506ms; code_index_sqlite_lock_cases_ms=20449ms

## run-1779849943

- patch: `/opt/workspace/relay-knowledge-spec/.git/relay-knowledge-self-iteration/patches-v2/run-1779849943.patch`
- score: 0.979772 (foundational=1.000000, competitive=0.996377, accuracy=0.998188, semantic_vector=1.000000, research_judge=n/a, performance=0.892052, stability=1.000000)
- cases: 92/92 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_hybrid_chunk_gate_tests.rs`
- key improvements: score_component:score 0.972124->0.9797723323836461; score_component:performance 0.849559->0.8920524101828967; metric:self_iteration_cargo_fmt_check_ms 524.0->424.0; metric:skill_metadata_policy_cases_ms 423.0->302.0; metric:cargo_build_debug_ms 33465.0->627.0; metric:self_iteration_cargo_check_ms 684.0->542.0; metric:code_index_recovery_cases_ms 19506.0->1067.0; metric:code_index_sqlite_lock_cases_ms 20449.0->1770.0
- known degradations: metric:cargo_fmt_check_ms 3586.0->4517.0; metric:temporal_samples_go_index_ms 262.0->342.0; metric:temporal_samples_go_register_index_ms 363.0->423.0; metric:cross_language_syntax_fixture_register_index_ms 346.0->890.0; metric:cpp_syntax_fixture_query_p50_ms 302.0->339.0; metric:cpp_syntax_fixture_query_p95_ms 645.0->792.0; metric:c_syntax_fixture_index_ms 162.0->404.0; metric:c_syntax_fixture_register_index_ms 324.0->585.0
- latency metrics: cargo_fmt_check_ms=4517ms; self_iteration_cargo_fmt_check_ms=424ms; linux_glibc_compatibility_policy_ms=141ms; skill_metadata_policy_cases_ms=302ms; cargo_build_debug_ms=627ms; self_iteration_cargo_check_ms=542ms; code_index_recovery_cases_ms=1067ms; code_index_sqlite_lock_cases_ms=1770ms

## run-1779852601

- patch: `/opt/workspace/relay-knowledge-spec/.git/relay-knowledge-self-iteration/patches-v2/run-1779852601.patch`
- score: 0.997401 (foundational=1.000000, competitive=0.996377, accuracy=0.998188, semantic_vector=1.000000, research_judge=n/a, performance=0.989988, stability=1.000000)
- cases: 92/92 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query_symbol_ranking_tests.rs`, `src/relay_knowledge/storage/sqlite/code_query_symbols.rs`
- key improvements: score_component:score 0.974028->0.9974007392537516; score_component:performance 0.86014->0.9899880039057051; metric:cargo_fmt_check_ms 2462.0->866.0; metric:self_iteration_cargo_fmt_check_ms 383.0->121.0; metric:linux_glibc_compatibility_policy_ms 141.0->40.0; metric:skill_metadata_policy_cases_ms 262.0->80.0; metric:cargo_build_debug_ms 20167.0->120.0; metric:self_iteration_cargo_check_ms 584.0->121.0
- known degradations: none recorded
- latency metrics: cargo_fmt_check_ms=866ms; self_iteration_cargo_fmt_check_ms=121ms; linux_glibc_compatibility_policy_ms=40ms; skill_metadata_policy_cases_ms=80ms; cargo_build_debug_ms=120ms; self_iteration_cargo_check_ms=121ms; code_index_recovery_cases_ms=301ms; code_index_sqlite_lock_cases_ms=482ms

## run-1779847089

- patch: `/opt/workspace/relay-kownledge-process/.git/relay-knowledge-self-iteration/patches-v2/run-1779847089.patch`
- score: 0.960406 (foundational=1.000000, competitive=0.996377, accuracy=0.998188, semantic_vector=1.000000, research_judge=n/a, performance=0.784462, stability=1.000000)
- cases: 91/91 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query_symbols.rs`
- key improvements: score_component:score 0.842703->0.96040614541092; score_component:semantic_vector 0.0->1.0; gate:semantic_vector_provider_probe false->true; metric:self_iteration_cargo_check_ms 3352.0->564.0; metric:code_index_recovery_cases_ms 887.0->846.0; metric:code_index_sqlite_lock_cases_ms 1692.0->1168.0; metric:code_index_health_isolation_cases_ms 3371.0->2803.0; metric:temporal_samples_go_index_ms 15054.0->14002.0
- known degradations: score_component:performance 0.859586->0.7844624825566405; metric:cargo_fmt_check_ms 2158.0->2561.0; metric:self_iteration_cargo_fmt_check_ms 344.0->403.0; metric:temporal_sdk_go_index_ms 63471.0->108316.0; metric:temporal_sdk_go_register_index_ms 63552.0->108377.0; metric:grep_budget_fixture_index_ms 61.0->202.0; metric:grep_budget_fixture_register_index_ms 102.0->585.0; metric:grep_budget_fixture_query_p50_ms 61.0->142.0
- latency metrics: cargo_fmt_check_ms=2561ms; self_iteration_cargo_fmt_check_ms=403ms; linux_glibc_compatibility_policy_ms=122ms; cargo_build_debug_ms=343ms; self_iteration_cargo_check_ms=564ms; code_index_recovery_cases_ms=846ms; code_index_sqlite_lock_cases_ms=1168ms; code_index_health_isolation_cases_ms=2803ms

## run-1779849444

- patch: `/opt/workspace/relay-kownledge-process/.git/relay-knowledge-self-iteration/patches-v2/run-1779849444.patch`
- score: 0.962558 (foundational=1.000000, competitive=0.996377, accuracy=0.998188, semantic_vector=1.000000, research_judge=n/a, performance=0.796416, stability=1.000000)
- cases: 91/91 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/code/parser.rs`, `src/relay_knowledge/code/parser/records.rs`, `src/relay_knowledge/code/parser_identity_tests.rs`, `src/relay_knowledge/code/parser_tests.rs`
- key improvements: score_component:performance 0.784462->0.7964158487184357; metric:temporal_sdk_go_index_ms 108316.0->98164.0; metric:temporal_sdk_go_register_index_ms 108377.0->98247.0; metric:typescript_syntax_fixture_index_ms 101.0->61.0; metric:typescript_syntax_fixture_register_index_ms 162.0->123.0; metric:typescript_syntax_fixture_query_p50_ms 385.0->222.0; metric:typescript_syntax_fixture_query_p95_ms 2334.0->2036.0; metric:grep_budget_fixture_index_ms 202.0->101.0
- known degradations: metric:cargo_fmt_check_ms 2561.0->2720.0; metric:self_iteration_cargo_fmt_check_ms 403.0->443.0; metric:cargo_build_debug_ms 343.0->444.0; metric:self_iteration_cargo_check_ms 564.0->604.0; metric:code_index_recovery_cases_ms 846.0->966.0; metric:code_index_sqlite_lock_cases_ms 1168.0->1227.0; metric:code_index_health_isolation_cases_ms 2803.0->2976.0; metric:leveldb_cpp_register_index_ms 323.0->363.0
- latency metrics: cargo_fmt_check_ms=2720ms; self_iteration_cargo_fmt_check_ms=443ms; linux_glibc_compatibility_policy_ms=121ms; cargo_build_debug_ms=444ms; self_iteration_cargo_check_ms=604ms; code_index_recovery_cases_ms=966ms; code_index_sqlite_lock_cases_ms=1227ms; code_index_health_isolation_cases_ms=2976ms

## run-1779850939

- patch: `/opt/workspace/relay-kownledge-process/.git/relay-knowledge-self-iteration/patches-v2/run-1779850939.patch`
- score: 0.966565 (foundational=1.000000, competitive=0.996377, accuracy=0.998188, semantic_vector=1.000000, research_judge=n/a, performance=0.818679, stability=1.000000)
- cases: 91/91 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`, `src/relay_knowledge/storage/sqlite/code_batch/finalize_search.rs`, `src/relay_knowledge/storage/sqlite/code_batch/finalize_tests.rs`, `src/relay_knowledge/storage/sqlite/code_batch_search_tests.rs`
- key improvements: score_component:performance 0.796416->0.8186792443475324; metric:self_iteration_cargo_fmt_check_ms 443.0->383.0; metric:leveldb_cpp_index_ms 282.0->202.0; metric:leveldb_cpp_register_index_ms 363.0->303.0; metric:leveldb_cpp_query_p50_ms 342.0->183.0; metric:leveldb_cpp_query_p95_ms 669.0->243.0; metric:grep_budget_fixture_register_index_ms 465.0->384.0; metric:relay_teams_index_ms 346495.0->289934.0
- known degradations: metric:cargo_fmt_check_ms 2720.0->3038.0; metric:cargo_build_debug_ms 444.0->544.0; metric:self_iteration_cargo_check_ms 604.0->894.0; metric:code_index_recovery_cases_ms 966.0->2039.0; metric:code_index_sqlite_lock_cases_ms 1227.0->3260.0; metric:code_index_health_isolation_cases_ms 2976.0->5899.0; metric:temporal_samples_go_index_ms 13749.0->24850.0; metric:temporal_samples_go_register_index_ms 13831.0->24973.0
- latency metrics: cargo_fmt_check_ms=3038ms; self_iteration_cargo_fmt_check_ms=383ms; linux_glibc_compatibility_policy_ms=102ms; cargo_build_debug_ms=544ms; self_iteration_cargo_check_ms=894ms; code_index_recovery_cases_ms=2039ms; code_index_sqlite_lock_cases_ms=3260ms; code_index_health_isolation_cases_ms=5899ms

## run-1779854865

- patch: `/opt/workspace/relay-kownledge-process/.git/relay-knowledge-self-iteration/patches-v2/run-1779854865.patch`
- score: 0.971385 (foundational=1.000000, competitive=0.996377, accuracy=0.998188, semantic_vector=1.000000, research_judge=n/a, performance=0.845455, stability=1.000000)
- cases: 91/91 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`, `src/relay_knowledge/storage/sqlite/code_batch/finalize_search.rs`, `src/relay_knowledge/storage/sqlite/code_batch_search_tests.rs`
- key improvements: score_component:score 0.953379->0.9713848177515554; score_component:performance 0.745421->0.845455106671282; metric:cargo_fmt_check_ms 885.0->846.0; metric:cargo_build_debug_ms 19470.0->141.0; metric:self_iteration_cargo_check_ms 463.0->80.0; metric:code_index_recovery_cases_ms 4510.0->342.0; metric:code_index_sqlite_lock_cases_ms 5162.0->462.0; metric:code_index_health_isolation_cases_ms 7158.0->1026.0
- known degradations: metric:leveldb_cpp_query_p95_ms 849.0->1286.0; metric:cross_language_syntax_fixture_query_p95_ms 406.0->525.0; metric:c_syntax_fixture_register_index_ms 505.0->547.0; metric:c_syntax_fixture_query_p50_ms 362.0->509.0; metric:c_syntax_fixture_query_p95_ms 814.0->1150.0; metric:cpp_syntax_fixture_index_ms 81.0->202.0; metric:cpp_syntax_fixture_query_p50_ms 364.0->488.0; metric:cpp_syntax_fixture_query_p95_ms 786.0->1502.0
- latency metrics: cargo_fmt_check_ms=846ms; self_iteration_cargo_fmt_check_ms=120ms; linux_glibc_compatibility_policy_ms=40ms; cargo_build_debug_ms=141ms; self_iteration_cargo_check_ms=80ms; code_index_recovery_cases_ms=342ms; code_index_sqlite_lock_cases_ms=462ms; code_index_health_isolation_cases_ms=1026ms

## run-1779856832

- patch: `/opt/workspace/relay-kownledge-process/.git/relay-knowledge-self-iteration/patches-v2/run-1779856832.patch`
- score: 0.979657 (foundational=1.000000, competitive=0.996377, accuracy=0.998188, semantic_vector=1.000000, research_judge=n/a, performance=0.891409, stability=1.000000)
- cases: 91/91 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query_calls.rs`
- key improvements: score_component:score 0.965217->0.9796565281799409; score_component:performance 0.811191->0.8914090534956454; metric:cargo_fmt_check_ms 2853.0->2254.0; metric:self_iteration_cargo_fmt_check_ms 549.0->261.0; metric:linux_glibc_compatibility_policy_ms 203.0->80.0; metric:cargo_build_debug_ms 41352.0->342.0; metric:code_index_recovery_cases_ms 6288.0->824.0; metric:code_index_sqlite_lock_cases_ms 7013.0->1205.0
- known degradations: metric:self_iteration_cargo_check_ms 524.0->563.0; metric:temporal_sdk_go_index_ms 77853.0->91550.0; metric:temporal_sdk_go_register_index_ms 77955.0->91633.0; metric:nonstandard_layout_fixture_query_p95_ms 625.0->847.0; metric:cross_language_syntax_fixture_index_ms 5357.0->6140.0; metric:cross_language_syntax_fixture_register_index_ms 5598.0->6241.0; metric:temporal_go_workspace_repo_set_refresh_ms 1209.0->1267.0
- latency metrics: cargo_fmt_check_ms=2254ms; self_iteration_cargo_fmt_check_ms=261ms; linux_glibc_compatibility_policy_ms=80ms; cargo_build_debug_ms=342ms; self_iteration_cargo_check_ms=563ms; code_index_recovery_cases_ms=824ms; code_index_sqlite_lock_cases_ms=1205ms; code_index_health_isolation_cases_ms=3773ms
