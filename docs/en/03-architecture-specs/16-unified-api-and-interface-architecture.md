# Unified API and Interface Architecture

[English](../../en/03-architecture-specs/16-unified-api-and-interface-architecture.md) | [中文](../../zh/03-architecture-specs/16-unified-api-and-interface-architecture.md)

> Document version: 2.1
> Date: 2026-06-04
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

## 4. Control Plane API Surface

Service deployment control-plane APIs cover runtime/status/health/doctor, service manager plan, operator pause/resume/status, worker task/lease/dead-letter/checkpoint, storage topology and shard-catalog diagnostics, repository register/index/status/report, repository-set refresh, audit, authorization identity, QoS admission, and overload decisions.

New control-plane capabilities first define shared `api` request/response types and application service methods, then map to CLI, Web, MCP, or HTTP routes. Existing same-origin Web operations continue to use `/api/web/operations/execute`; external control-plane HTTP APIs use versioned `/api/v1/control/*` names. Code repository read APIs use versioned `/api/v1/code/repositories/{alias}/*` routes, including `/views` for graph-derived codebase understanding views. The current preview exposes read-only `status`, `health`, `service/status`, `storage/topology`, and code repository routes while keeping CLI JSON, Web, and MCP tool semantics compatible.

## 5. Error Model

Errors use stable classes: invalid input, unauthorized scope, not found, stale index, timeout, cancelled, overloaded, degraded backend, storage unavailable, and internal. Interface layers may translate wording but not semantics.

## 6. Streaming and Cancellation

Long queries, indexing, impact analysis, and agent requests support progress events and cancellation. Cancellation is not an abnormal crash; it releases budgets, stops follow-up worker scheduling, and writes audit/metrics.

## 7. Acceptance Criteria

- The same operation returns compatible semantics through CLI JSON, Web API, and MCP tools.
- New UI code does not call storage or indexing traits directly.
- `help --format json` describes command paths, parameters, defaults, read/write effects, and examples.
- New control-plane endpoints do not synchronously run unbounded indexing, unbounded scans, large external-provider batches, or shard migration; long-running work returns only task handles, checkpoints, and queryable status.

---

Navigation: Previous: [15. Resident Agent Graph Access Protocol](15-resident-agent-graph-access-protocol.md) | Next: [17. Background Service, Recovery, and Self-Healing](17-background-service-recovery-and-self-healing.md)
