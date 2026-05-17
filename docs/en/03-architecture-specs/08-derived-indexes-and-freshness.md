# Derived Indexes and Freshness

[English](../../en/03-architecture-specs/08-derived-indexes-and-freshness.md) | [中文](../../zh/03-architecture-specs/08-derived-indexes-and-freshness.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Derived indexes are valuable not only for recall speed but for explainable freshness. Every read model answers which scope, graph version, backend, model/dimension, stale state, and degraded reason it covers.

## 2. Index Families

| Index family | Purpose |
| --- | --- |
| `bm25` | Lexical recall, aliases, code symbols, chunks |
| `semantic` | Local semantic signatures or semantic summaries before external embedding |
| `vector` | Vector nearest neighbors, image/text embedding metadata |
| `graph_path` | Schema paths, entity neighborhoods, multi-hop candidates |
| `community` | Community summaries and global context |
| `code` | Code symbols, references, calls, imports, chunk FTS |

## 3. Freshness State Machine

```text
missing -> stale -> refreshing -> fresh
                 -> degraded
                 -> failed -> dead_letter
```

`fresh` means the index cursor covers the target graph version; it does not mean the facts are true. `degraded` may serve requests, but context packs explain missing families, backends, or scopes.

## 4. Refresh Scheduling

Refresh tasks come from mutation logs and explicit refresh requests. Scheduling satisfies:

- Queues are bounded and do not grow without limit.
- Tasks carry scope, family, target graph version, and source hash.
- Worker claims use leases and owners.
- Completion matches active lease, attempt, and target version.
- If graph version advances while a task runs, the cursor remains stale and follow-up work is enqueued.

## 5. Query Policy

Freshness policies include at least:

- `allow-stale`: return stale results with lag metadata.
- `wait-until-fresh`: wait for required indexes to reach the target version or return a stable timeout error.
- `require-fresh`: fail immediately on stale indexes without implicit refresh.

## 6. Acceptance Criteria

- `health` and context packs explain index lag, missing families, dead letters, and last errors.
- Explicit refresh enqueue failure returns a retryable error and never pretends to be fresh.
- Startup reconcilers replay missing refresh work from the mutation log.

---

Navigation: Previous: [7. Storage Engine and Mutation Log](07-storage-engine-and-mutation-log.md) | Next: [9. Hybrid Retrieval and Context Packing](09-hybrid-retrieval-and-context-packing.md)
