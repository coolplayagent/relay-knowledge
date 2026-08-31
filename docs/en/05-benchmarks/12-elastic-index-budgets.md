# Elastic Long Budgets for Large Repository Indexing

[English](12-elastic-index-budgets.md) | [中文](../../zh/05-benchmarks/12-elastic-index-budgets.md)

## Purpose

Large repository indexing no longer uses one fixed 180-second hard timeout. The 180-second value remains a historical baseline for regression comparison; the execution budget scales with repository size and observed throughput.

## Budget Calculation

Elastic mode is enabled by default: omitting `index_budget_mode` is equivalent to `elastic`. Only an explicitly selected fixed/strict mode disables scale-based calculation. For a Git source, the evaluator records the raw parent-tree count from the pinned ref with `git ls-tree -r -z --name-only <ref>`. An explicitly declared `expected_file_count` remains authoritative and must equal the product scope preview's exact selected count; the raw Git observation never overwrites it. When no expected count is declared, the raw observation supplies the elastic scaling input. The two counts may legitimately differ because the product scope excludes preset files or expands available gitlinks.

Here `N` is the declared expected count when present, otherwise the observed pinned-tree count, and finally the configured baseline count when neither is available. The budget is calculated in this order:

1. When a throughput baseline is configured:

   `index_budget_ms = N / baseline_files_per_second × 1000`

2. Otherwise:

   `index_budget_ms = baseline_index_budget_ms × N / baseline_file_count`

3. Clamp the result to `max_index_budget_ms`.

Registration adds the bounded `register_overhead_budget_ms`. The process timeout receives only a finite recovery margin; it does not bypass checkpoint, lease, freshness, or completion requirements.

## Persistence and Recovery Invariants

Elastic budgets change waiting time, not the indexing consistency contract:

- Every batch writes a durable staging manifest before the single writer
  commits facts, FTS rows, and checkpoint progress. Final freshness remains
  withheld behind the fenced code-and-software publication barrier.
- Workers use bounded attempt-scoped leases; orphan recovery reclaims expired
  work without preempting a live lease.
- Reset or worker restart resumes only after an exact checkpoint CAS inside
  session `begin`; an already committed batch index is a fence-validated pure
  no-op and cannot replace published facts or reset its manifest.
- Incomplete staging, edge finalization, or query-index construction cannot
  mark a scope fresh; status remains indexing, stale, or degraded.
- Parser, queue, batch, FTS-write, SQLite-transaction, and retry limits remain
  bounded.

### Query-index write-amplification policy

The stable query-index plan is version 3 with 17 ordered slots. Unit 1 retains
its legacy identity but is retired: the product neither creates nor drops
`code_repository_symbols_lookup`; if the name already exists, its complete
shape remains strict. A missing unit 1 is a stable skip only for current v3 or
coarse scans. Canonical v1/v2 cursors retain their parsed version across writer
quanta and cannot advance through unit 1 without the physical legacy index.
Database startup validates existing indexes and performs no query-index DDL.
Every fresh Restart prepares only chunk units 13 and 14 while their complete
shared owner is empty, regardless of path count or later byte/row batching; a
populated owner and every resume prepare neither, and all other heavy
descriptors remain deferred.

After the complete file prefix is durable, finalization creates at most one
missing required descriptor and advances the matching `v3:<ordinal>` state in
the same transaction. V1/v2 subphase and v2 repair tokens remain readable
without ordinal reinterpretation; current formatters always emit v3. FTS
writes, facts, staging manifests, leases, and freshness gates are unchanged.
Direct snapshot and database-import paths have no durable finalizer, so before
changing facts they may prepare required empty-owner indexes and then require
all required slots; a missing required index on a populated owner fails closed.

The motivating inactive Kubernetes snapshot measured the retired unit-1 index
at 420,921,344 bytes. `EXPLAIN QUERY PLAN` selected
`code_repository_symbols_name_path_lookup` for identity, scoped-identity, and
reference-resolution grouping; the snapshot contained 486,702 symbol
name/kind/path/hint groups over 2,879,261 references. These are diagnostic
observations, not a post-change wall-time claim. The non-smoke
`code_index_recovery_cases` self-iteration gate runs the `code_index_task_`
structural tests under fast and performance-focused evaluation. Those tests
fail if unit 1 is built again, if v1/v2 completed-prefix strictness or
cross-quantum version preservation is lost, or if fresh Restart stops preparing
chunk units 13/14 on an empty owner while continuing to defer every other
heavy index even for a one-path session.

A retained Kubernetes `finalizing:build_query_indexes:v2:12` cursor keeps its
original proof boundary: units 0 through 12, including physical unit 1, must
still validate exactly before v3 continues at chunk units 13 and 14. This
upgrade does not drop the 420,921,344-byte legacy index or pretend the inactive
snapshot completed; the early chunk-build benefit applies to future fresh
Restarts whose chunks owner is empty.

### Durable incremental clone and finalization rail

Every fenced clean incremental measurement now exercises the durable clone path rather than selecting a direct transaction by base size. The base checkpoint supplies the actual cumulative `committed_fact_row_count`; zero or absent proof triggers a prewrite full-staging fallback. A clone page remains bounded by the task's frozen row and byte quantum, advances a metadata-indexed keyset and checkpoint/progress CAS under the same fence, and skips affected owners without hiding their scan cost. Metadata is the authority for search copying; payload is fetched only after length admission, and each accepted page bulk-writes an exact contiguous FTS/metadata interval. The checked step proof is `5F + table_count + 4`, where `F` is the persisted fact proof rather than `batch_count × row_limit`. The terminal delta admission includes affected-owner cascade cleanup and the task-bound checkpoint receipt.

Workspace-free worktree overlays keep the direct fast path only while the complete overlay mutation fits that same frozen writer quantum. An over-budget overlay stays on its original task, fence, target scope, and synthetic identity: it stages the content-addressed worktree, clones the immutable clean base, and partitions the dirty delta deterministically by file ownership. Each worker step commits no more than one dirty batch, and a lease takeover resumes from the persisted batch count. The terminal quantum jointly admits owner cleanup, tombstones, fixed control rows, and the multi-batch receipt before any delta batch starts. Every indivisible file and its owned facts must fit one frozen byte/row quantum or fail before delta mutation. Receipt batch count measures parsed-file data work, so deletion-only affected-path metrics remain readable after clone ownership is removed without weakening parsed-file or SQLite-row bounds. The `code_index_task_` fast/performance recovery filter includes `oversized_worktree_code_index_task_delta_batches_and_recovers_between_leases`, which forces two dirty batches, expires and reclaims the lease between them, and asserts the exact receipt, terminal path, and completed checkpoint. This is the regression rail for the worktree fallback; raising queue, transaction, or retry limits is not an accepted substitute. Auto-workspace worktree projection stays fail closed until its manifest has a separately persisted recovery protocol.

The final dirty-batch handoff gives the complete unpublished target to `indexing`; the ordinary full finalizer must then perform query-index work, reference/import/call resolution, Maven effective dependency replacement with exact `F' = F - D + I`, grouped reference search, call rebuild, publication, workspace, and software projection. Response loss resumes that checkpoint without another clone or parse. Owner regressions cover missing-proof zero-write fallback, bounded multi-page reopen, stale-fence takeover rejection, deterministic dirty-batch lease takeover, terminal cleanup and terminal-path restoration, query-index repair, receipt byte boundaries, same-task response recovery, different-task neutral adoption, and Maven/reference semantic equivalence. The release-binary `index_performance_many_files` fast target is the performance acceptance rail: its 1,024-file base, three changed paths, two blob reads, two parsed files, completed task/checkpoint, and 3,000-ms incremental budget are asserted directly in benchmark CI. `index_performance_wide_mixed_files` retains the corresponding 2,048-file, 5,000-ms full/exhaustive rail. These budgets are not enlarged by the clone protocol.

## Observation and Interpretation

Reports should include observed file count, baseline values, computed budget, cap, cold-index duration, checkpoint progress, and final freshness. A task that is still progressing within its budget is not success until its durable checkpoint and finalization complete.

- **Completed within budget** means the task succeeded and the scope is fresh.
- **Still running within budget** means checkpoints continue to advance; it is
  not an early success.
- **Budget cap reached** means durable state remains recoverable and status
  exposes stale or degraded state without deleting published facts.
- **Lease or transaction failure** is a consistency/recovery regression, not
  merely a performance result.

## Cold Isolation and Shared Preload

Cold-index targets must not inherit graph or SQLite state from repositories that ran earlier. Each evaluation creates a unique, non-reused run root and home; heavyweight and full-scope external targets receive a repository-specific `RELAY_KNOWLEDGE_HOME` below that run home, while shared and isolated homes retain one global writer lock. The global lock prevents default repository parallelism from starting many disk-heavy cold writers at once and distorting latency through cross-store I/O contention. The harness collects repository commands, cases, metrics, and report evidence before cleanup. Recursive cleanup accepts only the exact non-symlink run/repository descendants after lexical-parent and canonical-parent validation, runs on error, and is skipped only with the explicit `--keep-workdirs` diagnostic option; it must never remove a reused or shared root.

This isolated result answers whether a repository can complete a true cold index within its elastic budget. It does not test shared preload, alias reuse, or order sensitivity. A small LevelDB workload intentionally remains on the shared evaluation home for that regression surface. Conversely, a passing shared-preload case cannot establish cold-index throughput because prior state may reduce work. Both signals are required and must be reported separately.

Repository-set members are never isolated from one another. Temporal and OpenTelemetry set members register and index in one shared evaluation home so the set overlay can resolve every member. The merged case configuration rejects `isolated_index_home=true` on any repository-set member.

Cold completion is strict: the checkpoint total must meet the configured floor, its committed-file count must equal its total-path count, the repository indexed-file count must cover that total, the durable task must be `succeeded`, the checkpoint must be `completed`, and repository status must be `fresh` with `stale=false`. The task transition is itself receipt-gated: it requires publication evidence for the same task and fresh target scope, so a stale attempt, a partial terminal label, or parsed-file volume cannot establish success. Every isolated repository has an implicit one-file floor unless it declares a higher value; the shared OpenTelemetry members declare a floor explicitly. The current `repo index` JSON carries these task, checkpoint, and repository fields, but does not expose a dedicated software-projection status object. Projection completion cannot become a separate harness assertion until the product response exposes its state, stale flag, and last error.

## Software-Projection Tail-Latency Contract

Completing code facts does not complete a repository index. Fenced full and incremental flows must continue through software projection. In a single SQLite database, software status, code scope/repository freshness, checkpoint completion, and the publication receipt become visible together only after projection succeeds. A partitioned store first commits code and software facts to the target shard while its catalog route remains `staged`, owned by that durable task through `staged_task_id`, and hidden from active-only reads. It then uses one control-database transaction to revalidate the fence and staged owner, activate the repository/scope route, mirror repository freshness/status, and insert the publication receipt. This is retryable convergence around the shard/control boundary, not cross-database atomicity: a pre-control crash resumes the staged shard, while a post-control crash reuses the receipt. Durable task `succeeded` is the immediately following separate fenced completion; it requires that receipt and matching fresh publication state, and the external worker response waits for the terminal task state.

The lifecycle-loader regression signals are `candidate_document_count`, `candidate_chunk_count`, and `candidate_materialized_bytes`. Its candidate set is derived from the existing build, IaC, and design parser support sets; SQLite filters ordinary source, with ceilings of 32,768 documents, 262,144 chunks, and 256 MiB. One candidate document is materialized in stable line order and shared across all three collectors, while persistence reuses prepared statements. The unit fixture fixes the amplification regression with 2,000 unrelated 4,096-byte Rust chunks plus one Cargo manifest: only one document, one chunk, and fewer than 128 bytes may cross the loader boundary. This metric protects I/O/materialization amplification; it does not replace real-repository end-to-end latency, FTS, edge, checkpoint, or task-terminal acceptance.

The software-file projection has a separate storage-amplification guard. Its owner test projects 1,025 ordered paths, crossing two full 512-row pages and one tail row, and requires the exact path/role/status/version sequence with 1,025 distinct stable ids. A SQLite prepare-time authorizer must observe exactly one `software_files` insert preparation for the complete refresh. This guard fixes prepared-statement and `OFFSET` regressions locally; end-to-end acceptance still requires the existing isolated release-binary performance targets and unchanged freshness checks.

The current `repo index` JSON still lacks a dedicated software-projection status object, so the harness cannot report projection fact counts and last error as separate assertions. The publication barrier nevertheless guarantees that projection failure cannot produce a `completed` checkpoint or `fresh` scope, allowing strict terminal validation to reject early success indirectly.

## Parse-Stage Amplification Guardrails

The pre-change exhaustive report `manual-evaluate-1786623651584251770-0-2786323` is diagnostic evidence, not an accepted post-change result. In isolated homes, both 93,601-file Linux targets reached the finite command timeout (`linux_sample` 1,201.124 seconds and `linux_full` 1,201.224 seconds); Kubernetes reached 300.159 seconds and dotnet/runtime reached 330.166 seconds. The same run completed smaller repositories, so these timeouts preserve the large-repository performance regression surface rather than authorizing a larger timeout or skipped indexing stage.

Three focused owner guard groups cover parse-stage amplification. A row cap smaller than one file's facts must still emit that file atomically, retain the rest of its already parsed fetch group in FIFO order, and finish five files with exactly one Git batch read. Repeated calls for the same static language must return the same compiled tag-query allocation, different languages must not share it, and invalid query compilation must leave no cache entry. The worker-thread-local parser cache must also prove same-language instance reuse, different-language isolation, a 64-entry per-thread hard cap, and a successful independent parse after a zero-budget callback cancellation and reset. End-to-end acceptance still belongs to the existing release-binary `--categories performance` targets and their unchanged budgets; the query cache primarily removes non-C tag-query compilation, parser-instance reuse is expected to reduce repeated `Parser::new`/`set_language` fixed cost when one worker parses several files, and parsed-overflow reuse is the direct guard for fact-dense C/C++ repositories. Owner guards do not establish a wall-time result.

A retained Kubernetes candidate provides phase evidence, not an accepted latency result. At the 360-second cutoff its isolated clone had committed all 30,353 files in 61 batches, with 1,434,001 symbols, 2,879,261 references, and 215,501 chunks, and the durable checkpoint remained `finalizing:refresh_dependencies`. That checkpoint was updated at roughly 296 seconds, leaving about 64 seconds before timeout; because a checkpoint names the last completed phase, the next reference-search rebuild was the likely in-progress phase but had not durably advanced. This identifies reference-search finalization for bounded measurement; without an internal phase timer it does not establish the rebuild's exact duration, prove the new pagination speedup, or claim that it exceeds a task lease.

The first bounded persistence slice targets only the base-reference statement count: within each existing code-index batch under the bundled SQLite ceiling, one 16-bind execute per reference becomes `ceil(reference_count / 1,024)` multi-values executes with no more than 16,384 binds each; a lower connection variable limit reduces the effective row group. Bind vectors borrow record fields instead of cloning their strings, the fixed full-group SQL uses `prepare_cached`, and only the tail shape receives a one-off prepare. The direct owner regression crosses the default boundary with 1,025 ordered references, requires their two-group base facts and intermediate search documents to preserve input order, proves published-batch replay is idempotent, and injects a second-group uniqueness failure to require whole-fact-transaction rollback with a replayable staged manifest. Lower-limit cases require dynamic one-row groups at 31 variables, accept the inclusive 16-variable boundary for one row, and fail closed below it. The slice does not change FTS finalization or any other fact owner. These statement-count, allocation, and recovery guards do not claim a wall-time improvement; the unchanged isolated release-binary performance targets remain the acceptance authority.

The second bounded persistence slice applies the same statement-count reduction only to base symbols. A fixed full statement contains 1,024 input-ordered 17-column rows and 17,408 borrowed binds; a lower runtime variable limit narrows that group, and at most one smaller tail shape is prepared per call. Optional role JSON is the only fact-bind string materialized for each bounded group, while symbol-search content and input order remain under the existing inserter. Direct owner regressions cross the boundary with 1,025 symbols and cover the reduced and one-row variable limits, route-role JSON, documented and null fields, second-group failure, and caller rollback of facts, FTS rows, and metadata. They protect bounded statement amplification and transaction ownership, not wall time; the unchanged isolated release-binary performance target remains authoritative.

The third bounded persistence slice removes repeated SQL construction and preparation from the shared search-document writer and now clamps each six-column group to the runtime SQLite variable limit and at most 1,024 documents/6,144 binds. The default full-shape FTS insert has one process-wide SQL allocation and uses `prepare_cached`; a lower runtime full shape is connection-cached, while the sole smaller tail is prepared without populating a row-count-keyed cache. Direct owner regressions execute 1,025 documents as exactly two main FTS inserts, cover the exact 12/6/5-variable two-row/one-row/reject boundaries, start above the highest orphan FTS rowid, preserve the three affected-row/contiguous-interval checks, and require a tail ownership conflict to roll back the already flushed full group. This is evidence for bounded prepare/statement amplification and atomic recovery only. It does not claim a measured Kubernetes or other real-repository wall-time improvement; the unchanged isolated release-binary performance target remains authoritative.

The current release-candidate focused report `manual-evaluate-1787657485515273930-0-3038475.json` measured the 1,024-file fixture at 382 ms cold, 453 ms register plus cold, and 423 ms incremental, all inside the unchanged product budgets. Release build was 321/180,000 ms and the named persistence suite was 739/30,000 ms. All 346 gates, 119 cases, and 293 commands passed; the report recorded score 1.0, `score_accepted=true`, and `adoption_status=would_accept`. Manual evaluation created no commit. This closes the focused-fast rejection from the preceding working tree, but it does not close the failed Kubernetes rail or replace exhaustive evidence.

The fourth bounded persistence slice converts only the 12-column base-chunk owner from one execute per row to runtime-limit-clamped multi-values groups of at most 1,024 rows and 12,288 binds. A direct SQLite trace requires 1,025 facts to execute exactly two base-chunk inserts while FTS rowids retain input order. Boundary regressions set the runtime limit to 24 variables, accept the exact 12-variable one-row limit, and reject 11 variables without a write. A uniqueness failure in the tail must roll back the preceding 1,024-row group, FTS rows, metadata, and checkpoint advancement while retaining the staged batch for exactly-once replay. These are statement-count and recovery invariants, not a real-repository latency claim.

The fifth bounded persistence slice changes only the already admitted grouped reference-search build page. It replaces Rust-side six-field document materialization and repeated FTS `VALUES` flushes with one ordered `INSERT ... SELECT`, then verifies the exact `last_insert_rowid` interval and creates all metadata owners with one scoped statement. The existing page row/byte limits, lazy admission, checkpoint/progress CAS, transaction, and publication fence remain unchanged; a pre-existing `INT64_MAX` row fails before owner writes. The named gate traces a 1,025-group page as one main FTS insert plus one metadata insert and separately checks canonical blank-field content and rollback-safe rowid rejection. This mechanism has not passed the Kubernetes 210-second rail and must not be reported as an end-to-end speedup until an isolated release run does so.

Grouped reference-search finalization has direct plan and work evidence. All cleanup, discovery, and build ranges use distinct static first-page and continuation SQL without a nullable-parameter `OR`, and `EXPLAIN QUERY PLAN` must show indexed keyset ranges. Every page reserves two control mutations and charges conservative full owner/progress/checkpoint records. One lazy scan returns only an integer lookup key, cursor length, and row-byte bound; an 8 KiB cursor under a 4 KiB budget is rejected before cursor fetch, and an admitted page point-fetches only its final durable cursor. Build admission adds field lengths without concatenating the search content. The discovery page's returning UPSERT replaces the prior extra grouped count scan without changing its caps. On the deterministic 2,048-reference fixture with 128 already owned first-page groups, SQLite progress-handler measurement records 126,790 VM steps for the legacy nullable-range/count/upsert path and 56,472 for the production static-range/streaming/returning-upsert path, a reduction of 70,318 steps. This fixture proves the removed SQLite work directly; it does not substitute for Kubernetes wall time.

Ordinary-reference resolution has a separate production-mechanism gate for `finalizing:resolve_references:v1`. It requires multi-row static first/continuation keysets, two indexed `LIMIT 2` owner-length probes cached by page-local name and `(name,path)`, a single admitted non-call range UPDATE, and conservative complete owner/progress/checkpoint bytes with two control mutations. Tiny-byte fixtures prove that oversized reference or symbol payloads are rejected before payload materialization. A 1,025-row call-only page must advance its exact durable cursor while executing zero reference owner updates, zero path/name point-fetches, and exactly one final-cursor point-fetch; the later call-target stage separately proves that stale pre-resolved non-callable targets are still downgraded. A tenfold hot-symbol-tail fixture records the same scan/probe VM work at both sizes: first-page/continuation planning is 130/136 steps and the corresponding range UPDATE is 351/358 steps. Cursor digest drift, count drift, enlarged persisted limits, false zero count, non-tail EOF, rollback, reopen, and stale-fence tests must all fail closed or replay exactly. The driver bound remains the conservative `CODE_INDEX_FINALIZATION_MAX_STEPS + 4R + 6`; this gate does not widen row, byte, timeout, lease, FTS, or freshness budgets.

The `fast` profile runs these mechanisms in the isolated `code_index_persistence_performance_suite` stage after the shared library test target has been built. Its hard timeout remains 120 seconds and its key metric `code_index_persistence_performance_suite_ms` has a 30,000-ms budget. Benchmark CI requires both the named gate and the budgeted key metric, so removing chunk grouping, reintroducing nullable-range SQL, losing indexed plans, restoring the repeated discovery scan or per-row grouped cursor fetch, fetching over-budget grouped cursors/content, or making ordinary resolution materialize or update call-only pages fails the fast performance rail without enlarging any product resource budget.

Phase attribution must not compete with a live writer. A diagnostic run may use `--keep-workdirs`, but the database is inspected only after the product command exits and an operating-system handle check confirms that the main, WAL, and SHM files are no longer open. Copy that trio to an isolated temporary directory, then query only the copy with SQLite read-only URI mode and `query_only=ON`. The checkpoint `state`, committed/total file counts, batch count, last path, and finalization phase distinguish ingest amplification from a projection/finalization tail without checkpointing or otherwise mutating the source database.

The 2026-08-25 isolated release-binary diagnostic used the exact Kubernetes target configuration: commit `016a2bcfa48d4a56059ee5e878eb208ffccdb773`, all-files scope, no path or language filter, and a fresh runtime home. Attempt 1 was still active after the unchanged 210,000-ms budget and later failed closed at `finalizing:rebuild_reference_search:v2:discover:22` when the host wall clock advanced by about three hours; the resulting 10,861,839-ms wall-clock delta is invalid latency evidence. Generation 2 resumed without replaying facts and reached `completed`/`fresh` with 30,353 files, 1,434,001 symbols, 2,879,261 references, 268,075 chunks, and 4,771,612 committed fact rows. That recovery segment took about 244 seconds and therefore also exceeds 210 seconds by itself. This proves durable fence/reopen behavior but leaves the Kubernetes performance rail failed; it does not substitute for the pending exhaustive query-case report.

After the bounded shared-FTS and grouped-build changes, a second fresh-home run of the same release target completed in one attempt with `/usr/bin/time` reporting 592.72 seconds and exit code 0. Its task was `succeeded`, checkpoint was `completed`, status was `fresh`, and the counts exactly matched the 30,353 files, 1,434,001 symbols, 2,879,261 references, 268,075 chunks, and 4,771,612 base facts above. The command was still running at the 210-second observation point, so the key metric remains a clear failure at 2.82 times budget; normal completion and correct publication cannot turn that failure into acceptance. Seven formerly failing Kubernetes focused queries passed their current rank/evidence contracts on this index, but the complete Kubernetes/exhaustive report remains pending.

After raising the bounded reference, symbol, and chunk base-fact groups to 1,024 rows, a third fresh-home release run of the identical target again completed in one attempt with the same facts and terminal state. It took 607.03 seconds and was still running at the 210-second observation point, failing the unchanged rail by 2.89 times. This is 14.31 seconds, or about 2.4%, slower than the preceding single run. One sample per candidate cannot distinguish scheduler and storage variance from a small regression, but it does establish that the larger bounded base-fact groups did not close the end-to-end bottleneck. The 1,025-row trace tests remain mechanism evidence only.

A separate fresh-home run polled read-only task status about every 10 seconds to obtain coarse phase evidence. Because that polling competed with the live workload, its 612.08-second total is diagnostic and is not a performance sample. Relative to task creation, the first observed `build_query_indexes` state appeared at about 158 seconds; the query-index plus ordinary-reference interval reached `resolve_imports` at about 258 seconds; imports reached `resolve_call_targets` at about 286 seconds; grouped discovery and build occupied roughly the next 107 and 109 seconds; call rebuilding took about 30 seconds; and software projection occupied roughly the final 67 seconds before completion. The polling cadence and coarse checkpoint semantics prevent exact phase timing, but the observation identifies ordinary and grouped reference-wide finalization as the dominant bounded-measurement surface. It does not alter the failed 210-second rail or authorize skipped phases, larger budgets, or a latency claim for the cursor-fetch changes.

After the final-cursor and ordinary-call ownership changes, the first fresh-home attempt was invalidated by another host wall-clock discontinuity: all 30,353 files and 4,771,612 base facts had reached the `indexing` checkpoint about 153.5 seconds after task creation, then the wall clock advanced by roughly 2,830 seconds and the publication fence correctly rejected the expired generation. Generation 2 resumed without replaying those 61 fact batches and completed the remaining finalization in 386.35 monotonic seconds. Neither number is a valid single-attempt latency result, but together they preserve exact fail-closed and checkpoint-recovery evidence.

A second fresh home then produced the valid acceptance sample without status polling. One release-binary attempt completed in 564.99 monotonic seconds with task `succeeded`, checkpoint `completed`, status `fresh`, and the same 30,353 files, 1,434,001 symbols, 2,879,261 references, 268,075 chunks, and 4,771,612 committed base facts. It is 42.04 seconds (about 6.9%) faster than the immediately preceding 607.03-second sample and 27.73 seconds (about 4.7%) faster than the earlier 592.72-second sample. One sample per candidate cannot attribute that difference to the cursor/call changes, and the command was still active at 210 seconds; 564.99 seconds is 2.69 times the unchanged key budget, so the Kubernetes rail remains failed.

## Current Evaluation Example

Performance timings are valid only for the product-binary profile recorded by
the report. Every non-smoke self-iteration profile, including focused `fast`
performance runs, builds and executes `target/release/relay-knowledge`; the
harness itself may remain a debug binary. Workload previous/best baselines are
filtered by `product_binary_profile`, with legacy missing fields interpreted as
`fast=debug` and non-fast=`release`. The benchmark CI rejects a performance
report unless it records the release profile and release product path, so old
debug-fast measurements cannot silently become release performance evidence.

The Linux kernel example declares an exact selected scope of 93,601 files, a historical 34,150-file/180-second baseline, approximately 80 files per second, and a 1,800-second cap. The separate raw pinned-tree observation may be larger and remains diagnostic evidence.

The target configuration is in
`tools/self_iteration/cases/repository_index_performance_targets.json`.
`linux_full` declares `index_only_performance_target=true`, so exhaustive runs
measure its cold index even though it intentionally contributes no retrieval
case observation. Run the performance evaluation with:

```bash
./self-iterate.sh evaluate --use-current-candidate --profile exhaustive --categories performance
```

An index-only repository intentionally has zero retrieval cases, so case count
alone cannot prove that it ran. After its strict cold-completion validation
passes, its repository report includes `cold_index_result`: the unchanged raw
cold `repo index` payload with `scope`, `task`, `summary`, `checkpoint`, and
`status`. Ordinary repository reports retain the existing compact
`index_summary` meaning and omit this optional field. A failed strict
completion validation also omits it, so its presence cannot turn partial work
into terminal evidence.

The final acceptance check is repository-parameterized and asserts selection,
zero-case execution, both key budgets, the strict completion command, durable
terminal state, exact counts, freshness, and identity without naming a query:

```bash
report_path="$(ls -t .git/relay-knowledge-self-iteration/reports-v2/manual-evaluate-*.json | head -n 1)"
repository=linux_full
jq --arg repository "$repository" -e '
  ([.evaluation.gates[] | select(.passed | not)] | length) == 0 and
  ([.evaluation.cases[] | select(.passed | not)] | length) == 0 and
  ([.evaluation.repositories[] | select(.repository == $repository)] as $reports |
    ($reports | length) == 1 and
    ($reports[0] |
      (.cases | length) == 0 and
      ([.commands[] |
        select(.name == ($repository + "_cold_index_completion") and
               .exit_code == 0)] | length) == 1 and
      ([.metrics[] |
        select((.name == ($repository + "_cold_index_ms") or
                .name == ($repository + "_cold_register_index_ms")) and
               .key == true and .budget != null and .value <= .budget)] |
        length) == 2 and
      (.cold_index_result as $cold |
        $cold.task.state == "succeeded" and
        $cold.task.mode == "full" and
        $cold.checkpoint.state == "completed" and
        $cold.checkpoint.total_path_count > 0 and
        $cold.checkpoint.committed_file_count == $cold.checkpoint.total_path_count and
        $cold.status.state == "fresh" and
        $cold.status.stale == false and
        $cold.scope.indexed_file_count == $cold.checkpoint.total_path_count and
        $cold.summary.indexed_file_count == $cold.checkpoint.total_path_count and
        $cold.status.indexed_file_count == $cold.checkpoint.total_path_count and
        $cold.scope.repository_id != null and
        $cold.scope.repository_id == $cold.task.repository_id and
        $cold.scope.repository_id == $cold.summary.repository_id and
        $cold.scope.repository_id == $cold.checkpoint.repository_id and
        $cold.scope.repository_id == $cold.status.repository_id and
        $cold.scope.alias == $cold.task.alias and
        $cold.scope.alias == $cold.status.alias and
        $cold.scope.requested_ref == $cold.task.ref_selector and
        $cold.scope.scope_id != null and
        $cold.scope.scope_id == $cold.task.source_scope and
        $cold.scope.scope_id == $cold.summary.source_scope and
        $cold.scope.scope_id == $cold.checkpoint.source_scope and
        $cold.scope.scope_id == $cold.status.last_indexed_scope_id and
        $cold.scope.resolved_commit_sha != null and
        $cold.scope.resolved_commit_sha == $cold.task.resolved_commit_sha and
        $cold.scope.resolved_commit_sha == $cold.summary.resolved_commit_sha and
        $cold.scope.resolved_commit_sha == $cold.status.last_indexed_commit and
        $cold.scope.tree_hash != null and
        $cold.scope.tree_hash == $cold.task.tree_hash and
        $cold.scope.tree_hash == $cold.summary.tree_hash and
        $cold.scope.tree_hash == $cold.status.tree_hash and
        $cold.scope.path_filters == $cold.task.path_filters and
        $cold.scope.path_filters == $cold.status.path_filters and
        $cold.scope.language_filters == $cold.task.language_filters and
        $cold.scope.language_filters == $cold.status.language_filters)))
' "$report_path"
```

---

Navigation: [Benchmark and Evaluation Records](README.md) | Previous: [11. Coding-Agent E2E Evaluation Gate](11-coding-agent-e2e-evaluation.md)
