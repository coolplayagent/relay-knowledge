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
| Basic | Daily CLI arguments | `--source`, `--limit`, `--freshness`, `--format` |
| Advanced | Retrieval, network, QoS, MCP policy | embedding backend, request timeout, scope allow-list |
| Deployment | Installation, service manager, remote access | systemd, Windows Service, launchd, service dir |
| Diagnostic | CI, failure reproduction, temporary isolation | one-off home dir, browser test paths |

## 12.2 Runtime Directories

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

## 12.3 Retrieval Backends

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

## 12.4 Network and QoS

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

Proxy and certificate verification settings inherit `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY`, and `SSL_VERIFY`. Business modules do not read process environment directly.

Non-loopback HTTP binds should also configure MCP remote-client policy and origin/scope restrictions. QoS budget is admission control, not authentication; it limits connections, in-flight requests, queue depth, timeouts, and overload behavior.

## 12.5 MCP Policy

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

## 12.6 Workers, Audit, and OTLP

Background workers and agent audit:

```text
RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT
RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT
RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH
```

OTLP:

```text
RELAY_OTEL_ENDPOINT
RELAY_OTEL_TRACES
RELAY_OTEL_METRICS
RELAY_OTEL_EXPORT_TIMEOUT_MS
RELAY_OTEL_SERVICE_ENVIRONMENT
```

Behavior is described in [Chapter 10](10-workers-proposals-audit.md) and [Chapter 11](11-observability-and-telemetry.md).

## 12.7 Setup Interfaces

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
