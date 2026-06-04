# Service Deployment, Control Plane, and Data Plane

[English](../../en/03-architecture-specs/22-service-deployment-control-data-plane.md) | [中文](../../zh/03-architecture-specs/22-service-deployment-control-data-plane.md)

> Document version: 1.0
> Date: 2026-06-04
> Scope: Volume 3 architecture and algorithm whitepaper; GitHub issue #250

## 1. Design Conclusion

The `relay-knowledge` service deployment path is SQLite-first control-plane/data-plane separation, not an immediate dependency on external graph databases, message queues, or a Kubernetes operator. v1 preserves the local-first single-binary experience while making the current `service run`, HTTP `/api/*`, MCP, QoS, durable worker queues, operator state, and `partitioned_sqlite` topology the explicit service foundation.

The control plane owns configuration, authorization, APIs, task leases, audit, runtime status, topology catalogs, upgrade/rollback, diagnostics, and the resident master that supervises bounded worker pools. The data plane owns graph facts, code facts, derived indexes, query execution, and repository shards. Every interface must enter through application services and storage traits; Web, MCP, CLI, and workers must not directly access SQLite shards, external backends, or index files.

## 2. Competitive Technology Findings

Competitive systems show the direction, but they do not change the v1 default:

| Category | Examples | Useful capability | relay-knowledge decision |
| --- | --- | --- | --- |
| Graph database clusters | Neo4j, NebulaGraph, Memgraph | Separate database/server management, query/storage separation, read scaling, recovery | Borrow the boundary model only; adapters must implement the existing storage contract |
| Multi-model databases | SurrealDB | Single binary, embedded-to-distributed deployment, document/graph/vector/full-text integration | Preserve the single-binary experience; do not leak a multi-model query language into domain/API types |
| Vector databases | Qdrant, Milvus, Weaviate, LanceDB, pgvector | Vector shards, payload filtering, BM25+vector hybrid search, split index/query nodes | Treat as semantic/vector read-model adapter candidates; they cannot replace graph version or freshness contracts |
| Event and stream platforms | NATS JetStream, Kafka/Redpanda | Persistent messages, replay, consumers, Raft metadata/control planes | v1 keeps SQLite durable tasks; future event transport must not replace the mutation log |
| Workflow runtimes | Temporal | Service/worker separation, event history, recovery after failures | Borrow durable execution semantics only; tasks still use attempt-scoped leases, checkpoints, and dead letters |
| Embedded/edge storage | SQLite, libSQL/Turso, RocksDB-style engines | Local-first operation, replication options, low installation cost | SQLite remains the v1 default; remote or replicated backends must preserve backup, migration, doctor, and uninstall semantics |

References:

- Neo4j Operations Manual: <https://neo4j.com/docs/operations-manual/current/clustering/introduction/>
- NebulaGraph architecture: <https://docs.nebula-graph.io/3.8.0/1.introduction/3.nebula-graph-architecture/1.architecture-overview/>
- SurrealDB documentation: <https://surrealdb.com/docs/surrealdb>
- Qdrant overview: <https://qdrant.tech/documentation/overview/>
- Milvus overview: <https://milvus.io/docs/overview.md>
- NATS JetStream: <https://docs.nats.io/nats-concepts/jetstream>
- Kafka KRaft overview: <https://docs.confluent.io/platform/current/kafka-metadata/kraft.html>
- Temporal platform documentation: <https://docs.temporal.io/temporal>

## 3. Deployment Topologies

v1 supports and documents four topologies:

| Topology | Control plane | Data plane | Use case |
| --- | --- | --- | --- |
| `embedded_cli` | Application service inside the CLI process | `single_sqlite` | Temporary commands, tests, one-shot developer operations |
| `resident_single_process` | `service run` HTTP/Web/MCP/operator/master-worker pools | `single_sqlite` | Default resident service with minimal operations cost |
| `resident_partitioned_sqlite` | Primary SQLite control database | Per-repository SQLite shards | Local scaling for large and multi-repository workloads |
| `split_worker_preview` | Resident control service | Independent worker processes claim tasks before work | Future process-level scale-out; writes without leases are forbidden |

Long-running background operation must be hosted by systemd, Windows Service, or launchd. `run.sh --daemon` is for development validation, not formal installation. Any split worker or future remote worker must claim a durable task through the control plane and hold an attempt-scoped lease before it can trigger data-plane writes.

## 4. Control Plane Responsibilities

Control-plane APIs must cover:

- runtime/config/status/health/doctor without running long tasks or blocking query hot paths.
- service manager plan, definition write, operator pause/resume/status.
- worker task queue, lease, retry, dead-letter, checkpoint, progress, and reset.
- code-index master-worker diagnostics, including configured workers, active worker slots, queue depth, running leases, and retry/dead-letter state.
- storage topology, shard catalog, backup/migration/rollback/uninstall diagnostics.
- repository register/index/status/report/set overlay refresh.
- audit, authorization identity, request id, trace id, QoS admission, and overload decisions.

New control-plane interfaces must first define shared `api` request/response types and application service methods, then map them to CLI, Web, MCP, or HTTP routes. Interface layers must not duplicate business logic, read the storage catalog directly, renew worker tasks directly, or bypass QoS. The current read-only control-plane HTTP preview exposes `/api/v1/control/status`, `/api/v1/control/health`, `/api/v1/control/service/status`, and `/api/v1/control/storage/topology`.

## 5. Data Plane Responsibilities

The data plane only executes reads and writes authorized and budgeted by the control plane:

- transactional consistency for graph facts, mutation log, graph version, and index cursors.
- code repository files, symbols, references, chunks, software projections, checkpoints, and scope queries.
- bounded queries for BM25, semantic, vector, code retrieval, and canvas read models.
- persistence and query primitives for external graph/vector/storage adapters.

The data plane must not own service lifecycle, user authorization, scope policy, operator pause/resume, task scheduling, dead-letter recovery, or upgrade decisions. External backends must not present missing dependencies, authorization gaps, stale indexes, or storage pressure as fresh success.

## 6. Storage Extension Contract

New backends must pass the same contract tests:

- Writes either commit facts, mutation log, graph version, and affected cursors together or roll back fully.
- Reads explicitly carry graph version, source scope, limit, freshness policy, and budget.
- Backends may change physical sharding, indexes, and query planning, but not the domain fact model, mutation log, freshness, degraded reasons, or error kinds.
- The `partitioned_sqlite` primary database and shard directory are one runtime state set; backup, migration, doctor, uninstall, and rollback cannot process only the primary database.
- Each repository has at most one active writer task; cross-process or cross-backend deployments must preserve durable leases.

## 7. API Extension Contract

Control-plane HTTP routes use `/api/*`, and same-origin Web operations continue to use `/api/web/operations/execute`. External control-plane APIs use `/api/v1/control/*` or an equivalent name. The current preview exposes only read-only status, health, service status, and storage topology diagnostics while keeping CLI JSON, Web, and MCP tool semantics compatible.

API responses must include metadata, warnings/degraded state, freshness/truncation, stable error kind, and trace context. Long-running operations return only task handles, checkpoints, and queryable status; they must not synchronously run unbounded indexing, unbounded scans, large external-provider batches, or shard migration.

## 8. Acceptance Criteria

- `single_sqlite` rejects a runtime database with an active shard catalog.
- `partitioned_sqlite` doctor/status, backup, migration, and uninstall plans cover both the control database and shard directory, with storage diagnostics reporting active/staged/missing shard counts.
- Split-worker preview claims durable code-index tasks through `service worker run [--task-id <id>]`; workers cannot complete, fail, or write when no task was claimed, the lease expired, or the attempt does not match.
- `health`, `service status`, and Web diagnostics return bounded degraded status while the data plane is busy.
- New graph/vector/event/workflow adapters enter only as implementation details under storage, retrieval, net, or worker boundaries and do not change domain/API semantics.

---

Navigation: Previous: [21. Software Global Domain Modeling Architecture](21-software-global-domain-modeling.md)
