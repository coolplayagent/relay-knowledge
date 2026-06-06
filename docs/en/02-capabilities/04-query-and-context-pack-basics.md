# Query and Context Pack Basics

[English](./04-query-and-context-pack-basics.md) | [中文](../../zh/02-capabilities/04-query-and-context-pack-basics.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Query capability organizes graph facts, evidence, index state, and budgets into stable context packs. Users receive more than a result list; they receive an evidence bundle understood by CLI, Web, MCP, and ACP.

## User-visible Behavior

- `query` supports source scope, freshness, limit, and JSON output.
- Responses include graph version, indexed graph version, retrieval mode, source scope, and degraded reason.
- Context items include retriever sources, ranking signals, entities, source spans, structured facts, and code artifacts.
- `truncated` and `budget_used` state whether context was cut by budget.

## Competitive Features

A context pack is the structured boundary for agent citation. It combines text, graph paths, code artifacts, freshness, backend state, and ranking explanation in one response so callers do not have to infer why results appeared.

## Command/API Entry Points

```bash
relay-knowledge query "SQLite graph state"   --source docs   --freshness wait-until-fresh   --limit 8   --format json
```

## Degradation and Diagnostics

`degraded_reason` may come from stale indexes, graph-only mode, backend unavailability, parser degradation, or budget exhaustion. Callers read metadata instead of relying only on item counts. The `staleness_hint` field on each retrieval hit provides a structured alternative to the `stale` boolean for inspecting per-result freshness.

## Related Architecture Chapters

- [Hybrid Retrieval and Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)

---

Navigation: Previous: [3. Evidence and Graph Facts](03-evidence-and-graph-facts.md) | Next: [5. Hybrid Retrieval Advantage](05-hybrid-retrieval-advantage.md)
