# Hybrid Retrieval Advantage

[English](./05-hybrid-retrieval-advantage.md) | [中文](../../zh/02-capabilities/05-hybrid-retrieval-advantage.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Hybrid retrieval is the central competitive capability in Book 2. It combines BM25, local semantic token read models, local hashed-vector ANN, configurable external semantic/vector backends, graph evidence fallback, code graph documents, schema paths, temporal events, community summaries, and RRF.

## User-visible Behavior

- Query results carry retriever sources and ranking explanation.
- BM25 indexes generated lexical aliases for entities and code symbols, but aliases are not returned as canonical labels.
- Graph paths preserve node labels, edge fact id, predicate, supporting evidence ids, confidence, status, and version range.
- Temporal, community, and code graph signals can appear in the same context pack as evidence hits.

## Competitive Features

Full-text search misses conceptual similarity, vector search can miss exact symbols, and graph queries lack natural-language recall. Hybrid retrieval fuses these signals and then budgets context, serving fact QA, code location, multi-hop relations, and agent context construction together.

## Command/API Entry Points

```bash
relay-knowledge query "retry policy graph path"   --freshness wait-until-fresh   --limit 10   --format json
```

## Degradation and Diagnostics

When semantic/vector backends are disabled or cursors are stale, BM25 and graph evidence remain usable. `context_pack.backend_statuses` explains configured backend, model, dimension, scope post-filter, and indexed graph version.

## Related Architecture Chapters

- [Hybrid Retrieval and Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Semantic/Vector Provider Architecture](../03-architecture-specs/10-semantic-vector-provider-architecture.md)

---

Navigation: Previous: [4. Query and Context Pack Basics](04-query-and-context-pack-basics.md) | Next: [6. Freshness and Index Recovery](06-freshness-and-index-recovery.md)
