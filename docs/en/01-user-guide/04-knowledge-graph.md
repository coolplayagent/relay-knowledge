# Chapter 4: Knowledge Graph

[English](../../en/01-user-guide/04-knowledge-graph.md) | [中文](../../zh/01-user-guide/04-knowledge-graph.md)

This chapter covers general knowledge graph ingest, query, inspection, and derived index refresh. Code repository graphs are covered separately in Chapter 5.

## 4.1 Ingest Evidence

The minimal ingest command requires a source scope and text content:

```bash
relay-knowledge ingest \
  --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust \
  --entity SQLite \
  --format json
```

`--source` isolates a source scope such as `docs`, `repo:core`, or a product domain. `--entity` can be repeated to attach entity labels. A successful write creates a new `graph_version` and updates the freshness state for derived indexes such as BM25, semantic, and vector.

CLI `ingest` accepts plain text evidence. Integrations that need source spans, confidence, claims, events, typed relations, or multimodal extraction metadata should use the shared API or adapter layer, which reuses the same graph mutation, index refresh, and audit paths.

## 4.2 Query a Context Pack

Basic query:

```bash
relay-knowledge query "SQLite graph state" \
  --source docs \
  --limit 8 \
  --format json
```

Require indexes to catch up to the latest graph version:

```bash
relay-knowledge query "SQLite graph state" \
  --source docs \
  --freshness wait-until-fresh \
  --limit 8 \
  --format json
```

Read only graph facts:

```bash
relay-knowledge query "SQLite graph state" \
  --source docs \
  --freshness graph-only \
  --format json
```

The JSON response contains display-compatible `results`, an agent-oriented `context_pack`, `indexes`, `index_cursors`, and `index_refresh` diagnostics. For auditable references, prefer `context_pack.items[*].ranking`, `graph_facts`, `graph_paths`, `source_span`, `context_pack.provenance_trace`, `backend_statuses`, `budget_used`, `truncated`, `degraded_reason`, and `index_refresh.stale_reasons`. `provenance_trace` records the graph version, routed intent, visited nodes/edges, cited evidence, visited-but-uncited context, ranking contributions, stale/degraded state, and trace truncation within the authorized scope; agent adapters count it against the context budget and preserve cited results first when the budget is tight. `index_cursors` report scoped BM25, semantic, and vector cursor state, including backend/model metadata and last errors when present.

Hybrid retrieval combines BM25, local semantic signatures, local hashed-vector ANN, structured graph facts, schema paths, temporal/community context, code graph documents, and configurable provider backend metadata. Candidates are initially combined with reciprocal-rank fusion and then selected by local deterministic rerank. Entity lexical aliases help recall but do not replace canonical labels.

## 4.3 Inspect Graph State

View graph statistics:

```bash
relay-knowledge graph inspect --format json
```

Refresh one or more index families:

```bash
relay-knowledge index refresh --kind bm25 --format json
relay-knowledge index refresh --kind semantic --kind vector --format json
```

Without `--kind`, the service refreshes the index families it currently considers necessary. The refresh path uses a bounded refresh queue, leases, retries, dead letters, and stale diagnostics. Explicit refresh failures are not reported as fresh.

Queries with `wait-until-fresh` use the same explicit refresh path instead of rebuilding indexes without bounds on the query hot path. JSON response `diagnostics.stale_reasons` lists index families and scoped cursors that are still stale or failed.

## 4.4 Structured Facts

CLI `ingest` writes plain evidence and entity labels. The shared API also supports richer facts:

- Evidence `source_path`, spans, confidence, status, and multimodal extraction metadata.
- Typed relations, claims, and events.
- Structured facts linked to evidence IDs, with span, confidence, and version range validation after deserialization.

Retrieval uses only `accepted` or `proposed` evidence as context candidates. `rejected` and `superseded` evidence can still appear in graph inspection, but they do not enter BM25 or graph-evidence retrieval candidates.

## 4.5 Multimodal Evidence

The current schema can record `text_span`, `image_asset`, `ocr_text`, `caption`, `image_embedding`, `table`, and `layout_region`. Derived evidence such as OCR, captions, and image embeddings can reference parent evidence; retrieval merges them by parent so that multiple derivatives from one image do not consume context pack budget repeatedly.

Real OCR, captioning, table/layout extraction, and image embedding work should run as background workers or maintenance tasks. Worker-produced derived evidence is submitted through the shared API `commit_multimodal_extraction`, which checks parent evidence, derived modality, and extractor identity before reusing normal ingest, bounded index refresh, and cursor metadata paths. The query hot path only reads committed evidence/read models.

## 4.6 Semantic and Vector Backends

Semantic/vector retrieval uses local deterministic read models by default. When connecting an external embedding worker, first declare backend mode and model metadata:

```bash
RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external
RELAY_KNOWLEDGE_VECTOR_BACKEND=external
RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL=text-embed-3-small
RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL=clip-vit-b32
RELAY_KNOWLEDGE_EMBEDDING_DIMENSION=1536
```

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` and `RELAY_KNOWLEDGE_VECTOR_BACKEND` support `local`, `external`, and `disabled`. `disabled` skips the corresponding semantic/vector retriever and read model refresh.

After connecting an external embedding worker, run:

```bash
relay-knowledge provider probe --format json
relay-knowledge index refresh --kind semantic --kind vector --format json
```

`provider probe` validates configuration and returns redacted diagnostics. Real read model freshness is still determined by `health`, `index refresh`, and cursor/backend metadata in query responses.
