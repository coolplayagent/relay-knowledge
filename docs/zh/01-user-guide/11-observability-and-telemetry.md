# 第 11 章 可观测性与遥测

[中文](../../zh/01-user-guide/11-observability-and-telemetry.md) | [English](../../en/01-user-guide/11-observability-and-telemetry.md)

可观测性用于解释检索质量、index freshness、QoS overload、worker recovery、agent audit 和外部 provider degradation。它不改变业务结果，也不应成为服务启动的硬依赖。

## 11.1 本地诊断面

优先通过这些入口查看当前状态:

```bash
relay-knowledge status --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
relay-knowledge audit query --limit 50 --format json
```

重点关注 graph version、index lag、refresh queue diagnostics、`index_refresh.stale_reasons`、runtime directories、HTTP bind、QoS budgets、agent protocol status、telemetry status 和 degraded reason。

## 11.2 Prometheus Metrics

MCP 服务暴露:

```text
GET /mcp/metrics
```

该 endpoint 返回 Prometheus text 格式快照，覆盖当前 graph version、index refresh queue depth、dead-letter count、QoS in-flight/queued request count 和每个 index 的 stale 状态。请求仍通过 MCP router 和 QoS admission，不绕过网络预算。

## 11.3 OTLP 配置

常驻服务可把 traces 和 metrics 发送到 OpenTelemetry Collector 的 OTLP HTTP endpoint:

```text
RELAY_OTEL_ENDPOINT
RELAY_OTEL_TRACES
RELAY_OTEL_METRICS
RELAY_OTEL_EXPORT_TIMEOUT_MS
RELAY_OTEL_SERVICE_ENVIRONMENT
```

默认 endpoint 是 `http://127.0.0.1:4318`。启用 traces 后使用 `/v1/traces`，启用 metrics 后使用 `/v1/metrics`；当 endpoint 已经包含其中一个 signal path，另一个 signal 会改写到同级 path。

## 11.4 启用 OTLP

建议先让 Collector 在本机监听，再开启:

```bash
RELAY_OTEL_ENDPOINT=http://127.0.0.1:4318 \
RELAY_OTEL_TRACES=true \
RELAY_OTEL_METRICS=true \
RELAY_OTEL_SERVICE_ENVIRONMENT=local \
relay-knowledge service run --web --mcp streamable-http
```

`RELAY_OTEL_EXPORT_TIMEOUT_MS` 默认 5000，并用于服务停止时 flush OTLP providers。

## 11.5 降级语义

Exporter 初始化或导出失败不会阻止服务启动；错误会作为 telemetry diagnostics 暴露。单个 signal 失败不会阻断另一个 signal，trace exporter 失败时仍保留本地 tracing fallback。

排障时查看 `service doctor --format json` 的 `runtime.telemetry.last_error`。Collector 不可用只影响 observability，不表示 graph retrieval 不可用。
