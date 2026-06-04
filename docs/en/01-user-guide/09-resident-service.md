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

`service run` runs the startup index reconciler before accepting resident adapter requests when possible, then drains the durable code-index queue and repository-set overlay refresh queue with bounded resident workers. Without MCP or Web enabled, the command still waits as a foreground service for a shutdown signal.

HTTP `/api/health` and CLI `health` are liveness-safe entrypoints: they take a short-budget read-only snapshot, do not queue index refresh work, and do not wait for large repository indexing to finish. If the storage read lane is busy, health returns a cached or minimal degraded response with `storage_busy`, stale metadata, or a degraded reason. Normal code queries are not excluded by this behavior; `allow-stale` queries read the latest compatible completed committed scope while the target ref and filters are still indexing, and `wait-until-fresh` is the mode that requires the target scope to be finalized.

## 9.2 Service Run in Web

The Web service run operation only returns the current service runtime snapshot through `/api/web/operations/execute`. It is used to inspect the configuration and MCP state that would be used. Actual resident services must be started by CLI, `run.sh`, or the platform service manager.

## 9.3 Service Manager

Service manager v1 generates platform definitions and command previews. It does not automatically run privileged installation commands:

```bash
relay-knowledge setup profile service --format json
relay-knowledge service plan install --format json
relay-knowledge service definition write --format json
```

Linux returns a systemd user service plan, macOS returns a launchd plist plan, and Windows returns service XML/PowerShell planning output. Runtime state, graph databases, indexes, audit, and worker queues still use platform data/state/log/cache directories resolved by `paths`; they are not written to the release extraction directory.

When `partitioned_sqlite` is enabled, service doctor, backup, migration, and uninstall confirmation must cover both the primary database and the `stores/repositories/` shard directory. Moving only the primary database leaves code facts invisible and is not a successful migration or rollback.

## 9.4 Silent Update Operator

View, pause, or resume the background update operator:

```bash
relay-knowledge service operator status --format json
relay-knowledge service operator pause
relay-knowledge service operator resume
```

Silent updates must be user-configurable, observable, and reversible. They may refresh graph data and derived indexes only within authorized scopes, and they expose freshness, stale, paused, degraded, and failure states.

## 9.5 Running Guidance

For short development checks, prefer foreground commands or `run.sh`:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
```

For long-running background operation, use `service plan` and `service definition write` to generate platform service-manager configuration. A user or installer should then perform the privileged installation step. Do not replace systemd, Windows Service, or launchd with unmanaged CLI loops. Runtime data, logs, caches, worker queues, and dead-letter data must stay in `paths`-managed directories, not the release extraction directory or repository directory.
