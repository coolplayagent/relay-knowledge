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

MCP does not expose arbitrary index refresh, repository indexing, or filesystem traversal. MCP itself cannot start those writes; explicit CLI/Web actions remain available, while an enabled managed watcher may independently reconcile and publish checked-out commits through the durable queue.

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

## File diagnostics and content integrity (#393)

Indexed-version freshness and content coverage are independent. `freshness.state=fresh` means the requested version is indexed, not that every file parsed completely. Repository status, reports and query freshness include `content_integrity`: `state` (`complete`, `partial`, `unknown`), `degraded_file_count` (distinct paths) and `source_scope`. Missing fields in older responses mean `unknown`. The legacy `degraded_reason` remains diagnostic text and must not alone trigger reindexing.

```powershell
relay-knowledge repo diagnostics demo --ref HEAD --limit 50 --format json
relay-knowledge repo diagnostics demo --ref HEAD --path src --limit 50 --cursor $nextCursor --format json
```

Pages default to 50 diagnostics, capped at 200, ordered by path and message. Reuse the same ref and path filters with `next_cursor`; moving HEAD or publishing a more specific scope does not change the pinned snapshot. Continuations read scope metadata and diagnostic rows from the same storage transaction and validate the repository and resolved commit. Cursors bind normalized path filters by a fixed-size fingerprint, so large accepted filters do not produce unusable continuations. A removed snapshot produces an error. `repo report` retains its 20-entry summary and exposes `degradation_summary_truncated` and `diagnostics_command`. Healthy hits do not prove full coverage: missing facts can affect omitted files and cross-file relationships.

HTTP: `GET /api/v1/code/repositories/{alias}/diagnostics`, with `ref`, JSON-array-string `path_filters`, `limit` and `cursor`. CLI supports `--remote`. MCP: `relay_code_diagnostics`, with `repository`, `ref_selector`, `path_filters`, `limit`, `cursor`, subject to authorization and context budgets.

Existing diagnostic tables are reused; no migration or reindex is required. Update agents to inspect `content_integrity` when upgrading; older versions can still report overall `degraded` for partial content. Stale versions, unfinished tasks and graph-only responses retain conservative handling. Out-of-scope external dependencies remain unresolved edge metadata rather than file parse degradation.

Diagnostics use the shared `CodeQueryReadStore` capability after storage contract separation. Framework-graph queries also retain scope-wide content integrity independently of version freshness.

Partitioned report routing stays in the existing repository owner, preserving its active-snapshot check and control-store fallback while keeping the adapter facade within its size budget.
