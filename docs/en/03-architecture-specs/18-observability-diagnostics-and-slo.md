# Observability, Diagnostics, and SLO

[English](../../en/03-architecture-specs/18-observability-diagnostics-and-slo.md) | [中文](../../zh/03-architecture-specs/18-observability-diagnostics-and-slo.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Observability is not extra logging; it is the architecture control plane. Retrieval quality, index freshness, QoS overload, worker recovery, agent audit, and external provider degradation are diagnosable, measurable, and traceable.

## 2. Signal Model

| Signal | Content |
| --- | --- |
| Logs | Structured events, error classes, redacted context |
| Metrics | Queue depth, latency, freshness lag, drops, timeouts, hit counts |
| Traces | Request spans, retriever spans, storage spans, worker spans, adapter spans |
| Health | Service state, index state, provider state, QoS state, degraded reason |
| Audit | Agent/runtime identity, scope, action, decision, result metadata |

## 3. Trace Context

Every user request and background task carries a trace id. Context packs, audit events, worker tasks, mutation logs, and index cursors can be correlated through trace id or graph version.

## 4. SLO Candidates

- Query p95 latency stays within configured budget.
- Fresh scopes have zero stale lag.
- Worker dead-letter rate stays below threshold.
- QoS drops and timeouts carry explicit overload reasons.
- MCP/Web/CLI errors for the same operation use consistent classes.

## 5. Diagnostic Interfaces

CLI health, service doctor, Web diagnostics, MCP resources, and Prometheus metrics read the same diagnostic aggregation layer. UI may reorganize display but does not infer business state independently.

## 6. Acceptance Criteria

- Every degraded response states the degraded family, reason, and recovery entry point.
- Collector export failures do not interrupt local service operation.
- Diagnostics do not leak secrets, private endpoint tokens, or unauthorized paths.

---

Navigation: Previous: [17. Background Service, Recovery, and Self-Healing](17-background-service-recovery-and-self-healing.md) | Next: [19. Installation, Release, and Upgrade](19-installation-release-and-upgrade.md)
