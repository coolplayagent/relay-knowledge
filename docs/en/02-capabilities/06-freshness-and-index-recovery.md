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

The resident service detects source code file changes for registered repositories and automatically pushes worktree-overlay index tasks to the durable code-index task queue.

### Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `RELAY_KNOWLEDGE_WATCHER_ENABLED` | `true` | Enable/disable file watching |
| `RELAY_KNOWLEDGE_WATCHER_DEBOUNCE_MS` | `3000` | Event debounce window (ms) |
| `RELAY_KNOWLEDGE_WATCHER_MAX_WATCH_DIRS` | `1024` | Maximum watched directories |
| `RELAY_KNOWLEDGE_WATCHER_HASH_CACHE_CAPACITY` | `4096` | Content hash cache capacity |

### How It Works

1. **Event detection**: Uses the `notify` crate for cross-platform (Linux inotify, macOS FSEvents, Windows ReadDirectoryChangesW) detection of file create/modify/delete events
2. **Debounce**: Rapid consecutive file change events are merged within a configurable time window
3. **Content hash filtering**: FNV-1a content hash skips save operations with no actual content change
4. **Path filtering**: Automatically ignores `.git/`, `target/`, `node_modules/`, `__pycache__/` directories and binary files
5. **Initial-index guard**: Repositories are watched only after a completed full index provides `last_indexed_scope_id`, so worktree overlays never create a partial first index
6. **Incremental task generation**: Changed files produce `CodeIndexTaskSeed` records with `CodeIndexRequest` payloads in `WorktreeOverlay` mode, entering the same durable queue used by code-index workers, leases, retries, and dead-letter handling; overlay fingerprints include the changed-path set and content generation so a later save cannot be deduplicated into an older queued or running task
7. **Repository lifecycle sync**: Repositories registered, refreshed, or removed while the service is running are watched, updated, or unwatched through the watcher command channel; watch failures degrade diagnostics instead of silently mutating only in-memory state

### Status Monitoring

Watcher state is exposed through the `service status` API with the following diagnostics:

- `state`: disabled / active / degraded / failed
- `watched_repository_count`: number of watched repositories
- `total_events_received`: total file change events received
- `total_events_filtered`: events filtered out
- `total_index_tasks_queued`: incremental index tasks generated
- `total_events_dropped`: events dropped when the bounded debounce channel is full or closed
- `degraded_reason`: reason for degradation (e.g., watch directory limit exceeded)

### Resource Protection

- `max_watch_dirs` cap prevents inotify/fd exhaustion
- The debounce event channel and watcher command channel are bounded
- Queue failures set the watcher to degraded while preserving existing worker retry/dead-letter behavior for tasks that were durably accepted
- Watch failures degrade gracefully (Degraded state) without affecting query hot paths
- Unsupported platforms auto-disable (Disabled state)

## Related Architecture Chapters

- [Derived Indexes and Freshness](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [Background Service, Recovery, and Self-Healing](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)

---

Navigation: Previous: [5. Hybrid Retrieval Advantage](05-hybrid-retrieval-advantage.md) | Next: [7. Multimodal Evidence Capability](07-multimodal-evidence-capability.md)
