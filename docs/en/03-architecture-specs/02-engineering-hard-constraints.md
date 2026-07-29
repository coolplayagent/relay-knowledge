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

The foundational HTTP boundary is physically contained in `net/http/`: `mod.rs` owns configuration, client/server runtime, timeouts, cancellation, and graceful shutdown; `qos_admission.rs` and `qos_client.rs` isolate inbound and outbound QoS; and `mod_tests.rs` verifies the facade. The `net/` parent must not regain `http.rs` or `http_tests.rs`.

### 3.1 Environment Variable Boundary

The internals of `env` maintain a one-way data flow: `variables` only owns accepted variable names; `error` and `overrides` own the stable error model and typed override data; `value_parser` extracts and validates paths, strings, booleans, and positive integers from an already normalized snapshot; `platform` owns platform detection, key case normalization, platform directory inputs, and the process `SystemRoot` read; and only `config` may capture the complete process environment and assemble the public configuration. `mod.rs` preserves the existing `env::*` facade and must not accumulate parsing rules again. Corresponding unit tests live in sibling `config_tests`, `platform_tests`, and `value_parser_tests` files and are attached by their matching implementation owners, not by the facade, so configuration assembly, platform rules, and scalar validation fail independently.

The dependency direction is fixed as `error`/`variables` → `value_parser` → `platform`; `overrides` only composes those typed platform values, and outermost `config` depends on the other modules. Error, override, and variable-catalog modules must not depend back on configuration assembly, and `std::env` reads must not spread outside the `env` directory.

### 3.2 Code Repository Application Workflows

`application::code_repository` partitions internal ownership by use case: the `repository` directory owns registration, removal, status, reports, staleness annotation, and worktree-overlay validation; `indexing` owns index execution, durable task leases, checkpoints, scope previews, and worker administration; `query` owns versioned-scope retrieval, feature flags, and freshness diagnostics; and `impact` owns diff impact analysis. These modules expose stable APIs through the same `RelayKnowledgeService` and depend inward only on `domain`, `code`, and `storage` contracts; they must not duplicate workflows or depend back on CLI, Web, MCP, or other adapters.

The Web adapter is grouped under `interfaces::web`: `mod.rs` owns router composition and shared response/error boundaries; `code_api`, `code_index_request`, `code_view_request`, `files`, and `model_config` own their named HTTP contracts; and focused tests stay in the same directory. The facade explicitly attaches `code_api_integration_tests` and `files_integration_tests` because they execute assembled routers and shared application services across module boundaries; implementation-local units remain paired with their exact owner. Do not restore root-level `web_*` siblings or open sockets from this adapter.

The MCP adapter is physically grouped under `interfaces::agent::mcp`: `mod.rs` owns server composition, streamable-HTTP dispatch, QoS admission, cancellation, and tool coordination; `json_rpc` owns protocol initialization validation, session/error envelopes, response encoding, and typed request-ID identity; `tool_contract` owns freshness parsing, argument/domain error mapping, MCP request context, and stable tool result envelopes; and named child modules own audit, HTTP contract, resources, prompts, state, authorization, registry, and code-tool behavior. `json_rpc_tests` and `tool_contract_tests` are paired with those owners instead of accumulating protocol and tool-mapping primitives in root facade tests. The `code_tools` subtree keeps tool dispatch and argument mapping in `mod.rs`, bounded agent-output policy in `agent_budget`, and derived codebase-view execution in `codebase_view`; facade and budget tests stay beside those owners as `mod_tests` and `agent_budget_tests`. Root adapter tests stay beside its facade as `mod_tests`, `protocol_tests`, `tool_tests`, `software_tool_tests`, `feature_flag_tool_tests`, and `runtime_guardrail_tests`; reusable test storage and HTTP transport fixtures are explicitly named `test_support` and `transport_harness`. Do not restore root-level `mcp_*` files, a sibling `code_tools.rs`, or issue-number test module names outside these ownership boundaries.

The CLI adapter is grouped under `interfaces::cli`: `mod.rs` owns global option parsing, dispatch, and the stable public CLI surface; `spec` owns machine-readable command contracts; `render` owns output serialization; `repo`, `repo_set`, and `setup` own their command families; and focused parsing, naming, remote, service, map, and version tests live under `tests`. Command modules retain their established logical names where white-box access or compatibility requires them, but root-level `*_cli` prefix buckets are forbidden.

Codebase understanding views are grouped under `application::code_repository::views`: `service` only orchestrates scope selection, freshness, and responses; `architecture`, `business_domains`, `dependency_tour`, `process_flow`, and `affected_scope` each own one derivation algorithm; and `builder` plus `rules` provide bounded construction and deterministic classification. Focused unit tests pair with their implementation owner, while the facade explicitly owns `affected_scope_integration_tests` and `dependency_tour_integration_tests` because those scenarios exercise service dispatch, builders, rules, and derivation algorithms together. View tests remain colocated with this directory instead of using ambiguous flat `views_*` filenames.

Source fallback retrieval is grouped under `application::code_repository::source_fallback`: `execution` is the sole I/O orchestrator; `plan` decides whether and how bounded fallback runs; `identity`, `filters`, `scoring`, and `results` own coverage, request constraints, ranking, and result merging; and `imports`, `surface`, and `worktree` isolate evidence-specific boundaries. The facade explicitly owns `surface_integration_tests` because those scenarios exercise the `plan` + `results` + `surface` composition rather than one implementation unit; focused tests remain paired with their exact implementation owners. Modules outside this directory must not depend directly on these internal algorithm helpers.

The `indexing` directory is a strict workflow boundary: `mod.rs` coordinates full and incremental execution, `state` owns persisted index inspection and reuse, `task` owns durable leases and worker recovery, `queue` owns bounded overlay task submission, `fast_path` owns validated fresh-index reuse, and `tasks` owns task administration. Only the lease-recovery operation needed during repository registration is exposed to its parent; internal indexing helpers must not leak into query or adapter code. The `repository` directory similarly keeps its service implementation in `mod.rs`, registered status and checkpoint selection in `status`, result freshness annotation in `staleness`, worktree-overlay validation in `worktree`, and white-box fixtures beside the owned behavior. Shared `scope` retains scope resolution and filter compatibility, while `blocking`, `errors`, `clock`, and `worktree_ref` isolate runtime, error, persisted-time, and overlay-identity boundaries. Do not restore root-level `repository_*`, `worktree_freshness`, `index_*`, `fast_index`, `queue`, or `tasks` buckets.

### 3.3 Repository Domain Ownership

Repository domain types are grouped under `domain::code::repository`: `registration` owns registration, selectors, ranges, and index requests; `retrieval_request` owns query kinds, qualifiers, limits, and retrieval layers; `indexed_records` owns persisted file, symbol, reference, relationship, diagnostic, and tombstone records; `repository_status` owns status, scope previews, totals, and reports; `retrieval_results` owns query and feature-flag results; and `scope_identity` is the only owner of versioned snapshot scope encoding. `validation` remains private to the directory. Do not restore the mixed `repository.rs` or `repository_helpers.rs` files.

### 3.4 Model Provider Ownership

`model_provider` keeps profile normalization in `profile_config`, fallback policy in `fallback`, durable JSON writes in `persistence`, provider HTTP and response diagnostics in `connectivity`, and catalog fetch plus catalog data interpretation in `catalog`. Cross-module protocol tests live in `protocol_tests`; production behavior must not be recombined into a generic helper module.

### 3.5 Dependency Parser Ownership

Dependency parsing groups shared syntax by the format it interprets: `cargo_source` classifies Cargo lock sources, `npm_lock` interprets npm references and lock entries, `python_requirements` parses Python requirement syntax, `toml_inline_table` reads TOML dependency fields, and `gradle_notation` parses Gradle calls and coordinates. Ecosystem parsers depend on these narrow modules; a cross-ecosystem `support` module is prohibited.

Dependency parsing is physically contained in `code/parser/dependencies/`: `mod.rs` owns manifest classification, ecosystem dispatch, and stable fact assembly; ecosystem parsers and shared format primitives use responsibility-named files; and `mod_tests.rs` verifies the facade. The parent parser directory must not regain a same-named `dependencies.rs` file or parent-relative `#[path]` indirection that hides ownership.

C/C++ parse recovery is physically contained in `code/parser/recovery/`: `mod.rs` owns the bounded recovery decision and declaration-shape validation, `scan.rs` owns literal-aware code scanning, `line_classification.rs` owns recoverable line classification, and `type_body.rs` owns decorated-type body validation. Focused language and recovery units keep paired `mod_tests` or implementation-named tests, while the parser facade explicitly owns the C/C++ `parser_integration_tests` and `gcc_recovery_integration_tests` scenarios because they exercise the complete parse entry point across language adapters, syntax parsing, and recovery. The parser parent must not regain `recovery.rs`; language adapters may consume this narrow recovery contract but must not duplicate its rules.

### 3.6 SQLite Storage Boundaries

SQLite storage keeps evidence and stable ID generation in `evidence_identity`, mutation reads in `mutation_log`, commit-time validity normalization in `graph_version`, and diagnostic row counts in `table_stats`. Storage modules must import these explicit boundaries instead of accumulating unrelated persistence behavior in a generic helper module.

The SQLite adapter root is physically contained in `storage/sqlite/`: `mod.rs` owns `SqliteGraphStore`, bounded blocking-worker entry points, schema orchestration, graph-fact validation, and root test declarations, while responsibility-named child modules own the persistence details. The `storage/` parent must not regain `sqlite.rs`; root test modules must remain beside `sqlite/mod.rs` without `sqlite/`-prefixed path redirects.

Root graph-store behavior is verified by sibling `graph_storage_tests.rs`; retrieval schema migration and BM25 fallback integration scenarios are grouped under the explicitly named `graph_retrieval_tests` directory and declared by that graph-storage test owner. Do not restore the ambiguous `graph_tests.rs` plus `graph_tests/` pair or mix these graph-store scenarios into code-graph fact tests.

SQLite connection lifecycle is logically and physically contained in `storage::sqlite::connection_runtime`: `maintenance` owns connection pragmas, WAL checkpointing, and maintenance diagnostics; `read_pool` owns the bounded read-connection lanes and lock deadlines; and `retry` owns the bounded transient-SQLite retry policy. SQLite persistence modules must address these capabilities through `connection_runtime` instead of flattening them back into the SQLite root.

The partitioned SQLite adapter is physically contained in `storage/partitioned/`: `mod.rs` owns the public store and trait implementations; catalog, control delegates, diagnostics, retention, routing, status, and totals use responsibility-named files; and `mod_tests.rs` verifies the cross-shard contract. The `storage/` parent must not regain `partitioned.rs`, `partitioned_tests.rs`, or relative `#[path]` redirects into this domain.

Software projection persistence is physically and logically contained in `storage::sqlite::software`: the SQLite root declares the domain and the code-store adapter imports it as a sibling instead of owning it through a relative path. `mod.rs` owns schema and projection orchestration, `graph.rs` materializes and queries graph-derived files, topics, and relationships, and dependency usage, lifecycle, and query scope retain their own responsibility-named modules. The SQLite-root `scope_filters.rs` owns indexed-scope coverage predicates shared by code retrieval and software projection so neither domain imports the other's private helper; `scope_filters_tests.rs` owns the corresponding path, language, and indexed-scope invariants. `mod_tests.rs` verifies the root projection lifecycle and `projection_tests.rs` verifies filtered projection reads. The `storage/sqlite/` parent must not regain `software.rs`, `software_graph.rs`, or software root-test files.

Maven effective-model resolution is physically contained in `storage/sqlite/maven/model/`: `mod.rs` coordinates document resolution and inheritance, `parse.rs` owns POM decoding, and `effective.rs` owns effective dependency, plugin, profile, and property construction. The Maven parent must not regain `model.rs` or relative `#[path]` redirects into the model domain.

Core code-query white-box tests are grouped under `storage/sqlite/code_query/tests/unit/`: `mod.rs` owns general query planning, fallback, ranking, and outage invariants, while `case_intent_tests.rs` owns the case-intent fixture family. The `tests` parent declares this group as `unit`; it must not restore the generic `test_modules::tests` identity, a sibling `unit.rs`, or a relative redirect into the unit-test group.

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

Code-query persistence is grouped under `storage::sqlite::code_query`: `mod.rs` coordinates bounded retrieval layers; `calls`, `imports`, `symbols`, and `hybrid` own edge- or plan-specific behavior; `scoring` owns focused ranking signals; and `accuracy` owns end-to-end ranking fixtures. Shared query regressions remain under `tests`, partitioned into `calls`, `ranking`, `generated`, and `hybrid`; cross-cutting unit, score, identity, excerpt, field-filter, line-context, and SBOM cases stay as named root children. Generic row decoding, excerpts, identifiers, line ranges, routes, references, and SBOM retrieval remain named production root children because they cross focused subdomains. No query or test directory may become a new flat prefix bucket.

Relevance primitives are grouped under `storage::sqlite::code_query::relevance`: `tokens` normalizes terms, `text_scoring`, `symbol_scoring`, and `call_scoring` own their ranking domains, `symbol_identity` owns scoped identity matching, `candidate_plan` owns bounded candidate layers, and `filters` plus `fts` own SQL and FTS construction. `mod.rs` is only the internal relevance surface; do not restore a broad `code_query_support` file or root-level `code_query_*` siblings.

The SQLite code-store facade and its directly owned persistence behaviors are grouped physically under `storage::sqlite::code`: `mod.rs` coordinates store traits and references sibling persistence domains, while `feature_flags`, `generated`, `impact`, `routes`, `search`, and `symbols` own their named code-store behaviors. Scope cleanup, removal, status, and report ownership is grouped under `code::lifecycle`, with each paired unit-test file beside its implementation. Facade regressions, metadata/status cases, shared fixtures, and support code remain in the same directory with descriptive names. Do not simulate this ownership with a flat family of root-level `code_*` files or move lifecycle files back to the facade root.

SQLite connection execution concerns are physically grouped under `storage::sqlite::connection_runtime`: `maintenance` owns writer pragmas, WAL checkpoints, and maintenance diagnostics; `read_pool` owns bounded read-connection selection and deadlines; and `retry` classifies bounded transient lock retries. Their paired unit tests stay beside the owner. The root `sqlite.rs` remains the store facade and references these modules explicitly; do not restore these runtime files to the crowded SQLite root.

### 3.7 Code Index Foundations

Cross-cutting code-index primitives use responsibility-bearing top-level modules: `content_identity` owns stable IDs and content hashes, `language_metadata` owns language detection and language-level metadata, and `generated_detection` owns generated-source classification. Do not group unrelated primitives under a `common` directory; new primitives belong with the behavior they describe.

### 3.8 Service Lifecycle Planning

Service lifecycle ownership is split by boundary: `application::service::lifecycle_plan` validates requests, builds install/upgrade/rollback/uninstall step plans, and coordinates execution; `lifecycle_plan::platform_service` alone selects platform service-definition names, renders systemd/launchd/Windows Service definitions, declares platform permissions, and builds service-manager commands; `lifecycle_plan::execution` owns blocking file and process execution. Platform rendering and command quoting must not return to the lifecycle step planner.

The lifecycle-plan domain is physically contained in `application/service/lifecycle_plan/`: `mod.rs` owns planning and execution coordination, `execution.rs` and `platform_service.rs` are named child boundaries, and `mod_tests.rs`, `review_tests.rs`, and `review_followup_tests.rs` remain colocated. The parent `application/service/` directory must not regain flat `lifecycle_plan.rs` or `lifecycle_plan_*_tests.rs` files.

#### 3.8.1 Domain Model Ownership

`domain` is a stable public facade over five real Rust subdomains rather than a flat collection assembled with production `#[path]` aliases. `core` owns validation errors, source scopes, graph versions, entity identity, and index state; `graph` owns multimodal evidence, mutations, and retrieval contracts; `code` owns repository records, requests, index tasks, repository sets, staleness, views, and workspace contracts; `knowledge` owns the knowledge-map contract; and `operations` owns worker/service lifecycle and software-global projection contracts. Dependencies remain acyclic: graph, code, and knowledge build on core, code may consume graph retrieval policy, and operations composes core, graph, and code contracts. The root preserves the public `domain::*` facade but must not regain path aliases that hide physical ownership.

Every stateful or validating domain implementation attaches its descriptive sibling `*_tests.rs` file directly. Repository registration, scope identity, retrieval requests, repository status, and repository-index summaries keep separate tests beside their actual owners; a cross-owner `domain/code/repository_tests.rs` bucket is forbidden. Pure serialization-only record files may remain without artificial tests, but they must not become a reason to move another owner's assertions into a facade test.

### 3.9 Self-Iteration Evaluator Ownership

`tools/self_iteration::evaluator` is grouped by evaluation stage and evidence type: `runtime` owns top-level orchestration, concurrency limits, contracts, reporting, and result assembly; `quality` owns gate policy and execution; `workloads` is partitioned into repository, repository-set, agent, CLI, file, and semantic-vector evaluation; `fixtures` owns only generated-repository fixtures and their write lifecycle; and `judge` owns research-judge settings, prompts, backends, and outcome contracts. The evaluator root is a declaration-only facade that exposes `evaluate_candidate` and `EvaluationRun`. Runtime must separate `contracts`, `concurrency`, `reporting`, `finish`, and `orchestration`; workloads may depend only on the lower runtime contracts, concurrency, and reporting services, while orchestration composes workloads. Workload-specific JSON case failure mapping stays in `workloads::case_scoring` so reporting does not depend back on workloads. Each behavioral runtime owner attaches its sibling test file, including repository work-plan, bounded parallel-map, finish serialization, and reporting invariants. The `quality` subtree must keep gate contracts at its domain root, separate policy from execution, and attach focused tests directly to both owners; production gate assembly must not use `include!` or mix workload-selection assertions into quality-policy tests. The `judge` subtree must keep its shared evaluation input at the domain root, preserve one-way composition from evaluation into settings, prompt, backend, and outcome owners, and attach a sibling test file to every owner. Shell command parsing belongs to judge settings rather than outcome validation, and production judge assembly must not use `include!` or a cross-owner test-support fragment. The `workloads` subtree must use real Rust modules for agent, CLI, file, repository, repository-set, selection, and semantic-vector owners. Shared case-failure and payload-constraint behavior belongs to a focused `case_scoring` module rather than a file or repository workload, every behavioral source file attaches its same-directory owner test file, and production or test assembly must not use `include!`. The `fixtures` subtree must use real Rust modules for language/source families, agent-workflow sources, repository assembly, and the shared file-writer boundary. Repository and writer tests attach directly to those owners; production fixture assembly must not use `include!`, and fixture source constants must not live in workload execution modules. Evaluator unit tests stay beside the boundary they verify and use traceable `*_tests.rs` names. Do not restore evaluator-root test assembly, `evaluator_tail`, cross-responsibility `evaluator_tests`, or flat `evaluator_*` files in the `tools/self_iteration/src` root.

### 3.10 Self-Iteration Scoring Ownership

`tools/self_iteration::scoring` must use real Rust modules. `mod.rs` owns observation, public score, and private stage contracts; `ranked` owns ranking-evidence matching, `evaluation` owns aggregate score assembly, `decision` owns rejection policy only, `capability` owns capability-ceiling/performance/stability components, `change_detection` owns all cross-run change extraction, `case_fields` owns typed JSON case access, and `score_math` owns bounded average/clamp primitives. Every behavior owner directly attaches its sibling `*_tests.rs` contract, while `mod_tests` only validates observation contracts; production code and tests must not use `include!` to merge stages or test scopes into an implicit namespace. Do not restore root-level `scoring_ranked` or `scoring_tests`, introduce a generic `common` bucket, move change extraction back into `decision`, or recombine distinct scoring stages into one score file.

### 3.11 Self-Iteration Configuration Ownership

`tools/self_iteration::config` must use real Rust modules: `mode` owns modes and strategies, `jobs` owns typed parallelism inputs, `categories` owns category sets, `model` owns the public configuration contract, `parse` coordinates CLI parsing, `category_exclusions` applies exclusion policy, `job_plan` resolves resource budgets, and `value_parser` validates scalar arguments. `mod.rs` only maintains constants and the stable facade. Every behavior owner directly attaches its sibling `*_tests.rs` contract, while `mod_tests` only checks the facade-wide documentation contract; production code and tests must not use `include!` to merge these boundaries into an implicit namespace. Do not restore a root `config.rs` that combines the model, parser, budgets, and inline tests.

### 3.12 Self-Iteration History and Memory Ownership

`tools/self_iteration::history` must use real Rust modules: `runs` owns run loading and workload/profile selection, `persistence` owns report/run writes and record construction, `export` owns CSV/SVG rendering, and `run_state` interprets adoption and evaluation state. `mod.rs` only owns `HistoryPaths` and the stable facade, while `synthesis` builds bounded history summaries. The `memory` subtree also uses real modules: `api` coordinates public memory queries and writes, `records` constructs typed memory entries, `store` owns the atomic JSONL and Markdown boundary, `summaries` owns bounded prompt/report rendering, and `metadata` extracts normalized record evidence. Every behavior owner directly attaches its sibling `*_tests.rs` contract; production code must not use `include!` to merge these boundaries into an implicit namespace. Callers express dependencies through the `history` facade, `history::synthesis`, or `history::memory`. Do not restore root-level `history_synthesis.rs` or `memory.rs`, cross-boundary test buckets, or a monolithic `history.rs` with a large inline test module.

### 3.13 Self-Iteration Unattended Workflow Ownership

Unattended operation is the `tools/self_iteration::workflow::unattended` subdomain, not a top-level sibling that creates a circular dependency with `workflow`. It must use real Rust modules for the long-running lifecycle, durable state, cycle selection, candidate attempts, evaluation persistence, derived configuration, metadata, category rotation, macro triggers, deep checks, and outcome policy. `mod.rs` owns only shared stage contracts, policy constants, and the subdomain facade; every implementation imports its dependencies explicitly, and production code must not use `include!` to merge the workflow into an implicit namespace. State, category-rotation, and trigger unit tests stay beside and are attached by their matching implementation files. Do not restore a top-level `unattended` module, a root `unattended.rs`, or a workflow/unattended dependency cycle.

### 3.14 Self-Iteration Codex Generation Ownership

`tools/self_iteration::codex` separates process execution, command construction, normal prompt construction, unattended prompt construction, history-derived prompt context, and command-result mapping into the real Rust modules `execution`, `command`, `prompt`, `unattended_prompt`, `history_context`, and `result_mapping`. Every behavior owner attaches its sibling `*_tests.rs` contract directly, while `mod_tests` verifies only `CodexResult`. `mod.rs` owns the result contract and facade only; production `include!` assembly is forbidden, and a root `codex.rs` must not recombine external-process policy, prompt policy, history formatting, and inline tests.

### 3.15 Self-Iteration Workflow Ownership

`tools/self_iteration::main` is only the binary composition root. `tools/self_iteration::workflow` must use real Rust modules named for mode dispatch, loop control, manual evaluation, generated iterations, candidate evaluation, documentation gating, score persistence, report metadata, adopted-optimization documentation, terminal output, pacing, and run identities. `mod.rs` only declares these modules and exposes the crate-facing workflow facade; every implementation imports its dependencies explicitly, and production code must not use `include!` to merge the workflow into one implicit namespace. Run-identity and documentation-gate unit tests stay beside and are attached by their implementations. Cross-workflow callers consume capabilities through the crate facade; do not restore orchestration, persistence, documentation logic, or inline tests to `main.rs`.

### 3.16 Self-Iteration Process Boundary Ownership

`tools/self_iteration::command` owns external-process contracts in `mod.rs`, child lifecycle and timeout handling in the real `execution` module, pipe reader/writer workers in `pipes`, progress events in `logging`, bounded output selection in `output`, and failed-result construction in `failure`. Every behavior owner attaches its sibling `*_tests.rs` unit contract directly, while `mod_tests` verifies only the public command/result contracts. Production `include!` assembly is forbidden. Do not restore a root `command.rs` that combines process orchestration, worker plumbing, observability, formatting, and inline tests.

### 3.17 Self-Iteration Case Configuration Ownership

`tools/self_iteration::cases` separates recursive case-file loading, deterministic object/array merging, typed JSON field access, and repository grouping into the real Rust modules `loading`, `merge`, `fields`, and `grouping`. Each behavior owner attaches its sibling `*_tests.rs` unit contract directly; `mod.rs` only declares the modules and preserves the public facade. Production `include!` assembly is forbidden because it erases module ownership and makes sibling files share one implicit namespace. `tools/self_iteration/cases.json` is the bounded workload manifest and global-suite owner; repository query targets live in descriptive included JSON files, including dedicated project-alias, relay-teams, Linux, LevelDB, Spring Framework, and Kubernetes files. Do not restore a root `cases.rs` that combines configuration I/O, merge policy, access helpers, grouping, and inline tests, or grow the manifest into another monolithic query-case file.

### 3.18 Self-Iteration Research Plan Ownership

`tools/self_iteration::research_plan::mod` owns the input contract and declares `render` as a real Rust module, while `render` owns deterministic plan rendering and explicitly attaches the sibling `render_tests` unit contract. Keep these files together under the research-plan domain; production `include!` assembly is forbidden, the facade must not own rendering tests, and rendering plus inline tests must not return to a root `research_plan.rs`.

### 3.19 Self-Iteration Candidate Git Ownership

`tools/self_iteration::candidate_git` owns the patch snapshot contract, bounded Git command execution, worktree inspection, patch capture/path extraction, and candidate rejection/commit lifecycle in the real Rust modules `mod`, `command`, `dynamic_command`, `worktree`, `patch`, and `lifecycle`. Each behavior owner attaches its sibling `*_tests.rs` contract directly; the explicitly named `git_repository_fixture` is test-only infrastructure for isolated repositories. Production `include!` assembly is forbidden. Loop sleeping belongs to `workflow::pacing`, not the Git boundary. Use the explicit `candidate_git` name at call sites; do not restore an ambiguous root `git_ops.rs` or mix workflow pacing into repository mutation.

### 3.20 Production and Unit-Test File Ownership

Production Rust files must not embed a `#[cfg(test)] mod` implementation. Each unit-test module lives in a descriptive sibling `*_tests.rs` file and is attached from its production owner with an explicit test-only `#[path]`; the production file remains the only owner of the module declaration so white-box visibility and test identity stay stable. The `api` contracts apply this one-to-one pairing to `agent`, `code_repository`, `error`, and `stream`; application-layer repository, indexing, repository-set, view, knowledge, service, and update units follow the same rule. Code ingestion and indexing units apply it across language metadata, generated detection, identity, index planning/snapshots, parser workspaces/languages, and source discovery. Domain core, graph, code, repository, workspace, knowledge-map, runtime, and software contracts also keep their unit tests in explicit sibling files, even when the declaration occurs before later production types. Bootstrap, evaluation, top-level indexing, network/QoS, observability, paths, retrieval, and watcher foundations use the same pairing without weakening their ownership boundaries. Storage contract tests must exercise every optional `CodeRepositoryStore` default so unsupported leases, checkpoints, bounded candidate lookups, repository sets, views, and software projections remain explicit rather than silently succeeding. The partitioned storage `mod_tests` contract covers empty-control delegation, indexed-shard routing, task leases, repository-set control state, and staged checkpoint finalization for `PartitionedSqliteKnowledgeStore`. Interface tests stay inside their owning CLI, Web, ACP, MCP, audit, and policy adapter directories; the MCP HTTP/JSON-RPC fixture boundary is named `transport_harness`. SQLite code-store, code-query, scoring, import/call planning, view, retrieval, maintenance, retry, pool, and schema tests remain beside their precise persistence owner; `code_tasks` owns its lifecycle, retention, status, lease, and reset suites directly, while `record_mapping` isolates task/checkpoint row decoding and SQL projection construction. Repository-set membership, overlay, and workspace suites are owned by `code_set`, and durable refresh-task tests are owned by `refresh_tasks`; the outer code-store facade must not attach them. Candidate-path filtering, FTS planning, generated exclusion, legacy import, and fallback tests are attached directly by `candidate_paths`, not by the wider snapshot facade. Partitioned SQLite integration data builders live in `partitioned_sqlite_fixtures`. Do not merge these tests back into production files or create a shared catch-all test bucket.

The `code_workspace` owner attaches both its facade `mod_tests` suite and focused `lookup_tests`; the outer code-store facade must not attach workspace normalization tests.

SQLite import-query target, generated filtering, ranking, and foundational ranking suites are attached by `code_query::imports`. Ambiguous-callee unit and generated-filter suites are attached by `ambiguous_callees`; the outer code-store facade must not attach either subdomain's tests.

Symbol query owns `mod_tests` and generated-filter suites directly. Typed function-value parsing and ranking live in the focused `symbols::typed_function_value` module so symbol SQL retrieval, surface interpretation, and tests do not accumulate in one near-limit file.

Application repository, derived-view, runtime, and shared-service facade tests use the explicit `mod_tests` name. Do not restore ambiguous `tests.rs` files for these module owners.

Code feature-flag extraction, route parsing, and source-search facade tests also use `mod_tests`; behavior-focused child suites keep their descriptive names.

CLI render, repository, repository-set, and setup adapters pair their `mod.rs` owners with `mod_tests.rs`; the Web router facade follows the same convention while cross-router suites retain explicit integration names.

The model-provider facade pairs `mod.rs` with `mod_tests.rs` for profile, catalog, probe, discovery, and fallback behavior.

SQLite canvas, code-graph, code-schema, code-view, file-index, Maven, operations, and retrieval owners pair their facades with `mod_tests.rs`. Focused schema, ranking, migration, and persistence suites keep descriptive filenames; generic `tests.rs` is forbidden in these storage domains.

SQLite code-query hybrid chunk evidence admission lives in `hybrid::chunk_gate`, paired with `chunk_gate_tests`; direct-result admission, FTS query construction, candidate budgeting, and chunk-result merging keep their tests beside their own production owners. The code-query facade only orchestrates layers and must not regain hybrid evidence-density or language-scope policy.

Root SQLite schema compatibility belongs to the file-only `sqlite/schema` group: `initialization` owns core graph DDL and fact-evidence backfill, `columns` owns legacy column repair, `marker` owns schema fingerprints, and `migration` owns safe obsolete-schema preparation. Each stateful schema owner keeps its focused tests in that directory; the SQLite store facade must not embed DDL or migration loops.

SQLite software dependency-usage persistence, language matching support, and its unit contract live in the file-only `software/dependency_usage` group. The parent software projection facade must not regain dependency-usage implementation or test files.

Within dependency usage, `schema` exclusively owns table creation and the one-time projection invalidation decision, including the existing-table no-op contract. Matching and persistence code must not issue schema DDL or decide whether historical projections become stale.

Dependency-usage `persistence` owns scoped deletion, idempotent row writes, bounded filtered reads, import-evidence row mapping, and graph-version reconstruction. Its paired tests cover round-trip mapping, path/language filters, and scope deletion; the workflow owner must not embed SQL projection or row-decoding logic.

Dependency-usage `matching` owns the immutable component-key index, manifest-owner narrowing, Cargo alias evidence, and bounded cross-language import key normalization, together with the matching test suite. The `mod` workflow only coordinates evidence, matching, confidence intersection, deduplication, and deterministic ordering, and it must short-circuit before import reads when no component can match.

The ACP adapter, prompt-context builder, and their paired tests live in the file-only `interfaces/agent/acp` group while preserving the public `interfaces::agent::acp` path. The parent agent directory must not flatten ACP session or context implementation files beside MCP, audit, and policy domains.

ACP initialization, session, prompt, progress-update, result, and error wire contracts belong to `acp::protocol`, with `protocol_tests` covering JSON field names, omission rules, and state transitions. The adapter facade only re-exports these public types and orchestrates session requests; it must not embed serialization DTOs again.

ACP session identity, active-request cancellation channels, and automatically cleaned-up leases belong to `acp::session_registry`. That owner normalizes untrusted client metadata, with paired tests covering session lookup, cancellation notification, explicit release, and drop cleanup; the adapter facade must not maintain shared maps or mutexes directly.

ACP prompt scope authorization, freshness parsing, resource limit/context-byte validation, and domain-request construction belong to `acp::prompt_mapping`. `prompt_context` only executes validated graph or codegraph requests and summarizes results; dependencies remain `prompt_context -> prompt_mapping`, with no reverse dependency that would create a cycle.

Worktree-overlay indexing lives in the `code/index/worktree_overlay` directory, with physical filenames that describe the `dirs`, `git_overlay`, `overlay_plan`, `overlay_scope`, and `untracked` responsibilities. Do not restore `worktree_overlay_*` prefixed files at the `code/index` root or weaken bounded-change, Gitlink-expansion, or scope-filtering contracts.

Worktree-overlay hash markers, deletion sets, parse queues, and same-content skip decisions belong to `worktree_overlay::recording`, with sibling unit tests fixing the binary framing and delete-then-recreate semantics. The main overlay orchestration and Gitlink handling reuse this recording boundary and must not implement divergent overlay-hash input protocols.

Worktree-overlay Gitlink output aggregation, child-path deletion replay, and the scope-aware recorder belong to `worktree_overlay::gitlink_recording`. That owner uses the shared `recording` protocol, with paired unit tests proving retained, out-of-scope, and deleted child paths stay distinct; the Gitlink state machine must not duplicate the recorder or invent new markers directly.

The top-level `code` facade pairs `mod.rs` with sibling `mod_tests.rs`; source discovery, layout, submodule, filesystem, and worktree-overlay scenario tests remain grouped under `code/tests/source`, with their reusable fixture owner in `code/tests/fixtures.rs`. Do not restore a sibling `tests.rs` alongside the scenario-test directory or move facade invariants into the source scenarios.

Every remaining sibling test attachment is explicit: runtime, service, repository/source-fallback/view workflows, code feature/search boundaries, and SQLite Maven, view, schema, batch, graph, workspace, operation, indexing, retrieval, snapshot, and root adapters declare their concrete test filename with test-only `#[path]`. Implicit `#[cfg(test)] mod name;` file resolution is forbidden because renames or same-named directories would otherwise hide the physical owner.

The self-iteration config, scoring, and history facades keep their facade contract assembly in sibling `mod_tests.rs` files. The evaluator root has no behavior and therefore no facade test bucket: runtime, quality, judge, fixture, and workload tests attach directly to their precise owners, including the repository-set workload's sibling `repository_set_tests.rs` provenance contract. Test bodies must not return to production files or a cross-owner evaluator test assembly.

The self-iteration Codex adapter attaches `command_tests`, `execution_tests`, `history_context_tests`, `prompt_tests`, `unattended_prompt_tests`, and `result_mapping_tests` from their exact production owners through explicit test-only `#[path]` declarations. Each test file contains test items directly; production-time `include!` expansion, facade-owned behavior tests, and same-named nested test modules are forbidden.

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
