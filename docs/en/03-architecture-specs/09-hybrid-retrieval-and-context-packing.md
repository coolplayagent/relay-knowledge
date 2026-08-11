# Hybrid Retrieval and Context Packing

[English](../../en/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md) | [中文](../../zh/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)

> Document version: 2.3
> Date: 2026-08-11
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Hybrid retrieval is the core algorithmic surface. Plain vector retrieval handles similarity; plain BM25 handles exact terms. `relay-knowledge` must answer terminology, concepts, multi-hop relations, time facts, code symbols, and impact analysis, so recall, structural expansion, fusion, rerank, and context packing form one algorithm.

## 2. Query Flow

```text
normalize query
  -> resolve source scope and freshness policy
  -> plan retriever families
  -> lexical (flat or single-FTS routed) / semantic / vector / graph / code / local file recall
  -> candidate normalization and dedup
  -> weighted reciprocal-rank fusion
  -> graph expansion and local rerank
  -> context pack budgeting
  -> response with provenance and freshness metadata
```

No retriever bypasses scope filters, authorization policy, or freshness policy. Request-level `disabled_retriever_sources` is applied to BM25/code-graph rows, graph evidence, semantic, vector, graph path, temporal, and community-summary sources before fusion.

The query planner first classifies intent: exact term, conceptual, multi-hop, temporal, code symbol, impact, file path, file content, or mixed agent context. Each intent selects retriever families and budgets. For example, filename/path queries prefer `local_file_path` and metadata, while content questions enter `local_file_content`, BM25, or semantic/vector paths.

For code intent, recall order is tree-sitter code graph, SQLite FTS/BM25, semantic/vector supplement, and only then bounded internal exact-text source fallback. Product runtime fallback inherits source scope, path/language filters, authorization, and freshness policy, and searches materialized indexed-commit candidates rather than a dirty worktree. It can produce source span evidence only; it cannot declare new graph edges or override edge confidence. Agent or maintainer inspection may use `rg` or `grep -RIn`, but that is a bounded development search technique, not a product query-path substitute.

## 3. Fusion Model

The baseline fusion uses weighted RRF:

```text
score(candidate) = sum(weight_i / (k + rank_i)) + structural_bonus - penalty
```

`structural_bonus` comes from source authority, direct graph paths, accepted lifecycle, exact symbol matches, exact file path/basename matches, fresh indexes, and evidence confidence. `penalty` comes from stale lag, degraded backends, ambiguous entities, low confidence, unauthorized candidate rejection, or duplicate parent evidence.

Multi-stage reranking is allowed after RRF, but it only processes bounded candidate windows and preserves each retriever's rank contribution. BM25, vector, graph path, code edge, and file path scores are not linearly added before calibration.

## 4. Hierarchical BM25 Routing Boundary

Graph BM25 owns one scored and routed FTS5 read model:

```text
bounded topical terms (path / labels / aliases / content)
  -> scope64 partition token + scope-qualified SimHash10 group + aggregate route metadata
  -> one graph_bm25 MATCH:
     {business columns}:(query)
     AND routing_key:(scope token)
     AND routing_key:(selected groups, when admitted)
  -> hidden-rank identity window -> rowid hydrate
```

`graph_bm25` is the only FTS corpus. Its `routing_key` is indexed so SQLite intersects the business query, a scoped request's scope64 partition token, and any selected group tokens in one virtual-table `MATCH`; explicit business-column scoping prevents routing tokens from satisfying user text. The fixed BM25 column weight for `routing_key` is zero, so it has no direct term-score contribution. It nevertheless contributes to FTS5 document length and corpus average document length: v4 scores can differ from the pre-v4 baseline, while a document common to routed and flat execution over the same v4 table must have bitwise-identical scores. The ordinary SQL `source_scope` predicate remains the hard authorization filter rather than trusting the hash token.

The coarse owner uses a bounded 10-bit SimHash over path, labels, aliases, and content. `source_scope` participates in the indexed inventory, stable 64-bit partition token, and scope-qualified group identity, but not in topical SimHash. The versioned fingerprint is `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4`: each document inventory has at most 256 distinct safe-ASCII terms of at most 128 bytes, and a routable query has at most 32 such terms. The selector uses an aggregate-only, `A`-like adaptation, `global_idf * log2(1 + group_collection_frequency)^2`, rather than the paper's exact `A`. It does not implement `B(c,Q)` or claim document-level query-term co-occurrence. This adaptation is not the paper's balanced topical LDA and does not inherit its performance results.

Routing may run only when graph/route versions and algorithm identity are current, startup-reconciled source/global/sidecar populations agree, and the query is within bounded ASCII-term limits. For every query term, persisted global document frequency from `graph_bm25_route_term_totals` must equal a business-column-only `graph_bm25 MATCH` observation bounded to `df + 1`; every term must be at or below 20% of the global corpus, and all probes together may reserve at most 65,536 postings. The population must also pass minimum-size, group-count, per-group-size, skew, and selected-document-fraction gates. Matching groups must exceed the selection budget; the last selected group must then match more query terms than the first rejected group or lead its coarse score by at least 5%. An all-selected or ambiguous boundary falls back. All other admission failures disable hierarchy and continue through the established flat/fallback lexical path. A non-ASCII query bypasses the ASCII-only routing-term extractor and follows its existing flat FTS/fallback behavior. A routed query retries flat BM25 when it returns no rows or fewer distinct candidates than the requested limit under approximate selection. Transient SQLite query errors have bounded retries and then make the BM25 source unavailable for that search; non-transient planning or query errors surface.

The approximation boundary is explicit: same-v4 common-document score parity is required, but selected-group recall is not rank-safe. FTS5's hidden `rank` column applies the fixed `bm25(...)` weights and orders a bounded identity window; a second bounded query hydrates those identities by rowid. Exact ties that straddle the identity-window `LIMIT` do not promise deterministic membership. A routed hit's ranking explanation records the full algorithm fingerprint, `aggregate_tf_idf`, selected/matching group counts, selected/population document counts, and approximation state. Successful flat FTS rows preserve the planner's stable `hierarchical_bm25 fallback=<reason>` explanation, including `no_candidate_reduction`, `coarse_score_margin`, and the post-route `routed_candidate_retry`. If FTS has no row and a later LIKE/trigram fallback supplies a hit, that hit keeps the later layer's existing explanation behavior.

SQLite schema marker v4 owns one global `graph_bm25`, route state/documents/groups, per-group collection-frequency terms, and persisted global route-term document frequencies. Route documents contain document identity/kind/scope/path, `created_graph_version`, observable `label_gram_state`, group token, bounded term-count JSON, and an `fts_rowid NOT NULL UNIQUE` sidecar that is paired with `document_id` for exact mutation and verification. Document-write transactions maintain route-state document count and aggregate statistics with set-based JSON operations. Fresh-open reconciliation compares authoritative, active-global, route-document, grouped, semantic, vector, and persisted state populations plus route algorithm/version/freshness and semantic/vector generation markers; missing, incompatible, or count-inconsistent derived state is reconstructed from authoritative evidence and code documents. Canonical identity and per-row tokenizer consistency are checked while planning/finalizing a reconstruction that another stale/schema/count gate has already triggered; equal-count per-row drift alone does not trigger a fresh-open rebuild. The query hot path reads persisted version/count/DF values rather than running full-table `COUNT` or `SUM`, then applies the bounded business-column probe described above.

Reconstruction uses a shadow FTS generation. A durable owner/expiry lease publishes `building` together with a phase/cursor checkpoint and a fixed semantic/vector rebuild plan, so an expired attempt can be taken over and resumed. Each transaction admits at most 128 documents, 4 MiB of estimated authoritative source bytes, 8,192 labels, and 8,192 links. If the first document alone exceeds one or more work budgets, it runs in an isolated transaction and emits a warning with bounded identity fields; this progress exception is not an absolute per-document byte bound. The prior flat `graph_bm25` remains readable, while semantic, vector, and fuzzy lexical fallback are paused to avoid cross-generation companion reads. Stale label/semantic/vector rows are removed afterward with bounded rowid-keyset cleanup. Current evidence and code writers acquire `IMMEDIATE` transactions and reject writes while the lease is building. After completeness checks, a short transaction renames active to `graph_bm25_retired`, shadow to active, route state to `fresh`, and the schema marker to current; retired cleanup occurs after commit. A graph search holds one deferred read transaction across enabled sources, so a concurrent swap cannot split its SQLite snapshot.

Fuzzy labels are independently bounded to 256 labels per document, 1,024 UTF-8 bytes per label, 8,192 distinct grams per document, and an 8,192 distinct document-label-posting query probe. Multiple matching query grams for one normalized document label consume one posting; the label-gram primary key and the 64-query-gram cap therefore bound the joined rows behind each posting. Limit skips persist through `label_gram_state`, and posting exhaustion is an observable fuzzy-only degradation. Historical unscoped fallback uses version-leading global indexes for the semantic authorized-corpus probe, route label-state probe, and `label_lower` hydrate; scope-leading indexes remain for scoped requests. These bounds cover the BM25 and lexical-fallback owners only. Existing graph-evidence, path, temporal, community, and other hybrid layers are not thereby proven end-to-end bounded and require separate query-plan evidence.

Because v4 changes the global derived FTS schema, a binary-only rollback is not an exact rollback of the old numerical score baseline. A pre-v4 binary can provide its established flat behavior but does not maintain v4 route metadata or honor the current application's rebuild fence; any old-binary write also restores an older schema marker. On the next v4 open, that marker transition invalidates even superficially compatible route state and forces authoritative reconstruction before routing. Exact old-schema scores require restoring a pre-v4 database checkpoint. Upgrade/rebuild therefore requires exclusive database access, and old/new writers must never operate concurrently on one database.

## 5. Graph Expansion

Graph expansion starts from high-confidence candidates and stays within budget:

- Entity neighborhoods.
- Direct relation/claim/event paths.
- Schema-guided paths.
- Temporal predecessor/successor links.
- Code symbol reference/call/import edges.
- Local file path/content evidence relations.

Expansion results carry path provenance; they are not returned as opaque related context.

## 6. Context Pack

A context pack is the stable evidence bundle for agents and UI. It includes query metadata, retriever sources, rank explanations, context items, source spans, graph paths, structured facts, code artifacts, local file artifacts, freshness, degraded state, budgets, truncation reasons, and a traversal provenance trace. `provenance_trace` is a bounded query-time explanation object, not a persisted background task; within the authorized scope it records the graph version, routed intent, visited nodes/edges, cited evidence, visited-but-uncited context, ranking contributions, stale/degraded state, and redaction/truncation summary. Storage search outcomes apply the request-level trace budget before returning, and application/agent adapters re-apply final context budgets after rerank and citation marking so cited evidence remains auditable. Response-level truncation flags include trace budget truncation, not only result-count truncation.

Packing favors diversity and citability. Duplicate hits from the same parent evidence, symbol, or source span merge; low-confidence expansions do not displace direct evidence.

The codegraph context pack is a specialized one-call orchestration for coding agents. It runs bounded hybrid, definition, and symbol entry queries, expands top seeds through references, callers, callees, and imports, then deduplicates by file, symbol, edge, and line span before enforcing `max_context_bytes`. Its response separates entry points, related symbols, graph paths, impact hints, and code excerpts, each with retrieval layer, score, line range, and provenance. It reuses the existing code graph read model and freshness policy; it does not add storage schema, start background refresh, or replace diff-based impact analysis.

## 7. Acceptance Criteria

- Exact-term, conceptual, multi-hop, temporal, and code-symbol queries have corresponding retriever signals.
- Filename/path and file-content queries distinguish path, metadata, content, and change-cursor freshness.
- Results explain item source, rank contribution, and freshness.
- The named `bm25_hierarchy_suite` self-iteration gate proves same-v4 bitwise score parity for common documents, the hidden-rank single-FTS plan plus rowid hydration, scope/business/group intersection, fixture candidate reduction, every-term 20% DF admission, the 65,536-posting validation bound, current/complete route admission, historical global-index and fuzzy-limit guards, a bounded selected-document fraction, durable takeover/checkpoint/work-budget/swap invariants, and synthetic Recall@10 >= 0.9 on a generated 4,096-document flat-oracle fixture. It does not promise tied-boundary membership, natural-corpus quality, or an end-to-end bound for all hybrid layers; a versioned natural-vocabulary fixture must report Recall@k, p50/p95, and fallback rate before performance claims are accepted.
- Hierarchical selection never bypasses source scope or graph-version policy, and every failed admission gate retains the established non-hierarchical lexical path.
- Code exact-text fallback hits preserve `text_fallback` provenance and return degraded reasons when candidate-path or budget limits are hit; manual agent inspection documents the `rg`/`grep -RIn` fallback path separately.
- Broad-scope code exact-text fallback first narrows candidate paths through the indexed FTS read model using query, path filters, and language filters; it falls back to bounded scope enumeration only when the query has no indexed candidates.
- Degraded backends produce explicit degradation metadata instead of silent absence.

## 8. Smart Query Identifier Extraction

Query preprocessing (`retrieval/terms.rs`) recognizes and extracts code identifier patterns from natural language query text to improve FTS/BM25 recall:

| Pattern | Example | Extraction |
| --- | --- | --- |
| PascalCase / CamelCase | `UserService`, `signInWithGoogle` | Original + split parts |
| snake_case | `user_service`, `max_retries` | Original |
| SCREAMING_SNAKE_CASE | `MAX_RETRIES`, `API_KEY` | Original |
| dot.notation | `app.isPackaged` | Split segments |
| ALL_CAPS abbreviations | `REST`, `HTTP`, `LRU` | Original |
| Lowercase identifiers (3+ chars) | `render`, `parse`, `undo` | Original |

Stop-word filtering covers at least 80 common English words (the, and, for, with, from, how, what, etc.), excluding words that cannot correspond to code symbols during identifier extraction. Stem variant expansion generates matching candidates for English verb/noun inflections (connecting → connect, connected; renderer → render), broadening match coverage. Extracted PascalCase/CamelCase identifiers receive 1.5x weight in BM25/FTS queries, snake_case/SCREAMING_SNAKE identifiers receive 1.3x weight, and lowercase identifiers use a base weight of 0.8x.

---

Navigation: Previous: [8. Derived Indexes and Freshness](08-derived-indexes-and-freshness.md) | Next: [10. Semantic/Vector Provider Architecture](10-semantic-vector-provider-architecture.md)
