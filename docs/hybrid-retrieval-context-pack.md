# Hybrid Retrieval Context Pack

This document describes the current Phase 1 retrieval behavior implemented by
`RelayKnowledgeService::retrieve_context`.

## What It Does

`retrieve_context` now returns an auditable context pack instead of only a flat
list of evidence hits. The response keeps the existing `results` array for CLI
and Web compatibility, and adds:

- `context_pack`: graph version, source scope, freshness policy, truncation
  state, backend availability, and per-item source/ranking metadata.
- `fusion`: the ranking algorithm and candidate count. Phase 1 uses reciprocal
  rank fusion with `k = 60`.
- `budget_used`: requested limit, candidate count, returned count, and packed
  context bytes.
- `truncated`: whether one or more matching candidates were omitted because of
  the request limit.

Each result includes `retriever_sources`, `ranking`, entity projections,
optional source span, supporting structured facts, and optional code artifact
metadata. `ranking` records the retriever source, source-local rank, raw source
score, and a short explanation so agents can cite why an item was selected.

## Retrieval Sources

Phase 1 uses three concrete recall paths:

- `bm25`: SQLite FTS5 BM25 over evidence content, entity labels, source scope,
  source path, code symbols, and code chunks.
- `graph_evidence`: deterministic graph evidence/entity term overlap fallback.
- `code_graph`: code symbol and chunk documents inserted from the tree-sitter
  code graph into the shared BM25 read model.

`semantic` and `vector` remain explicit index families in freshness metadata.
When those backends are unavailable, `backend_statuses` records an unavailable
state for each backend, whether scope post-filtering would have been applied,
and the fallback reason. BM25 plus graph evidence retrieval remains usable and
the response still reports index freshness.

## Graph Facts

Graph mutations now support evidence metadata and structured facts:

- evidence source path, source span, confidence, status, and graph version;
- typed relations between entity labels;
- claims with subject, predicate, object, evidence ids, confidence, status, and
  version range;
- events with linked entities, optional valid-time text, confidence, status, and
  version range.

Structured facts are persisted in SQLite and counted in graph inspection and
mutation log responses. Entity cleanup preserves entities referenced by evidence,
relations, claims, and events.

The ingest API accepts structured facts alongside evidence. The basic CLI still
writes evidence and entity labels, while API adapters can supply evidence
`source_path`, `span`, `confidence`, `status`, and relation/claim/event records
that reference evidence ids.
Those structured facts must reference supporting evidence ids so they can be
returned through retrieval. Ingest revalidates deserialized spans, confidence
scores, and version ranges before persistence. Evidence with `rejected` or
`superseded` status remains inspectable in the graph but is excluded from BM25
and graph-evidence retrieval candidates.

## Freshness And Snapshot Behavior

Retrieval always executes against an explicit graph version. BM25 documents store
their `created_graph_version`, so queries do not return evidence or code graph
documents written after the requested snapshot.

Freshness policies are unchanged:

- `allow_stale`: return results and mark stale metadata when an index lags.
- `wait_until_fresh`: refresh stale index metadata before querying.
- `graph_only`: bypass index metadata and return graph-only degraded context.

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
`context_pack.items[*].graph_facts`, `context_pack.items[*].source_span`, and
`context_pack.backend_statuses` when an agent needs source attribution, fact
provenance, or degradation handling.
