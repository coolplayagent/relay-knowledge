# Evidence and Graph Facts

[English](./03-evidence-and-graph-facts.md) | [中文](../../zh/02-capabilities/03-evidence-and-graph-facts.md)

> Document version: 2.1
> Date: 2026-08-30
> Scope: Book 2 capability guide

## Capability Positioning

Evidence and graph facts are the foundation of GraphRAG. The system does not treat text snippets as answers; it organizes evidence, entities, relations, claims, events, source spans, and confidence into traceable graph state.

## User-visible Behavior

- `ingest` writes source-scoped evidence and entity labels.
- Structured APIs can write source path, span, confidence, status, typed relations, claims, and events.
- Structured facts reference supporting evidence ids.
- `rejected` and `superseded` evidence is not returned as default retrieval context.
- The repository software ontology uses first-class `SoftwareStatement` records to retain source kind, same-scope evidence references, extractor id/version, assertion mode, resolution state, validity, observation time, confidence, and fact state. Inspect them with `repo software --kind statements|conflicts`.
- `entity_key` identifies stable software identity without a commit or source scope, while `occurrence_id` identifies one appearance in a snapshot and evidence set. Snapshot and runtime-event kinds intentionally remain scope-bound.

## Competitive Features

Ordinary RAG often stores chunks only. `relay-knowledge` stores auditable relationships between evidence and graph facts, so context packs can expose one-hop graph paths, claim state, event versions, and supporting evidence instead of natural-language snippets only.

## Command/API Entry Points

```bash
relay-knowledge ingest   --source docs   --content "Rust async services isolate blocking SQLite work"   --entity Rust   --entity SQLite   --format json

relay-knowledge graph inspect --format json

relay-knowledge repo software repository --kind statements --format json
relay-knowledge repo software repository --kind conflicts --format json
```

## Degradation and Diagnostics

Writes revalidate confidence, span, and version ranges. Structured facts without supporting evidence do not become accepted facts directly. The software ontology also validates predicate domain/range, object cardinality, validity intervals, observed timestamps, cross-scope references, and extractor completeness. A failed statement remains `rejected`, and diagnostics are returned with the software response. Graph inspection confirms evidence, entities, relations, claims, events, and current graph version.

## Related Architecture Chapters

- [Multimodal Evidence Ingestion](../03-architecture-specs/05-multimodal-evidence-ingestion.md)
- [Graph Fact Model and Versioning](../03-architecture-specs/06-graph-fact-model-and-versioning.md)
- [Software Global Domain Modeling Architecture](../03-architecture-specs/21-software-global-domain-modeling.md)

---

Navigation: Previous: [2. Local-first Runtime and CLI](02-local-first-runtime-and-cli.md) | Next: [4. Query and Context Pack Basics](04-query-and-context-pack-basics.md)
