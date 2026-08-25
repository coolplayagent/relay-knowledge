# Knowledge Graph Research Summary

[English](02-knowledge-graph-research.md) | [中文](../../zh/04-research/02-knowledge-graph-research.md)

[Documentation index](../README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

> Project: `relay-knowledge`
> Date: 2026-05-11
> Audience: architecture, core implementation, and later technology selection
> Conclusion type: paper and engineering research applied to this project

## Research Positioning

| Dimension | Conclusion |
| --- | --- |
| Sources | Knowledge-graph construction, KG refinement, RAG, GraphRAG, BM25/HNSW/RRF, and property-graph standardization papers, checked against graph database engineering practice. |
| Goal | Define relay-knowledge as a continuously evolving, retrievable, explainable, and recoverable graph service rather than a one-time triple store. |
| Competitive focus | Property graphs, provenance, graph versions, index freshness, three-layer retrieval, and event-driven refresh form the long-term foundation. |
| Scenarios and future | Supports document graphs, code graphs, GraphRAG context packs, agent queries, background index recovery, and future dynamic knowledge plus multimodal evidence. |

## 1. Executive Summary

`relay-knowledge` is not intended merely to store triples. Its goal is a
continuously updated, retrievable, explainable knowledge-graph core shared by
CLI and Web interfaces. Recent work on knowledge-graph construction, GraphRAG,
hybrid retrieval, and graph-database standardization supports five directions:

1. Use a labeled property graph (LPG) internally. Nodes and edges carry
   properties, provenance, versions, confidence, and temporal metadata.
   RDF/OWL can be import, export, or interoperability formats without becoming
   the only internal model.
2. Remain event-driven and async-first. Ingest, extraction, entity resolution,
   graph writes, index refresh, and retrieval evaluation form a traceable event
   chain with bounded queues, timeouts, and cancellation.
3. Build three retrieval layers from the beginning: BM25 keyword retrieval,
   semantic retrieval, and vector ANN. Fuse results with RRF or a weighted
   strategy, then organize context using graph neighborhoods, evidence, and a
   reranker.
4. Treat index freshness as a correctness property. Each graph mutation creates
   a `graph_version`; BM25, semantic/vector indexes, and community summaries
   record the graph version they represent.
5. Do not make LLM extraction the sole source of graph quality. KG construction
   spans acquisition, refinement, and evolution, requiring entity merge,
   relationship validation, conflict detection, human/rule correction, and a
   reversible event log.

## 2. Research Threads and Key Papers

### 2.1 Knowledge-Graph Construction: From Extraction to Evolution

Zhong et al. divide automatic KG construction into knowledge acquisition,
refinement, and evolution [R1]. `relay-knowledge` therefore cannot stop at a
one-time import. Real systems continuously receive documents, code, web pages,
database records, and user input, and the graph must remain consistent while
they change.

Engineering implications:

- Represent pipeline state with events such as `IngestedDocument`,
  `ExtractedFact`, `ResolvedEntity`, `GraphMutation`, and
  `IndexRefreshRequested`.
- Treat LLM extraction as candidate facts, not final truth. Every candidate has
  a source, confidence, extractor version, and evidence span.
- Separate construction from refinement. Construction creates nodes and edges;
  refinement handles merges, completion, deletion, conflict labels, and graph
  evolution.

### 2.2 LLM-Assisted KG Construction: Useful, but Constrained

A 2025 survey describes how LLMs are turning ontology engineering, extraction,
and fusion into language-driven workflows [R2]. Separate research evaluating
LLMs as direct KG constructors identifies unresolved sentence-level extraction,
predefined-schema dependence, and weak structural/semantic evaluation [R3].

Engineering implications:

- An LLM is not the sole source of truth. It may propose entities,
  relationships, types, and summaries; graph writes require structural
  validation.
- Support schema-first and schema-free modes. Core domain types remain
  schema-first; exploratory imports can be schema-free but carry
  `proposed_type` and `confidence`.
- Prefer structured JSON extraction output, followed by Rust-domain validation
  of ids, types, edge direction, property types, and evidence references.

### 2.3 KG Refinement: The Lasting Coverage/Correctness Trade-off

Paulheim notes that a knowledge graph generally cannot be both perfectly
complete and perfectly correct, especially under heuristic or automatic
extraction [R4]. Refinement must therefore be a core workflow, not a post-hoc
script.

Engineering implications:

- Nodes and edges need `confidence`, `confidence_tier`, `evidence_refs`,
  `created_by`, `updated_by`, `valid_from_version`, and `valid_to_version`.
- Do not overwrite conflicts immediately. Preserve candidates with `proposed`,
  `accepted`, `rejected`, or `superseded` state.
- Make automatic merge rules explainable through label normalization, aliases,
  external ids, embedding similarity, and neighborhood overlap; record the
  merge itself as an event.

### 2.4 RAG and GraphRAG: From Local Chunks to Global Structure

Lewis et al. combine a parametric model with external non-parametric memory so
retrieval can ground generation [R5]. Ordinary RAG is strong for local facts but
limited on global questions, cross-document themes, and complex relationship
chains.

Microsoft GraphRAG builds an entity graph from source documents and generates
community summaries for global sensemaking [R6]. Its local search starts from
query-relevant entities, gathers connected entities, relationships, covariates,
and source chunks, and organizes them in a context window [R7].

KG2RAG emphasizes fact-level chunk expansion and context organization [R8].
HybridRAG combines graph and vector evidence [R9]. HyperGraphRAG highlights that
binary edges are not ideal for every complex event, multi-party relationship,
or causal chain [R10].

Engineering implications:

- Retrieval should return an `AnswerContext`, not text chunks alone: matched
  nodes and edges, evidence spans, paths, index versions, and ranking reasons.
- Preserve community/topic summaries for global questions; use entity linking,
  bounded neighborhood expansion, and evidence ranking for local questions.
- Do not require a hypergraph database in v1. An `Event` or `Claim` node can
  connect multiple participants while leaving room for future hyperedges.

### 2.5 Hybrid Retrieval: BM25, Semantic, Vector, and Fusion

BM25 remains a strong baseline for exact lexical matching [R11]. HNSW is a
widely used graph structure for high-recall approximate vector neighbors [R12].
RRF is a simple, effective way to merge ranked lists whose scores use different
scales [R13].

The three layers are:

| Layer | Goal | Input | Output |
| --- | --- | --- | --- |
| BM25 keyword | Exact terms, entity names, code symbols, phrases | Query text | Matching documents, chunks, and node properties |
| Semantic | Intent, synonyms, entity linking, graph expansion | Query text plus graph state | Entities, relationship paths, and topic candidates |
| Vector ANN | Semantically similar spans and embedding neighbors | Query embedding | Top-k embedding records |

Engineering implications:

- BM25 covers entity labels, aliases, relationship labels, document titles,
  chunks, and source paths.
- Vector rows carry `embedding_model`, `dimension`, `source_hash`, and
  `graph_version`; model upgrades can retain multiple generations.
- Start fusion with RRF because it does not assume score comparability. Train a
  weighted model or reranker only after an evaluation set exists.
- Every result remains traceable to evidence and graph version; generated prose
  alone is not a retrieval result.

### 2.6 Graph Databases and Standardization

Property graphs underpin many graph databases. Angles formalizes the property
graph database model [R14]. ISO/IEC 39075:2024 standardized GQL in 2024 [R15],
showing that property-graph query interoperability is maturing. The project need
not implement GQL immediately, but should not reduce its query model to a thin
wrapper over one vendor API.

Selection guidance:

- Define `GraphStore`, `GraphQuery`, `GraphMutationLog`, and `IndexStore`
  interfaces first so the domain remains storage-independent.
- A zero-dependency local path can combine SQLite with embedded lexical/vector
  components for developer use and deterministic tests.
- SurrealDB is a possible Rust-native/multimodel adapter; its documented graph,
  full-text, and vector surfaces may reduce the number of components [R16].
- Neo4j, NebulaGraph, Memgraph, and other external graph systems serve
  enterprise and graph-algorithm paths. Neo4j has full-text and vector indexes
  [R17], with a higher deployment and operations cost.

## 3. Architecture Recommendations

### 3.1 Stable Layers

1. `domain`: entities, relationships, evidence, events, versions, and errors;
   database-free and unit-testable.
2. `graph_store`: graph transactions, traversal, version reads, and mutation
   logs behind async traits.
3. `indexing`: BM25, embeddings, vector ANN, community summaries, and index
   versions, consuming graph events.
4. `retrieval`: query understanding, entity linking, hybrid recall, graph
   expansion, reranking, and context packing.
5. `interfaces`: CLI, Web, and agent/API adapters that call the core service
   without copying domain behavior.

### 3.2 Minimum Graph Model

The early `KnowledgeEntity { id, label }` plus `GraphVersion` was a useful
starting point but insufficient for the research conclusions. The target model
includes:

- `Entity`: id, kind, label, aliases, properties, evidence refs, confidence,
  and version range.
- `Relation`: id, kind, source/target ids, properties, evidence refs,
  confidence, and version range.
- `Evidence`: id, source URI/hash, span, extractor, and creation time.
- `GraphEvent`: event id/type, graph version, payload, trace id, and timestamp.

Represent a complex statement such as “A affected B at time T for reason R” as
an `Event` node connected through `PARTICIPATES_IN`, `CAUSES`, `OCCURRED_AT`, or
similar typed relations rather than flattening everything into binary-edge
properties.

### 3.3 Event-Driven Index Refresh

```text
Source ingest
  -> extraction requested
  -> facts extracted
  -> entities resolved
  -> graph mutation committed
  -> index refresh requested
  -> BM25/vector/semantic indexes refreshed
  -> retrieval ready at graph_version
```

- Each stage uses a bounded channel, maximum queue length, timeout,
  cancellation token, and retry limit.
- Publish refresh only after the graph commit. Refresh failure does not roll
  back accepted graph facts; it records index lag.
- Responses expose graph and index versions so a caller can wait, degrade, or
  accept a stale marker.
- Embedding, bulk parsing, and large-file reads run behind explicit workers, not
  on async executors.

### 3.4 Retrieval and Answer Flow

1. Parse the query into keywords, embedding input, and candidate entity
   mentions.
2. Run BM25, vector ANN, and entity-linking/graph retrieval concurrently.
3. Fuse with RRF; rerank by evidence quality, graph distance, freshness, and
   authorization.
4. Expand top nodes with bounded hops, edge kinds, token budget, and candidate
   count.
5. Build `AnswerContext` with evidence, entities/relations, paths, and ranking
   explanations.
6. If an LLM is used, restrict generation to the context and return citations.

### 3.5 Quality Evaluation

- **Graph quality:** entity duplication, relationship accuracy, evidence
  coverage, isolated-node ratio, and conflicting facts.
- **Retrieval quality:** recall@k, MRR, nDCG, path hit rate, and multi-document
  multi-hop success.
- **System quality:** index lag, event backlog, query p95/p99, graph/index
  version delta, and retry rate.

A small fixed fixture—such as 20 entities, 50 edges, 10 documents, and 30
queries—can establish the first gate. Evaluation should precede external-scale
deployment rather than follow it.

## 4. Staged Roadmap at the Time of Research

### Phase 1: Core Model and Local Test Loop

- Expand entities, relationships, evidence, and events in a unit-testable domain.
- Define async traits for storage, events, refresh, and retrieval.
- Implement a local/in-memory loop so tests do not require a live graph database.
- Establish a small fixture and retrieval evaluation.

### Phase 2: Three-Layer Retrieval and Versioned Indexes

- Index labels, aliases, chunks, and source paths with BM25.
- Add embeddings/vector ANN with model, dimension, hash, and graph version.
- Implement RRF and `AnswerContext`.
- Refresh after mutation and expose fresh/stale state.

### Phase 3: Graph Adapters and Shared Web/CLI Services

- Add replaceable adapters such as SurrealDB or Neo4j behind `GraphStore`.
- Keep CLI and Web on one core service; adapters only parse, authorize, and
  present.
- Add community summaries, path explanation, neighborhood browsing, and
  import/export.

### Phase 4: Advanced KG Construction and GraphRAG

- Add LLM extraction, entity disambiguation, relationship validation, and
  conflict handling.
- Add community/global retrieval for cross-document global questions.
- Model complex facts with `Claim`/`Event` nodes and evaluate hypergraphs only
  where justified.
- Add human correction, rollback, and audit workflows.

## 5. Design Principles

- Every graph fact is traceable to its source and extraction path.
- Automatic extraction is reversible and enters as candidate facts.
- Retrieval identifies its graph and index versions.
- Query paths degrade safely: BM25 and graph facts remain usable if vectors fail.
- CLI, Web, API, and agent adapters share the core service.
- Domain and retrieval contracts are not bound to one graph-database syntax.

## References

- [R1] Lingfeng Zhong, Jia Wu, Qian Li, Hao Peng, Xindong Wu. “A Comprehensive Survey on Automatic Knowledge Graph Construction.” 2023. <https://arxiv.org/abs/2302.05019>
- [R2] Haonan Bian. “LLM-empowered knowledge graph construction: A survey.” 2025. <https://arxiv.org/abs/2510.20345>
- [R3] Ruirui Chen et al. “Are Large Language Models Effective Knowledge Graph Constructors?” 2025. <https://arxiv.org/abs/2510.11297>
- [R4] Heiko Paulheim. “Knowledge graph refinement: A survey of approaches and evaluation methods.” Semantic Web, 2017. <https://journals.sagepub.com/doi/10.3233/SW-160218>
- [R5] Patrick Lewis et al. “Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks.” 2020/2021. <https://arxiv.org/abs/2005.11401>
- [R6] Darren Edge et al. “From Local to Global: A Graph RAG Approach to Query-Focused Summarization.” 2024/2025. <https://arxiv.org/abs/2404.16130>
- [R7] Microsoft GraphRAG documentation, “Local Search.” <https://microsoft.github.io/graphrag/query/local_search/>
- [R8] Xiangrong Zhu, Yuexiang Xie, Yi Liu, Yaliang Li, Wei Hu. “Knowledge Graph-Guided Retrieval Augmented Generation.” NAACL 2025. <https://aclanthology.org/2025.naacl-long.449/>
- [R9] Bhaskarjit Sarmah et al. “HybridRAG: Integrating Knowledge Graphs and Vector Retrieval Augmented Generation for Efficient Information Extraction.” 2024. <https://arxiv.org/abs/2408.04948>
- [R10] Haoran Luo et al. “HyperGraphRAG: Retrieval-Augmented Generation via Hypergraph-Structured Knowledge Representation.” 2025. <https://arxiv.org/abs/2503.21322>
- [R11] Stephen Robertson, S. Walker, S. Jones, M. M. Hancock-Beaulieu, M. Gatford. “Okapi at TREC-3.” 1995. <https://www.microsoft.com/en-us/research/publication/okapi-at-trec-3/>
- [R12] Yu. A. Malkov, D. A. Yashunin. “Efficient and robust approximate nearest neighbor search using Hierarchical Navigable Small World graphs.” 2016/2018. <https://arxiv.org/abs/1603.09320>
- [R13] Gordon V. Cormack, Charles L. A. Clarke, Stefan Buettcher. “Reciprocal rank fusion outperforms condorcet and individual rank learning methods.” SIGIR 2009. <https://doi.org/10.1145/1571941.1572114>
- [R14] Renzo Angles. “The Property Graph Database Model.” 2018. <https://ceur-ws.org/Vol-2100/paper26.pdf>
- [R15] ISO/IEC 39075:2024, Graph Query Language (GQL) standard notice. <https://www.gqlstandards.org/>
- [R16] SurrealDB documentation, “Using SurrealDB as a Vector Database.” <https://surrealdb.com/docs/surrealdb/models/vector>
- [R17] Neo4j Cypher Manual, semantic indexes. <https://neo4j.com/docs/cypher-manual/current/indexes/semantic-indexes/>

---

Navigation: Previous: [1. Industry Capability Snapshot 2026](01-industry-capability-snapshot-2026.md) | Next: [3. arXiv Knowledge Graph Paper Insights](03-arxiv-knowledge-graph-paper-insights.md)
