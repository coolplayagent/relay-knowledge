# Engineering Hard Constraints

[English](../../en/03-architecture-specs/02-engineering-hard-constraints.md) | [中文](../../zh/03-architecture-specs/02-engineering-hard-constraints.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

This chapter is the hard contract for Book 3. Implementation, documentation, tests, release, and operations changes must satisfy it; these rules are not optional guidance and cannot be postponed as follow-up work.

Advanced architecture is earned through clear boundaries, acyclic dependencies, recoverable state, bounded resources, and verifiable behavior.

## 2. Architecture Constraints

- **Async first**: I/O, graph database access, index refresh, ingestion, and service orchestration expose async APIs.
- **No blocking hot paths**: CPU-heavy, disk-heavy, or blocking work runs behind explicit workers, maintenance tasks, or blocking boundaries.
- **Bounded resources**: event pipelines, network entry points, index refresh, and background tasks have queue depth, budgets, timeouts, cancellation, backpressure, and overload behavior.
- **Facts separated from read models**: GraphStore is the source of truth; BM25, semantic, vector, summary, community, and code indexes are derived read models.
- **Acyclic dependencies**: crates, modules, traits, services, adapters, and configuration objects do not form cycles.
- **Clear code-source directory authority**: Git-managed code repositories use the tracked tree as the indexing directory authority, so tracked source must not be skipped only because it lives under names such as `build/`, `dist/`, `vendor/`, or `third_party/`; non-Git source directories default to source/config/documentation whitelist scanning so build products, caches, and dependency copies do not enter the index unless an explicit path opts into that broad directory. A narrow non-Git path such as `src` must not opt into sibling broad directories or walk unrelated filtered siblings before selection; an unfiltered non-Git scan must not walk directories that cannot contribute to the default whitelist; `--path .` is the explicit whole-root opt-in for broad directories. Git probe failures on real Git metadata must not silently fall back to filesystem indexing, and source fallback must not read live files for a stale scoped `filesystem:` commit. Non-Git synthetic hashes must be derived from the effective indexed scope after source-layout discovery, non-Git pre-scope hashing must not read files excluded by the file preset unless an explicit path filter opts into that file, non-Git ref resolution, source fallback verification, and impact path collection must include effective path and language filters, queued synthetic refs, synchronous full-snapshot reads, and full-index or delta live-byte reads must be verified before accepting bytes, non-Git file byte/hash/metadata materialization must reject final-path and ancestor-directory symlink replacements, explicit stored `filesystem:` refs plus source fallback verification, impact collection, impact partitioning, and deleted-symbol extraction must resolve through filesystem scope identity before dynamic source-kind or Git probes, repository-set members and freshness checks with narrower filters must reuse compatible broader non-Git scopes, incremental deletion must account for previous discovered roots, explicit non-Git incremental `base_ref` values must load that stored base scope, active non-Git task matching must compare with the task's effective filters for narrower stale reads, non-Git impact paths must return no changes when scoped base/head refs match, and Git ref normalization and fresh full-index checks must not perform full tree walks.
- **Performance must generalize**: improvements come from data structures, ranking signals, indexing strategy, query planning, batching, concurrency boundaries, or storage layout, not enumerated fixture cases.

## 3. Foundational Ownership

| Module | Sole responsibility | Forbidden |
| --- | --- | --- |
| `env` | Environment loading, parsing, validation, redacted diagnostics | Direct environment reads elsewhere |
| `paths` | Platform paths and runtime/data/log/cache directories | Runtime path construction elsewhere |
| `net` | Sockets, HTTP clients/servers, listeners, network loops | Network capability creation elsewhere |
| `net::http` | HTTP over a mature async runtime/library | Blocking sockets, thread-per-connection, busy polling |
| `net::qos` | Admission control, source/tenant limits, priority, budgets, overload metrics | Resource consumption before QoS |

Named platform process inputs follow the same boundary. During process bootstrap, `env::windows_system_root_from_process` captures Windows `SystemRoot`, `paths::windows_tasklist_command` resolves the executable, and `RuntimeConfiguration::process` passes that result into service recovery. Application workflows must neither call `std::env` nor construct platform executable paths while recovering workers or invoking service managers.

### 3.1 Environment Variable Boundary

The internals of `env` maintain a one-way data flow: `variables` only owns accepted variable names; `error` and `overrides` own the stable error model and typed override data; `value_parser` extracts and validates paths, strings, booleans, and positive integers from an already normalized snapshot; `platform` owns platform detection, key case normalization, platform directory inputs, and the process `SystemRoot` read; and only `config` may capture the complete process environment and assemble the public configuration. `mod.rs` preserves the existing `env::*` facade and must not accumulate parsing rules again. Corresponding unit tests live in `config_tests`, `platform_tests`, and `value_parser_tests` so configuration assembly, platform rules, and scalar validation fail independently.

The dependency direction is fixed as `error`/`variables` → `value_parser` → `platform`; `overrides` only composes those typed platform values, and outermost `config` depends on the other modules. Error, override, and variable-catalog modules must not depend back on configuration assembly, and `std::env` reads must not spread outside the `env` directory.

### 3.2 Code Repository Application Workflows

`application::code_repository` partitions internal ownership by use case: `repository` owns registration, removal, status, and reports; `index_workflow` owns index execution, durable task leases, checkpoints, and scope previews; `query` owns versioned-scope retrieval, feature flags, and freshness diagnostics; and `impact` owns diff impact analysis. These modules expose stable APIs through the same `RelayKnowledgeService` and depend inward only on `domain`, `code`, and `storage` contracts; they must not duplicate workflows or depend back on CLI, Web, MCP, or other adapters.

The Web adapter is grouped under `interfaces::web`: `mod.rs` owns router composition and shared response/error boundaries; `code_api`, `code_index_request`, `code_view_request`, `files`, and `model_config` own their named HTTP contracts; and focused tests stay in the same directory. Do not restore root-level `web_*` siblings or open sockets from this adapter.

The CLI adapter is grouped under `interfaces::cli`: `mod.rs` owns global option parsing, dispatch, and the stable public CLI surface; `spec` owns machine-readable command contracts; `render` owns output serialization; `repo`, `repo_set`, and `setup` own their command families; and focused parsing, naming, remote, service, map, and version tests live under `tests`. Command modules retain their established logical names where white-box access or compatibility requires them, but root-level `*_cli` prefix buckets are forbidden.

Codebase understanding views are grouped under `application::code_repository::views`: `service` only orchestrates scope selection, freshness, and responses; `architecture`, `business_domains`, `dependency_tour`, `process_flow`, and `affected_scope` each own one derivation algorithm; and `builder` plus `rules` provide bounded construction and deterministic classification. View tests are colocated with this directory instead of using ambiguous flat `views_*` filenames.

Source fallback retrieval is grouped under `application::code_repository::source_fallback`: `execution` is the sole I/O orchestrator; `plan` decides whether and how bounded fallback runs; `identity`, `filters`, `scoring`, and `results` own coverage, request constraints, ranking, and result merging; and `imports`, `surface`, and `worktree` isolate evidence-specific boundaries. Modules outside this directory must not depend directly on these internal algorithm helpers.

Shared code-repository behavior is partitioned by explicit responsibility: `index_task` owns durable task leases and worker recovery, `index_state` owns persisted index inspection and reuse, `scope` owns scope resolution and filter compatibility, and `repository_status` owns registered status lookup and checkpoint selection. `blocking`, `errors`, and `clock` isolate their respective runtime, error, and persisted-time boundaries. Callers must depend directly on the responsible module instead of reintroducing an ambiguous `support`, `helper`, or utility aggregation layer.

### 3.3 Repository Domain Ownership

Repository domain types are grouped under `domain::code::repository`: `registration` owns registration, selectors, ranges, and index requests; `retrieval_request` owns query kinds, qualifiers, limits, and retrieval layers; `indexed_records` owns persisted file, symbol, reference, relationship, diagnostic, and tombstone records; `repository_status` owns status, scope previews, totals, and reports; `retrieval_results` owns query and feature-flag results; and `scope_identity` is the only owner of versioned snapshot scope encoding. `validation` remains private to the directory. Do not restore the mixed `repository.rs` or `repository_helpers.rs` files.

### 3.4 Model Provider Ownership

`model_provider` keeps profile normalization in `profile_config`, fallback policy in `fallback`, durable JSON writes in `persistence`, provider HTTP and response diagnostics in `connectivity`, and catalog fetch plus catalog data interpretation in `catalog`. Cross-module protocol tests live in `protocol_tests`; production behavior must not be recombined into a generic helper module.

### 3.5 Dependency Parser Ownership

Dependency parsing groups shared syntax by the format it interprets: `cargo_source` classifies Cargo lock sources, `npm_lock` interprets npm references and lock entries, `python_requirements` parses Python requirement syntax, `toml_inline_table` reads TOML dependency fields, and `gradle_notation` parses Gradle calls and coordinates. Ecosystem parsers depend on these narrow modules; a cross-ecosystem `support` module is prohibited.

### 3.6 SQLite Storage Boundaries

SQLite storage keeps evidence and stable ID generation in `evidence_identity`, mutation reads in `mutation_log`, commit-time validity normalization in `graph_version`, and diagnostic row counts in `table_stats`. Storage modules must import these explicit boundaries instead of accumulating unrelated persistence behavior in a generic helper module.

Local-file persistence is grouped under `storage::sqlite::file_index`: `mod.rs` owns root lifecycle, file metadata, path search, and aggregate diagnostics, while `content` owns content entries, chunks, FTS, freshness cursors, and content search. Only `file_index::content::search` is visible to the SQLite store adapter; the remaining content-index primitives stay private to the directory. `tests`, `content_tests`, and `retirement_tests` verify metadata, content, and retirement behavior respectively. Do not restore flat `file_index_*` sibling modules.

Graph-canvas persistence is grouped under `storage::sqlite::canvas`: `mod.rs` owns budget validation, knowledge-graph projection, and the snapshot builder; `code` only projects code files, symbols, references, and source-path links; and `tests` covers both projections plus mixed canvases. Code-projection helpers stay private to the canvas directory. Do not restore a top-level `canvas_code` sibling whose ownership depends on a filename prefix.

Code-graph fact persistence is grouped under `storage::sqlite::code_graph`: `mod.rs` owns the schema, version-bounded fact replacement and search, row decoding, and metadata validation, while `tests` verifies the same storage boundary. Do not separate its tests into the SQLite root or use a repeated `code_graph_tests` filename.

Durable operational persistence is grouped under `storage::sqlite::operations`: `mod.rs` owns worker tasks, proposals and conflicts, audit events, service-operator state, their row decoding, and stable task IDs; `tests` verifies those workflows through the storage interface. The SQLite root must not own operation-specific test modules.

Index lifecycle persistence is grouped under `storage::sqlite::indexing`: `mod.rs` owns cursor state, refresh orchestration, validation, and stable refresh-task identity; `cursor_metadata`, `schema`, and `task_queue` isolate those responsibilities; and `refresh_tests`, `queue_tests`, and `schema_migration_tests` remain beside the boundary they verify. Do not place index-lifecycle tests or prefixed `index_refresh_*` implementation files in the SQLite root.

Three-layer graph retrieval persistence is grouped under `storage::sqlite::retrieval`: `mod.rs` owns schema initialization, document materialization, retrieval coordination, and shared scoring inputs; named child modules own advanced graph paths, BM25 and bounded fallback, context assembly, derived documents, label trigrams, schema migration, aliases, and ranking. Their focused tests stay in the same directory. Do not restore parent-level `retrieval_*` files or path overrides that hide the physical ownership boundary.

Maven persistence is grouped physically under `storage::sqlite::maven`; `mod.rs` coordinates build/dependency projection, `model` owns raw and effective POM models, `xml` owns bounded XML extraction, `pom_path` owns repository-bounded relative POM resolution, and `property_interpolation` owns bounded recursive property expansion. Focused and review-regression tests stay in the same directory. These rules must not be combined in a generic Maven support module or hidden behind parent-relative path overrides.

Checkpointed code-batch persistence is grouped under `storage::sqlite::code_batch`: `mod.rs` owns session start, bounded batch application, checkpoints, and finalization coordination; `dependencies`, `progress`, and the `finalize` subtree own their narrower write phases. Session-finalization, TypeScript-finalization, and search-materialization regressions stay in the same directory. `storage::sqlite::code` may call this boundary but must not own batch-specific test modules.

Code-snapshot persistence is grouped under `storage::sqlite::code_snapshot`: `mod.rs` owns snapshot validation, transactional application, scope replacement, status publication, and legacy-database import coordination; `candidate_paths`, `fingerprints`, `snapshot_import`, and `import_compat` own their named read or compatibility boundaries. Candidate-path, progress-accounting, and import regressions stay in the same directory. Do not encode this ownership through repeated `code_snapshot_*` files in the SQLite root.

Codebase-view persistence is grouped under `storage::sqlite::code_views`: `mod.rs` coordinates snapshot assembly, while `affected`, `call_focus`, `dependencies`, and `truncation` own their bounded derivations and `tests` verifies the combined projection. Keep these files together instead of scattering prefix-related siblings through the SQLite root.

Durable code-index tasks are grouped physically under `storage::sqlite::code_tasks`: `mod.rs` owns queueing, attempt-scoped leases, bounded retry, completion/failure, reset, checkpoints, and scope retention; `worktree` protects active overlay base scopes; and focused queue, lease, reset, retention, and status tests stay beside that boundary. The logical test modules may remain code-facade siblings for white-box access, but their files must not return to the SQLite root.

Repository-set persistence is grouped under `storage::sqlite::code_set`: `mod.rs` owns set membership, overlay refresh, cross-repository edge matching, and status; `manifest` owns bounded module-key derivation; `refresh_tasks` owns durable refresh-task leases and retry; and set, workspace, manifest, and refresh-task tests stay in the same directory. Facade-level test visibility must not be used as a reason to scatter `code_set_*` files through the SQLite root.

Monorepo-workspace persistence is grouped under `storage::sqlite::code_workspace`: `mod.rs` owns automatic workspace sets, package mappings, cross-member import resolution, and workspace-format normalization; `tests` covers lifecycle and mapping invariants, while `lookup_tests` covers language-specific import normalization. Do not restore root-level `code_workspace_*` siblings.

Code-index schema ownership is grouped under `storage::sqlite::code_schema`: `mod.rs` owns current tables, indexes, and initialization order; `migrations` owns bounded compatibility transformations; `route_schema` owns route-specific DDL; and `tests` verifies schema and migration invariants. Do not split these files across the SQLite root using `code_schema_*` prefixes.

Code-query persistence is grouped under `storage::sqlite::code_query`: `mod.rs` coordinates bounded retrieval layers; `calls`, `imports`, `symbols`, and `hybrid` own edge- or plan-specific behavior; `scoring` owns focused ranking signals; `accuracy` owns end-to-end ranking fixtures; and `tests` owns shared query regressions. Generic row decoding, excerpts, identifiers, line ranges, routes, references, and SBOM retrieval remain named root children because they cross those focused subdomains. No query directory may become a new flat prefix bucket.

Relevance primitives are grouped under `storage::sqlite::code_query::relevance`: `tokens` normalizes terms, `text_scoring`, `symbol_scoring`, and `call_scoring` own their ranking domains, `symbol_identity` owns scoped identity matching, `candidate_plan` owns bounded candidate layers, and `filters` plus `fts` own SQL and FTS construction. `mod.rs` is only the internal relevance surface; do not restore a broad `code_query_support` file or root-level `code_query_*` siblings.

### 3.7 Code Index Foundations

Cross-cutting code-index primitives use responsibility-bearing top-level modules: `content_identity` owns stable IDs and content hashes, `language_metadata` owns language detection and language-level metadata, and `generated_detection` owns generated-source classification. Do not group unrelated primitives under a `common` directory; new primitives belong with the behavior they describe.

### 3.8 Service Lifecycle Planning

Service lifecycle ownership is split by boundary: `application::service::lifecycle_plan` validates requests, builds install/upgrade/rollback/uninstall step plans, and coordinates execution; `lifecycle_plan::platform_service` alone selects platform service-definition names, renders systemd/launchd/Windows Service definitions, declares platform permissions, and builds service-manager commands; `lifecycle_plan::execution` owns blocking file and process execution. Platform rendering and command quoting must not return to the lifecycle step planner.

### 3.9 Self-Iteration Evaluator Ownership

`tools/self_iteration::evaluator` is grouped by evaluation stage and evidence type: `runtime` owns top-level orchestration, concurrency limits, and result assembly; `quality` owns gate policy and execution; `workloads` is partitioned into repository, repository-set, agent, CLI, file, and semantic-vector evaluation; `fixtures` owns only generated-repository fixtures and their write lifecycle; and `judge` owns research-judge settings, prompts, backends, and outcome contracts. Evaluator unit tests stay beside the boundary they verify and use traceable `*_tests.rs` names. Do not restore `evaluator_tail`, cross-responsibility `evaluator_tests`, or flat `evaluator_*` files in the `tools/self_iteration/src` root.

### 3.10 Self-Iteration Scoring Ownership

`tools/self_iteration::scoring` keeps observation types and the public score contract in `mod.rs`, ranked-evidence matching in `ranked`, total-score assembly in `evaluation`, rejection policy in `decision`, capability-ceiling/performance/stability components in `capability`, cross-run delta detection in `change_detection`, and stateless numeric/JSON primitives in `common`. Focused unit tests stay in the same directory. Do not restore root-level `scoring_ranked` or `scoring_tests` files, or recombine distinct scoring phases into one scoring file.

### 3.11 Self-Iteration Configuration Ownership

`tools/self_iteration::config` keeps modes and strategies, category sets, the public configuration model, CLI parsing, category exclusions, job budgets, and scalar validation in `mode`, `categories`, `model`, `parse`, `category_exclusions`, `job_plan`, and `value_parser` respectively. `mod.rs` only maintains constants and the stable facade; parsing, category, unattended-mode, documentation-contract, and job-budget unit tests stay in the same directory. Do not restore a root `config.rs` that combines the model, parser, budgets, and inline tests.

## 4. HTTP and QoS

HTTP is implemented over non-blocking operating-system event mechanisms, such as epoll, kqueue, or IOCP through a mature async runtime. All inbound and outbound network work passes through QoS policy before consuming resources.

Network entry points support connection budgets, request budgets, body limits, timeouts, cancellation, graceful shutdown, rate limits, queue-depth metrics, drop metrics, and overload responses.

## 5. Code Quality Constraints

- No tracked source, test, documentation, script, or workflow file may exceed 1000 lines. Generated release lockfiles required by locked builds, currently `Cargo.lock`, are exempt and must stay machine-generated.
- Do not add shallow functions; functions must validate, transform, isolate boundaries, manage resources, map errors, add observability, or coordinate real workflows.
- Do not keep dead code, TODO stubs, unused public APIs, untested speculative extension points, or commented-out implementations.
- Project identity constants live in the `project` module; module-local operational defaults stay with the owning module.
- `unsafe` is prohibited by default unless the boundary, reason, and tests are explicit.

## 5.1 File Watcher (fs.watch) Constraints

- File watching uses the `notify` crate for cross-platform support (Linux inotify, macOS FSEvents, Windows ReadDirectoryChangesW).
- Watch events must be debounced within a configurable window to prevent unbounded task generation from high-frequency file changes.
- Content hash filtering (`ContentHashCache`) must skip save operations with no actual content change.
- `max_watch_dirs` must cap the maximum watched directory count to prevent fd/inotify watch resource exhaustion.
- Watch failures must auto-degrade (Degraded state) and must not affect query hot paths or the async runtime.
- Watcher configuration must load through the `env` module environment variable override mechanism; no other module may read watcher environment variables directly.
- Watcher state and diagnostics must be exposed through the `service status` API.
- Incremental index tasks (`CodeIndexTaskSeed`) must enter the durable task queue; durable task leases, checkpoints, and bounded retry must not be skipped.

## 6. Documentation and Test Constraints

- Code, configuration, behavior, tests, workflows, benchmarks, installation, and operations changes include matching documentation refreshes.
- Unit-test and integration-test gates remain distinct.
- Rust line coverage stays above 90%, including invariants, error branches, boundaries, async cancellation, and backpressure.
- Browser integration gates install Playwright Chromium, for example `uv run --extra dev python -m playwright install --with-deps chromium`.
- Documentation changes check links, numbering, line limits, and stale state.

## 7. Acceptance Criteria

- A new module can name its ownership boundary and show why it does not create a dependency cycle.
- New background or network behavior states budgets, failure modes, cancellation, and observability metrics.
- New retrieval or performance work explains a general mechanism, not only why one example passes.

---

Navigation: Previous: [1. Architecture Vision and Algorithm Map](01-architecture-vision-and-algorithm-map.md) | Next: [3. Foundational Runtime](03-foundational-runtime.md)
