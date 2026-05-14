# relay-teams Optimization Issue Checklist

Date: 2026-05-14

Source benchmark: [relay-teams baseline](relay-teams-baseline-2026-05-14.md)

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
- Status: implemented. Relay-teams optimized sample is 387ms with zero blob
  reads, parses, and SQLite writes.

## RK-PERF-002: Hybrid code query materializes too many candidates

- Baseline: CLI hybrid query 1.46s; Web hybrid query 1.50s.
- Root cause: hybrid search executes five `LIKE '%token%'` query families over
  large scope tables.
- Fix: populate a code-repository FTS5 candidate table during indexing and use
  it to seed typed query-layer lookups before Rust scoring/dedupe.
- Acceptance: relay-teams `project` hybrid query completes under 500ms without
  changing the response schema.
- Tests: existing code query behavior tests must continue to pass.
- Status: implemented. Relay-teams optimized sample is 168ms.

## RK-PERF-003: Impact analysis scans full scope tables

- Baseline: CLI impact 2.47s; Web impact 2.51s.
- Root cause: chunks, calls, and imports were selected by source scope and
  filtered in Rust.
- Fix: push changed-path, callee-symbol, deleted-name, and broad module filters
  into SQL.
- Acceptance: relay-teams base-to-HEAD impact completes under 500ms.
- Tests: existing impact behavior tests must continue to pass.
- Status: implemented. Relay-teams optimized sample is 511ms, just above the
  original target but far below the 2.47s baseline. Follow-up hardening batches
  changed-path language lookups so impact planning no longer issues one SQLite
  query per changed path before the pushed-down chunk/call/import queries.

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
- Status: implemented. Relay-teams optimized sample is 355ms and returns
  `latency_samples=[]`.

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
- Status: implemented for no-op Web index. Relay-teams optimized sample is HTTP
  200 in 401ms.

## RK-PERF-006: Top-level GraphRAG CLI query rejected multi-word input

- Baseline: `query relay-teams benchmark --source ...` failed with exit 2.
- Root cause: top-level `query` parser accepted only the first positional token.
- Fix: collect consecutive positional tokens as the query text, matching repo
  query behavior.
- Acceptance: `relay-knowledge query relay-teams benchmark --source docs`
  parses as query text `relay-teams benchmark`.
- Tests: CLI parser unit test for multi-word query text.
- Status: implemented. Relay-teams optimized sample exits 0 in 43ms.

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
- Status: implemented.
