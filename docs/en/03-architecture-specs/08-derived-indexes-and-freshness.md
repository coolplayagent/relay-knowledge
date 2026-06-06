# Derived Indexes and Freshness

[English](../../en/03-architecture-specs/08-derived-indexes-and-freshness.md) | [中文](../../zh/03-architecture-specs/08-derived-indexes-and-freshness.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Derived indexes are valuable not only for recall speed but for explainable freshness. Every read model answers which scope, graph version, backend, model/dimension, stale state, and degraded reason it covers.

GraphRAG query responses expose the same `index_cursors` and `index_refresh` diagnostics as health and explicit refresh responses, so BM25, semantic, vector, and scoped cursor lag are visible at the point where an answer is produced.

## 2. Index Families

| Index family | Purpose |
| --- | --- |
| `bm25` | Lexical recall, aliases, code symbols, chunks |
| `semantic` | Local semantic signatures or semantic summaries before external embedding |
| `vector` | Vector nearest neighbors, image/text embedding metadata |
| `graph_path` | Schema paths, entity neighborhoods, multi-hop candidates |
| `community` | Community summaries and global context |
| `code` | Code symbols, references, calls, imports, chunk FTS |
| `local_file_path` | Normalized local file path, basename, directory tokens, extension, path trigrams/posting lists |
| `local_file_metadata` | File size, mtime, hash, MIME, language, permission snapshot, symlink/hidden/system metadata |
| `local_file_content` | File text chunks, BM25/trigram, optional semantic/vector metadata |
| `local_file_change_cursor` | Windows USN, macOS FSEvents, Linux inotify/fanotify, or bounded rescan cursor |

## 3. Local File Index Contract

Local file retrieval does not depend on Everything, Spotlight, Windows Search, locate, or any other external search tool. Platform event mechanisms may become watcher backends later, but relay-knowledge derived indexes must recover independently through bounded scans of authorized roots and persistent cursors.

Filename/path, metadata, and content indexes are separate. Interactive file location must not wait for OCR, archive expansion, large-file hashing, embeddings, or full-content extraction. Every file query applies source scope, authorized root, exclude/ignore rules, permission snapshot, and freshness policy before entering the candidate window.

`local_file_change_cursor` records last event, overflow, missed event, scan watermark, last scan error, and stale reason. If platform events are lost or cursors become invalid, queries return degraded or stale metadata and trigger bounded rescan instead of silently reporting freshness.

Local file-query responses carry a top-level freshness object with `state`, `graph_version`, source scope, root id, root cursors, index lag, stale/degraded reason, direct-source-read requirements, bounded-rescan requirements, returned paths that need direct reads, and agent instructions. Current public states are `fresh`, `pending`, `paused`, `stale`, `degraded`, and `overflow`. Overflow is a stale-answer control state: it means a bounded scan hit its configured budget, so callers may inspect stale indexed paths under `allow-stale` but `wait-until-fresh` must reject the answer until a bounded rescan completes successfully.

## 4. Freshness State Machine

```text
missing -> stale -> refreshing -> fresh
                 -> degraded
                 -> failed -> dead_letter
```

`fresh` means the index cursor covers the target graph version; it does not mean the facts are true. `degraded` may serve requests, but context packs explain missing families, backends, or scopes.

File-index freshness includes both graph version and file cursor watermark. A stale content index does not make the path index unavailable; responses state which layer is stale.

## 5. Refresh Scheduling

Refresh tasks come from mutation logs and explicit refresh requests. Scheduling satisfies:

- Queues are bounded and do not grow without limit.
- Tasks carry scope, family, target graph version, and source hash.
- Worker claims use leases and owners.
- Completion matches active lease, attempt, and target version.
- If graph version advances while a task runs, the cursor remains stale and follow-up work is enqueued.

Local file refresh tasks also carry root id, platform cursor, scan budget, max files per root, and content extraction budget. Content indexing, OCR, archives, and large-file parsing run only as background worker items, never on query hot paths.

## 6. Query Policy

Freshness policies include at least:

- `allow-stale`: return stale results with lag metadata.
- `wait-until-fresh`: wait for required indexes to reach the target version or return a stable timeout error.
- `require-fresh`: fail immediately on stale indexes without implicit refresh.
- `graph-only`: bypass derived indexes and return only authoritative graph facts or freshness diagnostics when no graph fact family exists for the request.

## 7. Acceptance Criteria

- `health` and context packs explain index lag, missing families, dead letters, and last errors.
- Explicit refresh enqueue failure returns a retryable error and never pretends to be fresh.
- Startup reconcilers replay missing refresh work from the mutation log.
- Local filename queries do not depend on content indexes; file query output states path, metadata, content, and change-cursor freshness or degradation, including bounded-scan overflow and direct source-read instructions.

---

Navigation: Previous: [7. Storage Engine and Mutation Log](07-storage-engine-and-mutation-log.md) | Next: [9. Hybrid Retrieval and Context Packing](09-hybrid-retrieval-and-context-packing.md)
