# relay-teams Baseline 2026-05-14

[English](../../en/05-benchmarks/01-relay-teams-baseline-2026-05-14.md) | [中文](../../zh/05-benchmarks/01-relay-teams-baseline-2026-05-14.md)

This is the English documentation page for `05-benchmarks/01-relay-teams-baseline-2026-05-14.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

Date: 2026-05-14

Test repository: `/opt/workspace/relay-teams`

- Branch: `improve-memory-skill-draft-status-ui`
- HEAD: `fa3c0ddc9d81400b8d5e58ab7600dd557a056816`
- Base ref for incremental tests: `0a4e709c86f25d4fd475113f20d78f9a99498c37`
- Runtime home: `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/home`
- Update runtime home: `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/update-home`
- Raw benchmark logs: `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214`
- Binary: `target/release/relay-knowledge`
- Web bind: `127.0.0.1:8791`

Related records:

- [Optimization study](03-relay-teams-optimization-study-2026-05-14.md)
- [Optimization issue checklist](02-relay-teams-optimization-issues-2026-05-14.md)

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

Result: `1 passed in 2.04s`.

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
- Wall time: 47.45s
- Peak RSS: 360,504 KiB
- SQLite data file: 438,771,712 bytes
- Indexed files: 1,653
- Symbols: 28,125
- References: 187,993
- Chunks: 28,436
- SQLite writes: 445,951
- Degraded files: 218

No-op HEAD reindex:

- Command: `repo index relay-teams --ref HEAD --format json`
- Wall time: 0.38s
- Peak RSS: 14,512 KiB
- `changed_path_count=0`
- `skipped_unchanged_count=1653`
- Blob reads: 0
- Parsed files: 0
- SQLite writes: 0

Incremental update, measured in a separate runtime by first indexing the base
commit and then updating to HEAD:

- Base full index: 60.77s
- `repo update relay-teams --base 0a4e709... --head fa3c0dd...`: 7.56s
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
| `status` | 0 | 129 |
| `health` | 0 | 134 |
| `graph inspect` | 0 | 134 |
| `ingest` | 0 | 145 |
| `query relay-teams --freshness wait-until-fresh` | 0 | 129 |
| `query relay-teams --freshness graph-only` | 0 | 131 |
| `query relay-teams benchmark` | 0 | 129 |
| `index refresh --kind bm25 --kind semantic --kind vector` | 0 | 131 |
| `index refresh --kind bm25` | 0 | 143 |
| `repo status` | 0 | 130 |
| `repo report --format json` | 0 | 400 |
| `repo report --format markdown` | 0 | 384 |
| `repo scope preview --ref HEAD` | 0 | 167 |
| `repo query --kind hybrid` | 0 | 156 |
| `repo query --kind symbol` | 0 | 141 |
| `repo query --kind definition` | 0 | 143 |
| `repo query --kind references` | 0 | 130 |
| `repo query --kind callers` | 0 | 138 |
| `repo query --kind callees` | 0 | 141 |
| `repo query --kind imports` | 0 | 144 |
| `repo impact base..HEAD` | 0 | 521 |
| `repo update main..HEAD after indexing HEAD` | 1 | 134 |
| `provider probe` | 0 | 6 |
| `worker status` | 0 | 134 |
| `worker run-once --kind extractor` | 0 | 132 |
| `worker run-once --kind ocr` | 0 | 127 |
| `worker run-once --kind vision` | 0 | 134 |
| `proposal list` | 0 | 89 |
| `proposal show` | 0 | 90 |
| `proposal reject` | 0 | 87 |
| `proposal accept` | 0 | 104 |
| `proposal supersede` | 0 | 84 |
| `audit query` | 0 | 131 |
| `service status` | 0 | 132 |
| `service doctor` | 0 | 131 |
| `service plan install` | 0 | 132 |
| `service plan uninstall` | 0 | 126 |
| `service definition write` | 0 | 126 |
| `service operator status` | 0 | 134 |
| `service operator pause` | 0 | 135 |
| `service operator resume` | 0 | 132 |

The `repo update main..HEAD after indexing HEAD` failure is the documented
precondition that the currently indexed scope must match the incremental base
ref. The separate update runtime above measures the valid base-to-head path.

## Web HTTP Baseline

The Web service was started with:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-refresh-20260514-224214/home \
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs,src,frontend,relay-teams-benchmark \
target/release/relay-knowledge service run --web --mcp streamable-http
```

Measured with `curl` against same-origin HTTP endpoints:

| Web case | HTTP | ms |
| --- | ---: | ---: |
| `GET /` | 200 | 0 |
| `GET /api/health` | 200 | 6 |
| `GET /api/project/status` | 200 | 0 |
| `GET /api/service/status` | 200 | 0 |
| `GET /mcp/metrics` | 200 | 7 |
| `retrieve.context` | 200 | 1 |
| `graph.ingest` | 200 | 10 |
| `graph.inspect` | 200 | 8 |
| `index.refresh` | 200 | 2 |
| `provider.embedding.probe` | 200 | 0 |
| `worker.status` | 200 | 1 |
| `worker.run-once` | 200 | 4 |
| `proposal.list` | 200 | 1 |
| `proposal.show` | 200 | 0 |
| `proposal.reject` | 200 | 2 |
| `proposal.accept` | 200 | 12 |
| `proposal.supersede` | 200 | 2 |
| `audit.query` | 200 | 0 |
| `code.repo.register` | 200 | 10 |
| `code.repo.status` | 200 | 0 |
| `code.repo.query` hybrid | 200 | 17 |
| `code.repo.query` symbol | 200 | 5 |
| `code.repo.query` definition | 200 | 5 |
| `code.repo.query` references | 200 | 5 |
| `code.repo.query` callers | 200 | 4 |
| `code.repo.query` callees | 200 | 5 |
| `code.repo.query` imports | 200 | 9 |
| `code.repo.impact` | 200 | 269 |
| `code.repo.index` no-op | 200 | 162 |
| `code.repo.update` HEAD..HEAD | 200 | 37 |
| `code.repo.update` main..HEAD after indexing HEAD | 400 | 4 |
| `service.doctor` | 200 | 2 |
| `service.run.streamable_http` | 200 | 2 |

Browser integration:

```bash
uv run --extra dev pytest tests/browser
```

Result: `1 passed in 2.04s`.

Headless Chromium live page-load baseline against `http://127.0.0.1:8791/`,
5 samples with `wait_until="networkidle"`:

- Mean: 528.36ms
- Median: 527.74ms
- Min: 521.41ms
- Max: 536.21ms
- Browser navigation `loadEventEnd`: 9.6ms to 16.2ms
- Live dashboard displayed repository code totals and did not show the previous
  empty-code-graph state.

## Historical Baseline Issues And Re-test Status

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

- Repeated full index now uses the no-op fast path: 0.38s, zero blob reads, zero
  parses, zero SQLite writes.
- Web no-op `code.repo.index` now returns HTTP 200 in 162ms instead of timing
  out after 30s.
- Top-level CLI GraphRAG query now accepts multi-word positional input.
- Default source chunking no longer includes the large JSONL dataset dumps, and
  `uv.lock` is retained only as SBOM metadata instead of an unknown source file.
- Live Web dashboard no longer shows the code graph as empty after repository
  indexing.
