# Service Deployment, Control Plane, and Data Plane Documentation Refresh Audit 2026-06-04

[English](../../en/06-verification/10-service-deployment-control-data-plane-2026-06-04.md) | [中文](../../zh/06-verification/10-service-deployment-control-data-plane-2026-06-04.md)

> Document version: 1.3
> Prepared: 2026-06-04
> Scope: GitHub issue #250, Book 3 Chapter 22, related architecture chapters, resident service user guide, README files, and this documentation sync record.

## 1. Refreshed Content

- Added Book 3 Chapter 22, "Service Deployment, Control Plane, and Data Plane."
- Turned issue #250 service deployment goals into the `embedded_cli`, `resident_single_process`, `resident_partitioned_sqlite`, and future `split_worker_preview` topologies.
- Synchronized storage, unified API, background service, installation/upgrade, resident service, and top-level README guidance so control-plane APIs, data-plane shards, split-worker leases, and backup/migration/uninstall boundaries are explicit.
- Follow-up implementation adds storage topology diagnostics to `service status`/`health`, read-only `/api/v1/control/*` preview routes, `service plan` runtime state paths and warnings, and the `service worker run [--task-id <id>]` split-worker preview CLI.
- After Codex review, the control-plane implementation was hardened so `health` returns degraded diagnostics instead of an API error when a partitioned shard is missing, storage topology diagnostics stay inside the same health timeout budget, and `/api/v1/control/service/status` uses a read-only status path that does not enqueue index refresh work.
- After Codex re-review, read-only control-plane behavior was hardened further: `/api/v1/control/status` no longer opens graph storage, `/api/v1/control/storage/topology` reads the catalog directly and returns graph-zero metadata for cold partitioned runtimes, and read-only service status does not recover code-index leases.
- After the latest Codex review, cold read-only control behavior was closed out: `/api/v1/control/health` and `/api/v1/control/service/status` no longer open cold storage, and topology diagnostics report active partitioned catalogs even when the runtime is configured as `single_sqlite`.
- A follow-up review tightened bounded diagnostics: cold control health and topology probes now return degraded diagnostics under the topology budget, service worker run responses report the post-run graph version, and service status counts proposed backlog rows without materializing the full proposal list.

## 2. Source Verification

The competitive analysis covers graph databases, multi-model databases, vector databases, event-stream platforms, workflow runtimes, and embedded/edge storage. References are official product documentation, mainly Neo4j, NebulaGraph, SurrealDB, Qdrant, Milvus, NATS JetStream, Kafka KRaft, and Temporal.

The sources inform architecture direction, not v1 dependency selection. The v1 default remains SQLite-first, local zero-dependency, async-first, QoS-protected, bounded-worker, durable-task-lease, and platform-service-manager based.

## 3. Index Consistency

- `docs/zh/README.md` and `docs/en/README.md` add the Book 3 Chapter 22 entry.
- Chapter 21 navigation now links forward to Chapter 22.
- `README.md` and `README.zh-CN.md` add service topology and control-plane/data-plane guidance.
- `docs/zh/README.md` and `docs/en/README.md` add the Appendix B.10 entry.

## 4. Verification Notes

Recommended verification commands:

```bash
rg -n "第 22 章|Chapter 22|22-service-deployment-control-data-plane|B.10" docs README.md README.zh-CN.md
rg -n "split_worker_preview|resident_partitioned_sqlite|control plane|data plane" docs/zh docs/en README.md README.zh-CN.md
rg -n "/api/v1/control|service worker run|runtime_state_paths|missing_shard_count" src docs/zh docs/en
cargo test --all-targets --all-features service_status_reports_partitioned_storage_diagnostics -- --nocapture
cargo test --all-targets --all-features control_service_status_does_not_queue_index_refresh_work -- --nocapture
cargo test --all-targets --all-features read_only_service_status_does_not_recover_expired_code_index_leases -- --nocapture
cargo test --all-targets --all-features cold_control_status_and_topology_do_not_open_partitioned_storage -- --nocapture
cargo test --all-targets --all-features cold_control_health_and_topology_bound_busy_catalog_probe -- --nocapture
cargo test --all-targets --all-features control_topology_reports_partitioned_catalog_under_single_config -- --nocapture
wc -l docs/zh/03-architecture-specs/22-service-deployment-control-data-plane.md \
  docs/en/03-architecture-specs/22-service-deployment-control-data-plane.md \
  docs/zh/06-verification/10-service-deployment-control-data-plane-2026-06-04.md \
  docs/en/06-verification/10-service-deployment-control-data-plane-2026-06-04.md
cargo test --all-targets --all-features
```

Rust implementation changes must pass focused storage/service/Web/CLI tests and the full `cargo test --all-targets --all-features` gate.
