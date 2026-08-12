# Freshness and Index Recovery

[English](./06-freshness-and-index-recovery.md) | [中文](../../zh/02-capabilities/06-freshness-and-index-recovery.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Freshness capability tells users which graph and index versions retrieval results correspond to. The system does not pretend stale indexes are fresh and does not let background refresh grow without bounds.

## User-visible Behavior

- `freshness` supports `allow-stale`, `wait-until-fresh`, and `graph-only`.
- Query, health, and index refresh responses return `index_cursors[*]`.
- `index_refresh.stale_reasons[*]` explains lag, failure, and last error by index family and scoped cursor.
- Ingest, query, index refresh, health, service doctor, and service startup share the bounded refresh queue.
- Each code retrieval hit carries a `staleness_hint` field alongside the legacy `stale` boolean. Current states are `{ "state": "fresh" }`, `{ "state": "pending_index" }`, and `{ "state": "stale" }`; `pending_index` means a matching refresh task is still queued, running, or retrying, so callers should read direct source before relying on that hit. Per-file timestamp payloads are intentionally omitted until the code graph stores file modification and indexed-at times.

## Competitive Features

Many RAG systems only say results exist. This system explains whether results are fresh, which backend lags, which scope is stale, whether a task dead-lettered, and whether explicit refresh failed because of queue capacity.

## Command/API Entry Points

```bash
relay-knowledge index refresh --kind bm25 --format json
relay-knowledge query SQLite --freshness wait-until-fresh --format json
relay-knowledge health --format json
```

## Degradation and Diagnostics

Common states include stale index, graph-only, backend unavailable, semantic/vector degraded, failed cursor, and dead letter. Diagnostic reconcilers do not automatically revive dead-letter tasks; only explicit retry or refresh paths do.

## File Watcher (fs.watch) Incremental Indexing

The resident service detects source file changes and checked-out Git commit advances for registered repositories. Both paths push durable code-index tasks; neither writes graph state directly from the watcher event loop.

### Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `RELAY_KNOWLEDGE_WATCHER_ENABLED` | `true` | Enable/disable file watching |
| `RELAY_KNOWLEDGE_WATCHER_DEBOUNCE_MS` | `3000` | Event debounce window (ms) |
| `RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS` | `5000` | Bounded periodic checked-out `HEAD` reconciliation interval (ms) |
| `RELAY_KNOWLEDGE_WATCHER_MAX_WATCH_DIRS` | `1024` | Maximum watched directories |
| `RELAY_KNOWLEDGE_WATCHER_HASH_CACHE_CAPACITY` | `4096` | Content hash cache capacity |

### How It Works

1. **Event detection**: Uses the `notify` crate for cross-platform (Linux inotify, macOS FSEvents, Windows ReadDirectoryChangesW) detection of file create/modify/delete events. `.git/HEAD`, refs, packed refs, and HEAD-log events are treated as low-latency commit hints
2. **Debounce**: Rapid consecutive file change events are merged within a configurable time window
3. **Content hash filtering**: FNV-1a content hash skips save operations with no actual content change
4. **Scope filtering**: Ignores ordinary `.git/`, `target/`, `node_modules/`, `__pycache__/` contents and binary files, while admitting the narrow Git ref hints above; it then applies each repository scope's path and language filters before queueing overlay work
5. **Initial-index guard**: Repositories are watched only after a completed, non-stale full index provides `last_indexed_scope_id`, so worktree overlays never create a partial first index or run over stale reconfiguration state
6. **Worktree task generation**: Changed source files produce `CodeIndexTaskSeed` records with `CodeIndexRequest` payloads in `WorktreeOverlay` mode; overlay fingerprints include the changed-path set and content generation
7. **Commit reconciliation**: At startup and each bounded interval, the watcher resolves the current `HEAD` and tree outside the async hot path. This authoritative backstop covers linked worktrees plus missed or coalesced native events. If HEAD advanced, it pins the last published clean base and resolved head/tree into an `Incremental` task. A stable per-repository/ref/filter fingerprint coalesces repeat hints while that slot is unfinished
8. **Durable publication**: Full, manual incremental, and watcher incremental tasks use the same queue, attempt-scoped lease, bounded retry/backoff, dead-letter, one-writer-per-repository claim, and publication ordering. Each claim advances a repository-local generation that is checked inside every publishing SQLite transaction, so an expired detached attempt cannot commit after takeover. Full rebuilds additionally expose batch checkpoints; bounded incremental/worktree attempts publish in one snapshot transaction and expose task state rather than a per-path checkpoint. Startup and later ticks replay lag after a crash; the old fresh scope remains readable until publication completes
9. **Repository lifecycle sync**: Repositories registered, refreshed, or removed while the service is running are watched, updated, or unwatched through the watcher command channel; multiple repository scopes may share one root directory while remaining distinct targets, and watch failures degrade diagnostics instead of silently mutating only in-memory state

### Status Monitoring

Watcher state is exposed through the `service status` API with the following diagnostics:

- `state`: disabled / active / degraded / failed
- `enabled`: configured watcher switch, including a disabled runtime with no live watcher object
- `commit_reconcile_interval_ms`: effective managed HEAD reconciliation interval
- `watched_repository_count`: number of watched repositories
- `total_events_received`: total file change events received
- `total_events_filtered`: events filtered out
- `total_index_tasks_queued`: incremental index tasks generated
- `total_commit_reconciliations`: repositories checked by commit reconciliation
- `total_commit_tasks_queued`: durable commit-update tasks accepted
- `total_commit_reconcile_failures`: bounded Git resolution or queue failures
- `total_events_dropped`: events dropped when the bounded debounce channel is full or closed
- `degraded_reason`: reason for degradation (e.g., watch directory limit exceeded)

### Resource Protection

- `max_watch_dirs` cap prevents inotify/fd exhaustion
- The debounce event channel and watcher command channel are bounded
- The content hash cache advances only after a matching worktree-overlay task is durably queued, so transient queue failures remain retryable by the next matching file event
- Queue failures set the watcher to degraded while preserving existing worker retry/dead-letter behavior for tasks that were durably accepted
- Watch failures degrade gracefully (Degraded state) without affecting query hot paths
- Unsupported platforms auto-disable (Disabled state)

### Retention and Recovery

Successful publication computes a protected set before deletion: the union of the active scope and a rolling window of the two latest successful publications (normally including active), the latest successful incremental predecessor, the clean base of any active worktree overlay, plus every unfinished task target/base and repository-set pin. An unprotected scope is atomically marked `retiring` and excluded from reads. Each maintenance transaction then advances one durable scope-GC phase, whose physical deletion of older code facts, FTS/search rows, software projection rows, checkpoints, workspace state, or scope metadata is capped at 512 rows in aggregate across affected application tables. Same-tree commits share content under a bounded 256-row commit-alias window. Finished task history is bounded per repository to 128 succeeded and 64 failed/dead-letter/cancelled rows, preserving the latest success for each retained scope. Status exposes maintenance progress and errors; a pruned ref requires a full reindex.

This publication barrier refreshes the code repository facts, their FTS/search documents, and the software global-model projection derived from that source scope. It does not claim atomic publication of the repository-agnostic Knowledge Graph or separate semantic/vector generations.

## Related Architecture Chapters

- [Derived Indexes and Freshness](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [Background Service, Recovery, and Self-Healing](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)

---

Navigation: Previous: [5. Hybrid Retrieval Advantage](05-hybrid-retrieval-advantage.md) | Next: [7. Multimodal Evidence Capability](07-multimodal-evidence-capability.md)
