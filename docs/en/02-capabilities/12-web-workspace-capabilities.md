# Web Workspace Capabilities

[English](./12-web-workspace-capabilities.md) | [中文](../../zh/02-capabilities/12-web-workspace-capabilities.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

The Web workspace organizes local capability into a visual operations surface. It is not separate business logic; it reuses application services for diagnostics, query, ingestion, code, index, and service operations.

## User-visible Behavior

The Web workspace reads `/api/project/status`, `/api/health`, and `/api/web/operations/execute` from the same origin. It shows graph version, health, index lag, GraphRAG readiness, runtime budgets, refresh recovery, stale reasons, the operation composer, and execution results.

## Competitive Features

The Web operation composer generates typed command/request previews so users see payload and command semantics before execution. The Rust Web adapter reuses application services instead of duplicating CLI logic.

## Command/API Entry Points

```bash
relay-knowledge service run --web
curl http://127.0.0.1:8791/api/health
```

With `--mcp streamable-http`, Web and MCP routes share the same HTTP listener and QoS budget.

## Degradation and Diagnostics

Web execute requests are constrained by HTTP body budgets and loopback policy. When MCP is not enabled, Web still provides diagnostics and the local operation composer.

## Related Architecture Chapters

- [Unified API and Interface Architecture](../03-architecture-specs/16-unified-api-and-interface-architecture.md)
- [Observability, Diagnostics, and SLO](../03-architecture-specs/18-observability-diagnostics-and-slo.md)

---

Navigation: Previous: [11. Semantic/Vector Provider Backend](11-semantic-vector-provider-backend.md) | Next: [13. Agent Access Capabilities](13-agent-access-capabilities.md)
