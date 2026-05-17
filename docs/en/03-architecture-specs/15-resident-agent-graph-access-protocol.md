# Resident Agent Graph Access Protocol

[English](../../en/03-architecture-specs/15-resident-agent-graph-access-protocol.md) | [中文](../../zh/03-architecture-specs/15-resident-agent-graph-access-protocol.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

The resident process exposes the knowledge substrate as an auditable local service for agents and tools. The protocol layer is advanced because it provides tools, resources, prompts, sessions, cancellation, QoS, scope policy, and audit, instead of exposing raw CLI commands to agents.

## 2. MCP Capabilities

MCP Streamable HTTP exposes:

- Graph retrieval tools.
- Graph inspection tools.
- Authorized code query and code impact tools.
- Health, service status, and index status resources.
- Retrieval planning and code impact prompts.

MCP does not expose arbitrary index refresh, repository indexing, or filesystem traversal. Those operations require explicit user CLI/Web action.

## 3. Session and Transport

The server validates initialize and issues an unpredictable session id. Clients send initialized notification; later requests carry the session header and protocol version. Cancellation binds to the session and in-flight operation.

## 4. ACP / Local Adapter

ACP or local session adapters use the same unified API and expose progress, artifacts, cancellation, and context packs. They do not own separate business logic and do not bypass equivalent scope-policy authorization.

## 5. Result Shape

Agent-facing results include items, graph paths, structured facts, code artifacts, freshness, degraded state, budgets, truncation, audit id, and stable errors. All citable content has source provenance.

## 6. Acceptance Criteria

- Unauthorized agent requests are rejected before execution.
- Cancellation releases budget and writes audit events.
- MCP and ACP return the same application-service semantics without interface drift.

---

Navigation: Previous: [14. Open Agent Runtime Adapter Architecture](14-open-agent-runtime-adapter-architecture.md) | Next: [16. Unified API and Interface Architecture](16-unified-api-and-interface-architecture.md)
