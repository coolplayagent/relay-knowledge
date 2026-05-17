# relay-teams Optimization Study 2026-05-14

[English](../../en/05-benchmarks/03-relay-teams-optimization-study-2026-05-14.md) | [中文](../../zh/05-benchmarks/03-relay-teams-optimization-study-2026-05-14.md)

This is the English documentation page for `05-benchmarks/03-relay-teams-optimization-study-2026-05-14.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

Date: 2026-05-14

Baseline: [relay-teams CLI and Web Benchmark Baseline](01-relay-teams-baseline-2026-05-14.md)

This document records the implementation-oriented performance analysis for the
slow paths found in the relay-teams benchmark. It is intended to be used as the
"before" reference when comparing optimized runs.

## Hot Paths

### No-op repository indexing

Baseline:

- `repo index relay-teams --ref HEAD --format json`: 86.56s after an already
  fresh HEAD index.
- Result reported `changed_path_count=1658` and `skipped_unchanged_count=0`.

Observed implementation path:

- `index_code_repository` always called `build_index_snapshot` for
  `CodeIndexMode::Full`.
- `build_full_snapshot` resolved the ref, listed tracked files, read every blob
  with `git show`, parsed every selected file, and applied a full replacement.
- Existing fingerprints were only useful for incremental and worktree overlay
  modes.

Optimization:

- Resolve the requested commit and tree hash before full indexing.
- If storage already has a fresh matching repository scope for the resolved
  commit and effective filters, return an index response from persisted status
  and report metadata without parsing or writing.

Expected optimized behavior:

- Same-ref no-op full index should report `changed_path_count=0`,
  `skipped_unchanged_count=indexed_file_count`, zero blob reads, zero parses,
  and zero SQLite writes.
- Target: under 300ms for relay-teams no-op full index. The latest
  2026-05-14 re-test sample was 0.38s, which verifies the fast path with zero
  blob reads, parses, and SQLite writes but misses the original latency target.

Latest optimized result from `/tmp/relay-knowledge-relay-teams-refresh-20260514-224214`:

- No-op full index: 380ms.
- `changed_path_count=0`, `skipped_unchanged_count=1653`.
- `blob_read_count=0`, `parsed_file_count=0`, `sqlite_write_count=0`.

### Code hybrid query

Baseline:

- `repo query --kind hybrid`: 1.46s.
- Web `code.repo.query` hybrid: 1.50s.

Observed implementation path:

- Hybrid query runs symbols, references, calls, imports, and chunks
  sequentially.
- Candidate SQL uses `lower(...) LIKE '%token%'`, which can only use the
  `source_scope` prefix of existing indexes.
- relay-teams rows at baseline: 28,125 symbols, 187,993 references, 187,992
  calls, 11,534 imports, and 28,441 chunks.

Optimization:

- Add a code-repository FTS5 candidate table populated alongside repository
  symbols, references, calls, imports, and chunks.
- Use FTS candidates for each code query layer before returning to the typed
  tables for response construction.
- Keep existing behavior and result shape while avoiding broad `%token%` scans.

Expected optimized behavior:

- Hybrid query should avoid materializing every matching reference/call row for
  common tokens.
- Target: under 500ms for the relay-teams `project` hybrid query.

Latest optimized result:

- `repo query relay-teams --query project --kind hybrid --ref HEAD`: 160ms
  through CLI and 64ms through Web.

### Code impact analysis

Baseline:

- `repo impact base..HEAD`: 2.47s.
- Web `code.repo.impact`: 2.51s.

Observed implementation path:

- `chunks_for_paths` selected all chunks for the indexed scope and filtered
  changed paths in Rust.
- `callers_for_symbols` selected all calls for the indexed scope and filtered
  callee symbols/deleted names in Rust.
- `importers_for_modules` selected all imports for the indexed scope and then
  applied module matching in Rust.

Optimization:

- Push changed path filtering into the chunks SQL query.
- Push callee symbol/deleted-name filtering into the calls SQL query.
- Push broad module candidate filtering into imports SQL, retaining exact
  boundary matching in Rust.

Expected optimized behavior:

- Impact analysis should scale with changed paths and seed symbols rather than
  full scope table sizes.
- Target: under 500ms for the relay-teams base-to-HEAD impact case.

Latest optimized result:

- `repo impact` for `0a4e709...` to `fa3c0dd...`: 521ms through CLI and
  269ms through Web.

### Repository report

Baseline:

- `repo report --format json`: 4.22s.
- `repo report --format markdown`: 3.56s.
- Embedded latency samples were 1.21s to 1.32s each.

Observed implementation path:

- The storage report itself is aggregate metadata and representative query
  selection.
- Application code then executed up to 3 hybrid repository searches to populate
  latency samples.

Optimization:

- Do not run latency samples by default.
- Preserve representative query names so operators can run explicit query
  benchmarks separately.

Expected optimized behavior:

- Default report should be metadata-only and fast.
- Target: under 300ms for relay-teams report generation.

Latest optimized result:

- `repo report relay-teams --format json`: 400ms.
- `latency_samples=[]`.

### Web code indexing

Baseline:

- Web `code.repo.index`: HTTP 408 after 30.015s.

Observed implementation path:

- The Web operation endpoint executes `code.repo.index` synchronously under the
  normal HTTP request timeout.
- The timeout was amplified by the no-op full index rebuild behavior.

Optimization:

- The no-op fast path should make repeated same-scope Web index requests return
  quickly.
- Full cold indexing is still long-running and should later move to a queued
  operation/progress handle rather than a single blocking request.

Expected optimized behavior:

- Repeated Web index for an already fresh relay-teams scope should return 200
  before the HTTP timeout.
- Cold full indexing remains a background-operation follow-up.

Latest optimized result:

- Web `code.repo.index` against the already fresh relay-teams scope returned
  HTTP 200 in 162ms.

## Re-test Checklist

After optimization, rerun the same release binary and runtime pattern from the
baseline:

```bash
cargo build --release
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-benchmark-after \
  target/release/relay-knowledge repo register /opt/workspace/relay-teams --alias relay-teams --format json
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-benchmark-after \
  target/release/relay-knowledge repo index relay-teams --ref HEAD --format json
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-benchmark-after \
  target/release/relay-knowledge repo index relay-teams --ref HEAD --format json
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-benchmark-after \
  target/release/relay-knowledge repo query relay-teams --query project --kind hybrid --ref HEAD --limit 10 --format json
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-benchmark-after \
  target/release/relay-knowledge repo impact relay-teams --base 0a4e709c86f25d4fd475113f20d78f9a99498c37 --head fa3c0ddc9d81400b8d5e58ab7600dd557a056816 --format json
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-benchmark-after \
  target/release/relay-knowledge repo report relay-teams --format json
```

Record optimized wall-clock times next to the baseline values before treating
the performance issue as closed.

## Optimized Summary

| Case | Baseline | Optimized |
| --- | ---: | ---: |
| No-op `repo index` | 86.56s | 0.38s |
| `repo query --kind hybrid` | 1.46s | 0.160s CLI / 0.064s Web |
| `repo impact` | 2.47s | 0.521s CLI / 0.269s Web |
| `repo report --format json` | 4.22s | 0.400s |
| Web no-op `code.repo.index` | 30.015s / HTTP 408 | 0.162s / HTTP 200 |
| Top-level multi-word `query` | exit 2 | 0.129s / exit 0 |

The latest cold full index sample was 47.45s with 360,504 KiB peak RSS. The
index still populates the code-repository FTS candidate table as the query
latency tradeoff; continue measuring the same external repository if cold
indexing becomes the primary bottleneck.
