# Lossless Cold-Index Verification 2026-09-17

[English](17-lossless-cold-index-2026-09-17.md) | [中文](../../zh/06-verification/17-lossless-cold-index-2026-09-17.md)

## 1. Conclusion and Scope

This change preserves index facts and query behavior while reducing configuration
scanning, line-number calculation, and temporary FTS parameter copies. Across three
runs, the real-repository cold-index median fell from 31.585 to 30.017 seconds
(-4.96%); full diagnostic preview fell from 8.827 to 6.413 seconds (-27.35%).
Cold-index ranges overlap, so 4.96% is not a guaranteed improvement for other
repositories. Every optimized preview sample was faster than every baseline sample.

This is focused A/B evidence for pinned snapshots, not a complete self-iteration
fast/exhaustive acceptance or cross-platform release certification. Historical
v1.1.17 timings are excluded from this comparison.
The focused scope selects eight timing scenarios, executes all eight, and skips
zero; three runs per version and scenario give 48 timing samples. Eighteen query
cases each have one behavior-comparison pair. The unrun complete evaluator is not
counted as passed.

## 2. Implementation and Invariants

- Configuration scanning first searches the remaining string for the API pattern.
  Absent patterns bypass character decoding and quote-state work. Matching input
  retains existing escape, quote, and UTF-8 offset semantics, with static dispatch
  for the quote predicate.
- The first configuration line-number lookup caches CR/LF/CRLF break positions for
  that file; subsequent lookups use binary search. Production parser input remains
  bounded to 512 KiB.
- FTS batch parameters borrow strings from pending documents instead of copying
  six columns. Batches still admit at most 1,024 documents and honor SQLite's
  actual variable limit.

All configuration support facts, getter evidence, excerpts, and search documents
remain. Storage format, fact version, ranking, unresolved states, task leases,
one writer per repository, checkpoint recovery, publication barriers, and budgets
are unchanged. No runtime setting, installation dependency, or service is added;
the optimization itself requires no migration or rebuild of existing indexes.
The file-local line cache adds temporary storage, so this is not a memory-saving claim.

## 3. Binary Identity and Environment

| Item | Identity |
| --- | --- |
| Before | Existing release package for `main df33178641576afcda6b47724ff1f780d4679174` |
| After | Working change on `perf/lossless-cold-index`, based on the same commit |
| Before binary SHA-256 | `6d8547cc29a9387d6b64cee6547e1d3801a053a91909162201a8c72e57037b69` |
| After binary SHA-256 | `ee0329e5732e1f875c708b9b8f5597f7408bcbae08f9bc27e4accf537fe03e4d` |
| Source-input digest | `37ca9294c213efaf9048a81d2d893a93bac49258dad83e83ea62d96264d097ae` |
| Build | Rust 1.98.1, Linux x86_64, release; after uses `--locked --all-features` |
| Runtime | Ubuntu 24.04 / WSL2, glibc 2.39, Intel i7-1260P, 16 logical CPUs |
| Real corpus | `relay-teams`, `fa3c0ddc9d81400b8d5e58ab7600dd557a056816` |

The source digest covers 2,010 `src` and Cargo input files: normalize CRLF to LF,
hash each file, then hash the path-sorted JSON array of `{path, sha256}` objects.
Every file was compared with the actual build snapshot. Reports, test drivers,
and documentation are outside this product-source digest.

Build artifacts reside on Windows drive C to control disk usage. Timed binaries,
corpora, and independent runtime homes reside on native WSL ext4. No compilation
or tests from this task overlapped timing. Each version uses three fresh runtime
homes, ordered before/after, after/before, before/after. OS page caches are retained:
"cold" means no application index, not an empty OS cache. Monotonic timing and GNU
time capture wall time, CPU, and peak RSS, with a 300-second command timeout and
free-disk guard. No sample was selectively discarded.

## 4. Timing and Resources

Values are three-run medians in seconds. Change is `(after / before - 1) × 100%`.

| Case | Before | After | Change |
| --- | ---: | ---: | ---: |
| Real-repository cold index | 31.585 | 30.017 | -4.96% |
| Real-repository full scope preview | 8.827 | 6.413 | -27.35% |
| Real-repository unchanged index | 0.1133 | 0.1107 | -2.28% |
| C fragment cold index | 0.3568 | 0.3487 | -2.28% |
| 1,024-file cold index | 0.7080 | 0.5494 | -22.41% |
| 1,024-file incremental index | 0.8461 | 0.7660 | -9.46% |
| 2,048-file cold index | 3.3796 | 2.8562 | -15.49% |
| 2,048-file incremental index | 3.5673 | 3.1079 | -12.88% |

The 1,024/2,048 labels identify existing source-file fixture sizes. Full registered
scopes actually index 1,025/2,084 files including manifests and other files;
both versions use the same scope.

Real cold-index samples are before `[26.791, 34.888, 31.585]` and after
`[29.235, 33.090, 30.017]`. Preview samples are before `[8.663, 8.827, 9.648]`
and after `[6.420, 6.017, 6.413]`. Scheduling and storage variation also affect
small fixtures; three samples do not establish statistical significance.
Query probes have one pair each and verify behavior, not latency percentiles.

Median cold-index user CPU falls from 145.39 to 90.36 CPU seconds (-37.85%);
preview falls from 100.24 to 64.90 (-35.26%). CPU time accumulates across threads
and must not be interpreted as wall time. Less scanning work with unchanged
persistence, FTS, and finalization is consistent with the larger preview gain
and smaller end-to-end gain. This run does not isolate phases or individual changes.

Median cold-index peak RSS instead rises from 938.4 to 1,011.8 MiB (+7.81%);
preview falls from 315.4 to 302.7 MiB. The peak change has not been independently
attributed. Database plus WAL remains approximately 1.475 GiB, with page-level
differences and approximately unchanged filesystem output. Neither smaller
databases nor lower cold-index memory usage is demonstrated.

## 5. Behavioral Parity and Budgets

The first real-corpus pair compares normalized JSON row hashes and sorted table
SHA-256 digests for all 1,545,613 rows in 16 fact/search tables, excluding time
columns and runtime I/O diagnostics. All match, including type ownership JSON,
configuration metadata, excerpts, diagnostics, FTS content, and search rowid mappings.

| Content | Identical counts |
| --- | ---: |
| Files / symbols | 1,835 / 40,720 |
| References / calls / imports | 263,817 / 237,634 / 12,136 |
| Chunks | 39,057 |
| Configuration evidence / bindings | 33,006 / 60,887 |
| FTS documents / metadata | 427,980 / 427,980 |
| File diagnostics | 91 |

The other tables cover dependencies, framework nodes/edges, routes, and path
tombstones. No diagnostics or support facts were removed. Ordered results match
for seven query kinds (hybrid, symbol, definition, references, callers, callees,
imports) and three existing environment-variable configuration queries. Only
scope, freshness, and metadata envelopes are excluded from configuration comparison.

The three existing self-iteration fixtures also have identical facts across five
full/incremental snapshot pairs and eight ordered query cases. Full and incremental
index executions check a succeeded task, completed checkpoint, and exact commit;
the real corpus also checks fresh status. Unchanged repeat-index responses contain
existing scope/status/summary information: all six are fresh with the correct
commit, but have no task/checkpoint fields and are not evidence of a new completed
task. Incremental cases retain at most two blob reads and two parsed files.
All samples meet unchanged budgets: C cold index 5 seconds,
1,024-file cold/incremental 12/3 seconds, and 2,048-file cold/incremental 30/5 seconds.
The complete evaluator was not run, so these results are not its acceptance score.

## 6. Quality Verification

Release build, all-target check, all-target Clippy, formatting, 218 Markdown
documents, skill metadata, and Chromium browser verification passed. The UT rerun
has 4,493 passed, zero failed, and one ignored subprocess fixture explicitly
invoked by bounded Git tests. Integration passed 171/171 in 81.79 seconds of test
execution. The self-iteration harness passed 243/243 and its Clippy gate passed.
The scan-work suite passed 2/2, persistence performance passed 20/20, and the
deterministic benchmark passed 1/1. Their command wall times are 5.79, 8.92, and
15.25 seconds including Cargo checks or compilation, not product indexing times.

`cargo llvm-cov --lib --bins --all-features --fail-under-lines 90` reports unit-test
line coverage of **90.04445% (156,590/173,903 lines)**, without integration coverage.
The lexical scanner and line cache cover 56/56 and 18/18 lines; the SQLite search
owner covers 347/378. The coverage run also passes all 4,493 tests. Miri passes
17/17 `domain::core::` tests with strict provenance, symbolic alignment, and
deterministic concurrency. The final AddressSanitizer attempt passes the same
4,493 tests with zero failures and the existing ignored fixture. Test execution
takes 1,683.99 seconds and the command takes 1,712.63 seconds, exiting 0 with leak
detection enabled.

The first local ASan build hit OOM within its 6 GiB memory scope before tests
started; kernel and scope diagnostics are retained. Using 1,024 codegen units and
1 GiB of temporary additional swap completed compilation, but the combined build
and test attempt reached its 2,400-second limit and exited 124, which is not a pass.
The next attempt reuses the same artifact and changes `malloc_context_size` from
its default 30 to 5. This only shortens allocation/deallocation diagnostic stacks;
AddressSanitizer and `detect_leaks=1` remain enabled, with unchanged assertions and
product budgets. The final run retains the 6 GiB memory and 3 GiB swap scope, eight
test threads, and a 3,600-second limit. After normal completion, the temporary swap
file was disabled and removed. These local settings do not replace the PR's native
Linux ASan gate with its default settings.

The initial two-thread UT attempt reached the local 1,800-second process limit
without an assertion failure in completed tests. Its log is preserved; the same
compiled suite is rerun with eight test threads. This does not change product
resource budgets and is outside the performance measurement window.

The eight-thread integration attempt in the mounted source directory had one
health/query isolation failure and was stopped after preserving its log. The same
test executable passed alone in both mounted and native directories (about
1.88/1.84 seconds for the whole case), retaining its two-second response assertion.
The original complete assertion message was not captured, so a specific timeout
or code cause is not established. Cargo was then found to inject 38 dynamic-library
search directories, 36 on drive C, causing repeated cross-filesystem probes in
subprocesses. `ldd` confirmed that the executable only needs Linux system libraries.
Moving source alone was still slow, so that incomplete attempt was stopped. Full
integration passed from native source with two test threads and
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER='env -u LD_LIBRARY_PATH'`. This runner
only removes redundant library search paths from test subprocesses; product code
and assertions are unchanged. The A/B driver directly starts release binaries
and does not inherit Cargo's test library paths.

## 7. Evidence and Reproduction Limits

Local raw evidence resides in ignored `target/lossless-cold-index-20260917/`.
This tracked report retains the publishable findings; that local directory is
not a shared artifact location accessible from other machines.

| Raw report | SHA-256 |
| --- | --- |
| `comparison.json` | `831fd008c25f54dea75547cc33ee533e926cb058ba91edd0a96b121fbca42fb5` |
| `fixtures/comparison.json` | `1cb84c573804b38549fc0c5bf8ed041dfd456548fcdbfe00d4b49a54669cf431` |
| `summary.json` | `d86e3facc99f6acf14c1823436ce7e6e2218c8178727505df7bc6f1bbf467411` |

On POSIX, use the pinned snapshots and alternating order above, with independent
runtime homes for the two release binaries. Run `repo register`,
`repo index --ref <commit>`, `repo scope preview --ref <commit>`, and an unchanged
repeat index. Fixture definitions come from
`tools/self_iteration/cases/repository_index_performance_targets.json` and its
existing generators. The new deterministic `configuration_scan_work_suite` is
part of the fast gate and rejects repeated character/quote work for absent API
patterns; existing persistence tests protect FTS ordering, bounds, rollback, and recovery.

---

Navigation: [Verification records](README.md) | Previous: [16. Map and Graph Storage Acceptance](16-map-graph-storage-acceptance-2026-09-07.md)
