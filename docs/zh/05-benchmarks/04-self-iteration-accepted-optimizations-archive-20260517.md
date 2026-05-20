# 自迭代采纳优化归档：20260517 早期详细记录

本归档保存 `04-self-iteration-accepted-optimizations.md` 中的 20260517 早期详细条目，避免主记录文件超过 1000 行。

## 20260518T220331Z-20260519T-import-usage compacted from primary log

- summary: primary log details for accepted finalize optimization, production call-site demotion, type-relationship and caller/path ranking, TypeScript import/finalize work, header declaration ranking, function-flow ranking, and same-file import alias usage ranking were compacted on 20260520 to keep `04-self-iteration-accepted-optimizations.md` below the 1000-line repository hard cap; raw details remain available in their original patch/report artifacts.

## 20260517T030641Z

- patch: `/opt/workspace/relay-knowledge/.git/relay-knowledge-self-iteration/patches/20260517T030641Z.patch`
- score: 0.990729 (accuracy=1.0, performance=0.938194, stability=1.0)
- cases: 32/32 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/code/identity/imports.rs`, `src/relay_knowledge/code/parser_import_resolution_tests.rs`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`, `src/relay_knowledge/storage/sqlite/code_batch_finalize_tests.rs`
- key improvements: score_component:score 0.3->0.990729; score_component:accuracy 0.0->1.0; score_component:stability 0.0->1.0
- known degradations: score_component:performance 1.0->0.938194
- latency metrics: cargo_build_release_ms=65969ms; cargo_fmt_check_ms=812ms; cargo_clippy_ms=239ms; cargo_test_ms=8869ms; relay_teams_index_ms=88245ms; relay_teams_query_p50_ms=134ms; relay_teams_query_p95_ms=460ms; leveldb_cpp_index_ms=21528ms

## 20260517T045508Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260517T045508Z.patch`
- score: 0.928126 (foundational=1.0, competitive=0.730952, accuracy=0.865476, semantic_vector=1.0, performance=0.953884, stability=1.0)
- cases: 35/35 passed
- changed paths: `docs/en/01-user-guide/03-cli-command-reference.md`, `docs/zh/01-user-guide/03-cli-command-reference.md`, `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/application/service.rs`, `src/relay_knowledge/application/service/tests.rs`
- key improvements: score_component:stability 0.980769->1.0; metric:cargo_build_release_ms 29514.0->99; metric:cargo_fmt_check_ms 840.0->505; metric:cargo_clippy_ms 9952.0->148; metric:cargo_test_ms 24471.0->4987; gate:semantic_vector_provider_probe failed->passed {"metadata":{"trace_id":"trace-1778994380534019012","request_id":"req-1778994380534019012","graph_version":0,"stale":false},"ok":true,"provider":"openai_compatible","model":"embedding-3","dimension":1024,"latency_ms":590,"error_code":"rate_limited","error_message":"{\"error\":{\"code\":\"1113\",\"message\":\"Insufficient balance or no resource package. Please recharge.\"}}","retryable":true}
- known degradations: metric:semantic_vector_provider_probe_ms 477.0->601
- latency metrics: cargo_build_release_ms=99ms; cargo_fmt_check_ms=505ms; cargo_clippy_ms=148ms; cargo_test_ms=4987ms; relay_teams_index_ms=81125ms; relay_teams_query_p50_ms=127ms; relay_teams_query_p95_ms=426ms; leveldb_cpp_index_ms=19553ms

## 20260517T051540Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260517T051540Z.patch`
- score: 0.937063 (foundational=1.0, competitive=0.766667, accuracy=0.883333, semantic_vector=1.0, performance=0.953965, stability=1.0)
- cases: 35/35 passed
- changed paths: `docs/en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:score 0.928331->0.937063; score_component:competitive_capability 0.730952->0.766667; score_component:accuracy 0.865476->0.883333; metric:cargo_build_release_ms 55106.0->105; metric:cargo_fmt_check_ms 726.0->502; metric:cargo_clippy_ms 191.0->156; metric:cargo_test_ms 8267.0->4570; metric:relay_teams_query_p95_ms 459.0->427.0
- known degradations: metric:leveldb_cpp_index_ms 19068.0->19723
- latency metrics: cargo_build_release_ms=105ms; cargo_fmt_check_ms=502ms; cargo_clippy_ms=156ms; cargo_test_ms=4570ms; relay_teams_index_ms=81658ms; relay_teams_query_p50_ms=128ms; relay_teams_query_p95_ms=427ms; leveldb_cpp_index_ms=19723ms

## 20260517T055803Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260517T055803Z.patch`
- score: 0.942612 (foundational=1.0, competitive=0.788095, accuracy=0.894048, semantic_vector=1.0, performance=0.95588, stability=1.0)
- cases: 35/35 passed
- changed paths: `docs/en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_path_ranking.rs`
- key improvements: score_component:score 0.1->0.942612; score_component:foundational_capability 0.0->1.0; score_component:competitive_capability 0.0->0.788095; score_component:accuracy 0.0->0.894048; score_component:semantic_vector 0.0->1.0; score_component:stability 0.0->1.0
- known degradations: score_component:performance 1.0->0.95588
- latency metrics: cargo_build_release_ms=97ms; cargo_fmt_check_ms=465ms; cargo_clippy_ms=157ms; cargo_test_ms=3817ms; relay_teams_index_ms=65523ms; relay_teams_query_p50_ms=132ms; relay_teams_query_p95_ms=421ms; leveldb_cpp_index_ms=19160ms

## 20260517T062729Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260517T062729Z.patch`
- score: 0.954226 (foundational=1.0, competitive=0.835714, accuracy=0.917857, semantic_vector=1.0, performance=0.95297, stability=1.0)
- cases: 35/35 passed
- changed paths: `docs/en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:score 0.942406->0.954226; score_component:competitive_capability 0.788095->0.835714; score_component:accuracy 0.894048->0.917857; metric:cargo_build_release_ms 55501.0->34955; case:rt_fuzzy_constant_checkpoint_version {'passed': True, 'rank': 3, 'false_positive_count': 0}->{'passed': True, 'rank': 1, 'false_positive_count': 0} rank_improved
- known degradations: metric:cargo_fmt_check_ms 729.0->780; metric:cargo_test_ms 8288.0->8625
- latency metrics: cargo_build_release_ms=34955ms; cargo_fmt_check_ms=780ms; cargo_clippy_ms=213ms; cargo_test_ms=8625ms; relay_teams_index_ms=80016ms; relay_teams_query_p50_ms=128ms; relay_teams_query_p95_ms=437ms; leveldb_cpp_index_ms=19399ms

## 20260517T063652Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260517T063652Z.patch`
- score: 0.968417 (foundational=1.0, competitive=0.892857, accuracy=0.946429, semantic_vector=1.0, performance=0.952029, stability=1.0)
- cases: 35/35 passed
- changed paths: `docs/en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_path_ranking.rs`
- key improvements: score_component:score 0.954226->0.968417; score_component:competitive_capability 0.835714->0.892857; score_component:accuracy 0.917857->0.946429; metric:cargo_build_release_ms 34955.0->99; metric:cargo_fmt_check_ms 780.0->489; metric:cargo_clippy_ms 213.0->144; metric:cargo_test_ms 8625.0->3828; metric:relay_teams_index_ms 80016.0->65576
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=99ms; cargo_fmt_check_ms=489ms; cargo_clippy_ms=144ms; cargo_test_ms=3828ms; relay_teams_index_ms=65576ms; relay_teams_query_p50_ms=128ms; relay_teams_query_p95_ms=442ms; leveldb_cpp_index_ms=19944ms

## 20260517T065546Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260517T065546Z.patch`
- score: 0.97715 (foundational=1.0, competitive=0.928571, accuracy=0.964286, semantic_vector=1.0, performance=0.950075, stability=1.0)
- cases: 35/35 passed
- changed paths: `docs/en/01-user-guide/03-cli-command-reference.md`, `docs/en/03-architecture-specs/10-semantic-vector-provider-architecture.md`, `docs/en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/01-user-guide/03-cli-command-reference.md`, `docs/zh/03-architecture-specs/10-semantic-vector-provider-architecture.md`, `docs/zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md`, `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/retrieval/provider.rs`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`, `src/relay_knowledge/storage/sqlite/code_query_path_ranking.rs`
- key improvements: score_component:score 0.968513->0.97715; score_component:competitive_capability 0.892857->0.928571; score_component:accuracy 0.946429->0.964286; metric:cargo_build_release_ms 37016.0->127; metric:cargo_fmt_check_ms 788.0->502; metric:cargo_clippy_ms 241.0->141; metric:cargo_test_ms 8390.0->3878; metric:relay_teams_index_ms 80120.0->72221
- known degradations: metric:leveldb_cpp_index_ms 19206.0->22453; metric:semantic_vector_provider_probe_ms 1166.0->1243
- latency metrics: cargo_build_release_ms=127ms; cargo_fmt_check_ms=502ms; cargo_clippy_ms=141ms; cargo_test_ms=3878ms; relay_teams_index_ms=72221ms; relay_teams_query_p50_ms=130ms; relay_teams_query_p95_ms=474ms; leveldb_cpp_index_ms=22453ms
