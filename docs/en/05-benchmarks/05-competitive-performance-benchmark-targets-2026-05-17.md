# Competitive and High-Performance Benchmark Targets 2026-05-17

[English](../../en/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md) | [中文](../../zh/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md)

This page turns competitive and high-performance research into the metrics future benchmarks should track. It is not a measured run; it is a target list for regression gates and optimization experiments.

## 1. Retrieval Quality Metrics

| Scenario | Metrics |
| --- | --- |
| Hybrid graph QA | Recall@k, MRR, negative rejection, stale rejection, graph path coverage, context pack token budget. |
| Code retrieval | Exact symbol rank, caller/callee rank, import/reference resolution rate, source fallback recall/provenance, false positive count, impact precision, query p50/p95/p99. |
| Local file retrieval | Filename/path query p50/p95/p99, content query p50/p95/p99, permission-filter cost, candidate window size, stale/degraded rate. |

## 2. Indexing Performance Metrics

| Scenario | Metrics |
| --- | --- |
| Cold graph/code/file index | Indexed item count, elapsed time, peak RSS, write batch count, parse/extract throughput, index size. |
| Incremental update | Changed item count, affected item count, refresh elapsed time, cursor lag, missed event count, fallback rescan count. |
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

## 5. Regression Principles

- Do not solve quality failures by enumerating benchmark queries, paths, symbols, or fixture names.
- Performance improvements must explain a general mechanism such as candidate pushdown, index structure, batching, cache, incremental update, or concurrency boundary.
- Source fallback is only bounded exact-text recovery; candidate lookup failures and exhausted budgets must record degraded reasons and must not bypass structured ranking or scope authorization.
- Filename and content queries need separate budgets; content indexing failures must not slow file location.
- Every metric must be reproducible from CLI, Web, or the benchmark harness and record commands, environment variables, and data versions.

## 6. Related Documents

- [Competitive, High-Performance, and Local File Retrieval Research 2026](../04-research/08-competitive-performance-research-2026.md)
- [Derived Indexes and Freshness](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [Hybrid Retrieval and Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Code Retrieval Ranking and Impact Analysis](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)
- [C/C++ Syntax Self-Iteration Evaluation Set 2026-05-20](06-c-cpp-syntax-self-iteration-evaluation.md)
- [Multilingual Syntax Self-Iteration Evaluation Set 2026-05-20](07-multilingual-syntax-self-iteration-evaluation.md)
