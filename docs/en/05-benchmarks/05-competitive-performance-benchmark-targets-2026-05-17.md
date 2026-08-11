# Competitive and High-Performance Benchmark Targets 2026-05-17

[English](../../en/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md) | [中文](../../zh/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md)

This page turns competitive and high-performance research into the metrics future benchmarks should track. It is not a measured run; it is a target list for regression gates and optimization experiments.

## 1. Retrieval Quality Metrics

| Scenario | Metrics |
| --- | --- |
| Hybrid graph QA | Recall@k, MRR, negative rejection, stale rejection, graph path coverage, context pack token budget. |
| Hierarchical graph BM25 | Flat-versus-routed Recall@k, selected-document fraction, route admission/fallback rate, same-v4 bitwise common-document score parity, single-FTS route-intersection plan conformance, routed/flat p50/p95. |
| Code retrieval | Exact symbol rank, caller/callee rank, import/reference resolution rate, source fallback recall/provenance, false positive count, impact precision, query p50/p95/p99. |
| Local file retrieval | Filename/path query p50/p95/p99, content query p50/p95/p99, permission-filter cost, candidate window size, stale/degraded rate. |

## 2. Indexing Performance Metrics

| Scenario | Metrics |
| --- | --- |
| Cold graph/code/file index | Indexed item count, elapsed time, peak RSS, write batch count, parse/extract throughput, index size. |
| Incremental update | Changed item count, affected item count, refresh elapsed time, cursor lag, missed event count, fallback rescan count. |
| Hierarchical BM25 derived rebuild | Authoritative/active-FTS/shadow-FTS/route-document/grouped/state/semantic/vector counts, durable owner/expiry and takeover state, phase/cursor and semantic/vector-plan recovery, transactions bounded by 128 documents, 4 MiB estimated source bytes, 8,192 labels, and 8,192 links, isolated oversized-document count and bounded-warning count, stale-row keyset cleanup, old-flat-reader availability, activation/retired-cleanup duration, peak RSS, WAL and temporary disk high-water marks, resulting graph/route/schema-marker state. |
| No-op refresh | Elapsed time, blob/file reads, SQLite writes, queued tasks created, freshness state. |
| Background worker | Queue depth, lease recovery count, dead-letter count, retry count, worker saturation, timeout count. |

## 3. Local File Retrieval Fixture Set

Future work should prepare three fixture levels:

- Small: 1K-10K files covering common documents, source files, hidden directories, ignore rules, and permission filtering.
- Medium: 100K-500K files covering multiple roots, deep directories, duplicate basenames, binaries, and large-file skips.
- Stress: 1M+ files or generated path lists focused on path/trigram/posting lists, metadata filters, watcher lag, and bounded rescan.

Each fixture should include:

- Exact filename queries, fuzzy path queries, extension queries, and directory-scoped queries.
- Content term queries, phrase queries, and combined size/mtime/MIME filters.
- Delete, rename, move, permission change, watcher overflow, and cursor invalidation recovery scenarios.

## 4. High-Performance Algorithm Observability Fields

Retrieval traces and benchmark output should record:

- Retriever family, candidate count, post-filter count, RRF rank contribution, rerank score, and truncation reason.
- Scope, authorization root, index cursor, graph/file/code version, stale lag, and degraded reason.
- Code source fallback trigger reason, candidate-file count, materialized bytes, `text_fallback` hit count, and candidate/budget degraded reason.
- Query latency breakdown: normalize, filter, candidate recall, scoring, graph expansion, context packing, and storage IO.
- Worker latency breakdown: enqueue, lease wait, scan/parse/extract, write batch, cursor commit, and reconcile.
- Hierarchical BM25: the `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4` algorithm version, aggregate signal name, scope64/group-token plan shape, selected/matching group count, selected/population document count, selected-document fraction, approximation state, per-term persisted/observed DF, reserved validation postings, high-frequency/budget fallback reason, fuzzy label/input-byte/gram/posting work, and whether the harness observed a flat retry.
- Hierarchical BM25 rebuild: lease owner/expiry/state, phase/cursor checkpoint, fixed semantic/vector plan, active/shadow/retired generation, documents/source bytes/labels/links and transactions per phase, isolated oversized-document and bounded-warning counts, writer-fence rejections, paused companion reads, stale-row cleanup cursor, swap duration, and whether one search snapshot observed exactly one active generation.

## 5. Deterministic Hierarchical BM25 Gates

The hierarchical-BM25 suite uses fixed generated fixtures, fixed source scopes, deterministic insertion order, and checked-in expectations. Its production-write/query-path fixture currently has one synthetic query and a flat `graph_bm25` oracle; it does not provide a checked-in natural-vocabulary query list or pin a runtime SQLite version. Test output must distinguish an implemented unit invariant from a corpus measurement; paper results are never copied into relay-knowledge result rows.

| Gate | Deterministic rule | Current evidence state |
| --- | --- | --- |
| Same-v4 score parity | For every common document under comparison, routed and flat execution against the same schema-v4 `graph_bm25` table must produce identical BM25 rank bits. This gate does not compare v4 scores with the v3 schema because the indexed zero-weight `routing_key` changes FTS5 length statistics. | `bm25_hierarchy_suite_preserves_global_score_in_single_fts_intersection` covers one representative common document; the corpus harness must extend it across all common routed/flat hits. |
| Single-FTS route intersection, hidden rank, and candidate-domain reduction | Execute the business-column query together with the zero-weight scope64 token and any selected scope-qualified group tokens in one `graph_bm25 MATCH`; an independent SQL scope predicate remains mandatory authorization. On every supported SQLite build exercised by CI, `EXPLAIN QUERY PLAN` must contain exactly one `graph_bm25` virtual-table plan node. The plan computes hidden rank for a bounded identity window, then hydrates by rowid through the `fts_rowid NOT NULL UNIQUE` route-document sidecar; it must not introduce a second route FTS. The fixed three-document fixture must retain two authorized flat candidates and one routed candidate. The 4,096-document synthetic production-write/query-path fixture must keep its planned-MATCH result domain below the flat result domain (currently 448 versus 768). | `bm25_hierarchy_suite_preserves_global_score_in_single_fts_intersection`, `bm25_hierarchy_suite_partitions_scoped_common_terms_and_keeps_sql_authority`, and `bm25_hierarchy_suite_production_routes_preserve_recall_and_reduce_candidate_domain` cover plan shape, result-domain reduction, partitioning, and authorization. Result-domain row counts are not FTS postings, VM steps, or latency measurements; equal-score cutoff membership is not deterministic. |
| Persisted-DF admission and probe budget | For every query term, persisted global DF must equal a business-column `MATCH` count bounded to `df + 1`; any term above 20% of global documents forces flat fallback, and the sum of reserved probes must not exceed 65,536 postings. | `bm25_hierarchy_suite_activates_only_for_complete_current_routes` covers mixed selective/common-term fallback, and `bm25_hierarchy_suite_bounds_term_validation_postings` covers the aggregate cap. A natural-query harness must also report observed probe work and mismatch fallback rate. |
| Sidecar, historical probes, and fuzzy-label bounds | `graph_bm25_route_documents` must retain `fts_rowid NOT NULL UNIQUE`, `created_graph_version`, and observable `label_gram_state`. Historical unscoped authorized-corpus, label-state, and `label_lower` probes must choose version-leading global indexes; scoped probes retain dedicated indexes. Fuzzy work stops at 256 labels per document, 1,024 UTF-8 bytes per label, and 8,192 distinct grams per document; exhausting the 8,192-posting query budget must be observable. | Schema-marker, migration-EQP, unscoped-fallback-EQP, lifecycle, and label-trigram tests cover the exact indexes, hydrate identity, limits, and exhaustion signal. These are bounded-work and plan-shape invariants, not natural-corpus quality measurements. |
| Shadow rebuild, fencing, and snapshot activation | Publish a durable `building` lease with owner/expiry, phase/cursor checkpoint, and fixed semantic/vector plan; keep the old flat FTS readable and allow an expired attempt to take over and resume. Each transaction accepts at most 128 documents, 4 MiB of estimated authoritative source, 8,192 labels, and 8,192 links. One document that exceeds a work budget runs alone and emits a bounded-identity warning; this guarantees progress but is not an absolute per-document byte bound. Reject current evidence/code writers through an `IMMEDIATE` fence, pause semantic/vector/fuzzy reads, and atomically commit active-to-retired/shadow-to-active renames with route `fresh` and the schema marker. | Named tests cover checkpoint takeover, every cumulative work budget, oversized-document isolation and warning bounds, the writer fence, paused companion reads, complete-reader activation, and swap rollback. Contention latency, WAL/disk high-water marks, and full-search snapshot behavior remain benchmark outputs rather than claimed production measurements. |
| Selected-document fraction | Every admitted routed plan is at or below the product cap of 25%. The fixed 5,000-document/100-group synthetic regression must continue to select 500 documents (10%) when 40 groups match. | Covered by `bm25_hierarchy_suite_bounds_selected_document_fraction`; this is a synthetic budget result, not a production-corpus measurement. |
| Coarse-boundary separation | Matching groups must exceed the budget. The last selected group must match more query terms than the first rejected group or have at least a 5% coarse-score lead; all-selected and equal-score dispersed-singleton cases must use flat fallback. | Covered by `bm25_hierarchy_suite_skips_routes_that_cannot_reduce_candidates` and `bm25_hierarchy_suite_falls_back_when_coarse_scores_do_not_separate`; these are safety admission invariants, not a recall proof. |
| Recall@k | For every fixed query and each declared `k`, compare routed IDs with the flat top-k oracle and report `|routed_top_k intersect flat_top_k| / k`, plus aggregate and worst-query values. The generated 4,096-document deterministic fixture requires Recall@10 >= 0.9. A natural-vocabulary release floor must be checked into its fixture manifest before claiming acceptable corpus quality. Equal-score cutoff membership is not required to be deterministic. | The named suite covers only the synthetic Recall@10 floor. Natural-corpus recall and performance remain unmeasured, so no production floor or speedup is claimed. |
| Query p50/p95 | Under one recorded hardware/storage/SQLite profile, run the same versioned warmup, repetition count, query order, and cache-state protocol for routed and flat paths; report both absolute latency and ratio. A ceiling is accepted only after the first reproducible baseline is checked in. | Not yet measured; the paper's latency and throughput are not a baseline. |
| Fallback rate | Count route-eligible attempts, routed completions, flat FTS retries, and later lexical-fallback outcomes separately; report `flat_retries / route_eligible_attempts`. Classify stable hierarchy reasons from successful flat FTS rows and classify FTS-empty LIKE/trigram outcomes from harness control flow rather than requiring a hierarchy explanation on those hits. Gate against a checked-in fixture expectation. | Not yet measured; flat-FTS explanation plumbing is covered, but no corpus fallback rate is claimed. |

The deterministic unit gates run as the named `bm25_hierarchy_suite` product gate in self-iteration `fast`, `full`, and `exhaustive` profiles. They protect same-schema score parity, one-FTS hidden-rank/rowid-hydrate plan shape, hard scope authorization, candidate reduction, every-term DF admission, the 65,536-posting validation cap, durable resumable shadow rebuilds with four cumulative work budgets, paused companion reads, version-leading historical indexes, fuzzy-label limits, set-based bounded aggregate maintenance, selection budget, and one synthetic Recall@10 floor. Retriever-source disabling is expected to suppress every disabled source family rather than only a subset. These gates do not establish deterministic membership at an equal-rank cutoff, rank safety, natural-corpus recall or performance, low-contention migration, or a speedup. Their hierarchical-BM25 bounds also do not imply that every pre-existing layer of the complete hybrid pipeline is end-to-end bounded. An optimization is not described as performance-complete until versioned natural-vocabulary Recall@k, p50/p95, selected-document-fraction, fallback-rate, rebuild-time, and temporary-resource rows are present with commands, corpus identity, and environment metadata.

## 6. Regression Principles

- Do not solve quality failures by enumerating benchmark queries, paths, symbols, or fixture names.
- Performance improvements must explain a general mechanism such as candidate pushdown, index structure, batching, cache, incremental update, or concurrency boundary.
- Source fallback is only bounded exact-text recovery; candidate lookup failures and exhausted budgets must record degraded reasons and must not bypass structured ranking or scope authorization.
- Filename and content queries need separate budgets; content indexing failures must not slow file location.
- Every metric must be reproducible from CLI, Web, or the benchmark harness and record commands, environment variables, and data versions.

## 7. Related Documents

- [Competitive, High-Performance, and Local File Retrieval Research 2026](../04-research/08-competitive-performance-research-2026.md)
- [Derived Indexes and Freshness](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [Hybrid Retrieval and Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Hierarchical BM25 Analysis 2026](../04-research/12-hierarchical-bm25-analysis-2026.md)
- [Code Retrieval Ranking and Impact Analysis](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)
- [C/C++ Syntax Self-Iteration Evaluation Set 2026-05-20](06-c-cpp-syntax-self-iteration-evaluation.md)
- [Multilingual Syntax Self-Iteration Evaluation Set 2026-05-20](07-multilingual-syntax-self-iteration-evaluation.md)
