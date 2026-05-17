# Competitive, High-Performance, and Local File Retrieval Research 2026

[English](../../en/04-research/08-competitive-performance-research-2026.md) | [中文](../../zh/04-research/08-competitive-performance-research-2026.md)

> Document version: 1.0
> Prepared: 2026-05-17
> Scope: GraphRAG, hybrid search, vector indexes, code search, local file retrieval, graph storage, authorization graphs, high-performance algorithms, and SRE practice.

## 1. Research Positioning

| Dimension | Conclusion |
| --- | --- |
| Sources | Official product and specification docs, system papers, database and search-engine engineering material, internal benchmarks, and current architecture constraints. |
| Goal | Turn external high-performance systems practice into actionable relay-knowledge recommendations for competitive capability, indexing, retrieval, background service operation, and benchmarks. |
| Competitive judgment | The durable advantage is not a single GraphRAG algorithm; it is local-first operation, version consistency, authorization, unified code/file/graph retrieval, explainable context packs, and recoverable background indexing. |
| Performance principle | Narrow candidates before ranking, filter authorization and scope before recall, isolate hot and cold paths, and attach budget, freshness, and degradation diagnostics to every index and worker. |

## 2. Cross-Domain Conclusions

- GraphRAG papers and products converge on query routers, local/global retrieval, community summaries, path organization, and incremental refresh. Unbounded k-hop expansion or larger top-k windows mostly increase noise and token cost.
- High-performance vector retrieval depends on HNSW, PQ/IVF, disk-resident graph indexes, quantization, filter-aware search, and multi-stage reranking. Vector indexes are candidate and ranking signals, not fact sources.
- Mature full-text and hybrid search combines inverted indexes, BM25/BM25F, trigram or posting-list indexes, RRF, and phased ranking. Rank-based fusion is safer when retriever scores are not calibrated to the same scale.
- Competitive code search combines exact symbols, trigram/regex, BM25, AST structure, reference/call/import edges, language and path filters, revision scopes, and impact analysis instead of relying on code embeddings alone.
- Fast local file retrieval must separate filename/path, metadata, content, and change cursors into independent read models. Everything, Spotlight, Windows Search, plocate, and ripgrep are mechanism references, not runtime dependencies.
- Large graph and authorization systems emphasize caching, relationship models, causal ordering, front-loaded permission checks, and low-latency authorization. Context packs and file/code queries must not defer authorization until after final truncation.
- High-performance resident operation requires bounded queues, leases, dead letters, replay, adaptive concurrency, timeouts, cancellation, correlated traces/metrics/logs, and explicit overload behavior.

## 3. Reference Map

| Domain | Representative references | Lessons to absorb |
| --- | --- | --- |
| GraphRAG and Hybrid RAG | Microsoft GraphRAG/DRIFT, LightRAG, E^2GraphRAG, ROGRAG, Practical GraphRAG, PolyG, EA-GraphRAG | Query-mode selection, local/global fusion, entity-chunk bidirectional indexes, incremental graph construction, path pruning, and result verification. |
| Vector retrieval | HNSW, FAISS, DiskANN, ScaNN, ACORN, Vespa constrained ANN | Proximity graphs, quantization, disk residency, filter-aware ANN, target hits, and recall/latency/memory tradeoffs. |
| Full-text and hybrid search | Lucene BM25, Vespa hybrid/phased ranking, OpenSearch RRF, Azure AI Search RRF | Inverted indexes, RRF, phased ranking, front-loaded filtering, and explainable rank contribution. |
| Code search | GitHub Code Search/Blackbird, Sourcegraph/Zoekt, Tree-sitter, ripgrep, persistent trigram indexes | Trigram candidates, regex literal extraction, symbol priority, AST chunks, ignore rules, parallel traversal, and versioned indexes. |
| Local file search | Everything, Windows Search, Spotlight/FSEvents, Linux inotify/fanotify, plocate | Filename and metadata first, system change journals, posting lists and trigrams, permission visibility, and bounded rescans after cursor overflow. |
| Graph storage and authorization graphs | Facebook TAO/RAMP-TAO, Google Zanzibar | Relationship-graph caching, read-heavy optimization, permission relations before retrieval, external consistency, and low-latency authorization checks. |
| Storage and updates | RocksDB/LSM, WAL, mutation logs, materialized views | Batched writes, background compaction, crash recovery, incremental views, and hot-query isolation. |
| Runtime reliability | Google SRE overload, Envoy adaptive concurrency, OpenTelemetry | Overload protection, adaptive concurrency, retry suppression, end-to-end traces, metrics, and error classes. |

## 4. Local File System Retrieval Recommendations

Local file retrieval should be its own derived-index family rather than an attachment to evidence or code repository indexes. Split it into four read models:

| Read model | Content | Design requirement |
| --- | --- | --- |
| `local_file_path` | Normalized path, basename, directory tokens, extension, path trigram/posting list | Supports millisecond-level filename and path lookup; source scope, authorized roots, ignore/exclude rules, and freshness policy apply before candidates are scored. |
| `local_file_metadata` | Size, mtime, hash, MIME, language, owner/permission snapshot, hidden/system attributes, symlink state | Powers filtering, ranking, and diagnostics; missing metadata must not block path queries. |
| `local_file_content` | Text chunks, BM25/trigram index, optional semantic/vector metadata | Content indexing is selective by file type, size, and resource budget; large files, binaries, OCR, and archives run through background workers. |
| `local_file_change_cursor` | Windows USN, macOS FSEvents, Linux inotify/fanotify, or bounded rescan cursor | Records last event, overflow, missed event, scan watermark, stale reason, and next reconcile entry point. |

Recommended query flow:

```text
normalize file query
  -> resolve authorized local file scope
  -> apply path/exclude/permission/freshness filters
  -> path and metadata candidate recall
  -> optional content/semantic recall
  -> RRF or phased rerank
  -> return hits with freshness, cursor, permission, truncation, and degraded metadata
```

Hard constraints:

- Do not require Everything, Spotlight, Windows Search, locate, or external daemons to function; platform capabilities may become watcher backends or import sources later.
- Keep filename/path indexing separate from content indexing so slow extraction does not delay interactive file location.
- If change events are unreliable or overflow, return degraded or stale reasons and trigger bounded rescan instead of silently reporting freshness.
- Permission and scope filtering must run before candidate windows so unauthorized paths do not enter ranking, traces, or context packs.
- File-system workers must obey queue capacity, scan timeout, max file bytes, max files per root, and IO budgets.

## 5. High-Performance Algorithm Implications

- **Candidate narrowing**: use inverted lists, trigrams, path tokens, symbol names, and scope/path/language filters to reduce candidates to bounded windows before expensive scoring.
- **Hybrid fusion**: use RRF or phased ranking for BM25, semantic, vector, graph path, code edge, and file path candidates; only combine raw scores linearly when they share source and scale.
- **Path pruning**: multi-hop graph retrieval uses query intent, schema paths, edge confidence, time range, and max token/edge/hop budgets instead of unbounded neighborhood expansion.
- **Incremental first**: Git diffs, mutation logs, file change cursors, and source hashes drive refresh; full rescans are reserved for cold start, reconciliation, or invalid cursors.
- **Hot/cold isolation**: query hot paths read committed read models; OCR, embedding, parsing, content extraction, compaction, and large-file hashing run behind worker or maintenance boundaries.
- **Cache and invalidation**: cache keys include scope, graph version, index cursor, query policy, and authorization summary; graph/file/code changes must explain affected indexes.
- **Concurrency control**: admission control, adaptive concurrency, timeout, cancellation, and retry backoff are performance features, not operations extras.

## 6. Improvement Recommendations

| Priority | Recommendation | Acceptance signal |
| --- | --- | --- |
| P0 | Define local file retrieval as four read models: `local_file_path`, `local_file_metadata`, `local_file_content`, and `local_file_change_cursor`. | Docs state that filename queries do not depend on content indexes and every file query returns freshness or degraded reason. |
| P0 | Record candidate window, filter count, RRF contribution, truncation reason, and stale lag for code, file, and graph hybrid retrieval. | Context pack and benchmark docs include observable fields and p95/p99 metrics. |
| P1 | Add a file-content indexing route: text chunk BM25/trigram first, semantic/vector optional, OCR/archive/large-file processing through workers. | Filename and content queries have separate latency budgets; content failures do not affect path indexes. |
| P1 | Introduce a query router for exact terms, conceptual questions, multi-hop, code symbols, file paths, impact, and temporal queries. | Each query class has explicit retriever families, budgets, and degradation behavior. |
| P1 | Add cold indexing, incremental update, no-op refresh, watcher lag, and queue lag to benchmark gates. | Benchmark chapters record targets, collection commands, and regression thresholds. |
| P2 | Evaluate pluggable platform watcher backends and ANN backends. | Missing backend capability degrades to bounded rescan or local lexical read models. |

## 7. Sources

- Microsoft GraphRAG query engine: https://microsoft.github.io/graphrag/query/overview/
- Microsoft Research DRIFT Search: https://www.microsoft.com/en-us/research/blog/introducing-drift-search-combining-global-and-local-search-methods-to-improve-quality-and-efficiency/
- LightRAG: https://arxiv.org/abs/2410.05779
- E^2GraphRAG: https://arxiv.org/abs/2505.24226
- ROGRAG: https://aclanthology.org/2025.acl-demo.58/
- HNSW: https://arxiv.org/abs/1603.09320
- FAISS billion-scale similarity search: https://arxiv.org/abs/1702.08734
- DiskANN: https://papers.nips.cc/paper/9527-diskann-fast-accurate-billion-point-nearest-neighbor-search-on-a-single-node
- Google ScaNN: https://research.google/blog/announcing-scann-efficient-vector-similarity-search/
- Vespa nearest neighbor and hybrid search: https://docs.vespa.ai/en/querying/nearest-neighbor-search
- OpenSearch RRF hybrid search: https://opensearch.org/blog/introducing-reciprocal-rank-fusion-hybrid-search/
- Sourcegraph Code Search: https://sourcegraph.com/docs/code-search/features
- Zoekt: https://github.com/sourcegraph/zoekt
- ripgrep performance notes: https://burntsushi.net/ripgrep/
- Everything indexes and USN journal: https://www.voidtools.com/support/everything/indexes
- Everything FAQ: https://www.voidtools.com/faq/
- Apple FSEvents: https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/TechnologyOverview/TechnologyOverview.html
- Linux inotify: https://man7.org/linux/man-pages/man7/inotify.7.html
- plocate: https://plocate.sesse.net/
- Google Zanzibar: https://www.usenix.org/conference/atc19/presentation/pang
- Meta RAMP-TAO: https://engineering.fb.com/2021/08/18/core-infra/ramp-tao/
- RocksDB: https://rocksdb.org/index.html
- Google SRE cascading failures: https://sre.google/sre-book/addressing-cascading-failures/
- Google SRE overload: https://sre.google/workbook/overload/
