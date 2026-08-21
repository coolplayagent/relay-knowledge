# Code Index Retention

## 1. Purpose

Code-index cleanup has two policy layers and one physical deletion mechanism:

- Scope retention bounds historical index generations inside one repository.
- Repository retention bounds how many eligible repositories keep a published index.
- Durable phased scope GC performs every physical index deletion selected by either policy.

Repository retention is automatic maintenance, not a repository removal operation. It keeps the repository registration and aliases so later indexing can rebuild an active scope without registering again.

## 2. Scope Retention

Ordinary retention is unchanged. After a successful publication it protects the set union of:

- the active scope and two most recent successful scopes, where active normally belongs to that two-scope window;
- the latest successful incremental predecessor;
- each active worktree overlay clean base;
- every unfinished task target and base;
- every repository-set member pin.

An unprotected old scope is atomically marked `retiring` and receives a durable scope-GC job. Readers and incremental-base selection exclude retiring scopes immediately. Later bounded maintenance transactions delete facts, search documents, software projections, checkpoints, workspace state, and scope metadata in ordered phases.

## 3. Repository Retention

`RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES` is an integer in `1..=i64::MAX` with default 10. The upper bound matches SQLite's persistent integer range and is validated before maintenance starts. A repository counts when it has a current published scope. Membership in a user-managed repository set excludes it from both the count and candidate selection. The automatic workspace set is identified by its deterministic set ID derived from the repository ID; its editable alias is not trusted for authorization or exemption decisions.

Scheduling runs after successful publication and before each bounded retention pass used by the resident service or `repo index-worker`. At most one repository-retention parent job is globally active. If eligible indexed repositories exceed the limit, the scheduler selects the repository with the oldest current successful publication and persists:

- `repository_id`;
- `initial_scope`, the active scope observed during selection;
- `cutoff_ms`, the scheduling timestamp;
- `cutoff_publication_generation`, the successful publication generation observed for the initial scope;
- phase, timestamps, and last error.

Candidate discovery reads at most 64 rows from the durable `code_repository_retention_activity` projection in `(activity_ms, repository_id)` index order in one scheduling transaction. Changes that can affect a repository's current scope or successful-publication activity enqueue that repository in `code_repository_retention_activity_dirty`. Before scanning, each scheduling transaction refreshes at most 64 dirty repositories through indexed point lookups. If dirty work remains, maintenance stays pending and candidate selection waits rather than reading stale projection rows. Schema marker version 6 creates and backfills the projection during upgrade.

Dirty-activity enqueue is idempotent through an explicit indexed existence check rather than a trigger-local conflict policy. SQLite can propagate an outer statement's UPSERT conflict policy into trigger statements, so same-tree commit publication and terminal-task reuse must never depend on `INSERT OR IGNORE` to deduplicate one repository's dirty marker. A marker-independent compatibility check atomically replaces legacy dirty-activity trigger definitions on the next file-backed open so existing databases receive the invariant without rewriting indexed facts or rebuilding derived retrieval.

If a candidate page does not establish whether the limit is exceeded, the scheduler persists a singleton scan cursor, eligible count, oldest candidate, and catalog revision, then resumes from that cursor on a later maintenance pass or after process restart. A changed limit, active parent job, projection refresh, or mutation affecting candidate order or eligibility discards the cursor and restarts from the first page. An unfinished scan or activity refresh is active maintenance, so `repo index-worker` reports `maintenance_active=true` until the work finishes or creates a parent job. Before creating a parent job, the selected repository is revalidated against its current scope, retirement state, and user-managed set membership.

The durable parent survives process restart. A maintenance pass loads it and selects child scopes through the existing scope-GC state machine. Repository mode intentionally does not apply the ordinary active/latest-two protection to scopes that existed before the cutoff.

## 4. Concurrency and Protection

Repository cleanup does not block index admission and does not cancel queued, retrying, or running tasks. It protects:

- target and base scopes referenced by unfinished tasks;
- successful publication generations newer than a nonzero `cutoff_publication_generation`; a zero parent watermark, generation-zero legacy rows, and checkpoints use the inclusive `cutoff_ms` fallback;
- an active scope that differs from `initial_scope`, including a same-millisecond concurrent publication;
- the latest incremental predecessor required by such a concurrent publication;
- active worktree bases.

When the initial active scope starts retiring, the repository's current scope pointer is atomically cleared and its state returns to `registered` and stale. The repository row, root, aliases, task history, and parent job remain. A new task may publish while older child GC phases continue.

If the repository becomes a member of a user-managed set after scheduling, maintenance removes the parent and stops retiring additional scopes. Partitioned maintenance refreshes repository mode from the control result before invoking the shard, so the same pass cannot forward a stale repository cutoff. A child scope already marked `retiring` still completes because readers have already stopped treating it as live.

In partitioned storage, the control database also detects when `initial_scope` was republished at the cutoff millisecond with a higher publication generation. It forwards that scope as a shard retention pin until the parent job converges, preventing shard cleanup from treating the same-millisecond publication as pre-cutoff data.

## 5. Completion and Observability

Single-SQLite mode completes the parent only when no repository-mode prunable scope and no child scope-GC job remains. Partitioned SQLite merges control and shard retention state and completes the parent only after both sides converge; catalog routes remain governed by the existing final-phase ordering.

`repo status` retention output includes the optional repository-retention parent job together with child scope-GC jobs. `maintenance_pending` remains true while either kind of job is present. The parent reports repository, initial scope, time and publication-generation cutoffs, the active child GC phase, timestamps, and the latest child error.

Post-cutoff successful scopes are deduplicated across task and checkpoint publication records before the bounded history limit is applied. If the distinct result still exceeds the bound, retirement pauses and the parent remains pending instead of completing from incomplete evidence. Partitioned completion also requires an untruncated merged scope listing.

## 6. Required Tests

Regression coverage must verify:

- the default is 10, positive overrides work, and zero is rejected;
- user-managed set members are excluded while automatic-workspace members still count, independent of aliases and candidate-page position;
- activity projection refresh and candidate discovery are bounded to 64 rows per scheduling transaction, use the activity-order index without a temporary sort, and resume after reopening SQLite;
- repeated dirty-activity enqueue remains idempotent inside outer scope and task UPSERT statements, including same-tree commit publication;
- the oldest eligible successful publication is selected;
- parent and child jobs resume after reopening SQLite;
- first-pass logical retirement precedes physical deletion;
- registration and aliases survive whole-repository index cleanup;
- unfinished work, post-cutoff publication generations, same-millisecond incremental bases, and same-millisecond higher-generation republishes survive;
- duplicate task/checkpoint publication records are deduplicated before history bounding;
- parent phase and error fields follow the active child GC job;
- joining a user-managed set stops additional retirement;
- incomplete publication history keeps the parent pending;
- partitioned control and shard cleanup complete the parent only after convergence.
