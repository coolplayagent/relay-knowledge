# Durable Worktree Delta Batching

## Problem

A worktree overlay is semantically different from a clean Git snapshot. Its
target is the immutable synthetic identity `worktree:<base>:<overlay-hash>`,
while the queued task initially owns a pending scope derived from
`worktree:pending:<base>`. A small overlay can clone the base and replace its
dirty paths in one fenced SQLite transaction. When that complete surface
exceeds the task's frozen writer quantum, returning `DurableStagingRequired`
cannot authorize conversion to a clean full index, early success, a larger
transaction, or an unmanaged retry loop.

The existing durable incremental protocol already clones an immutable base in
bounded pages, but its dirty delta and terminal handoff were one transaction.
That left a valid worktree task unable to complete even though each file-owned
part of the delta could fit independently.

## Decision

Worktree indexing is direct-first. A typed direct-admission overflow enters the
same fenced durable clone protocol with the original task and resource budget:

1. Atomically rebind the pending task/fence to the content-addressed worktree
   scope and create the empty unpublished target plus clone progress.
2. Clone the immutable clean base through metadata-indexed keyset pages,
   excluding affected owners while charging their scan work.
3. Freeze a deterministic dirty-delta plan from the snapshot digest and file
   order. Each file owns all of its symbols, references, imports, dependencies,
   feature flags, framework facts, routes, chunks, diagnostics, and eventual
   calls.
4. Commit at most one `CodeIndexBatch` per worker step. Persisted checkpoint
   `batch_count` is the replay cursor; a new lease generation resumes at the
   next batch and does not replay a committed batch.
5. In one separately admitted terminal transaction, insert tombstones, remove
   clone progress/affected-path ownership, write the task-bound multi-batch
   receipt, restore `last_path` to the true maximum target path, and hand the
   complete unpublished target to the existing `indexing` finalizer.
6. Reuse ordinary query-index repair, reference/import/call resolution,
   grouped search rebuild, publication, and business/software projection. No
   finalization stage is skipped.

## Ownership

| Concern | Owner |
| --- | --- |
| Direct-first dispatch and durable resume | `storage::sqlite::code::snapshot::mod` |
| Immutable-base clone and typed progress | `snapshot::durable_clone` |
| Clone-complete and active-delta validation | `snapshot::durable_clone::delta` |
| Deterministic file-owned plan | `snapshot::durable_delta::batches` |
| One replay-safe batch per worker step | `snapshot::durable_delta::mod` |
| Terminal cleanup, tombstones, receipt, and path restoration | `snapshot::durable_handoff` |
| Rebound publication identity | `application::code_repository::indexing::workflow::publication` |
| Synthetic-ref parsing and nested-query pinning | `application::code_repository::{worktree_ref,scope}` |

Dependencies remain one-way from the snapshot coordinator into clone, delta,
handoff, and existing batch/finalization owners. The application does not
duplicate storage phase logic.

## Budget and consistency invariants

- File, byte, row, queue, retry, and lease bounds are unchanged. Arithmetic
  that derives batch or terminal capacity is checked and fails before writes.
- Batch membership is deterministic and file-owned. A fact whose path has no
  file owner is an invariant error. One already globally bounded indivisible
  file may occupy a batch alone, but never absorbs another file after crossing
  a frozen threshold.
- Calls are rebuilt from call-shaped references during finalization, but their
  eventual rows are still charged to the owner file's delta batch.
- The terminal quantum jointly accounts for affected-path/progress cleanup,
  tombstones, fixed checkpoint/fence control rows, and the encoded receipt.
- Repository, base, target, tree, filters, delta digest, resource budget, task,
  active attempt, and publication fence are revalidated around every mutation.
- A worktree task is never reinterpreted as a clean full task. Missing legacy
  fact proof, incompatible base, stale fence, changed digest, corrupt progress,
  or target mismatch fails closed without publishing partial facts.
- A resolved `worktree:<base>:<hash>` selector is an immutable graph identity,
  disjoint from `worktree:pending:<base>`. Nested context queries reuse it and
  never send it to Git ref resolution.

## Current boundary

The durable overlay path deliberately rejects non-empty auto-workspace
projection state. The CLI leaves workspace detection disabled by default. An
API/Web worktree request that enables it fails closed rather than dropping the
workspace manifest or converting the overlay to a clean snapshot. Supporting
that combination requires a separately designed, persisted and bounded
workspace manifest that survives finalization recovery.

## Verification contract

Focused storage and adapter tests must cover deterministic planning, orphan
rejection, terminal-control admission, receipt scaling, pending/resolved
identity disjointness, pinned nested queries, and exact lease takeover between
two dirty batches. The recovery filter used by the fast/performance profile
must include the worktree end-to-end test:

```bash
cargo test --all-targets --all-features code_index_task_ -- --nocapture
```

The real repository must also complete a product-binary worktree index and a
subsequent context query at the pinned synthetic identity. Final acceptance
requires the unchanged release-product self-iteration budgets, full Rust
quality gates, 90% line coverage, documentation validation, and both repository
maps to validate:

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
python3 tools/docs/check_docs.py
relay-knowledge map validate --type all --format json
```

This design changes no installation, upgrade, service-manager, runtime-path,
configuration-migration, or uninstall behavior.

Dated implementation and validation evidence is recorded in
`docs/en/06-verification/15-durable-worktree-delta-and-pinned-query-2026-08-31.md`
and its Chinese counterpart.
