# Architecture Vision and Algorithm Map

[English](../../en/03-architecture-specs/01-architecture-vision-and-algorithm-map.md) | [中文](../../zh/03-architecture-specs/01-architecture-vision-and-algorithm-map.md)

> Document version: 2.1
> Date: 2026-05-28
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

`relay-knowledge` is a **local-first knowledge substrate**. It is not an agent runtime and it is not a thin wrapper around a vector database. It combines evidence, graph facts, derived indexes, retrieval algorithms, recovery workers, and agent-facing protocols into one verifiable knowledge layer.

Its architectural advantage comes from the composition of five mechanisms:

1. **Evidence-anchored graph facts**: entities, relations, claims, events, and code structure must trace back to evidence and source scope.
2. **Versioned graph state separated from derived indexes**: GraphStore is the source of truth; BM25, semantic, vector, community, and code indexes are freshness-aware read models.
3. **Hybrid recall plus structural expansion**: lexical, semantic, vector, graph-path, and code-symbol signals flow into a shared fusion and context-packing path.
4. **Recoverable background architecture**: refresh, OCR, embedding, parsing, and maintenance tasks run behind bounded workers, leases, retries, and dead letters.
5. **Open agent access**: MCP, ACP, future A2A gateways, and SDK bridges use only the unified API and cannot pierce into storage or indexing internals.

## 2. System Layers

```text
CLI / Web / MCP / ACP / future A2A
        |
        v
Unified API and Interface Contracts
        |
        v
Application Services: policy, orchestration, freshness, budgets
        |
        +--> Retrieval: BM25, semantic, vector, graph expansion, rerank
        +--> Indexing: mutation log consumers and scoped read models
        +--> Storage: graph facts, evidence, versions, mutation log
        +--> Background Workers: parsing, OCR, embedding, recovery
        +--> Observability: logs, metrics, traces, diagnostics
        |
        v
Domain Model: source scope, evidence, facts, code graph, errors
```

Dependencies move inward only. UI surfaces, protocol adapters, and workers must not directly access SQLite, tree-sitter parsers, embedding clients, or index writers; they request application services that enforce budgets, authorization, and freshness policy.

The `src/relay_knowledge/domain` source tree is organized by cohesive domain
responsibility while keeping the crate-level `crate::domain::{...}` API stable:
`core/` owns source scopes, graph versions, indexes, errors, and base entities;
`graph/` owns mutation facts, evidence extraction metadata, and retrieval
context contracts; `code/` owns code graph facts, repository indexing/query
requests, repository sets, dependencies, and call-target rules; `operations/`
owns worker, proposal, service-operation, audit, and software global-modeling
types; `knowledge/` owns knowledge-map topics, sources, routes, and history.

## 3. Algorithm Map

| Algorithm domain | Goal | Main path |
| --- | --- | --- |
| Source scope | Keep knowledge authorized, versioned, and auditable | Normalize source identity, resolve snapshots/change sets, bind index partitions |
| Graph fact modeling | Turn LLM and parser output into traceable facts | Evidence anchoring, claim lifecycle, graph version, mutation log |
| Hybrid retrieval | Cover exact terms, conceptual similarity, multi-hop relations, and code impact | BM25 + semantic + vector + graph expansion + RRF + rerank |
| Context packing | Turn recall results into agent-citable evidence | Deduplication, grouping, budgets, source spans, graph paths, freshness metadata |
| Code graph | Make code retrieval understand symbols, references, calls, and change impact | tree-sitter captures, stable symbol ids, incremental indexing, impact propagation |
| Recovery | Restore consistency after crashes, restarts, and partial failures | Persistent cursors, bounded queues, leases, reconcilers, dead letters |

## 4. Reading Order

Book 3 progresses from global to local, from foundations to advanced behavior, and from architecture to operations:

1. Chapters 1-3 define the vision, hard constraints, and foundational runtime.
2. Chapters 4-8 define source, evidence, graph facts, storage, and index freshness.
3. Chapters 9-13 define retrieval, semantic/vector backends, code graphs, tree-sitter extraction, and code impact analysis.
4. Chapters 14-16 define open agent runtime access, resident graph protocols, and unified interfaces.
5. Chapters 17-19 define self-healing services, observability, release, installation, and upgrade behavior.

## 5. Non-goals

- Do not import external agent framework types into domain, storage, retrieval, or indexing.
- Do not make vector storage the source of truth.
- Do not fix benchmarks by enumerating fixture-specific queries or paths.
- Do not run large scans, embedding, OCR, full index rebuilds, or database compaction on query hot paths.

## 6. Acceptance Criteria

- A reader can start here and understand why the system is not ordinary RAG, full-text search, or an agent plugin.
- Every later chapter maps back to one algorithm domain or runtime boundary in this chapter.
- Architectural advancement is expressed through mechanisms, states, boundaries, and measurable acceptance criteria rather than marketing language.

---

Navigation: Next: [2. Engineering Hard Constraints](02-engineering-hard-constraints.md)
