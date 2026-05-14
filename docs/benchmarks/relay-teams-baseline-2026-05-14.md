# relay-teams CLI and Web Benchmark Baseline

Date: 2026-05-14

Test repository: `/opt/workspace/relay-teams`

- Branch: `improve-memory-skill-draft-status-ui`
- HEAD: `fa3c0ddc9d81400b8d5e58ab7600dd557a056816`
- Base ref for incremental tests: `0a4e709c86f25d4fd475113f20d78f9a99498c37`
- Runtime home: `/tmp/relay-knowledge-relay-teams-benchmark-20260514-144539/home`
- Update runtime home: `/tmp/relay-knowledge-relay-teams-benchmark-20260514-144539/update-home`
- Raw benchmark logs: `/tmp/relay-knowledge-relay-teams-benchmark-20260514-144539`
- Binary: `target/release/relay-knowledge`
- Web bind: `127.0.0.1:8791`

Related records:

- [Optimization study](relay-teams-optimization-study-2026-05-14.md)
- [Optimization issue checklist](relay-teams-optimization-issues-2026-05-14.md)

The `relay-teams` worktree was clean during this run. Git-backed indexing
resolved refs to committed tree objects.

## Host and Toolchain

- OS: Linux 6.17.0-23-generic x86_64
- CPU: 12th Gen Intel Core i7-1260P, 16 logical CPUs
- Rust: `rustc 1.95.0`, `cargo 1.95.0`
- Node: `v24.14.0`
- uv: `0.10.10`

Build gates used before benchmarking:

```bash
cargo build --release
npm --prefix web install
npm --prefix web run build
```

Browser gate used during Web verification:

```bash
uv run --extra dev python -m playwright install chromium
uv run --extra dev pytest tests/browser
```

Result: `1 passed in 1.80s`.

## Repository Scope

`repo scope preview relay-teams --ref HEAD` selected:

- Files: 1,653
- Bytes: 22,063,153
- Unsupported files: 218
- Generated or heavy files: 0
- Expected degraded files: 218
- Languages: Python 1,430 files / 19,910,475 bytes; unknown 218 files /
  2,145,214 bytes; JavaScript 3 files / 4,737 bytes; Bash 2 files / 2,727 bytes

Largest selected files:

| Path | Bytes |
| --- | ---: |
| `tests/unit_tests/frontend/test_project_view_ui.py` | 288,044 |
| `tests/unit_tests/frontend/test_model_profiles_ui.py` | 224,391 |
| `tests/unit_tests/boards/test_todo_service.py` | 204,354 |
| `docs/core/api-design.md` | 200,851 |
| `tests/unit_tests/sessions/runs/test_run_service_recovery.py` | 199,826 |

## Index Baseline

Cold full index:

- Command: `repo index relay-teams --ref HEAD --format json`
- Wall time: 46.31s
- Peak RSS: 360,460 KiB
- SQLite data file: 438,763,520 bytes
- Indexed files: 1,653
- Symbols: 28,125
- References: 187,993
- Chunks: 28,436
- SQLite writes: 445,951
- Degraded files: 218

No-op HEAD reindex:

- Command: `repo index relay-teams --ref HEAD --format json`
- Wall time: 0.39s
- Peak RSS: 14,420 KiB
- `changed_path_count=0`
- `skipped_unchanged_count=1653`
- Blob reads: 0
- Parsed files: 0
- SQLite writes: 0

Incremental update, measured in a separate runtime by first indexing the base
commit and then updating to HEAD:

- Base full index: 64.21s
- `repo update relay-teams --base 0a4e709... --head fa3c0dd...`: 7.36s
- Changed paths: 4
- Blob reads: 1
- Parsed files: 1
- SQLite writes: 104

## CLI Baseline

All timings are single-process wall-clock samples in milliseconds after the cold
index was available unless stated otherwise.

| Command case | Exit | ms |
| --- | ---: | ---: |
| `version` | 0 | 0 |
| `help --format json` | 0 | 0 |
| `status` | 0 | 80 |
| `health` | 0 | 80 |
| `graph inspect` | 0 | 90 |
| `ingest` | 0 | 90 |
| `query relay-teams --freshness wait-until-fresh` | 0 | 80 |
| `query relay-teams --freshness graph-only` | 0 | 80 |
| `query relay-teams benchmark` | 0 | 80 |
| `index refresh --kind bm25 --kind semantic --kind vector` | 0 | 80 |
| `index refresh --kind bm25` | 0 | 80 |
| `repo status` | 0 | 80 |
| `repo report --format json` | 0 | 260 |
| `repo report --format markdown` | 0 | 260 |
| `repo scope preview --ref HEAD` | 0 | 100 |
| `repo query --kind hybrid` | 0 | 100 |
| `repo query --kind symbol` | 0 | 90 |
| `repo query --kind definition` | 0 | 90 |
| `repo query --kind references` | 0 | 80 |
| `repo query --kind callers` | 0 | 90 |
| `repo query --kind callees` | 0 | 90 |
| `repo query --kind imports` | 0 | 90 |
| `repo impact base..HEAD` | 0 | 340 |
| `repo update main..HEAD after indexing HEAD` | 1 | 80 |
| `provider probe` | 0 | 0 |
| `worker status` | 0 | 80 |
| `worker run-once --kind extractor` | 0 | 80 |
| `worker run-once --kind ocr` | 0 | 80 |
| `worker run-once --kind vision` | 0 | 80 |
| `proposal list` | 0 | 90 |
| `proposal show` | 0 | 80 |
| `proposal reject` | 0 | 80 |
| `proposal accept` | 0 | 90 |
| `proposal supersede` | 0 | 80 |
| `audit query` | 0 | 80 |
| `service status` | 0 | 80 |
| `service doctor` | 0 | 80 |
| `service plan install` | 0 | 80 |
| `service plan uninstall` | 0 | 80 |
| `service definition write` | 0 | 80 |
| `service operator status` | 0 | 80 |
| `service operator pause` | 0 | 90 |
| `service operator resume` | 0 | 90 |

The `repo update main..HEAD after indexing HEAD` failure is the documented
precondition that the currently indexed scope must match the incremental base
ref. The separate update runtime above measures the valid base-to-head path.

## Web HTTP Baseline

The Web service was started with:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-benchmark-20260514-144539/home \
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs,src,frontend,relay-teams-benchmark \
target/release/relay-knowledge service run --web --mcp streamable-http
```

Measured with `curl` against same-origin HTTP endpoints:

| Web case | HTTP | ms |
| --- | ---: | ---: |
| `GET /` | 200 | 0 |
| `GET /api/health` | 200 | 5 |
| `GET /api/project/status` | 200 | 1 |
| `GET /api/service/status` | 200 | 1 |
| `GET /mcp/metrics` | 200 | 7 |
| `retrieve.context` | 200 | 2 |
| `graph.ingest` | 200 | 9 |
| `graph.inspect` | 200 | 5 |
| `index.refresh` | 200 | 1 |
| `provider.embedding.probe` | 200 | 0 |
| `worker.status` | 200 | 2 |
| `worker.run-once` | 200 | 5 |
| `proposal.list` | 200 | 1 |
| `proposal.show` | 200 | 1 |
| `proposal.reject` | 200 | 2 |
| `proposal.accept` | 200 | 10 |
| `proposal.supersede` | 200 | 2 |
| `audit.query` | 200 | 1 |
| `code.repo.register` | 200 | 5 |
| `code.repo.status` | 200 | 2 |
| `code.repo.query` hybrid | 200 | 24 |
| `code.repo.query` symbol | 200 | 4 |
| `code.repo.query` definition | 200 | 4 |
| `code.repo.query` references | 200 | 5 |
| `code.repo.query` callers | 200 | 4 |
| `code.repo.query` callees | 200 | 8 |
| `code.repo.query` imports | 200 | 6 |
| `code.repo.impact` | 200 | 268 |
| `code.repo.index` no-op | 200 | 170 |
| `code.repo.update` HEAD..HEAD | 200 | 36 |
| `code.repo.update` main..HEAD after indexing HEAD | 400 | 3 |
| `service.doctor` | 200 | 1 |
| `service.run.streamable_http` | 200 | 1 |

Browser integration:

```bash
uv run --extra dev pytest tests/browser
```

Result: `1 passed in 1.80s`.

Headless Chromium live page-load baseline against `http://127.0.0.1:8791/`,
5 samples with `wait_until="networkidle"`:

- Mean: 530.01ms
- Median: 530.71ms
- Min: 522.75ms
- Max: 536.51ms
- Browser navigation `loadEventEnd`: 12.1ms to 17.8ms
- Live dashboard displayed repository code totals and did not show the previous
  empty-code-graph state.

## Issues Found During This Baseline

1. Re-registering the same repository root under a different alias invalidates
   the previous alias.

   Status after follow-up fix: resolved. Duplicate-root registration now adds a
   persistent alias for the same repository id and preserves previous aliases.

   During Web testing, `code.repo.register` for `/opt/workspace/relay-teams`
   with alias `relay-teams-web` updated the existing repository row, after
   which `code.repo.status` for alias `relay-teams` returned
   `code repository 'relay-teams' is not registered`. The repository id and
   indexed totals were preserved under the new alias. This can surprise users
   who expect aliases to be stable or additive.

2. `repo update --base main --head HEAD` remains brittle after indexing HEAD.

   Status after follow-up fix: resolved when the base snapshot was indexed
   earlier. Incremental update now clones the persisted matching base scope even
   when the active repository status already points at HEAD.

   CLI returned exit 1 and Web returned HTTP 400 when the currently indexed
   scope was already HEAD. The valid sequence is to index the base ref in a
   separate/current scope first, then update to HEAD. This is consistent with
   current validation, but the Web composer and docs should make the precondition
   explicit.

3. Health still separates graph-code counters from repository-code totals.

   Status after follow-up fix: resolved. Service-level `health` and
   `graph inspect` now include repository code totals in graph code counters and
   still expose `repository_code_totals` as the repository-specific breakdown.

   `/api/health` reported `graph.code_file_count=0` while
   `repository_code_totals.indexed_file_count=1653`. The live Web dashboard now
   displays repository code totals correctly, so this is no longer a dashboard
   false-empty issue, but API consumers must use `repository_code_totals` for
   code-repository data.

## Resolved Since Previous Baseline

- Repeated full index now uses the no-op fast path: 0.39s, zero blob reads, zero
  parses, zero SQLite writes.
- Web no-op `code.repo.index` now returns HTTP 200 in 170ms instead of timing
  out after 30s.
- Top-level CLI GraphRAG query now accepts multi-word positional input.
- Default scope no longer includes the large JSONL dataset dumps or `uv.lock`;
  selected bytes dropped from 32,888,900 to 22,063,153.
- Live Web dashboard no longer shows the code graph as empty after repository
  indexing.
