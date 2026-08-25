# Hierarchical BM25 Analysis 2026

[English](../../en/04-research/12-hierarchical-bm25-analysis-2026.md) | [中文](../../zh/04-research/12-hierarchical-bm25-analysis-2026.md)

> Document version: 1.2
> Date: 2026-08-11
> Scope: Issue #350, paper evidence, implementation boundary, and evaluation plan

## 1. Sources and Decision

This note analyzes Umesh Deshpande and Swaminathan Sundararaman's *Hierarchical BM25: Lexical Search at Billion-Document Scale* using the [arXiv abstract](https://arxiv.org/abs/2608.00229) and [version 1 full text](https://arxiv.org/html/2608.00229v1). Numbers in the paper-results column below belong to that paper's corpus, hardware, and query mix. They are not relay-knowledge measurements.

The reusable idea is to make approximation a coarse routing decision while preserving corpus-wide BM25 scoring for every document that reaches fine search. relay-knowledge adopts that separation, but not the paper's complete clustering, storage, or two-signal selector.

## 2. What the Paper Establishes

The paper starts from a one-billion-document flat BM25 index of about 400 GB and reports 4–12 second disk-backed query latency. Its two-level design keeps a coarse index over roughly 1,000 topical, size-balanced clusters resident, selects a bounded set of clusters, and exhaustively searches the selected fine indexes using global `N`, document frequency, and average document length. This avoids the cross-cluster bias that would result from scoring each cluster with local IDF.

Cluster selection combines two signals:

```text
A(c, Q) = sum(idf(t) * (log2(max(f_c(t), 1)))^2)
Score(c, Q) = A(c, Q) + lambda * B(c, Q)
```

`A` measures aggregate query-term concentration in a cluster. `B` uses document-level postings for a selected set of discriminative, widely spread terms and records the strongest same-document co-occurrence evidence in each cluster. The paper starts with `lambda = 1`. Balanced topical LDA with capacity-capped assignment and local splitting prevents a popular topic from creating an oversized cluster that would defeat the query budget.

The paper reports about 4.4 GB resident memory and about 300 ms for sixteen-term queries at one billion documents, 4.7–5.6 times the throughput of its flat multi-threaded baseline, and about 32 warm-cache QPS versus fewer than 3 for the flat index. At the separate 500,000-document evaluation, visiting 5–10% of 500 clusters recovered 0.83–0.92 of the exhaustive result score. The paper explicitly leaves billion-scale recall, natural-vocabulary validation, isolation of the `B` signal's contribution, relevance-judged nDCG, and a direct document-reordered BlockMax-WAND comparison open.

These results demonstrate a cost/quality trade, not rank safety. A non-selected cluster can still contain a true global top-k document.

## 3. What relay-knowledge Implements

| Concern | Paper | relay-knowledge implementation |
| --- | --- | --- |
| Coarse groups | Roughly 1K balanced topical LDA clusters | Per-source-scope, content-driven 10-bit SimHash groups, giving at most 1,024 hash buckets per scope before empty buckets are removed |
| Group input | Topic vectors from selected lexical features | A bounded topical inventory of path, labels, aliases, and content; source scope has a zero-weight 64-bit partition token and participates in indexed statistics but is not a topical SimHash feature |
| Coarse metadata | Resident Level-1 routing structures | SQLite route state/document/group/term tables, an exact FTS-rowid sidecar, and persisted global route-term document frequencies |
| Selection signal | `A + lambda B` | An aggregate-only, `A`-like adaptation: `global_idf * log2(1 + group_collection_frequency)^2`; it is not the paper's exact `A`, and has no `B` or same-document co-occurrence index |
| Candidate and fine index | Per-cluster Level-2 indexes scored with global statistics | One global `graph_bm25` FTS5 table with an indexed `routing_key`; scoped queries intersect business terms, the scope64 partition token, and selected group tokens in one `MATCH` |
| Approximation | Cluster selection only | Route selection only; within the v4 index, a document shared by routed and flat results has bitwise-identical BM25 scores |
| Storage model | Fixed cache plus NVMe cluster indexes | Existing local SQLite runtime database; the paper's memory and latency bounds do not transfer |

The implementation fingerprint is `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4`. `topical4` means path, labels, aliases, and content determine the 10-bit SimHash group. `scope64-partition` means each document's indexed `routing_key` contains both a stable zero-weight 64-bit scope token and a scope-qualified group token. A scoped query intersects the explicitly business-column-scoped user terms, the scope token, and any selected group tokens in the same `graph_bm25 MATCH`; an unscoped query omits the scope token. The ordinary SQL `source_scope` predicate remains the hard authorization boundary and is never replaced by the hash token. `ascii-subset128b-256t` records the bounded safe-ASCII subset used for routing inventories, while `docidlen1` records the current FTS identity-length convention.

`bm25(graph_bm25, ...)` gives `routing_key` a fixed weight of zero, so scope and group tokens have no direct term-score contribution. They still participate in FTS5 document length and corpus average document length. Consequently, schema v4 changes the numerical baseline relative to a pre-v4 index; the parity claim is deliberately limited to routed versus flat execution over the same v4 table. Candidate selection uses FTS5's hidden `rank` column with the fixed `bm25(...)` weights and `ORDER BY rank`, first reading a bounded identity window and then hydrating the selected rows by FTS rowid. `graph_bm25_route_documents.fts_rowid` is `NOT NULL UNIQUE` and is paired with `document_id` during mutation and rebuild verification.

The selector reads persisted global document frequency from `graph_bm25_route_term_totals` and per-group collection frequency from `graph_bm25_route_terms`. For every query term, it verifies that persisted frequency with a business-column-only `graph_bm25 MATCH`, bounded to the expected frequency plus one row. The sum of these reserved probes is capped at 65,536 postings for the whole query. Every query term must have global document frequency at or below 20% of the corpus; one term above 20%, a probe-budget overflow, or any persisted/observed mismatch selects flat search. Document inventories remain capped at 256 distinct safe-ASCII terms of at most 128 bytes, and query routing accepts at most 32 terms under the same byte bound. These structures implement set-based aggregate maintenance; they are not a document-level co-occurrence index.

Derived reconstruction uses a shadow generation. Route state durably records owner/expiry, phase/cursor, and the semantic/vector rebuild plan before `graph_bm25_rebuild` is populated; an expired attempt can be taken over and resume from its checkpoint without silently changing that plan. Each transaction admits at most 128 documents, 4 MiB of estimated authoritative source bytes, 8,192 labels, and 8,192 links. A single document that exceeds one or more work budgets is isolated in its own transaction and emits a warning whose identity fields are bounded; this exception guarantees progress and is not an absolute per-document byte bound. The previous flat `graph_bm25` remains readable. Semantic, vector, and fuzzy lexical fallback are paused while state is `building`, and their stale rows are later removed by bounded rowid-keyset cleanup. Current evidence and code writers acquire an `IMMEDIATE` transaction and reject the write while the rebuild is active. Once identity, count, and tokenizer checks pass, one short transaction renames active `graph_bm25` to `graph_bm25_retired`, promotes the shadow, publishes route state `fresh`, and records the schema marker; the retired table is dropped only after commit. Each complete graph search runs inside one deferred read transaction, so its retrieval layers use one SQLite snapshot across the swap. The application fence is not understood by an older binary, so an upgrade that may rebuild v4 state requires exclusive database access with every old service and CLI writer stopped.

The route-document sidecar also stores `created_graph_version` and observable `label_gram_state`. Fuzzy label indexing accepts at most 256 labels per document, 1,024 UTF-8 bytes per label, and 8,192 distinct grams per document; limit skips and exhaustion of the 8,192 distinct document-label-posting query budget degrade only fuzzy fallback and remain observable. Multiple matching query grams for one normalized document label consume one posting. Historical unscoped fallback probes use version-leading global indexes for the semantic authorized-corpus check, route label-state check, and `label_lower` hydration, while scoped queries retain scope-leading indexes.

## 4. Admission Gates and Flat Fallback

Hierarchical routing is used only when all of the following hold:

- The requested graph version, current graph version, route-state version, and `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4` algorithm version agree and the route state is `fresh`.
- Fresh-open reconciliation has established equality across authoritative source, active global BM25, route-document, grouped-document, persisted route-state, semantic, and vector document counts; route algorithm/version/freshness and persisted semantic/vector generation markers are current. The write transaction maintains the persisted count and global document-frequency totals. Startup intentionally avoids an unbounded identity, per-row tokenizer, or aggregate deep scan. Canonical identity and tokenizer consistency are checked only after another stale/schema/count condition has triggered reconstruction, so equal-count per-row drift alone does not trigger a fresh-open rebuild. The query hot path does not run full-table `COUNT` or `SUM` reconciliation; instead, each actual query term's persisted global frequency must equal its bounded business-column `MATCH` observation.
- The selected population contains at least 4,096 documents and 8–2,048 non-empty groups.
- Every group contains at most 512 documents and no more than `max(2 * ceil(mean group size), 64)` documents.
- Those structural gates cap an admitted scoped population at 524,288 documents and an admitted unscoped population at 1,048,576 documents; larger populations use flat search. They are not evidence of billion-document product scale.
- Every query term has global document frequency at or below 20% of the corpus, and all per-term validation probes together reserve no more than 65,536 postings.
- The group budget, `ceil(group_count / 10)` clamped to 4–32, selects no more than 25% of the population.
- Matching groups must exceed the budget so that routing makes an approximate cut. The last selected group must then match more query terms than the first rejected group or have a coarse score at least 5% higher. An all-selected or ambiguous dispersed-singleton/equal-score boundary disables hierarchy.

Non-ASCII or over-budget routing queries, stale/historical versions, incomplete statistics, small or skewed populations, any query term above the 20% document-frequency ceiling, validation-probe budget exhaustion, oversized selections, or transiently unavailable routing state disable hierarchy and continue through the established flat/fallback lexical path. A non-ASCII query bypasses the safe-ASCII routing-term extractor and follows its existing flat FTS/fallback behavior. After an attempted routed query, an empty result or fewer distinct candidates than the requested limit retries flat BM25 before later fallback levels. Transient SQLite query errors have bounded retries and then make BM25 unavailable for that search; non-transient planning or query errors surface rather than being hidden as fallback. Failed routing admission therefore degrades only the optimization, while database failures retain their established error semantics. Request-level `disabled_retriever_sources` applies to BM25/code-graph rows, graph evidence, semantic, vector, graph path, temporal, and community-summary sources; disabling one source cannot be bypassed by merge or fallback orchestration.

Routed hits carry an explanation with the algorithm version, `aggregate_tf_idf` signal, selected/matching group counts, selected/population document counts, and whether selection was approximate. When flat FTS returns rows, those rows carry a stable `hierarchical_bm25 fallback=<reason>` explanation, including non-routable query, stale generation, incomplete route index/statistics, population guard, low selectivity, no candidate reduction, insufficient coarse-score margin, candidate budget, unavailable route state, or routed-candidate retry. If FTS itself returns no row and LIKE/trigram or another lexical fallback supplies the hit, that later layer retains its existing explanation behavior and is not guaranteed to carry the hierarchy reason.

## 5. Correctness Boundary and Risks

- **Same-schema score parity is narrower than result parity.** The same v4 FTS table gives a bitwise-identical score to a document shared by routed and flat results, but an approximate route may omit a higher-scoring document.
- **Equal-score cutoff membership is not deterministic.** The hidden-rank SQL window is ordered by BM25 rank, then the bounded in-memory window uses `(rank, visible evidence ID, document ID)` before parent collapse and hydration. When more documents tie exactly across the SQL `LIMIT` boundary, the implementation still does not promise which tied document enters that bounded window.
- **v4 is a scoring-schema migration.** A zero-weight `routing_key` still changes FTS5 document-length statistics, so v4 scores are not promised to equal the old-schema flat baseline.
- **SimHash is not balanced topical LDA.** The group-size and population gates reject unsafe layouts; they do not make accepted groups semantically optimal.
- **The selector is aggregate-only and `A`-like, not the paper's exact `A`.** Its transformed collection-frequency score cannot distinguish several discriminative terms appearing together in one document from those terms being scattered across documents. The boundary-separation gate rejects an ambiguous cutoff but does not reproduce the paper's `B(c,Q)`; that mechanism is deliberately not claimed or emulated.
- **Fallback is a safety valve, not a recall proof.** Returning at least the requested number of routed candidates does not prove that the true flat top-k is present.
- **Paper performance does not transfer.** SQLite, corpus size, vocabulary, storage, hardware, query length, and cache behavior differ. relay-knowledge needs its own Recall@k, selected-document fraction, fallback-rate, and p50/p95 measurements.
- **Authorization remains independent.** `source_scope` is a hard SQL filter and part of scope-specific route identity; it is not treated as evidence that a partition is topically balanced.
- **The application write fence is version-local.** Current evidence/code writers serialize with the rebuild and reject writes while its durable lease is active, but an older binary does not perform that check. Cross-version upgrade safety therefore depends on exclusive access, not on the application fence alone.
- **These bounds are BM25-local.** They do not prove an end-to-end bound for pre-existing graph-evidence, path, temporal, community, or other hybrid layers; those layers require their own query-plan and corpus measurements.

## 6. Acceptance and Next Evidence

The named `bm25_hierarchy_suite` self-iteration gate runs deterministic regressions that prove bitwise common-document parity within v4, the hidden-rank single-FTS plan and rowid hydration, scope-partition/business/group intersection, candidate reduction in the fixed fixture, fallback when any query term exceeds the high-frequency ceiling, the 65,536-posting validation bound, current/complete route admission, historical global-index plans, label/posting limits, ambiguous-cutoff guards, and the selected-document budget. It also covers durable rebuild takeover, phase/cursor and semantic/vector-plan persistence, writer fencing, document/source-byte/label/link transaction budgets, oversized-document isolation, complete-reader shadow swaps, swap rollback, and bounded route-term persistence. A generated 4,096-document fixture requires routed Recall@10 against its flat oracle to be at least 0.9. These are structural and synthetic deterministic gates, not natural-corpus quality evidence or production-speed evidence. A fixed, versioned natural-vocabulary fixture must still report Recall@k, selected-document fraction, query p50/p95, and fallback rate. Until those rows exist, no natural-corpus recall, latency, throughput, fallback value, deterministic tied-boundary membership, or end-to-end hybrid bound is claimed for relay-knowledge.

Adding `B(c,Q)` later requires a bounded document-level postings design, explicit resource budgets, an isolated effectiveness study, and evidence that recall improves enough to pay for its storage and query cost. It must not be approximated with a cluster-level term table, because that table cannot observe same-document co-occurrence.

## 7. Related Documents

- [Hybrid Retrieval Advantage](../02-capabilities/05-hybrid-retrieval-advantage.md)
- [Hybrid Retrieval and Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Installation, Release, and Upgrade](../03-architecture-specs/19-installation-release-and-upgrade.md)
- [Competitive and High-Performance Benchmark Targets](../05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md)

---

Navigation: Previous: [11. Software Global Modeling, CodeGraph, and Search Everything Comparison 2026](11-software-global-codegraph-search-everything-comparison-2026.md)
