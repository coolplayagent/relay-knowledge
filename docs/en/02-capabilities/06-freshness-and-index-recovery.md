# Freshness and Index Recovery

[English](./06-freshness-and-index-recovery.md) | [中文](../../zh/02-capabilities/06-freshness-and-index-recovery.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Freshness capability tells users which graph and index versions retrieval results correspond to. The system does not pretend stale indexes are fresh and does not let background refresh grow without bounds.

## User-visible Behavior

- `freshness` supports `allow-stale`, `wait-until-fresh`, and `graph-only`.
- Health and index refresh responses return `index_cursors[*]`.
- `index_refresh.stale_reasons[*]` explains lag, failure, and last error by index family and scoped cursor.
- Ingest, query, index refresh, health, service doctor, and service startup share the bounded refresh queue.

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

## Related Architecture Chapters

- [Derived Indexes and Freshness](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [Background Service, Recovery, and Self-Healing](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)

---

Navigation: Previous: [5. Hybrid Retrieval Advantage](05-hybrid-retrieval-advantage.md) | Next: [7. Multimodal Evidence Capability](07-multimodal-evidence-capability.md)
