# Storage Engine and Mutation Log

[English](../../en/03-architecture-specs/07-storage-engine-and-mutation-log.md) | [中文](../../zh/03-architecture-specs/07-storage-engine-and-mutation-log.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

The storage layer is a graph state machine, not a data-access helper. SQLite-first is the default product route because it provides local zero-dependency operation, transactions, WAL, FTS5, recursive CTEs, and low test cost. Business layers depend only on storage traits, not SQLite details.

## 2. Storage Boundary

```text
Application Services
        |
        v
Storage Facade: GraphStore, MutationLogStore, IndexStore, CodeGraphStore
        |
        +--> SQLite Adapter: WAL, transactions, FTS5, CTE
        +--> Future Adapters: SurrealDB, Neo4j, NebulaGraph, Memgraph
```

`domain` does not depend on SQL or connection pools; `retrieval` does not bypass the storage facade; `interfaces` do not duplicate storage logic.

## 3. Write Transaction

Graph writes follow a fixed flow:

```text
validate domain model
  -> begin transaction
  -> upsert evidence/entities/relations/claims/events/code facts
  -> append graph_mutations
  -> bump graph_version
  -> mark affected index cursors stale
  -> commit
  -> publish refresh work
```

Graph facts, mutation log entries, and graph version bumps are in the same transaction. Index refresh work is published only after commit.

## 4. Mutation Log

The mutation log is the spine of index recovery and audit. Each mutation includes at least graph version, affected scopes, affected entities, affected evidence, source hashes, fact kinds, index families, actor/runtime identity, and trace id.

Indexers consume mutation logs and scoped cursors; they do not scan the entire database to infer refresh work.

## 5. SQLite Runtime Model

SQLite operations may use a synchronous driver, but they are isolated behind blocking workers, dedicated connections, or pool boundaries and never occupy async executors. Writes use batch transactions; reads use prepared statements and compound indexes; FTS candidate windows apply scope/path/language filters before scoring.

## 6. Backend Evolution

Future graph database adapters implement the same contract tests. New backends do not change the domain fact model, mutation log semantics, or freshness contract; they change persistence and query primitives only.

## 7. Acceptance Criteria

- A storage write either commits facts, mutation, and version completely or rolls back completely.
- Index failure does not roll back graph writes, but it creates stale/degraded diagnostics.
- SQLite-specific optimizations do not leak into domain, API, or interface types.

---

Navigation: Previous: [6. Graph Fact Model and Versioning](06-graph-fact-model-and-versioning.md) | Next: [8. Derived Indexes and Freshness](08-derived-indexes-and-freshness.md)
