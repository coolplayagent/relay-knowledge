# Durable Worktree Delta and Pinned Query Verification 2026-08-31

[English](../../en/06-verification/15-durable-worktree-delta-and-pinned-query-2026-08-31.md) | [中文](../../zh/06-verification/15-durable-worktree-delta-and-pinned-query-2026-08-31.md)

> Date: 2026-08-31
> In-scope status: PASS
> Baseline revision: `6e78bdbac22e1a0875cee2b13434baffd3b52a17`
> Evaluated patch: 299,016 bytes; SHA-256 `882f30848b626308a0f6c78a51cfd6473a795ea4df1f171791ef0689aa20aa34`
> Final self-iteration report: `manual-evaluate-1788148749661647156-0-1647985.json`; SHA-256 `7dce856be5fc21f38ab3289542a0ec9a22c64f9e9e4a066892f25a6b11065903`
> Evidence boundary: durable worktree storage owners, lease recovery, pinned synthetic-ref queries, release-product fast/performance evaluation, full Rust targets, coverage, documentation, and repository maps; this does not certify exhaustive, agent-workflow, research-judge, browser, package, service, Kubernetes, or cross-platform gates

## 1. Objective and Root Cause

The previous record found that a real dirty-worktree index could exceed the
single direct SQLite writer quantum. Returning `DurableStagingRequired` was
correct admission behavior, but worktree dispatch had no legal transition into
the existing checkpointed pipeline. It therefore retried the same oversized
transaction while preserving the old fresh HEAD scope.

CodeSpec, the Knowledge Map, indexed repository context, and the real
`tools/self_iteration` recovery cases routed the defect to the snapshot
coordinator, durable clone, task receipt, publication fence, and synthetic-ref
query boundaries. The remediation keeps direct indexing for small overlays and
uses the same queued task, frozen resource budget, active attempt, and
publication fence when a typed over-budget result requires durable staging.

## 2. Durable State Machine and Bounded Ownership

The implemented state machine is:

1. Rebind the pending task atomically to the immutable
   `worktree:<base>:<overlay-hash>` target.
2. Clone the clean base in metadata-indexed keyset pages while excluding dirty
   file owners.
3. Freeze a deterministic delta plan in path order. A file owns all of its
   symbols, references, imports, dependencies, feature flags, framework facts,
   routes, chunks, diagnostics, and eventual calls.
4. Commit at most one delta batch per worker step. Persisted `batch_count` is
   the replay cursor across lease expiry and takeover.
5. Admit cleanup, tombstones, fixed control rows, and the variable-size receipt
   together in a separate terminal writer quantum. Restore `last_path` to the
   true maximum target path.
6. Enter the existing query-index, reference/import/call, search, software,
   business, and publication finalizer without skipping freshness checks.

Checked arithmetic rejects capacity overflow before mutation. Facts without a
file owner fail closed. One already globally bounded indivisible file may cross
a per-batch row or byte threshold alone, but it cannot absorb a following file.
The worktree task is never reinterpreted as a clean full index, and no queue,
batch, transaction, retry, source fallback, or timeout was made unbounded.

Resolved and pending worktree selectors are parsed as disjoint identities.
Nested context queries reuse the pinned resolved identity directly instead of
sending the synthetic value to Git ref resolution. Publication uses the
effective rebound lease identity, so an older attempt cannot publish a later
generation's staged facts.

## 3. Focused Recovery Evidence

The focused recovery rail was:

```bash
cargo test --all-targets --all-features code_index_task_ -- --nocapture
```

It passed 89 filtered library tests and 2 filtered integration tests. The new
end-to-end case,
`oversized_worktree_code_index_task_delta_batches_and_recovers_between_leases`,
forced two dirty batches, expired and reclaimed the lease between them, and
then completed without replaying the first batch. Its receipt reported two
delta batches, two parsed files, and 44 SQLite writes; its completed checkpoint
reported three total batches and the real lexicographic maximum path.

Additional owner tests passed for deterministic planning, indivisible-file
isolation, orphan rejection, terminal cleanup/tombstone/control/receipt
admission, multi-batch receipt scaling, pinned worktree context, publication
identity rebinding, and the CLI map namespace error order. The resume precheck
also proves it does not validate an unstaged target before the clone phase has
created it.

## 4. Release-Product Self-Iteration

The candidate ran through the public release-product evaluation path:

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

The first complete run was retained as a rejection rather than discarded:

| Report | Status | Cause |
| --- | --- | --- |
| `manual-evaluate-1788148459837113509-0-1638323.json` | rejected | release build 184,379 ms exceeded the unchanged 180,000-ms budget |
| `manual-evaluate-1788148749661647156-0-1647985.json` | `would_accept` | every selected gate, case, command, and metric passed |

The rejected report SHA-256 is
`2e4439c0dae5e06cd781b82ad55cf6ece888374a20725989acfac4763f0d1d96`.
Both runs completed 368/368 gates, 132/132 cases, and 307/307 command contracts;
the first was rejected only by the measured build budget. The accepted run
reported score `0.9989406099518459`, `score_accepted=true`, and
`adoption_status=would_accept`. Manual evaluation created no commit.

| Key metric | Observed | Budget | Result |
| --- | ---: | ---: | --- |
| Release build | 215 ms | 180,000 ms | PASS |
| Code-index recovery cases | 24,438 ms | 60,000 ms | PASS |
| Software fixture cold index | 514 ms | 15,000 ms | PASS |
| Software fixture register plus cold index | 551 ms | 18,000 ms | PASS |
| Software query p50 / p95 | 75 / 81 ms | 100 / 250 ms | PASS |
| 1,024-file cold index | 611 ms | 12,000 ms | PASS |
| 1,024-file register plus cold index | 772 ms | 13,000 ms | PASS |
| Many-file incremental index | 784 ms | 3,000 ms | PASS |
| C syntax query p95 | 127 ms | 180 ms | PASS |

Performance, stability, and semantic/vector metrics remained `1.0`. The
`agent_workflows` and `research_judge` suites were skipped by the selected
category and are not implied by the passing fast/performance result.

## 5. Full Tests and Coverage

The independent current-worktree Rust gate passed:

```bash
cargo test --all-targets --all-features
```

The result was 3,778 passed and 1 ignored library tests, 1 of 1 benchmark test,
and 156 of 156 integration tests. The ignored subprocess fixture remains an
explicit ignored result rather than a pass.

The exact coverage gate was:

```bash
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

The first exact run was retained as a failure: 15,431 missed lines out of
154,185 produced `89.99%`, below the hard threshold. It exposed uncovered
fail-closed branches in the new owners. Three focused tests now require
duplicate file ownership, an ordinal outside the frozen batch plan, and a
durable receipt without an immutable base identity to return typed errors.

The final result was 15,407 missed lines out of 154,185, or `90.01%` line
coverage, and passed the unchanged 90% threshold. That execution reran all
targets and features with 3,781 passed and 1 ignored library tests, 1 of 1
benchmark test, and 156 of 156 integration tests; it did not exclude new
storage owners or lower the required percentage.

## 6. Real Repository Replay

After implementing the durable transition and before adding this final record
and its map metadata, the release product binary indexed the actual shared
worktree:

```bash
target/release/relay-knowledge repo index relay-knowledge-reference --ref worktree --format json
```

Task `code-index-task:164a93ebb170174a` completed on attempt 1. It published
scope `git_snapshot:0c6a43ff14ae84f1` at resolved identity
`worktree:6e78bdbac22e1a0875cee2b13434baffd3b52a17:cd811b09b98f8588`.
The durable checkpoint recorded 58 changed paths, 3 deletions, 55 blob reads and
parsed files, 12,395 SQLite writes, 315,392 committed fact rows, one delta batch
and two total batches. Its true `last_path` was
`src/relay_knowledge/storage/sqlite/software/ontology/query_tests.rs`.

The resulting fresh scope contained 2,439 files, 42,985 symbols, 230,240
references, and 26,004 chunks. It still reported 20 degraded files overall and
2 degraded changed files; this is not a zero-degradation claim. The older task
recorded in Appendix B.14 remains valid historical evidence of the former
missing transition, but it no longer describes the current default CLI path.

This evidence snapshot necessarily predates the text of this record and its
generated map shards. Final handoff therefore also requires one last worktree
reconciliation and a pinned `repo context` query after all tracked text and map
mutations, reported with the change summary rather than treated as proof that
this document existed before it was written.

## 7. Documentation, Maps, and Scope Boundary

The implementation is synchronized with the bilingual worktree workflow,
incremental-indexing architecture, elastic-budget notes, engineering hard
constraints, self-iteration optimization ledger, the dedicated CodeSpec design,
and this bilingual verification record. CodeSpec and Knowledge Map roots are
mutated only through the product CLI and must validate after their generated
shards and history are written.

The durable overlay deliberately rejects non-empty auto-workspace projection
state. The CLI default leaves workspace detection disabled. API/Web callers
that explicitly enable it fail closed rather than lose manifest metadata or
publish a clean-snapshot identity. Supporting that combination requires a
separate bounded and persisted workspace-manifest design.

No installation path, package artifact, service-manager template, runtime data
directory, configuration migration, upgrade, rollback, or uninstall behavior
changed. Chapter 19 therefore requires no content change for this iteration.
This focused PASS is not overall release readiness and does not replace the
[Documentation and Self-Iteration Readiness Verification 2026-08-18](13-documentation-self-iteration-readiness-2026-08-18.md).

---

Navigation: Previous:
[14. Software-Global Evidence Priority Verification](14-software-global-evidence-priority-2026-08-31.md)
| Index: [Verification Records](README.md)
