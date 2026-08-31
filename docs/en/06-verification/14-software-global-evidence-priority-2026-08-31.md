# Software-Global Evidence Priority Verification 2026-08-31

[English](../../en/06-verification/14-software-global-evidence-priority-2026-08-31.md) | [中文](../../zh/06-verification/14-software-global-evidence-priority-2026-08-31.md)

> Date: 2026-08-31
> In-scope status: PASS
> Baseline revision: `6e78bdbac22e1a0875cee2b13434baffd3b52a17`
> Evaluated patch: 50,981 bytes; SHA-256 `19e85c4ed920d998100c1a5533258525ad965024a38b1e4ac0f194152298dd55`
> Final report: `manual-evaluate-1788132786594001932-0-1437486.json`; SHA-256 `91bcee863e5b9e62a7cd6dcd89ca6fb16cb26440827a3ae9d26cff10629e7fc6`
> Evidence boundary: release product binary, `fast --categories performance`, and current-change Rust and documentation gates; this does not certify exhaustive, agent-workflow, research-judge, Kubernetes, or overall release readiness

## 1. Objective and Analysis Scope

This iteration used CodeSpec, the Knowledge Map, the repository code graph, and
real `tools/self_iteration` cases to route analysis of passing but poorly
ordered `software_global_fixture` evidence. All 368 gates, 132 cases, and 307
commands passed at baseline. The defect was therefore not missing recall or an
incomplete schema; actionable evidence was ordered behind weaker candidates in
the same result set.

The code graph routed API, resource, and deployment reads to
`storage::sqlite::software::ontology::query`, topics to
`storage::sqlite::software::graph::topics`, and design evidence to
`storage::sqlite::software::lifecycle::design`. The implementation changes only
deterministic ordering at those read boundaries. Ingestion, projection, schema,
durable tasks, leases, checkpoints, freshness, and writer paths are unchanged.

## 2. Baseline, Algorithm, and Result

| Observation | Baseline rank | Final rank | Ordering signal |
| --- | ---: | ---: | --- |
| Primary API evidence | 2 | 1 | API schema, then code, then documentation |
| Primary resource evidence | 3 | 1 | Kubernetes, Terraform, Compose, systemd, launchd, Helm |
| Primary deployment evidence | 4 | 1 | service definition, IaC, runtime; deployment unit before runtime service |
| Primary topic evidence | 3 | 1 | nested document, Knowledge Map topic, root README heading |
| Primary design evidence | 2 | 1 | architecture, capability, module, API, software system |
| Statement provenance | 7 | 7 | intentionally unchanged; retained as a separate query-plan gap |

Every rule uses materialized kind, source, provider, path, language, name,
confidence, and identity fields and finishes with stable tie-breaks. It neither
widens query limits nor reads live source, enumerates fixtures, repositories,
queries, paths, or symbols, or introduces an unbounded sort into the statement
full-scope hot path.

The final score rose from `0.990520666` to `0.998940610`; accuracy rose from
`0.978456059` to `0.997592295`, foundational capability from `0.968750000` to
`1.000000000`, and competitive capability from `0.988162119` to `0.995184591`.
Semantic/vector, performance, and stability all remained `1.0`.

## 3. Release-Binary Self-Iteration Evidence

The evaluation entry point was:

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

The final report used `target/release/relay-knowledge`, `cached_home=false`, an
isolated evaluation home, and concurrency bounds of 16 global, 8 repository,
and 16 query jobs. All 368 gates, 132 cases, and 307 commands passed, producing
80 metrics. It recorded `score_accepted=true` and
`adoption_status=would_accept`; manual evaluation created no commit.

| Key metric | Observed | Budget | Result |
| --- | ---: | ---: | --- |
| Release build | 277 ms | 180,000 ms | PASS |
| Software fixture cold index | 651 ms | 15,000 ms | PASS |
| Software fixture register plus cold index | 701 ms | 18,000 ms | PASS |
| Software query p50 | 80 ms | 100 ms | PASS |
| Software query p95 | 90 ms | 250 ms | PASS |
| C syntax query p95 | 180 ms | 180 ms | PASS at the limit |

Three candidate reruns retained every budget and did not discard failures:

| Report | Status | Software cold / p50 / p95 | Rejection reason |
| --- | --- | --- | --- |
| `manual-evaluate-1788132101088846588-0-1415938` | rejected | 427 / 83 / 98 ms | cold release link was 230,130 ms against 180,000 ms |
| `manual-evaluate-1788132525019340332-0-1427696` | rejected | 575 / 85 / 93 ms | unrelated C fixture p95 was 184 ms against 180 ms |
| `manual-evaluate-1788132786594001932-0-1437486` | would_accept | 651 / 80 / 90 ms | no rejection reason; every budget passed |

The first two report SHA-256 digests are
`f2cb8001954bbc305345968ebdfd5361a4f1c66fbb641c7172a213a8877af6df` and
`533f14bdb926a27e41c28477492ac05de1addc7a4cdc00986e50b8b07bb89e45`.
The baseline report `manual-evaluate-1788131353759667735-0-1401277.json` has
SHA-256 `dfba514d400bb6374bc7344d2fd717f4adb8faab770ad86277d152ca77075a2e`.

The `agent_workflows` and `research_judge` suites were skipped because this run
selected only the performance category. Their skipped state cannot be replaced
by the 368 passing gates.

## 4. Owner Tests, Coverage, and Worktree Boundary

New owner tests directly fix the relative order of API code, Kubernetes
resources, systemd service definitions, architecture/capability/module
evidence, and nested-document topics. All 16 focused ordering tests passed, as
did all 6 focused architecture-boundary tests.

The full coverage command was:

```bash
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

The result was 15,340 missed lines out of 153,584, or `90.01%` line coverage,
passing the 90% hard gate. The same run reported 3,768 passed and 1 ignored
library tests plus 1 passed benchmark target.

Coverage also exposed a test-infrastructure defect unrelated to product
ordering but material to real dirty-worktree validation. The old line-budget
test read every cached path returned by `git ls-files`; deleting a superseded
Knowledge Map shard made that path absent and caused a false failure. The
corrected boundary enumerates existing tracked and untracked regular files,
ignores deleted index-only paths, and still fails closed for other metadata I/O
errors. A new isolated-Git-repository test fixes retained, deleted, and
untracked behavior, so the file-length gate is not weakened.

The final shared worktree also ran these independent gates. They validate the
verification documents, map updates, and test-infrastructure fix added after
the evaluated patch; they do not retroactively expand that patch's scope:

- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --all-targets --all-features`: PASS.
- `python3 tools/docs/check_docs.py`: PASS.
- CodeSpec and Knowledge Map validation: PASS.
- `git diff --check`: PASS.

The final full-test summary was 3,768 passed and 1 ignored library tests, 1 of 1
benchmark test, and 156 of 156 integration tests. It completed while the
Knowledge Map v20 worktree contained added, replaced, and deleted shards,
directly exercising the corrected path-enumeration boundary.

## 5. Final Code-Graph Consistency Boundary

The persisted baseline HEAD graph is fixed to scope
`git_snapshot:cd18dbccb30f1b8a` and commit
`6e78bdbac22e1a0875cee2b13434baffd3b52a17`. It is fresh and contains 2,426
files, 42,557 symbols, 229,444 references, and 25,935 chunks. Snapshot-bound
context, software, business, architecture, and dependency views remain
readable. Context routed this iteration to Chapter 21 architecture, the
software request contract, and the real software-global case. The historical
index still explicitly reports 20 degraded files and is not a zero-degradation
index.

At the time of this record, the final worktree requested incremental indexing
through:

```bash
relay-knowledge repo index relay-knowledge-reference --ref worktree --format json
relay-knowledge repo index-worker --task-id code-index-task:1a8b0c09a0d91999 --format json
```

The initial attempt and one durable-worker retry after its retry backoff failed
at the same boundary: the direct snapshot exceeded its writer quantum and
required the checkpointed full-index pipeline. The task remains `retrying` at
attempt 2, and the existing fresh HEAD scope was not replaced. The
512-file/16-MiB/150,000-row resource budgets were not enlarged, leases and
checkpoints were not bypassed, and SQLite was not edited directly. This record
therefore does not claim a fresh final-worktree code graph. Final changes are
verified by snapshot-bound HEAD routing, bounded `git diff` review, owner tests,
and full gates. This is retained as the historical state observed by B.14. The
later
[Durable Worktree Delta and Pinned Query Verification](15-durable-worktree-delta-and-pinned-query-2026-08-31.md)
implements the missing bounded transition and records a successful real
worktree replay; it supersedes the open-defect conclusion without rewriting
the earlier evidence.

## 6. Conclusion and Open Risks

`software-global-typed-evidence-priority` is accepted within this record's
focused performance scope. All five target evidence classes moved from a lower
position to rank 1, every case, gate, and unchanged budget remained green, and
software query p95 remained well below its 250-ms ceiling. The accepted change
is generalized materialized-evidence ordering, not a fixture name or path
special case.

This record does not close statement rank 7. Adding a full-scope `CASE` sort to
a query of up to 524,288 statements could amplify SQLite hot-path cost; any
follow-up requires a bounded index or query plan and regression evidence under
the same performance cases. The final C syntax p95 exactly matched its budget,
and the first two runs showed independent budget variance, so both remain
stability signals to monitor.

This is not an overall release-readiness result and does not replace exhaustive,
Kubernetes, browser, package, service, or cross-platform gates. Overall
readiness remains governed by
[Documentation and Self-Iteration Readiness Verification 2026-08-18](13-documentation-self-iteration-readiness-2026-08-18.md)
and any later dedicated records.

---

Navigation: Previous:
[13. Documentation and Self-Iteration Readiness Verification](13-documentation-self-iteration-readiness-2026-08-18.md)
| Index: [Verification Records](README.md)
| Next: [15. Durable Worktree Delta and Pinned Query Verification](15-durable-worktree-delta-and-pinned-query-2026-08-31.md)
