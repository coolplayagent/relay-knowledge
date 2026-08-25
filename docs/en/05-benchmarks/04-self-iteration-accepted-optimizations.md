# Self-Iteration Optimization Status Ledger

[English](../../en/05-benchmarks/04-self-iteration-accepted-optimizations.md) | [中文](../../zh/05-benchmarks/04-self-iteration-accepted-optimizations.md)

This page is the compact English companion to the self-iteration optimization
ledger. The Chinese primary ledger keeps the rolling detail and archives older
entries before they exceed the repository file-length cap.

The ledger contains `candidate`, `blocked`, `accepted`, and `rejected` work.
A candidate remains a candidate whenever its section still requires an A/B run,
all selected cases, a release product binary, or an environment gate. Only a
record with a completed scope, budgets, product-binary identity, result, and
durable reviewable evidence is accepted; expected impact alone never implies
acceptance. Paths below `.git/relay-knowledge-self-iteration/`, absolute local
workspace paths, and evaluator scratch directories are machine-local context,
not published evidence. A current acceptance record must cite a tracked dated
report with the revision or report digest, selected/executed/skipped counts,
profile, product binary, environment, budgets, and result.

## Cold-Build-Safe BM25 Quality Gate

- Root cause and evidence: repeated clean or cache-invalidated evaluations exhausted the former 120-second `bm25_hierarchy_suite` limit while Cargo was still compiling or linking; no test had started. With the exact library test target already built, all 50 tests complete in roughly 9 seconds. Counting cold compilation in the suite's 30-second metric also made that algorithm observation describe build state rather than BM25 behavior.
- Execution boundary: every non-smoke profile now runs `cargo test --lib --all-features --no-run` as an isolated `bm25_hierarchy_build` stage with the existing root Rust-gate ceiling of 1,200 seconds and no metric budget. Only after that stage succeeds and releases the Cargo build lock does an isolated `bm25_hierarchy_suite` stage retain the existing 120-second execution timeout and 30-second non-key whole-suite diagnostic budget.
- Invariants and risk: the named test filter, all features, 50 deterministic checks, `BM25_WORK`, and diagnostic budget are unchanged. The harness does not prewarm opportunistically, skip tests, inflate the suite budget, or wait without a bound. A machine that cannot compile the exact target within 1,200 seconds still fails preparation, while suite execution above 30 seconds remains visible in diagnostics/scoring but is not by itself a hard gate failure.

## Unified Release Product-Binary Evaluation

- Every non-smoke profile now builds and runs the release product binary while allowing the harness itself to remain a debug binary. The build gate and workload path share the same `ProductBinaryProfile` decision.
- Evaluation and outer reports record the product profile and path, while run records retain the product profile. Workload history and the profile-wide hard acceptance floor select only the same product-binary profile; legacy missing fields retain the old `fast=debug`, non-fast=`release` meaning.
- The best accepted score across product-binary profiles remains visible only as diagnostic comparison metadata. It cannot accept or reject a candidate. Regression tests protect both legacy fast-debug isolation and this acceptance-floor boundary.

## Child-Exit-Driven Command Timing

- Root cause and implementation: the evaluator previously checked `try_wait`, then slept for 20 ms whenever a child was still running. Short CLI queries therefore accumulated a fixed polling quantum and produced staircase-shaped latency observations. Command completion now waits on the operating system's child-exit notification, bounded by the earlier of the command timeout and the next 15-second progress heartbeat.
- Invariants: timeout still kills and reaps the child, stdout/stderr readers and optional stdin writer still converge before the result is returned, and progress logging retains its existing cadence. No query, fixture, or acceptance budget is widened.
- Regression: a focused command test runs sixteen two-millisecond children and requires their aggregate reported duration to remain below 240 ms. The threshold leaves broad scheduler headroom but fails the former 20-ms-per-child polling floor.

## Cold Staged Edge-Search Materialization

- Algorithm and architecture: checkpointed batches continue to persist every reference and import fact immediately, but they materialize intermediate reference/import FTS documents only when the batch scope is the repository's current `last_indexed_scope_id`. A new or retained non-active scope can already have a `code_repository_scopes` row; that registry row alone does not make the in-flight generation active and no longer triggers FTS work that reference/import finalization will replace.
- Invariants: reference and import resolution, call rebuilding, final edge-search rebuilding, checkpoint replay, task leases, publication fencing, and freshness transitions are unchanged. A cold staged scope remains stale and unpublished until all finalization and software-projection work succeeds; its final reference/import/call facts and search documents remain complete.
- Regression and measurement: direct persistence tests require a stale staged scope to have no reference/import search documents after batch apply, require finalize to rebuild complete language-tagged reference/import/call documents, and require the active-scope reindex path to keep immediate edge-search materialization. Cold A/B evaluation must compare batch FTS rows and register-to-index wall time on isolated homes without changing performance budgets or repository-specific behavior.

## Candidate: Durable Staged Reference-Search Pages

- Algorithm and boundary: an unpublished fenced full scope replaces per-occurrence reference FTS documents with grouped-v2 cleanup/discover/build pages. Discovery stores one searchable owner for each exact `(name, kind, path, target_hint)` identity plus its occurrence count; Kube's retained evidence was 486,702 groups for 2,879,261 occurrences (5.92x fewer reference FTS documents). Each page reserves its progress mutation and is capped by `min((max_rows_per_batch - 1) / 3, 32768)`, the configured byte budget, and 16 MiB. Query MATCH selects bounded groups, then exact indexed lookup expands occurrences without losing source ranges; fair water filling prevents a hot group from starving other groups, and a fixed VM-step limit returns observable capacity failure instead of silent partial recall.
- Recovery invariants: the canonical checkpoint token stores only protocol version, stage, and completed page ordinal; record/reference/group keyset cursors and frozen counts live in a checkpoint-owned progress table. Each page CAS-commits data, progress, and token under before/after fence validation, and a partitioned shard must prove the exact catalog staged owner on both sides. Rollback/reopen replays the same page; malformed state, stale fence, owner drift, count mismatch, oversized rows, missing manifest, or the checked `BASE + 3 * references + 4` hard-bound exhaustion fails closed. Legacy v1 state is reset to v2 page zero only by the leased writer after its durable budget is revalidated and its page limits are clamped.
- Evidence and acceptance: the retained Kubernetes candidate committed all 30,353 files and 2,879,261 references in 61 batches, reached `finalizing:refresh_dependencies` at about 296 seconds, and timed out at 360 seconds, leaving roughly 64 seconds in the likely reference-search phase without a later durable token. This is phase attribution only. Acceptance still requires an isolated release-binary A/B under the unchanged budget plus exact FTS `MATCH`, metadata-count, task-terminal, checkpoint, and freshness checks; source-level and unit-test evidence does not claim a wall-time win.

## Issue #354: Commit-Driven Knowledge Loop

- Architecture: manual `repo update` and managed checked-out-HEAD reconciliation now resolve immutable base/head/tree inputs and enter the durable code-index queue. Native Git ref notifications are latency hints; a bounded five-second reconciliation backstop covers linked worktrees, missed events, and restarts. Stable per-ref fingerprints coalesce repeated hints, and existing attempt-scoped leases preserve one active writer per repository.
- Resource and storage guardrails: a delta admits at most 512 changed paths, while task admission permits 32 unfinished tasks per repository and 256 globally. Publication retains the union of active and a rolling window of the two latest successes (normally overlapping), plus the latest incremental predecessor, active-worktree clean-base, unfinished-task, and repository-set pins. It atomically retires one old scope, then durable GC advances one scope-GC phase whose physical deletion is capped at 512 rows in aggregate across the affected code/search/software application tables per maintenance transaction. Finished task history is bounded to 128 success and 64 failure-class rows per repository.
- Regression evidence: existing 1024-file fast and 2048-file full performance fixtures continue to execute the real `repo update` path and enforce bounded blob-read/parse counts. Focused watcher integration tests cover commit task queueing without an explicit update call, repeat-hint coalescing, and reconciliation failure diagnostics. This orchestration change does not claim a new wall-time improvement, generic Knowledge Graph publication, or semantic/vector generation parity; those require separate measured evidence.

## Issue #168: Large-Repository Register-To-Index Throughput

- Algorithm and architecture: the default full-code-index batch now covers 512 files while retaining the 16 MiB blob cap and raising the bounded row cap to 150k. Checkpointed SQLite batch apply skips the empty-scope path-index existence probe for the first new batch, while later batches still keep collision cleanup and replay idempotency.
- Guardrails: the default fast self-iteration profile includes the generated `index_performance_many_files` repository with 1024 small Rust files, recording cold register/index and incremental-update latency through the real `repo register`, `repo index`, and `repo update` paths.
- Higher full-profile standard: full and exhaustive self-iteration now add `index_performance_wide_mixed_files`, a generated 2048-file Rust workspace with cross-shard bridge queries and separate cold index, register-plus-index, query p50, query p95, and query max budgets. The default fast profile is unchanged.
- Limits: no CLI/API shape, SQLite schema, parser fact, FTS document semantics, edge finalization, freshness/status, task lease, checkpoint, or source-fallback budget changed. Performance fixes must not skip indexing work, hide degraded states, use unbounded timeouts, or special-case repositories, paths, queries, symbols, or case ids.

## Issue #147: Cross-Language Call Graph

- Algorithm and architecture: call-target resolution keeps the original target hint and adds only constrained same-repository leaf candidates for cross-language boundaries. C/C++ calls keep direct symbol names, Go cgo maps `C.<name>` to `<name>` only from `.go` files, and Rust FFI/bindings paths add a leaf candidate only for `ffi`, `bindings`, `libc`, or `*_sys` prefixes.
- Invariants and limits: no SQLite schema, parser facts, FTS content, ranking weights, semantic/vector read model, CLI/API, or installation behavior changed. The capability is a static same-repository code-graph feature; it does not claim full build-system, linker, dynamic-loading, macro-generated call, external prebuilt SDK, or unindexed bindgen coverage.
- Guardrails: resolution only targets callable symbols, prefers a unique implementation over header or signature-only declarations, keeps ordinary namespace calls from collapsing to broad leaf aliases, and is covered by the default-fast `cross_language_syntax_fixture`.

## Issue #154: Query-Aware Source Fallback Candidates

- Algorithm and architecture: when exact source fallback needs broad scope paths, storage first narrows candidate files through indexed `code_repository_search` FTS with the query plus path and language filters. It falls back to bounded scope enumeration only when the query has no indexed candidate.
- Current runtime: product fallback now materializes Git blobs and searches them with the internal fixed-string scanner behind the blocking-worker boundary. It keeps the same 256 candidate-file, 8 MiB blob, 4096-byte line, result-limit, safe-path, path-filter, language-filter, and `text_fallback` provenance budgets. The product hot path no longer depends on an external `rg` process.
- Diagnostics: the historical issue text used `ripgrep candidate file budget exhausted`; the current diagnostic uses `source fallback candidate file budget exhausted` for the same bounded candidate exhaustion state. Existing structured symbol and definition hits remain valid when fallback is degraded.

## Issue #146: Nonstandard Source Layouts

- Algorithm and architecture: repository source normalization treats source roots as a layout set rather than a single top-level `src/` convention. The indexer and import/module resolver recognize `external_deps/`, `packages/`, `modules/`, `plugins/`, `extensions/`, `Sources/`, `lib/`, nested JVM roots, and C/C++ `include/` segments.
- Invariants and limits: source-root discovery still avoids widening a deliberately narrow `--path src` registration into broad dependency trees, while whole-repository Git scopes let tracked `vendor/` and `third_party/` paths participate like other tracked directories. TypeScript bare specifiers resolve only when a local indexed module candidate exists, and ambiguous local matches stay protected.
- Guardrails: `nonstandard_layout_fixture` is included in the default fast profile and covers Python, TypeScript, Go, Java, C++, and Swift source outside a top-level `src/` directory without repository, path, query, symbol, or case-id special casing.

## Issue #166: Registration Language Filters

- Algorithm and architecture: repository registration rejects non-empty language filters so mixed-language repositories keep their complete indexed language surface. Query-time `--language` remains the supported narrowing mechanism.
- Guardrails: the default fast self-iteration profile includes a generated cross-language registration case that expects `repo register --language cpp` to fail with the stable registration-language error.

## Issue #167: C External Header Macro Recovery

- Algorithm and architecture: C parser recovery now treats isolated typedef-style external-header declarations, module tables, and uppercase macro calls with declaration bodies as recoverable when structured symbols, references, imports, or calls are still extracted. Macro-generated C function symbols expand to the following compound body so call ownership remains available.
- Invariants and limits: missing Nginx/Kong-style headers stay unresolved import metadata with `target_hint`; they are not file degradation. Broken assignments, preprocessor-branch syntax errors, registration macros, and non-body data macros still surface diagnostics or stay out of the call graph.
- Guardrails: the default fast `c_syntax_fixture` includes unresolved `ngx_*` headers, a `KONG_ACCESS_PHASE` handler, typedef-style module tables, symbol/definition/callee/import cases, and no repository/path/query special casing. Local macro lookup now accepts spaced `# define` directives, continued directives, and bounded numeric/comparison `#if` conditions, and treats `#undef` plus inactive branches as unavailable so stale macro bodies cannot create caller ownership.

## Issue #185: GCC Extension Recovery

- Algorithm and architecture: C/C++ parser recovery now recognizes GCC/Clang declaration attributes and inline extensions such as `__attribute__`, `attribute`, `always_inline`, and `__always_inline` when the surrounding function or table declaration is otherwise well shaped.
- Invariants and limits: missing SDK headers such as `securec.h` stay unresolved import metadata, not `degraded_reason`; broken assignments and broken function bodies still report partial parser diagnostics.
- Guardrails: the default fast `c_syntax_fixture` covers GCC/EulerOS-style attribute/inline functions, PascalCase SDK types, unresolved `securec.h`, and definition/callee/symbol/import retrieval without SDK path or query special casing.

## 2026-08-25 Bounded Source-Fallback Diversity and High-Fanout Cases

- Algorithm: broad exact-text fallback keeps the existing 256-file and 8 MiB boundaries, but exposes a fair candidate pool of at most two matches per candidate path and 512 matches total before final top-k scoring. Within each path, code-shaped references are retained before comments and import-only mentions; reference scoring demotes explicit document/configuration languages and comment-only evidence without deleting them.
- Evaluation contract: unscoped high-fanout import/reference cases now assert the language, resolved target, target hint, or code evidence shared by every equivalent correct result. They no longer require one arbitrary importer path when the query contains no importer/path context. Existing scoped regressions continue to require their exact paths, so this corrects an invalid oracle rather than weakening path-filter behavior.
- Focused evidence: the original Alamofire `SessionDelegate` reference query moved the code usage in `Source/Core/Session.swift` from outside top 20 to rank 1. Spring `ObjectUtils` and Kubernetes package-import probes returned many correct equivalent production importers, which is the evidence for normalizing the broad cases; these focused probes do not claim exhaustive/Kubernetes performance acceptance.
- Invariants: no repository, path, symbol, query, fixture, or case id is enumerated in product code; candidate-file, blob-byte, line-byte, freshness, authorization, graph-edge, durable-task, and final result limits are unchanged.

## 2026-08-25 Focused Durable-Incremental Performance Acceptance

- Contract correction: a fenced durable-clone response may expose a completed checkpoint for the entire selected target scope. The harness now validates that full target count and exact repository/scope identity, while taking changed-path, blob-read, and parsed-file costs from the task-bound `incremental_summary` receipt. It rejects a delta-only checkpoint, a receipt from another task, stale scope identity, or a partial committed count.
- Accepted focused evidence: report `manual-evaluate-1787626639163458032-0-2616431.json` passed 79/79 gates and 18/18 cases with score `1.0` and `would_accept`. The release binary indexed the 1024-file fixture in 457 ms against 12,000 ms, completed register plus cold index in 558 ms against 13,000 ms, and completed the three-path incremental update in 436 ms against 3,000 ms while reading two blobs and parsing two files.
- Scope: this accepts the default fast 1024-file rail and the receipt-aware evaluator contract. It is not evidence that the 2048-file full fixture, exhaustive evaluation, Linux cold index, or the Kubernetes 210-second budget has passed.
- Follow-on bounded candidate: the shared six-column FTS writer now clamps a full flush to the runtime SQLite variable limit and at most 1,024 documents/6,144 binds instead of 256 documents. The named performance gate requires 1,025 documents to use exactly two main FTS inserts, covers the 12/6/5-variable two-row/one-row/reject boundaries, and retains rowid-interval, metadata-owner, rollback, and `INT64_MAX` fail-closed checks. This is accepted mechanism evidence only; the Kubernetes wall-time rail remains failed until a new isolated release run completes within 210 seconds.
- Follow-on grouped-build candidate: an already admitted reference-search build page now uses one ordered FTS `INSERT ... SELECT` and one scoped metadata insert instead of Rust-side document materialization and repeated `VALUES` flushes. The named gate traces 1,025 groups as one plus one statements and preserves canonical blank-field content, exact rowid intervals, same-transaction ownership, and `INT64_MAX` prewrite rejection. This is mechanism evidence only and does not change the failed Kubernetes wall-time status.
- Follow-on base-fact candidate: reference, symbol, and chunk multi-values statements now admit at most 1,024 input-ordered rows rather than 256, while their 16,384-, 17,408-, and 12,288-bind full shapes remain below bundled SQLite's 32,766-variable ceiling. Runtime connection limits still shrink each owner independently and fail closed below one row. The named gate requires 1,025 facts of every owner to form exactly two base statements and retains tail-failure rollback, staged replay, FTS order, and checkpoint/fence ownership. This is mechanism evidence only and does not change the failed Kubernetes wall-time status.
- Follow-on finalization candidate: grouped cleanup/discovery/build lazy scans now retain only the final admitted lookup key and point-fetch exactly one durable cursor per page. Ordinary reference-resolution pages continue to scan and count call references but leave their payload and owner untouched for the dedicated call-target phase; a 1,025-row call-only trace proves zero payload fetches, zero reference updates, and one final-cursor fetch, while the call-target regression still downgrades stale non-callable bindings. Deterministic grouped discovery work moved from 126,790 legacy VM steps to 56,472 production steps, a reduction of 70,318. These are mechanism and ownership-boundary guards, not Kubernetes wall-time acceptance.
- Kubernetes follow-up: the exact fresh-home release target first completed normally in one attempt at 592.72 seconds with succeeded/completed/fresh terminal state and exact 30,353-file scope. After the 1,024-row base-fact candidate, an identical fresh run completed in 607.03 seconds with the same facts and terminal state. After the final-cursor and ordinary-call ownership candidate, a clean monotonic single attempt completed in 564.99 seconds with the same facts and terminal state. That is 42.04 seconds (about 6.9%) faster than the immediately preceding sample, but one sample per candidate cannot establish causality and the result still fails the unchanged 210-second budget by 2.69 times. A separate clock-jump attempt and its 386.35-second generation-2 recovery remain diagnostic only. Seven previously failing focused queries pass their current evaluator contracts on an earlier index; neither focused correctness nor these mechanism tests constitute a complete exhaustive/Kubernetes acceptance report.
- Kubernetes phase diagnostic: a separate fresh-home run polled read-only status roughly every 10 seconds and completed in 612.08 seconds. Polling contaminated total latency, so it is not an acceptance sample. Coarse observations put base ingest near 158 seconds, query-index plus ordinary-reference work near another 100 seconds, grouped discovery/build near 107/109 seconds, call rebuild near 30 seconds, and software projection near 67 seconds. The cadence cannot provide exact phase timing, but it identifies reference-wide finalization for bounded optimization without authorizing a skipped stage or larger budget.
- Current-candidate verification: report `manual-evaluate-1787657485515273930-0-3038475.json` passed all 346 gates, 119 cases, and 293 commands with score 1.0, `score_accepted=true`, and `adoption_status=would_accept`; manual evaluation created no commit. The 1,024-file product rails were 382/12,000 ms cold, 453/13,000 ms register plus cold, and 423/3,000 ms incremental. Release build was 321/180,000 ms and the named persistence suite was 739/30,000 ms. This closes the preceding focused-fast release-build rejection while leaving exhaustive and the failed Kubernetes rail independent.

## Documentation Maintenance

- The primary Chinese accepted-optimization log is kept below the 1000-line hard cap by moving late detailed records to dated archive files.
- Capability and architecture pages in both languages document the current source-root, cross-language call-target, and internal source-fallback behavior.

## 2026-06-05 Research Self-Iteration Planning Mode

- Algorithm and architecture: `tools/self_iteration` now includes a read-only `research-plan` mode that turns the arXiv, X.com, Reddit, open-source, and systems-engineering deep research workflow into a Markdown plan covering source ledgers, credibility tiers, synthesis matrices, competitive issue extraction, bilingual docs, archive validation, and remote-main publication evidence.
- Invariants and limits: the mode does not call Codex, run evaluation, write self-iteration history, or change product CLI/API, indexing, storage, network, or release behavior. It gives future research iterations a reusable starting checklist while preserving real-source, independently testable issue, and documentation-archive requirements.

## 2026-08-02 Minute-Scale Cold and Incremental Index Evaluation

- Candidate generation now defaults to `gpt-5.6-sol` with `xhigh`. Performance fixtures recreate a repository-scoped runtime home and require cold task or parsed-file completion evidence, so a cached zero-change index cannot masquerade as a cold-index improvement.
- Cold Git blob loading first tries one bounded `cat-file --batch` and retains the existing missing-object/submodule fallback. Incremental Git indexing prefetches ordinary changed blobs within the existing 512-file/16 MiB budget instead of spawning one `git show` per changed file.
- The 1024-file fast fixture and 2048-file full fixture create a second commit with modified, added, and deleted paths, run `repo update` from the persisted base, record explicit cold/incremental metrics, and enforce bounded delta read/parse counts. Durable leases, checkpoints, single-writer ownership, retry policy, freshness, FTS writes, and edge finalization remain unchanged.
