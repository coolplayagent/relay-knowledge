# Chapter 12: Advanced Configuration

[English](../../en/01-user-guide/12-advanced-configuration.md) | [中文](../../zh/01-user-guide/12-advanced-configuration.md)

This chapter is a reference for environment variables and configuration layers. Normal local use does not require these variables. Use this chapter when isolating runtime directories, debugging network budgets, exposing MCP services, connecting external embedding workers, or reproducing CI issues.

## 12.1 Configuration Layers

The default `relay-knowledge` path is zero-config:

- Local SQLite storage.
- Platform default runtime directories.
- Local deterministic semantic/vector read models.
- Local HTTP listen address and conservative QoS defaults.
- MCP writes, remote listening, and silent updates disabled by default.

Advanced configuration is grouped by purpose:

| Layer | Purpose | Examples |
| --- | --- | --- |
| Basic | Daily CLI arguments | `--source`, `--limit`, `--freshness`, `--format`, `--remote` |
| Advanced | Retrieval, network, QoS, MCP policy | embedding backend, request timeout, scope allow-list |
| Deployment | Installation, service manager, remote access | systemd, Windows Service, launchd, service dir |
| Diagnostic | CI, failure reproduction, temporary isolation | one-off home dir, browser test paths |

## 12.2 Runtime Directories

Remote service access can be supplied once with `--remote http://host:8791`, or set in an automation profile as `RELAY_KNOWLEDGE_REMOTE_BASE_URL=http://host:8791`. The variable affects supported code repository index, incremental update, scope preview, status, query, feature-flag, impact, report, and software projection commands and blocks local fallback for `repo index --reset`, `repo index-worker`, and remote-selected `repo update`; unrelated local commands still use local runtime directory resolution. Remote dispatch validates only the remote URL and outbound network settings before contacting the service, while full local runtime, storage, retrieval, and path validation is delayed until the command uses local state.

Prefer default directories. For isolated one-off experiments, set one root:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-demo \
  relay-knowledge status --format json
```

Only take over the full layout when necessary:

```text
RELAY_KNOWLEDGE_CONFIG_DIR
RELAY_KNOWLEDGE_DATA_DIR
RELAY_KNOWLEDGE_STATE_DIR
RELAY_KNOWLEDGE_CACHE_DIR
RELAY_KNOWLEDGE_LOG_DIR
RELAY_KNOWLEDGE_TEMP_DIR
RELAY_KNOWLEDGE_RUNTIME_DIR
RELAY_KNOWLEDGE_SERVICE_DIR
```

All overrides must be absolute paths and must not contain `..`.

`RELAY_KNOWLEDGE_DATA_DIR` selects the directory for both `relay-knowledge.sqlite`
and `stores/repositories/`. It takes precedence over `RELAY_KNOWLEDGE_HOME/data`,
which takes precedence over existing Windows LocalAppData storage and platform
defaults. New Windows installations use
`D:\relay-knowledge\users\<user-sid>\data`; the SID is obtained from the Windows process token
and remains stable when LocalAppData moves. New default directories require
protected ACLs; unsafe directories fail with an actionable error. Explicit HOME/DATA
settings that select this SID layout retain its original account policy, including
LocalSystem service startup checks. Overrides elsewhere retain operator-managed
permissions. Existing
Windows data directories remain selected on upgrade; conflicting old/new directories require an explicit override.
Other platforms retain their normal data directories. Lifecycle plans inspect
an existing catalog read-only and reject `single_sqlite` when active partitioned
shards exist. Service definitions preserve the selected topology alongside the
data directory; missing storage still permits plans and uninstall.
An empty override is an error. See [installation and migration](01-install-and-runtime.md#14-zero-config-defaults)
for PowerShell examples, persistent user settings, service configuration, and
moving an existing database safely.

## 12.3 Storage Topology

The default storage topology is `single_sqlite` and stores all runtime state in
the main SQLite database under the runtime data directory. Use the partitioned
topology only when you want repository code facts isolated into one SQLite file
per registered repository:

```bash
RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite \
  relay-knowledge repo register /path/to/repository --format json
```

`partitioned_sqlite` keeps global control state, durable tasks, leases, audit,
and graph facts in the main database. Repository files, symbols, references,
chunks, checkpoints, and scoped code queries use shard files under
`stores/repositories/` in the runtime data directory. Repository-set overlay
refresh still requires `single_sqlite` until cross-shard import/export
aggregation is implemented.

After the main database contains an active partitioned shard catalog,
`single_sqlite` refuses to open that runtime state. Keep
`RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite` enabled, or perform an
explicit rollback that removes the shard catalog and shard files first.
Shard catalog entries are relocatable: restores recompute shard paths from the
repository id and the current runtime data directory, so move the main database
and `stores/repositories/` together.

## 12.4 Retrieval Backends

The default is local deterministic read models. Enable external backend metadata only when an external worker writes derived read models under the same metadata contract:

```bash
RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external \
RELAY_KNOWLEDGE_VECTOR_BACKEND=external \
RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL=text-embed-3-small \
RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL=clip-vit-b32 \
RELAY_KNOWLEDGE_EMBEDDING_DIMENSION=1536 \
relay-knowledge index refresh --kind semantic --kind vector --format json
```

Optional provider worker tuning:

```text
RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE
RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS
RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY
```

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` and `RELAY_KNOWLEDGE_VECTOR_BACKEND` accept `local`, `external`, or `disabled`. External provider configuration describes metadata and the worker contract; the query hot path does not synchronously call an external embedding service.

Rerank defaults to local deterministic selection and does not need a remote service:

```text
RELAY_KNOWLEDGE_RERANK_BACKEND=local
RELAY_KNOWLEDGE_RERANK_MODEL=relay-local-deterministic-rerank-v1
RELAY_KNOWLEDGE_RERANK_CANDIDATE_MULTIPLIER=4
RELAY_KNOWLEDGE_RERANK_MAX_CANDIDATES=64
RELAY_KNOWLEDGE_RERANK_TIMEOUT_MS=100
```

`RELAY_KNOWLEDGE_RERANK_BACKEND` accepts `local`, `external`, or `disabled`. `external` currently preserves the provider contract and degrades to local rerank; the query hot path does not synchronously call a remote rerank model.

## 12.5 Network and QoS

Resident service and MCP Streamable HTTP use `net::http` and `net::qos` for network capability:

```text
RELAY_KNOWLEDGE_HTTP_BIND
RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS
RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS
RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES
RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS
RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS
RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH
```

Proxy selection checks `HTTPS_PROXY`, `HTTP_PROXY`, then `ALL_PROXY` (including their lowercase forms), and inherits `NO_PROXY` and `SSL_VERIFY`. Authorities such as `host:port`, IPv4 addresses, and `[IPv6]:port` receive an `http://` prefix; existing HTTP(S) URLs are preserved. Invalid proxies (for example `http://`, invalid ports, or unsupported schemes) are ignored and connections go directly, with TLS verification policy preserved. `runtime.http_proxy_warning` in status/health and the `http_proxy` warning in setup doctor expose an `invalid_proxy` diagnostic without the original address or credentials. Local commands remain available. Outbound clients use the resolved policy without reading the rejected environment proxy again. Deployments requiring a proxy should correct configuration whenever this warning appears. Business modules do not read process environment directly.

`RELAY_KNOWLEDGE_HEALTH_STORAGE_BUDGET_MS` controls the health storage snapshot budget: 500ms by default, with a supported range of 1–30000ms. Try 5000 for a large database; `runtime.health_storage_budget_ms` reports the effective value. The budget covers snapshot and topology reads, including legacy fallback; initial database open and schema initialization remain startup work. Timeout responses retain `healthy=false`, `metadata.stale=true`, and `storage_busy`. Cancelled SQLite diagnostic queries release their read connections without increasing SQLite busy timeouts.

The health budget also covers model-profile loading and response assembly after a successful storage snapshot. After hot refresh, it uses the shortest HTTP request timeout ever issued by that network runtime to protect listeners retaining older configurations; restart the managed service to raise this ceiling. For partitioned storage, `last_maintenance_at_ms` is the oldest database maintenance time; missing history or a failed read makes it absent and produces inspection advice. The Web Runtime page displays proxy warnings, the effective budget, degradation reasons, and maintenance recommendations.

Health profile reads share one worker across the service and its clones, with no pending queue. After a response timeout, the worker retains admission until the underlying file read actually returns. Later probes use cached or uninspected summaries and mark the in-progress read and possible staleness in `runtime.model_profiles.error`. Completion updates the cache and releases admission. For a persistently hung read, inspect the mount/filesystem and restart through the service manager; repeated probes do not start additional reads.

The effective budget is the smaller of the configured value and `HTTP request timeout - min(100ms, request timeout / 2)`, including after network configuration refresh, leaving time for fallback JSON. Cached fallback reuses its model-profile summary; uncached fallback marks profiles as uninspected, without reading files. Catalog discovery and control/shard totals also use cancellable SQLite reads. Proxy warnings do not block setup doctor `configuration_ready`.

```powershell
$env:HTTPS_PROXY = "proxy.example:8080"
$env:RELAY_KNOWLEDGE_HEALTH_STORAGE_BUDGET_MS = "5000"
relay-knowledge health --format json
```

```bash
RELAY_KNOWLEDGE_HEALTH_STORAGE_BUDGET_MS=5000 relay-knowledge health --format json
```

`health.storage_recommendations` suggests checkpoint or service investigation for an observed WAL of at least 256MiB, maintenance older than seven days, or a recorded maintenance failure. Missing or timed-out snapshots only produce inspection advice; a timeout does not establish WAL pressure, and cached values remain stale. Start with `relay-knowledge service doctor --format json` and managed-service maintenance logs, then schedule a maintenance window for a WAL checkpoint. Health checks never run checkpoints or delete WAL files. Connectors should distinguish `runtime.http_proxy_warning` (configuration degradation) from `storage_busy`/`storage_cold` in `degraded_reason` (unavailable storage).

Version notices also use `net::http` and cache results under the runtime cache:

```text
RELAY_KNOWLEDGE_UPDATE_CHECK_ENABLED
RELAY_KNOWLEDGE_UPDATE_SOURCES
RELAY_KNOWLEDGE_UPDATE_CHECK_INTERVAL_MS
RELAY_KNOWLEDGE_UPDATE_GITHUB_REPO
```

By default, stable-version checks are enabled against both GitHub Releases and
crates.io with a 24-hour cache interval. Disabling this capability only stops
notices; `relay-knowledge version` still prints the local binary version. Release
metadata response bodies are capped by `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES`.
When update checks are disabled, source, repository, and interval overrides are
ignored so a notice-only setting cannot block runtime loading.

Non-loopback HTTP binds should also configure MCP remote-client policy and origin/scope restrictions. QoS budget is admission control, not authentication; it limits connections, in-flight requests, queue depth, timeouts, and overload behavior. Runtime budget expirations for MCP and local ACP work are included in the QoS timeout diagnostics. MCP requests use the queued admission path, so `qos_queued_total` and the matching Prometheus metric count work admitted through that queue-budget gate even when the request can enter active execution immediately.

## 12.6 MCP Policy

Complete MCP policy variables:

```text
RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED
RELAY_KNOWLEDGE_MCP_ENDPOINT
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES
RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE
RELAY_KNOWLEDGE_MCP_MAX_LIMIT
RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES
RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS
```

Default policy is read-only and local-first. Remote listening and unspecified scope both require explicit enablement. Registered code repository aliases can enter a process-local dynamic allow-list on first MCP access; unknown scopes still require `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES`.

## 12.7 Workers, Audit, and OTLP

Background workers and agent audit:

```text
RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT
RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT
RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES
RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH
```

`RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` controls the code-index worker pool
used by `service run`. The default is 2 and the runtime caps the value so
multiple index tasks can progress independently while SQLite writes still pass
through the single-writer lane.

`RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES` bounds repositories with
published code indexes that are not members of a user-managed repository set.
The default is 10 and the value must be in `1..=i64::MAX`, matching SQLite's
persistent integer range. Automatic-workspace sets do
not exempt a repository from this bound. Exceeding the bound schedules the
oldest eligible repository for durable phased index cleanup; registration,
aliases, queued/running tasks, and newly published scopes remain intact.

OTLP:

```text
RELAY_OTEL_ENDPOINT
RELAY_OTEL_TRACES
RELAY_OTEL_METRICS
RELAY_OTEL_EXPORT_TIMEOUT_MS
RELAY_OTEL_SERVICE_ENVIRONMENT
```

Behavior is described in [Chapter 10](10-workers-proposals-audit.md) and [Chapter 11](11-observability-and-telemetry.md).

## 12.8 Setup Interfaces

Advanced configuration does not need to be assembled manually from docs. The CLI provides two read-only setup entry points:

```bash
relay-knowledge setup doctor
relay-knowledge setup profile local
relay-knowledge setup profile agent-readonly
relay-knowledge setup profile service
relay-knowledge setup profile external-embedding
```

`setup doctor` checks runtime directories, network/QoS budgets, retrieval backend metadata, MCP policy, service directories, and worker budgets, returning `configuration_ready`, `live_health_checked=false`, `live_health_commands`, and `recommended_actions`. It does not open SQLite, migrate schema, or refresh indexes.

`setup profile` prints recommended environment variables, commands, and safety notes. It does not write `.env`, modify shell profiles, or run service-manager installation.
