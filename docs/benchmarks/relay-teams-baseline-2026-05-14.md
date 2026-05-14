# relay-teams CLI and Web Benchmark Baseline

Date: 2026-05-14

Test repository: `/opt/workspace/relay-teams`

- Branch: `improve-memory-skill-draft-status-ui`
- HEAD: `fa3c0ddc9d81400b8d5e58ab7600dd557a056816`
- Base ref for incremental tests: `0a4e709c86f25d4fd475113f20d78f9a99498c37`
- Runtime home: `/tmp/relay-knowledge-relay-teams-benchmark-20260514`
- Update runtime home: `/tmp/relay-knowledge-relay-teams-update-benchmark-20260514`
- Binary: `target/release/relay-knowledge`
- Web bind: `127.0.0.1:8791`

Related follow-up records:

- [Optimization study](relay-teams-optimization-study-2026-05-14.md)
- [Optimization issue checklist](relay-teams-optimization-issues-2026-05-14.md)

The `relay-teams` worktree was not clean during this run. Git-backed indexing
resolved refs to committed tree objects, so uncommitted worktree content is not
included in the code index baseline.

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

## Repository Scope

`repo scope preview relay-teams --ref HEAD` selected:

- Files: 1,658
- Bytes: 32,888,900
- Unsupported files: 223
- Generated or heavy files: 3
- Expected degraded files: 223
- Languages: Python 1,430 files / 19,910,475 bytes; unknown 223 files /
  12,970,961 bytes; JavaScript 3 files / 4,737 bytes; Bash 2 files / 2,727 bytes

Largest selected files:

| Path | Bytes |
| --- | ---: |
| `.agent_teams/evals/datasets/swebench-verified-full.jsonl` | 8,110,423 |
| `.agent_teams/evals/datasets/swebench-verified-100.jsonl` | 1,626,235 |
| `uv.lock` | 795,064 |
| `.agent_teams/evals/datasets/swebench-verified-10.jsonl` | 289,697 |
| `tests/unit_tests/frontend/test_project_view_ui.py` | 288,044 |

## Index Baseline

Cold full index:

- Command: `repo index relay-teams --ref HEAD --format json`
- Wall time: 82.57s
- User/sys time: 75.20s / 7.46s
- Peak RSS: 357,508 KiB
- SQLite data file: 255,684,608 bytes
- Indexed files: 1,658
- Symbols: 28,125
- References: 187,993
- Chunks: 28,441
- SQLite writes: 445,966
- Degraded files: 223

No-op HEAD reindex:

- Command: `repo index relay-teams --ref HEAD --format json`
- Wall time: 86.56s
- Result still reported `changed_path_count=1658` and `skipped_unchanged_count=0`

Incremental update, measured in a separate runtime by first indexing the base
commit and then updating to HEAD:

- Base full index: 82.93s
- `repo update relay-teams --base 0a4e709... --head fa3c0dd...`: 4.87s
- Changed paths: 4
- Blob reads: 1
- Parsed files: 1
- SQLite writes: 104

## CLI Baseline

All timings are single-process wall-clock samples in milliseconds after the cold
index was available unless stated otherwise.

| Command case | Exit | ms |
| --- | ---: | ---: |
| `version` | 0 | 9 |
| `status` | 0 | 47 |
| `health` | 0 | 54 |
| `graph inspect` | 0 | 59 |
| `ingest` | 0 | 68 |
| `query relay-teams --freshness wait-until-fresh` | 0 | 69 |
| `query relay-teams --freshness graph-only` | 0 | 51 |
| `index refresh` | 0 | 50 |
| `index refresh --kind bm25` | 0 | 45 |
| `repo status` | 0 | 44 |
| `repo report --format json` | 0 | 4,222 |
| `repo report --format markdown` | 0 | 3,558 |
| `repo scope preview --ref HEAD` | 0 | 81 |
| `repo query --kind hybrid` | 0 | 1,455 |
| `repo query --kind symbol` | 0 | 165 |
| `repo query --kind definition` | 0 | 160 |
| `repo query --kind references` | 0 | 571 |
| `repo query --kind callers` | 0 | 593 |
| `repo query --kind callees` | 0 | 600 |
| `repo query --kind imports` | 0 | 61 |
| `repo impact base..HEAD` | 0 | 2,471 |
| `provider probe` | 0 | 7 |
| `worker status` | 0 | 40 |
| `worker run-once --kind extractor` | 0 | 49 |
| `proposal list` | 0 | 47 |
| `proposal show` | 0 | 45 |
| `proposal reject` | 0 | 47 |
| `audit query` | 0 | 50 |
| `service status` | 0 | 51 |
| `service doctor` | 0 | 51 |
| `service plan install` | 0 | 42 |
| `service plan uninstall` | 0 | 35 |
| `service definition write` | 0 | 38 |
| `service operator status` | 0 | 32 |
| `service operator pause` | 0 | 28 |
| `service operator resume` | 0 | 35 |

`repo report` embeds representative hybrid query samples of 1,321ms, 1,272ms,
and 1,209ms for `_make_graph_node`, `_make_role_definition`, and
`_make_task_envelope`.

## Web HTTP Baseline

The Web service was started with:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-benchmark-20260514 \
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
target/release/relay-knowledge service run --web
```

Measured with `curl` against same-origin HTTP endpoints:

| Web case | HTTP | ms |
| --- | ---: | ---: |
| `GET /` | 200 | 18 |
| `GET /api/health` | 200 | 33 |
| `GET /api/project/status` | 200 | 21 |
| `GET /api/service/status` | 200 | 21 |
| `retrieve.context` | 200 | 30 |
| `graph.ingest` | 200 | 43 |
| `graph.inspect` | 200 | 24 |
| `index.refresh` | 200 | 20 |
| `provider.embedding.probe` | 200 | 28 |
| `worker.status` | 200 | 21 |
| `worker.run-once` | 200 | 25 |
| `proposal.list` | 200 | 17 |
| `proposal.show` | 200 | 19 |
| `proposal.reject` | 200 | 21 |
| `audit.query` | 200 | 14 |
| `code.repo.status` | 200 | 15 |
| `code.repo.query` hybrid | 200 | 1,498 |
| `code.repo.query` symbol | 200 | 142 |
| `code.repo.impact` | 200 | 2,512 |
| `code.repo.index` | 408 | 30,015 |
| `code.repo.update` | 400 | 21 |
| `service.doctor` | 200 | 19 |
| `service.run.streamable_http` | 200 | 18 |

Browser integration:

```bash
uv run --extra dev pytest tests/browser
```

Result: `1 passed in 2.24s`.

Headless Chromium live page-load baseline against `http://127.0.0.1:8791/`,
5 samples with `wait_until="networkidle"`:

- Mean: 573.19ms
- Median: 575.00ms
- Min: 556.68ms
- Max: 598.24ms
- Browser navigation `loadEventEnd`: 15.5ms to 29.2ms

## Issues Found

1. No-op repository indexing does not skip unchanged files.

   Re-running `repo index relay-teams --ref HEAD` after a fresh HEAD index took
   86.56s and reported all 1,658 files as changed. This defeats the expected
   refresh/no-op behavior and also makes Web code indexing exceed the default
   30s HTTP request timeout.

2. Web `code.repo.index` times out for this repository.

   `POST /api/web/operations/execute` with `operation=code.repo.index` returned
   HTTP 408 after 30.015s. Full indexing continues to be a long-running
   operation and should be exposed as queued/background work, or the Web composer
   should use a progress/operation handle instead of a single request.

3. Top-level CLI `query` rejects multi-word positional queries.

   `relay-knowledge query relay-teams benchmark --source relay-teams-benchmark`
   failed with exit 2 and `unexpected argument 'benchmark'`. The repo query CLI
   already collects multi-word query values, but the top-level GraphRAG query
   only accepts one positional token unless callers use the `--` escape form.

4. `repo update` requires the currently indexed scope to be the base ref.

   After the main runtime was indexed at HEAD, `repo update --base 0a4e709...
   --head fa3c0dd...` failed quickly because the repository was already indexed
   at HEAD. This behavior is consistent with the current implementation, but it
   is a usability trap for benchmarking and Web operation composition. The
   operation should either document this precondition clearly in the UI/CLI or
   support computing an incremental update from persisted base snapshots when
   available.

5. Scope preview includes large JSONL and lock files as `unknown`.

   The selected scope includes multi-megabyte `.jsonl` fixtures and `uv.lock` as
   unknown/text-like files. They contribute to selected bytes and degraded file
   counts. This may be acceptable for full-repository baselines, but default
   source presets should be reviewed if these files are not useful retrieval
   targets.
