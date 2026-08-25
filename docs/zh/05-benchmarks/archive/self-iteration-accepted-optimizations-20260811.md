# 自迭代采纳优化记录归档（2026-08-11）

本页从主记录中拆出历史运行详情，以保持每个跟踪文档低于 1000 行。主入口仍为[自迭代采纳优化记录](../04-self-iteration-accepted-optimizations.md)。

## run-1785705797

- patch: `/opt/workspace/relay-knowledge/.git/relay-knowledge-self-iteration/patches-v2/run-1785705797.patch`
- score: 0.982749 (foundational=1.000000, competitive=1.000000, accuracy=1.000000, semantic_vector=1.000000, research_judge=n/a, performance=0.904158, stability=1.000000)
- cases: 109/109 passed
- changed paths: `docs/en/03-architecture-specs/20-multi-repository-code-graph-overlay.md`, `docs/zh/03-architecture-specs/20-multi-repository-code-graph-overlay.md`, `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations-archive-20260803.md`, `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/application/code_repository/repository_set/query/mod.rs`, `src/relay_knowledge/application/code_repository/repository_set/query/mod_tests.rs`, `src/relay_knowledge/application/code_repository/repository_set/query/workflow.rs`, `src/relay_knowledge/application/code_repository/repository_set/query/workflow_tests.rs`, `src/relay_knowledge/storage/contracts/code.rs`, `src/relay_knowledge/storage/contracts/mod.rs`, `src/relay_knowledge/storage/partitioned/mod.rs`, `src/relay_knowledge/storage/sqlite/code/mod.rs`, `src/relay_knowledge/storage/sqlite/code/set/mod.rs`, `src/relay_knowledge/storage/sqlite/code/set/overlay/mod.rs`, `src/relay_knowledge/storage/sqlite/code/set/overlay/projection.rs`, `src/relay_knowledge/storage/sqlite/code/set/overlay/projection_tests.rs`
- key improvements: score_component:score 0.971867->0.9827485040194963; score_component:competitive_capability 0.987179->1.0; score_component:performance 0.859373->0.9041583556638684; case:typescript_syntax_hybrid_tsx_provider_flow false->true; metric:self_iteration_cargo_check_ms 364.0->262.0; metric:code_index_recovery_cases_ms 7884.0->1250.0; metric:code_index_sqlite_lock_cases_ms 7784.0->1107.0; metric:code_index_health_isolation_cases_ms 9586.0->2706.0
- known degradations: metric:temporal_samples_go_cold_index_ms 967.0->1070.0; metric:temporal_samples_go_cold_register_index_ms 1028.0->1158.0; metric:leveldb_cpp_cold_index_ms 384.0->424.0; metric:typescript_syntax_fixture_cold_index_ms 101.0->142.0; metric:typescript_syntax_fixture_cold_register_index_ms 162.0->203.0
- latency metrics: cargo_fmt_check_ms=4599ms; self_iteration_cargo_fmt_check_ms=484ms; linux_glibc_compatibility_policy_ms=121ms; skill_metadata_policy_cases_ms=222ms; cargo_build_debug_ms=263ms; self_iteration_cargo_check_ms=262ms; code_index_recovery_cases_ms=1250ms; code_index_sqlite_lock_cases_ms=1107ms

Adopted optimization notes:

Rust self-iteration v2 accepted this candidate through the independent tools/self_iteration harness. The candidate is expected to improve the general retrieval, indexing, evaluation, or harness behavior described by the changed paths and recorded metrics.
## run-1785718638

- patch: `/opt/workspace/relay-knowledge/.git/relay-knowledge-self-iteration/patches-v2/run-1785718638.patch`
- score: 0.997645 (foundational=1.000000, competitive=1.000000, accuracy=1.000000, semantic_vector=1.000000, research_judge=n/a, performance=0.986918, stability=1.000000)
- cases: 118/118 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code/set/overlay/export_index.rs`, `src/relay_knowledge/storage/sqlite/code/set/overlay/export_index_tests.rs`, `src/relay_knowledge/storage/sqlite/code/set/overlay/mod.rs`
- key improvements: none recorded
- known degradations: none recorded
- latency metrics: cargo_fmt_check_ms=4581ms; self_iteration_cargo_fmt_check_ms=485ms; linux_glibc_compatibility_policy_ms=121ms; skill_metadata_policy_cases_ms=243ms; cargo_build_debug_ms=283ms; self_iteration_cargo_check_ms=262ms; code_index_recovery_cases_ms=1228ms; code_index_sqlite_lock_cases_ms=946ms

Adopted optimization notes:

Rust self-iteration v2 accepted this candidate through the independent tools/self_iteration harness. The candidate is expected to improve the general retrieval, indexing, evaluation, or harness behavior described by the changed paths and recorded metrics.
