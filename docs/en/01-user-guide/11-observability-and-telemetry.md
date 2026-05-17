# Chapter 11: Observability and Telemetry

[English](../../en/01-user-guide/11-observability-and-telemetry.md) | [中文](../../zh/01-user-guide/11-observability-and-telemetry.md)

Observability explains retrieval quality, index freshness, QoS overload, worker recovery, agent audit, and external provider degradation. It does not change business results and should not be a hard dependency for service startup.

## 11.1 Local Diagnostics

Start with these entry points:

```bash
relay-knowledge status --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
relay-knowledge audit query --limit 50 --format json
```

Focus on graph version, index lag, refresh queue diagnostics, `index_refresh.stale_reasons`, runtime directories, HTTP bind, QoS budgets, agent protocol status, telemetry status, and degraded reason.

## 11.2 Prometheus Metrics

The MCP service exposes:

```text
GET /mcp/metrics
```

The endpoint returns a Prometheus text-format snapshot covering current graph version, index refresh queue depth, dead-letter count, QoS in-flight/queued request count, and stale state for each index. Requests still enter through the MCP router and QoS admission.

## 11.3 OTLP Configuration

The resident service can export traces and metrics to an OpenTelemetry Collector OTLP HTTP endpoint:

```text
RELAY_OTEL_ENDPOINT
RELAY_OTEL_TRACES
RELAY_OTEL_METRICS
RELAY_OTEL_EXPORT_TIMEOUT_MS
RELAY_OTEL_SERVICE_ENVIRONMENT
```

The default endpoint is `http://127.0.0.1:4318`. Traces use `/v1/traces` and metrics use `/v1/metrics`. When an endpoint already contains one signal path, the other signal is rewritten to the sibling path.

## 11.4 Enable OTLP

Start a local Collector first, then enable export:

```bash
RELAY_OTEL_ENDPOINT=http://127.0.0.1:4318 \
RELAY_OTEL_TRACES=true \
RELAY_OTEL_METRICS=true \
RELAY_OTEL_SERVICE_ENVIRONMENT=local \
relay-knowledge service run --web --mcp streamable-http
```

`RELAY_OTEL_EXPORT_TIMEOUT_MS` defaults to 5000 and is used to flush OTLP providers during service shutdown.

## 11.5 Degradation Semantics

Exporter initialization or export failures do not prevent service startup; errors are exposed as telemetry diagnostics. One failed signal does not block another signal, and trace exporter failure still keeps the local tracing fallback.

For troubleshooting, inspect `runtime.telemetry.last_error` in `service doctor --format json`. An unavailable Collector affects observability only; it does not mean graph retrieval is unavailable.
