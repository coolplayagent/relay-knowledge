# Chapter 9: Resident Service

[English](../../en/01-user-guide/09-resident-service.md) | [中文](../../zh/01-user-guide/09-resident-service.md)

The resident service hosts Web, API, MCP, the startup reconciler, and operational entry points. Development machines can run it in the foreground; long-running background operation must use the platform service manager.

Current service topologies are `embedded_cli`, `resident_single_process`, and `resident_partitioned_sqlite`. The first two use one runtime database; with `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite`, control state remains in the primary database while code-repository data moves into per-repository shards. Future split workers may run only after claiming durable tasks through the control plane; unmanaged background loops are not supported.

## 9.1 Foreground Service

Start MCP Streamable HTTP:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --mcp streamable-http
```

Start same-port Web/API/MCP:

```bash
./build.sh
./run.sh start --port 8791 --daemon
```

Underlying command:

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
target/release/relay-knowledge service run --web --mcp streamable-http
```

`service run` runs the startup index reconciler before accepting resident adapter requests when possible, then acts as the resident master for durable code-index and repository-set overlay refresh workers. The master owns configuration, startup lease recovery, bounded worker pool startup, queue supervision, and graceful shutdown. Code-index workers only claim leased tasks and execute bounded batches. The code-index pool defaults to 2 workers, is configured with `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT`, and is capped at 8. Without MCP or Web enabled, the command still waits as a foreground service for a shutdown signal.

Use `relay-knowledge service status --format json` to inspect `storage` and `code_index_workers`. `storage` reports the current topology, primary database path, `partitioned_sqlite` shard directory, active/staged/missing shard counts, runtime state paths, and missing-shard degraded reasons. `code_index_workers` reports configured worker count, active worker slots, queue depth, queued/running/retrying/dead-letter task counts, running leases, and last error. These diagnostics explain whether the master is idle, saturated, retrying work, waiting for another repository writer lease, or missing partitioned data-plane shard files.

HTTP `/api/health` and CLI `health` are liveness-safe entrypoints: they take a short-budget read-only snapshot, do not queue index refresh work, and do not wait for large repository indexing to finish. If the storage read lane is busy, health returns a cached or minimal degraded response with `storage_busy`, stale metadata, or a degraded reason. Normal code queries are not excluded by this behavior; `allow-stale` queries read the latest compatible completed committed scope while the target ref and filters are still indexing, and `wait-until-fresh` is the mode that requires the target scope to be finalized.

Stable external control-plane HTTP remains preview-scoped and currently exposes only read-only routes: `/api/v1/control/status`, `/api/v1/control/health`, `/api/v1/control/service/status`, and `/api/v1/control/storage/topology`. These routes reuse the shared API types used by CLI/Web/MCP and do not synchronously run indexing, migrations, or shard repair. When storage has not already been opened, the control health and service-status routes return graph-zero diagnostics from runtime configuration and bounded read-only topology probes instead of opening SQLite. Topology diagnostics also report active `partitioned_sqlite` shard catalogs discovered under a `single_sqlite` runtime so rollback or topology misconfiguration is visible before storage open fails. Service-status backlog fields are count diagnostics and must not materialize unbounded proposal or task lists during polling.

Remote code-repository CLI commands use stable endpoints on the same resident HTTP service: `POST /api/v1/code/repositories/{alias}/index`, `POST /api/v1/code/repositories/{alias}/scope/preview`, `GET /api/v1/code/repositories/{alias}/status`, `POST /api/v1/code/repositories/{alias}/query`, `POST /api/v1/code/repositories/{alias}/feature-flags`, `POST /api/v1/code/repositories/{alias}/impact`, `GET /api/v1/code/repositories/{alias}/report`, and `POST /api/v1/code/repositories/{alias}/software`. These endpoints reuse shared `api` request/response types and return stable `ApiError` JSON on failure. Non-loopback binds still require the remote-client policy to be enabled explicitly; QoS, request timeout, and body limits are shared with the Web/API service.

## 9.2 Service Run in Web

The Web service run operation only returns the current service runtime snapshot through `/api/web/operations/execute`. It is used to inspect the configuration and MCP state that would be used. Actual resident services must be started by CLI, `run.sh`, or the platform service manager.

## 9.3 Service Manager

Service manager v1 generates platform definitions and staged lifecycle plans. Dry-run is the default; explicit `service lifecycle <action> --execute` runs local file steps and platform service-manager commands. JSON API callers may send `execute: true` without also sending `dry_run: false`; explicit `dry_run: true` remains a dry-run request. Failed execution returns an operation error with the failed step id instead of wrapping a report with `failed_step_id` in a successful response.

```bash
relay-knowledge setup profile service --format json
relay-knowledge service plan install --format json
relay-knowledge service lifecycle install --dry-run --format json
relay-knowledge service definition write --format json
```

Linux returns a systemd user service plan, macOS returns a launchd plist plan, and Windows returns service XML/PowerShell planning output. Runtime state, graph databases, indexes, audit, and worker queues still use platform data/state/log/cache directories resolved by `paths`; they are not written to the release extraction directory.

When `partitioned_sqlite` is enabled, service doctor, backup, migration, and uninstall confirmation must cover both the primary database and the `stores/repositories/` shard directory. Moving only the primary database leaves code facts invisible and is not a successful migration or rollback.

`service plan install|upgrade|rollback|uninstall --format json` includes `runtime_state_paths`, `lifecycle_steps`, `rollback_steps`, `permission_requirements`, `package_manifest_checks`, the service name, the binary path, the service definition path, and the lifecycle checkpoint path. With `partitioned_sqlite`, it also includes the shard directory and adds a `warnings` entry that backup, migration, rollback, and uninstall confirmation must cover both the primary database and shard directory.

Execution remains explicit:

```bash
relay-knowledge service lifecycle install --execute --format json
relay-knowledge service lifecycle upgrade --execute --target-version 1.2.3 --install-dir /opt/relay-knowledge --format json
relay-knowledge service lifecycle rollback --execute --install-dir /opt/relay-knowledge --format json
relay-knowledge service lifecycle uninstall --execute --format json
```

If a lifecycle execution stage fails, the execution report records completed steps, failed step id, rollback steps attempted, and whether rollback completed. Rollback is only marked complete when every selected rollback step succeeds; failures before any mutating lifecycle step do not stop or uninstall an existing service. Installs with an explicit `--install-dir` reject an existing target binary instead of overwriting it; upgrades checkpoint an existing target binary and remove the copied binary during rollback when no prior binary backup existed. External service-manager and doctor commands are time-bounded so hung child processes return an execution report instead of waiting forever. The uninstall path removes service registration and generated service definitions, but runtime data stays in `runtime_state_paths` unless the user explicitly removes it.

## 9.4 Silent Update Operator

View, pause, or resume the background update operator:

```bash
relay-knowledge service operator status --format json
relay-knowledge service operator pause
relay-knowledge service operator resume
```

Silent updates must be user-configurable, observable, and reversible. They may refresh graph data and derived indexes only within authorized scopes, and they expose freshness, stale, paused, degraded, and failure states.

## 9.5 Split Worker Preview

`relay-knowledge service worker run [--task-id <id>] --format json` is the preview entrypoint for process-level split workers. It claims at most one durable code-index task, runs only after holding an attempt-scoped lease, and completes or fails through the same storage contract. If no task is claimed, the lease has expired, or the attempt does not match, it cannot write a successful result. This command does not replace the platform service manager and does not provide an unmanaged background loop.

## 9.6 Running Guidance

For short development checks, prefer foreground commands or `run.sh`:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
```

For long-running background operation, use `service plan` to inspect the staged plan and `service lifecycle ... --execute` only when the paths, permissions, and rollback plan are acceptable. Do not replace systemd, Windows Service, or launchd with unmanaged CLI loops. Runtime data, logs, caches, worker queues, and dead-letter data must stay in `paths`-managed directories, not the release extraction directory or repository directory.
