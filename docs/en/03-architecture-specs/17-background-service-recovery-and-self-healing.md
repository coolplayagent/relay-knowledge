# Background Service, Recovery, and Self-Healing

[English](../../en/03-architecture-specs/17-background-service-recovery-and-self-healing.md) | [中文](../../zh/03-architecture-specs/17-background-service-recovery-and-self-healing.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Background service operation is not an unmanaged CLI loop. Long-running refresh, indexing, maintenance, diagnostics, and silent updates run under platform service managers with bounded resources, persistent leases, startup reconciliation, and dead-letter recovery.

## 2. Runtime Modes

| Platform | Manager |
| --- | --- |
| Linux | systemd |
| macOS | launchd |
| Windows | Windows Service |

CLI may generate service definitions and run doctor checks, but it does not pretend to be the resident service manager.

## 3. Work Queues

Every background task has kind, scope, priority, budget, attempt, lease owner, lease expiry, target graph version, payload hash, and last error. Queue capacity is a hard ceiling; enqueue failure returns overload or retryable errors.

## 4. Reconciler

The startup reconciler:

- Replays mutation-log refresh work that was missed.
- Recovers expired leases.
- Preserves dead-letter isolation.
- Reports index lag, queue depth, stale scopes, and failed cursors.
- Corrects cursor state when graph version advances while a task is running.

## 5. Silent Updates

Silent updates are user-configurable, pausable, observable, and reversible. They refresh graph data and derived indexes only within authorized scopes and expose fresh, stale, paused, degraded, and failed states.
Resident local file indexing follows the same rule: scanners run only on
configured absolute roots, reject relative root configuration before scanning,
persist cursors and diagnostics, enforce scan/query timeout budgets, and report
truncated roots, scan errors, freshness, and lag instead of blocking query paths
or silently expanding to unapproved disks.

File-system watchers and scan workers degrade by platform capability: Windows may use USN cursors, macOS may use FSEvents cursors, and Linux may use inotify/fanotify or periodic bounded rescans. Event overflow, journal reset, permission changes, missing roots, and cursor invalidation become recoverable diagnostic states instead of unbounded whole-disk scans.

Cold code-repository full indexing uses the same recovery shape. `repo index` performs tracked source-layout discovery and then persists a code-index task with a source scope, input fingerprint, payload, resource budget, attempt count, retry cursor, and lease fields; foreground CLI starts only a bounded single-shot worker, and resident `service run` recovers expired code-index leases and orphaned `code-index-worker-<pid>` leases at startup before draining the durable queue with a bounded repository index worker pool controlled by `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT`. Distinct fingerprints can queue concurrently, but claim enforces at most one live writer per repository; different repositories can still claim independent leases, checkpoints, and retry state. Identical full-index fingerprints reuse unfinished work so the same source scope is not rebuilt twice. Expired running leases are recovered before claim and status paths report active work: recoverable attempts move to retry with `lease_expired` diagnostics, attempt-exhausted tasks move to dead-letter, and late workers cannot complete or fail a task after their lease has expired, been reclaimed, orphaned, or explicitly reset. Service startup checks `code-index-worker-<pid>` lease owners and recovers running tasks whose owner process has exited with `lease_orphaned`, while preserving leases for workers that are still alive. An explicit repository index reset may requeue unfinished code-index tasks by clearing lease owner, lease expiry, attempt count, retry cursor, and last-error fields, but it must not run while another task has an unexpired running lease for the same repository, delete completed indexed scopes, revive terminal dead-letter history, or bypass lease-guarded completion. Active workers renew the lease before expensive batch parsing, after each committed checkpoint batch, around finalization, and before task completion; stores that do not implement the optional recovery or renewal hooks keep those hooks as no-ops. Git blob materialization for cold batches uses bounded `git cat-file` commands with explicit stdin close and timeout handling; a stalled Git child returns a task failure for retry/dead-letter handling instead of keeping the lease forever. Checkpoint `updated_at_ms` remains visible for stuck-task diagnosis. Repository-set overlay refresh tasks use the same resident-service model: async refresh requests persist a leased task, and `service run` drains that queue with one repository-set overlay refresh worker. The workers mark retry or dead-letter on failure; code-index success also prunes old code scopes while retaining the active scope, the two newest completed scopes, and unfinished task scopes.

Large repository indexing must not block service liveness or ordinary read queries. SQLite writes must pass through a single writer lane with bounded transient busy/locked retry; health, graph/status/report, file query, and code query paths should prefer bounded read-only connections over committed snapshots. Lock contention must be surfaced through task status/checkpoints and bounded busy diagnostics, not by asking operators to kill competing `relay-knowledge` processes or by adding unbounded SQLite waits. `health` does not run diagnostic reconcile writes, does not queue refresh work, and returns stale/degraded `storage_busy` when it exceeds its short budget. Code queries with `allow-stale` read the previous completed scope while the requested ref is indexing and mark the response stale/degraded; `wait-until-fresh` is the mode that may reject an unfinished target scope.

Overload handling follows SRE and adaptive-concurrency principles: when queue, IO, CPU, or provider budgets saturate, the system rejects new background work first, delays low-priority content indexing, preserves query hot-path budget, and returns retryable, paused, or degraded states.

## 6. Acceptance Criteria

- Crashes and restarts do not lose required refresh work.
- Diagnostic paths do not automatically revive dead-letter tasks.
- CPU/IO-heavy background work does not block query hot paths.
- Watcher lag, scan backlog, cursor invalidation, and overload decisions are explainable through health and service doctor.

---

Navigation: Previous: [16. Unified API and Interface Architecture](16-unified-api-and-interface-architecture.md) | Next: [18. Observability, Diagnostics, and SLO](18-observability-diagnostics-and-slo.md)
