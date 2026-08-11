# Hybrid Retrieval Advantage

[English](./05-hybrid-retrieval-advantage.md) | [中文](../../zh/02-capabilities/05-hybrid-retrieval-advantage.md)

> Document version: 2.3
> Date: 2026-08-11
> Scope: Book 2 capability guide

## Capability Positioning

Hybrid retrieval is the central competitive capability in Book 2. It combines BM25, local semantic token read models, local hashed-vector ANN, configurable external semantic/vector backends, graph evidence fallback, code graph documents, bounded code exact-text source fallback, local file path/content read models, schema paths, temporal events, community summaries, and RRF.

## User-visible Behavior

- Query results carry retriever sources and ranking explanation.
- BM25 indexes generated lexical aliases for entities and code symbols, but aliases are not returned as canonical labels.
- Eligible large, balanced graph corpora can use bounded single-FTS hierarchical BM25; routed hits explain the algorithm, selected groups/documents, and whether group selection was approximate.
- Graph paths preserve node labels, edge fact id, predicate, supporting evidence ids, confidence, status, and version range.
- Temporal, community, and code graph signals can appear in the same context pack as evidence hits.
- Code exact-text fallback hits enter results with `lexical`/`text_fallback` provenance and are not presented as resolved graph edges.
- Local file results distinguish path, metadata, content, and change-cursor freshness; filename/path queries do not depend on content indexes.

## Competitive Features

Full-text search misses conceptual similarity, vector search can miss exact symbols, graph queries lack natural-language recall, and ordinary desktop file search usually lacks graph and agent context. Hybrid retrieval fuses these signals and then budgets context, serving fact QA, code location, local file location, multi-hop relations, and agent context construction together.

## Command/API Entry Points

```bash
relay-knowledge query "retry policy graph path"   --freshness wait-until-fresh   --limit 10   --format json
```

## Degradation and Diagnostics

When semantic/vector backends are disabled or cursors are stale, BM25 and graph evidence remain usable. `context_pack.backend_statuses` explains configured backend, model, dimension, scope post-filter, and indexed graph version.
Request-level `disabled_retriever_sources` is enforced for every graph-search source: BM25/code-graph rows, graph evidence, semantic, vector, graph path, temporal, and community summary. Merge and fallback orchestration do not reintroduce a disabled source.
When code source fallback hits candidate-path or budget limits, only exact-text fallback is degraded; existing BM25, code graph edge, and graph evidence candidates can still enter the context pack.
When local file content cursors are stale, path and metadata remain usable for file location; responses explain content staleness, watcher lag, or bounded-rescan state.

### Single-FTS Hierarchical BM25

For an eligible current graph version, graph BM25 assigns documents to scope-qualified, content-driven 10-bit SimHash groups. Each indexed `routing_key` contains a zero-weight scope64 partition token and a scope-qualified group token. Coarse selection uses global IDF and aggregate group term frequency only; it does not implement the paper's same-document co-occurrence signal. A scoped request intersects its explicitly business-column-scoped query, scope token, and selected group tokens in one `graph_bm25 MATCH`. The SQL `source_scope` predicate remains the hard authorization filter rather than trusting the hash token.

The indexed `routing_key` has a fixed BM25 weight of zero. Within schema v4, a document present in both routed and flat results therefore has a bitwise-identical score. The token still affects FTS5 document-length statistics, so v4 scores may differ from a pre-v4 flat index. The FTS plan orders a bounded identity window through the hidden `rank` column and then hydrates selected rows by rowid; the route sidecar stores `fts_rowid NOT NULL UNIQUE` together with document identity, graph version, and label state. Group selection remains approximate and is not rank-safe. Exact ties at a `LIMIT` boundary do not promise deterministic membership.

Routing is admitted only for current, complete, sufficiently large, and bounded-skew populations. It also enforces bounded ASCII query terms, group counts, group sizes, selected-group count, a maximum 25% selected-document fraction, and a separated coarse-score cutoff; an all-selected or ambiguous cutoff uses flat search. Every query term's persisted global document frequency must match a business-column `MATCH` probe bounded to `df + 1`, every term must be at or below 20% of the corpus, and all probes together may reserve at most 65,536 postings. A stale version, statistics mismatch, unsupported or non-ASCII routing query, small/skewed population, excessive selection, or temporarily unavailable routing state disables hierarchy and continues through the existing flat/fallback lexical path; non-transient planning errors surface instead of being hidden as fallback. The routing-term extractor accepts only ASCII, so a non-ASCII query bypasses hierarchy and follows its existing flat FTS/fallback behavior. An attempted routed query that is empty or produces fewer distinct candidates than requested retries flat BM25 before later fallbacks. Transient SQLite query errors receive bounded retries and, if they persist, leave BM25 unavailable for that search; non-transient query errors surface. Routed hits carry `hierarchical_bm25` selection fields. Successful flat FTS rows carry `hierarchical_bm25 fallback=<reason>`; hits produced only by later LIKE/trigram fallbacks retain those layers' existing explanation behavior.

SQLite schema marker v4 rebuilds `graph_bm25_rebuild` while the old flat FTS remains readable. Durable owner/expiry, phase/cursor, and semantic/vector-plan fields let an expired attempt be taken over and resumed. Each transaction admits at most 128 documents, 4 MiB of estimated source bytes, 8,192 labels, and 8,192 links. A single oversized document is isolated and emits a bounded-identity warning so progress continues; this is not an absolute per-document byte bound. Semantic, vector, and fuzzy lexical fallback pause during `building`, after which the shadow, route `fresh`, and marker activate atomically. Historical unscoped fallback probes use version-leading global indexes, while scoped probes retain scope-leading indexes. A graph search uses one read transaction across retrieval layers. Current evidence/code writers are fenced while the durable rebuild lease is `building`, but old binaries do not honor that application check. A binary-only rollback is not a rollback to the old numerical score baseline, and old-binary writes make v4 metadata stale. Restore a pre-v4 database checkpoint for exact old-schema scores; otherwise a later v4 startup reconciles and rebuilds derived state. Upgrades require exclusive database access, and old and new writers must not share one database concurrently.

The paper's billion-document latency, memory, throughput, and small-scale quality results are research evidence, not product measurements. The deterministic suite enforces synthetic Recall@10 >= 0.9, but natural-vocabulary Recall@k, p50/p95, and fallback rate remain separate, unmeasured product benchmarks. These BM25-local bounds also do not establish an end-to-end bound for the other pre-existing hybrid graph layers.

### BM25 Multi-level Fallback Strategy

The BM25 retrieval path implements a three-level fallback chain to maximize recall while preserving ranking quality:

```
FTS5 prefix match (BM25 scoring)
  ↓ empty result and query ≥ 2 chars
Exact name match (JSON-safe entity_labels LIKE / LOWER(content))
  ↓ empty result
LIKE substring search (content LIKE '%query%' ESCAPE '\')
  ↓ empty result and query ≥ 3 chars
Levenshtein fuzzy search (edit distance ≤ 1..2)
```

**Performance bounds**:
- Exact name match uses a JSON-encoded `LIKE '%"target"%'` pattern to support multi-label entities and escaped label characters
- LIKE fallback escapes `\`, `%`, and `_` before binding parameters
- All WHERE clauses wrap OR conditions in parentheses to ensure scope and version filtering applies to all branches
- Levenshtein uses a maintained `graph_bm25_label_grams` label gram index to collect scope/version candidates by query-specific gram overlap and label-length bounds instead of scanning graph documents or truncating arbitrary anchor rows
- Label gram schema and backfill are protected by the SQLite schema marker version, resume incomplete upgrades by comparing expected per-document grams, and cap query grams before building SQL bind parameters
- Each document admits at most 256 labels, 1,024 UTF-8 bytes per label, and 8,192 distinct grams; skips persist in `label_gram_state`, and exhausting the 8,192-posting fuzzy-query budget is reported as degradation
- Historical unscoped authorized-corpus, label-state, and `label_lower` hydration probes use version-leading global indexes; scoped queries retain their specialized scope-leading indexes
- Fuzzy matching applies the gram-overlap candidate cap before Rust Levenshtein scoring, then ranks matched names by edit distance before the matched-name cap
- Fuzzy result rows are fetched by batch-joining ranked names through label-gram document ids, preserve the name's edit-distance score in result ordering, and avoid per-name leading-wildcard scans or a single cross-name SQL `LIMIT` that could drop closer matches
- Fallback SQL limits rows before deterministic in-memory ordering so leading-wildcard LIKE paths do not require an unbounded SQL sort
- Edit distance upper bound adapts to query length: ≤ 4 chars → max dist 1, > 4 chars → max dist 2
- Fallback is total: if an earlier level returns results, later levels are skipped; results are deduplicated by document_id
- All SQL queries use `graph_bm25.` table prefix to disambiguate columns

**Applicable scenarios**:
- Typo correction (e.g., `getUssr` → `getUser`)
- Substring queries (e.g., `sign` → `signInWithGoogle`)
- Short queries where FTS prefix matching is too noisy

## Related Architecture Chapters

- [Hybrid Retrieval and Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Semantic/Vector Provider Architecture](../03-architecture-specs/10-semantic-vector-provider-architecture.md)
- [Hierarchical BM25 Analysis 2026](../04-research/12-hierarchical-bm25-analysis-2026.md)

---

Navigation: Previous: [4. Query and Context Pack Basics](04-query-and-context-pack-basics.md) | Next: [6. Freshness and Index Recovery](06-freshness-and-index-recovery.md)
