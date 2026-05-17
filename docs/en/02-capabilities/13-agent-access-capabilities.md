# Agent Access Capabilities

[English](./13-agent-access-capabilities.md) | [中文](../../zh/02-capabilities/13-agent-access-capabilities.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Agent access lets external runtimes use the knowledge substrate safely. The system provides MCP Streamable HTTP and local ACP session adapters, but it does not take over planning, handoff, or final-answer generation.

## User-visible Behavior

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs relay-knowledge service run --web --mcp streamable-http
```

The default MCP address is `http://127.0.0.1:8791/mcp`. Clients initialize, store `Mcp-Session-Id`, send initialized notification, and then call tools.

## Competitive Features

MCP tools expose retrieve context, inspect graph, health, service status, index status, authorized code graph query, and authorized code impact. MCP resources expose service, health, index, and metrics read-only context; prompts provide retrieval and code-impact templates.

## Command/API Entry Points

MCP does not expose arbitrary repository indexing; users explicitly run `repo index` or `repo update`. The local ACP session adapter reuses the same retrieval contract and supports progress, cancellation, context artifacts, QoS admission, and audit.

## Degradation and Diagnostics

Without allowed scopes, graph tools reject unspecified scope unless explicitly allowed. Remote bind is rejected by default; non-loopback listeners require explicit remote-client policy.

## Related Architecture Chapters

- [Open Agent Runtime Adapter Architecture](../03-architecture-specs/14-open-agent-runtime-adapter-architecture.md)
- [Resident Agent Graph Access Protocol](../03-architecture-specs/15-resident-agent-graph-access-protocol.md)

---

Navigation: Previous: [12. Web Workspace Capabilities](12-web-workspace-capabilities.md) | Next: [14. Operations and Worker Capabilities](14-operations-and-worker-capabilities.md)
