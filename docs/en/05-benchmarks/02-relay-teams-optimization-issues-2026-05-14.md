# relay-teams Optimization Issues 2026-05-14

[English](../../en/05-benchmarks/02-relay-teams-optimization-issues-2026-05-14.md) | [中文](../../zh/05-benchmarks/02-relay-teams-optimization-issues-2026-05-14.md)

This is the English documentation page for `05-benchmarks/02-relay-teams-optimization-issues-2026-05-14.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

Date: 2026-05-14

Source benchmark: [relay-teams baseline](01-relay-teams-baseline-2026-05-14.md)

## RK-PERF-001: No-op full index rebuilds the whole repository

- Baseline: 86.56s, `changed_path_count=1658`, `skipped_unchanged_count=0`.
- Root cause: full index mode did not check for a fresh matching scope before
  parsing and replacing rows.
- Fix: add a fast path that resolves commit/tree and returns persisted scope
  metadata when the requested full index is already fresh.
- Acceptance: repeated full index returns `changed_path_count=0`, zero blob
  reads, zero parses, zero SQLite writes, and completes under 300ms on
  relay-teams.
- Tests: application service test for repeated full index on the same HEAD.
- Status: implemented and re-verified for correctness, with zero blob reads,
  parses, and SQLite writes. The latest relay-teams sample is 380ms, so this
  run does not meet the original 300ms latency target and should remain under
  performance watch.

## RK-PERF-002: Hybrid code query materializes too many candidates

- Baseline: CLI hybrid query 1.46s; Web hybrid query 1.50s.
- Root cause: hybrid search executes five `LIKE '%token%'` query families over
  large scope tables.
- Fix: populate a code-repository FTS5 candidate table during indexing and use
  it to seed typed query-layer lookups before Rust scoring/dedupe.
- Acceptance: relay-teams `project` hybrid query completes under 500ms without
  changing the response schema.
- Tests: existing code query behavior tests must continue to pass.
- Status: implemented and re-verified. Latest relay-teams samples for the
  accepted `project` hybrid query are 160ms through CLI and 64ms through Web.

## RK-PERF-003: Impact analysis scans full scope tables

- Baseline: CLI impact 2.47s; Web impact 2.51s.
- Root cause: chunks, calls, and imports were selected by source scope and
  filtered in Rust.
- Fix: push changed-path, callee-symbol, deleted-name, and broad module filters
  into SQL.
- Acceptance: relay-teams base-to-HEAD impact completes under 500ms.
- Tests: existing impact behavior tests must continue to pass.
- Status: implemented and re-verified. Latest relay-teams samples are 521ms
  through CLI and 269ms through Web. The CLI single sample is slightly above
  the original 500ms target, while the Web path remains below target; keep
  watching this for run-to-run variance.

## RK-PERF-004: Repository report runs expensive latency samples by default

- Baseline: JSON report 4.22s; Markdown report 3.56s.
- Root cause: report generation ran up to three hybrid queries for latency
  samples.
- Fix: make the default report metadata-only and leave latency sampling to an
  explicit benchmark workflow.
- Acceptance: relay-teams `repo report --format json` completes under 300ms and
  returns an empty `latency_samples` list by default.
- Tests: application service test that report keeps representative query names
  but omits latency samples.
- Status: implemented and re-verified. Latest relay-teams sample is 400ms and
  returns `latency_samples=[]`. The default report still skips expensive
  latency samples, but this host sample is above the original 300ms target.

## RK-PERF-005: Web code index times out

- Baseline: Web `code.repo.index` returned HTTP 408 after 30.015s.
- Root cause: Web execute waits synchronously for repository indexing, and the
  no-op case was doing a full rebuild.
- Fix: no-op fast path should make repeated Web index requests complete before
  timeout.
- Acceptance: repeated Web index for an already fresh relay-teams scope returns
  HTTP 200 under 1s.
- Follow-up: cold full indexing should become a queued/background operation with
  progress rather than a single request.
- Status: implemented for no-op Web index and re-verified. Latest relay-teams
  sample is HTTP 200 in 162ms.

## RK-PERF-006: Top-level GraphRAG CLI query rejected multi-word input

- Baseline: `query relay-teams benchmark --source ...` failed with exit 2.
- Root cause: top-level `query` parser accepted only the first positional token.
- Fix: collect consecutive positional tokens as the query text, matching repo
  query behavior.
- Acceptance: `relay-knowledge query relay-teams benchmark --source docs`
  parses as query text `relay-teams benchmark`.
- Tests: CLI parser unit test for multi-word query text.
- Status: implemented and re-verified. Latest relay-teams sample exits 0 in
  129ms.

## RK-PERF-007: Default scope includes large unknown files

- Baseline: scope selected large JSONL fixtures and `uv.lock` as unknown files.
- Root cause: source preset includes some large non-code text-like assets.
- Fix: exclude `*.jsonl` dataset dumps and `uv.lock` from the default source
  preset while allowing explicit path-filter opt-in for users who need those
  assets as retrieval targets.
- Acceptance: scope preview reports these paths under `excluded_paths` with
  reason `excluded by source preset`; they no longer contribute to selected,
  unsupported, large/heavy, or degraded counts unless explicitly selected.
- Tests: scope selection and preview tests cover `.jsonl`, `uv.lock`, and
  explicit opt-in.
- Status: implemented and re-verified. Relay-teams selected bytes dropped from
  32,888,900 to 22,063,153, and the large JSONL dataset dumps plus `uv.lock`
  were absent from the largest selected files list.

## RK-PERF-008: Re-registering an existing repository root replaces the alias

- Baseline: during Web benchmarking, registering `/opt/workspace/relay-teams`
  as `relay-teams-web` preserved the existing repository id and indexed totals
  but made the previous `relay-teams` alias unavailable.
- Impact: users can accidentally invalidate existing CLI/Web commands that use
  the old alias while trying to add a second alias for the same root.
- Fix: add a `code_repository_aliases` table, backfill legacy aliases, resolve
  repository status through either repository id or any stored alias, and reject
  alias collisions across different repository ids.
- Acceptance: duplicate-root registration behavior is explicit in CLI/Web
  output and user docs, with tests covering old-alias behavior.
- Status: implemented. Application regression coverage verifies that registering
  the same Git root as `fixture-web` preserves the original `fixture` alias and
  resolves both aliases to the same repository id.

## RK-PERF-009: Incremental update precondition is easy to hit from Web/CLI defaults

- Baseline: `repo update relay-teams --base main --head HEAD` returned exit 1
  after the repository was indexed at HEAD; the equivalent Web operation
  returned HTTP 400. The latest re-test reproduced that documented precondition
  in 134ms through CLI and 4ms through Web when no base scope existed in that
  runtime. A separate runtime that first indexed the base commit completed
  `main -> HEAD` update in 7.56s.
- Fix: incremental snapshots now carry their resolved base commit, storage
  clones the matching persisted base scope instead of the active repository
  status, and the service reads previous file fingerprints from that base scope.
- Acceptance: Web composer, CLI help, and docs state the required sequence, or
  update can compute from persisted base snapshots when available.
- Status: implemented. Application regression coverage indexes a base commit,
  indexes a different active HEAD, then successfully updates from the persisted
  base scope.

## RK-PERF-010: Health graph-code counters could appear empty while repository totals were populated

- Baseline: `/api/health` reported `graph.code_file_count=0` while
  `repository_code_totals.indexed_file_count=1653`.
- Impact: API consumers that only read the graph counters could incorrectly
  conclude that code indexing had not run.
- Fix: service-level `health` and `graph inspect` responses now include
  repository code totals in the graph code counters while preserving
  `repository_code_totals` as the repository-specific breakdown. Repository
  totals also include parse-status counts so the combined graph counters stay
  internally consistent.
- Acceptance: after repository indexing, `health.graph.code_file_count` is at
  least `repository_code_totals.indexed_file_count`, and parse-status counts
  include repository files.
- Status: implemented with application regression coverage.
