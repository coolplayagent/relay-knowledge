# Graph Fact Model and Versioning

[English](../../en/03-architecture-specs/06-graph-fact-model-and-versioning.md) | [中文](../../zh/03-architecture-specs/06-graph-fact-model-and-versioning.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

The graph fact model turns unstructured evidence, LLM candidate output, code parser results, and human review into one traceable state machine. Answers are not facts, candidates are not commits, and indexes are not state; only accepted facts written through the mutation contract become graph state.

## 2. Core Fact Types

| Type | Purpose |
| --- | --- |
| `Entity` | Canonical objects such as people, organizations, systems, files, symbols, and concepts |
| `Relation` | Typed edges between entities |
| `Claim` | Evidence-backed, confidence-scored, lifecycle-aware statements |
| `Event` | Time facts with participants and evidence |
| `Evidence` | Source anchor for all facts |
| `CodeFact` | Code files, symbols, references, calls, imports, and chunks |

Structured facts reference supporting evidence. Facts without evidence can be proposals or diagnostics only; they do not enter the accepted graph.

## 3. Version Model

- `graph_version` is the monotonic system commit version.
- `valid_from` and `valid_to` are domain validity times, not commit time.
- The mutation log records affected scope, entities, evidence, source hashes, and index families.
- Derived index cursors state which graph version they cover; they do not change graph version.

## 4. Lifecycle

```text
proposed -> validated -> accepted
        -> rejected
accepted -> superseded
accepted -> deprecated
```

LLM, OCR, parser, and agent output defaults to proposed facts or derived evidence. Human review, rule validation, or trusted parser contracts move results into accepted state.

## 5. Conflict and Confidence

Conflicts do not delete prior facts. The system keeps competing claims, supporting evidence, confidence, validation notes, and conflict groups. Retrieval may choose defaults using lifecycle, confidence, freshness, and source authority, but provenance remains visible.

## 6. Acceptance Criteria

- Every accepted fact traces to evidence, mutation, and graph version.
- Derived index failure does not change fact graph version.
- Conflicting facts can coexist and expose evidence and state in context packs.

---

Navigation: Previous: [5. Multimodal Evidence Ingestion](05-multimodal-evidence-ingestion.md) | Next: [7. Storage Engine and Mutation Log](07-storage-engine-and-mutation-log.md)
