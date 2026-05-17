# Unified API and Interface Architecture

[English](../../en/03-architecture-specs/16-unified-api-and-interface-architecture.md) | [中文](../../zh/03-architecture-specs/16-unified-api-and-interface-architecture.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

The unified API is the shared semantic boundary for CLI, Web, MCP, ACP, and future SDKs. Interface layers parse parameters, render output, stream progress, and map errors; they do not duplicate core business logic.

## 2. API Contract

Stable requests express at least operation, scope, freshness policy, budget, format, trace context, authorization identity, and idempotency key. Responses express data, metadata, warnings, degraded state, freshness, truncation, and stable errors.

## 3. Interface Responsibilities

| Interface | Responsibility |
| --- | --- |
| CLI | Scriptable commands, JSON output, explicit read/write effects |
| Web | Operation composers, diagnostics, settings, visualized results |
| MCP/ACP | Agent tool/resource/prompt/session access |
| Local SDK | Embedded calls to stable API without runtime-specific types |

## 4. Error Model

Errors use stable classes: invalid input, unauthorized scope, not found, stale index, timeout, cancelled, overloaded, degraded backend, storage unavailable, and internal. Interface layers may translate wording but not semantics.

## 5. Streaming and Cancellation

Long queries, indexing, impact analysis, and agent requests support progress events and cancellation. Cancellation is not an abnormal crash; it releases budgets, stops follow-up worker scheduling, and writes audit/metrics.

## 6. Acceptance Criteria

- The same operation returns compatible semantics through CLI JSON, Web API, and MCP tools.
- New UI code does not call storage or indexing traits directly.
- `help --format json` describes command paths, parameters, defaults, read/write effects, and examples.

---

Navigation: Previous: [15. Resident Agent Graph Access Protocol](15-resident-agent-graph-access-protocol.md) | Next: [17. Background Service, Recovery, and Self-Healing](17-background-service-recovery-and-self-healing.md)
