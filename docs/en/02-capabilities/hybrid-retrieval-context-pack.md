# Hybrid Retrieval Context Pack

[English](./hybrid-retrieval-context-pack.md) | [中文](../../zh/02-capabilities/hybrid-retrieval-context-pack.md)

This is the English documentation page for `hybrid-retrieval-context-pack.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

This document describes the current Phase 4 retrieval behavior implemented by
`RelayKnowledgeService::retrieve_context`.

## What It Does

`retrieve_context` now returns an auditable context pack instead of only a flat
list of evidence hits. The response keeps the existing `results` array for CLI
and Web compatibility, and adds:

- `context_pack`: graph version, source scope, freshness policy, truncation
  state, backend availability, and per-item source/ranking metadata.
- `fusion`: the ranking algorithm and candidate count. Phase 1 uses reciprocal
  rank fusion with `k = 60`.
- `rerank`: post-fusion rerank diagnostics, including requested/effective mode,
  candidate count, returned count, degradation state, and optional reason.
- `budget_used`: requested limit, candidate count, returned count, and packed
  context bytes.
- `truncated`: whether one or more matching candidates were omitted because of
  the request limit.

Each result includes `retriever_sources`, `ranking`, entity projections,
optional source span, supporting structured facts, direct graph path evidence,
optional code artifact metadata, and optional `rerank` signal. `ranking`
records the retriever source, source-local rank, raw source score, and a short
explanation; `rerank` records the final local rerank score and explanation so
agents can cite why an item was selected.

## Retrieval Sources

The retrieval layer uses these concrete recall paths:

- `bm25`: SQLite FTS5 BM25 over evidence content, entity labels, generated
  entity-label aliases, source scope, source path, code symbols, generated code
  symbol aliases, and code chunks.
- `graph_evidence`: deterministic graph evidence/entity term overlap fallback.
- `code_graph`: code symbol and chunk documents inserted from the tree-sitter
  code graph into the shared BM25 read model.
- `semantic`: local token-signature read model over evidence and derived
  multimodal evidence, with model, dimension, source hash, scope, and graph
  version metadata in ranking explanations.
- `vector`: local hashed-vector ANN read model with deterministic vectors,
  scope post-filtering, graph-version filtering, model metadata, and source
  hashes.
- `graph_path`: schema-guided traversal over accepted relations, claims,
  events, and their supporting evidence.
- `temporal`: event retrieval for year terms and `as_of:<date>` constraints.
- `community_summary`: scoped summary hit for global/overview/community
  queries.

`semantic` and `vector` remain explicit index families in freshness metadata.
`backend_statuses` records the configured `local`, `external`, or `disabled`
read model mode, model name, dimension, scope post-filtering, indexed graph
version, and stale/unavailable reason when applicable. BM25 plus graph evidence
retrieval remains usable when a derived backend is disabled or stale, and the
response still reports index freshness.
BM25, semantic, vector, graph path, temporal, and community hits are fused with
RRF, then reranked before the final request limit is applied. The default
semantic/vector implementation is the local deterministic read model. External
OpenAI-compatible embedding providers can supply read-model metadata and probe
diagnostics through the same cursor and backend-status contract without changing
the context-pack shape.
Health and index-refresh diagnostics also expose scoped cursor metadata for
these index families: source hash, backend cursor, and model name/dimension when
a configured backend worker supplies them. The same diagnostics include
`stale_reasons`, a structured list of index-family and scoped-cursor reasons
that explain failed state, graph-version lag, and last-error conditions.

`semantic` and `vector` are explicit index families in freshness metadata.
The current v2 baseline uses local deterministic SQLite read models: semantic
documents store normalized concept tokens, while vector documents store hashed
token and character n-gram weights. If a future backend is unavailable,
`backend_statuses` still records the unavailable state and fallback reason.
BM25 plus graph evidence retrieval remains usable and the response still
reports index freshness.

## Graph Facts

Graph mutations now support evidence metadata and structured facts:

- evidence source path, source span, confidence, status, and graph version;
- evidence modality and extraction metadata for `text_span`, `image_asset`,
  `ocr_text`, `caption`, `image_embedding`, `table`, and `layout_region`;
- typed relations between entity labels;
- claims with subject, predicate, object, evidence ids, confidence, status, and
  version range;
- events with linked entities, optional valid-time text, confidence, status, and
  version range.

Structured facts are persisted in SQLite and counted in graph inspection and
mutation log responses. Entity cleanup preserves entities referenced by evidence,
relations, claims, and events.
Retrieval context items also expose direct `graph_paths` derived from those
facts. Each path keeps the participating node labels, the relation/claim/event
edge, supporting evidence ids, confidence, lifecycle status, and graph-version
validity range.

The ingest API accepts structured facts alongside evidence. The basic CLI still
writes evidence and entity labels, while API adapters can supply evidence
`source_path`, `span`, `confidence`, `status`, and relation/claim/event records
that reference evidence ids.
Those structured facts must reference supporting evidence ids so they can be
returned through retrieval. Ingest revalidates deserialized spans, confidence
scores, and version ranges before persistence. Evidence with `rejected` or
`superseded` status remains inspectable in the graph but is excluded from BM25
and graph-evidence retrieval candidates.
OCR, caption, table, layout, and image embedding maintenance workers submit
derived evidence through `commit_multimodal_extraction`, which enforces parent
evidence ownership and extractor identity before using the regular ingest and
index-refresh path. Retrieval uses the parent evidence id as the merge key, so
OCR and caption hits for the same image are returned as one grouped context item
instead of duplicate results.

## Freshness And Snapshot Behavior

Retrieval always executes against an explicit graph version. BM25 documents store
their `created_graph_version`, so queries do not return evidence or code graph
documents written after the requested snapshot.

Freshness policies are unchanged:

- `allow_stale`: return results and mark stale metadata when an index lags.
- `wait_until_fresh`: refresh stale index metadata before querying.
- `graph_only`: bypass index metadata and return graph-only degraded context.

When `RELAY_KNOWLEDGE_SEMANTIC_BACKEND` or
`RELAY_KNOWLEDGE_VECTOR_BACKEND` is `disabled`, that retriever is excluded from
candidate execution and its read-model refresh work is not scheduled. Semantic
and vector cursor model metadata is derived from the documents that were indexed,
not from runtime override labels.
When either backend is `external`, the remote provider is configured through
the `env` boundary. Query execution still reads local read-model tables and does
not call the provider on the hot path.

Rerank runs after RRF and before request-limit truncation. The default
`RELAY_KNOWLEDGE_RERANK_BACKEND=local` path is deterministic and scores query
term coverage across content, entity labels, graph facts, source paths, source
diversity, and structured evidence. `disabled` keeps RRF order only. `external`
is a reserved provider contract in this release and degrades to local rerank
with `rerank.degraded=true`; it does not call a remote model from the query hot
path.

## CLI Example

```bash
relay-knowledge ingest \
  --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust \
  --format json

relay-knowledge query SQLite \
  --source docs \
  --freshness wait-until-fresh \
  --format json
```

The query response contains both `results` and `context_pack`. Use `results` for
simple display. Use `context_pack.items[*].ranking`,
`context_pack.items[*].graph_facts`, `context_pack.items[*].graph_paths`,
`context_pack.items[*].source_span`, and `context_pack.backend_statuses` when an
agent needs source attribution, fact/path provenance, or degradation handling.
