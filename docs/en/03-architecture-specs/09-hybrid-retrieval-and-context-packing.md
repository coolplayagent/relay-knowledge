# Hybrid Retrieval and Context Packing

[English](../../en/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md) | [中文](../../zh/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Hybrid retrieval is the core algorithmic surface. Plain vector retrieval handles similarity; plain BM25 handles exact terms. `relay-knowledge` must answer terminology, concepts, multi-hop relations, time facts, code symbols, and impact analysis, so recall, structural expansion, fusion, rerank, and context packing form one algorithm.

## 2. Query Flow

```text
normalize query
  -> resolve source scope and freshness policy
  -> plan retriever families
  -> lexical / semantic / vector / graph / code recall
  -> candidate normalization and dedup
  -> weighted reciprocal-rank fusion
  -> graph expansion and local rerank
  -> context pack budgeting
  -> response with provenance and freshness metadata
```

No retriever bypasses scope filters, authorization policy, or freshness policy.

## 3. Fusion Model

The baseline fusion uses weighted RRF:

```text
score(candidate) = sum(weight_i / (k + rank_i)) + structural_bonus - penalty
```

`structural_bonus` comes from source authority, direct graph paths, accepted lifecycle, exact symbol matches, fresh indexes, and evidence confidence. `penalty` comes from stale lag, degraded backends, ambiguous entities, low confidence, or duplicate parent evidence.

## 4. Graph Expansion

Graph expansion starts from high-confidence candidates and stays within budget:

- Entity neighborhoods.
- Direct relation/claim/event paths.
- Schema-guided paths.
- Temporal predecessor/successor links.
- Code symbol reference/call/import edges.

Expansion results carry path provenance; they are not returned as opaque related context.

## 5. Context Pack

A context pack is the stable evidence bundle for agents and UI. It includes query metadata, retriever sources, rank explanations, context items, source spans, graph paths, structured facts, code artifacts, freshness, degraded state, budgets, and truncation reasons.

Packing favors diversity and citability. Duplicate hits from the same parent evidence, symbol, or source span merge; low-confidence expansions do not displace direct evidence.

## 6. Acceptance Criteria

- Exact-term, conceptual, multi-hop, temporal, and code-symbol queries have corresponding retriever signals.
- Results explain item source, rank contribution, and freshness.
- Degraded backends produce explicit degradation metadata instead of silent absence.

---

Navigation: Previous: [8. Derived Indexes and Freshness](08-derived-indexes-and-freshness.md) | Next: [10. Semantic/Vector Provider Architecture](10-semantic-vector-provider-architecture.md)
