# Tree-sitter Extraction and Incremental Indexing

[English](../../en/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md) | [中文](../../zh/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)

> Document version: 2.0
> Date: 2026-05-30
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Tree-sitter is the entry point for code structure, not a complete semantic analyzer. The architecture connects grammar registration, query capture, error degradation, incremental candidate narrowing, and index refresh into a recoverable pipeline. Unsupported languages or unrecoverable parse errors degrade local capability only and do not break retrieval; recoverable C/C++ macro, preprocessor, and decorator parse errors should keep structured facts available when extraction remains reliable.

## 2. Language Registry

Each language registration includes language id, file extensions, tree-sitter grammar, capture queries, comment rules, identifier segmentation, and fallback chunker. When grammar is missing, files still enter text chunk and BM25 paths. Query-time source fallback is not a grammar substitute; it only adds exact-text evidence from indexed source candidates and cannot create graph facts.

Configuration, build, and template grammars are registered under the code configuration module rather than runtime configuration. The supported surface includes Markdown, XML, Bazel/Starlark, Make, CMake, Dockerfile/Containerfile, Java properties, TOML, INI/`.conf`, YAML, JSON, Go module files, Ninja, Jinja2, and Go templates. These formats emit ordinary file, symbol, reference, import, dependency, feature-flag, and chunk facts so query APIs do not need a separate schema for configuration search. SQL is registered as a code grammar for `.sql` files; it emits schema object symbols for tables, views and materialized views, functions and procedures, triggers, and types, plus SQL object references and call references through the same repository code graph tables. Hierarchical configuration formats must emit stable dot-separated paths; arrays and array tables use `[]` instead of numeric indexes, for example `server.port`, `containers[].name`, and `bin[].name`.

Structured documentation and configuration files combine Tree-sitter AST extraction with product rules. Tree-sitter identifies Markdown headings, link definitions, inline links, JSON pair/array nodes, and INI section/setting nodes with ranges and error nodes; product rules normalize JSON arrays into `[]` paths, join INI sections and settings into dot-separated names, filter external or anchor-only Markdown links, and write local Markdown links as unresolved import metadata. Markdown and configuration files must keep a file-level chunk even when symbol-level chunks exist so body text, config values, and local parse failures remain retrievable through BM25/hybrid search.

Parser implementation must keep language-specific rules under cohesive language directories. Node-kind classification, language-specific import extraction, and C/C++ manual recovery belong under `src/relay_knowledge/code/parser/languages/<language>/`; shared parsing flow, syntax helpers, text validation, dependency manifest parsing, and chunk construction remain in parser-level modules.

## 3. Capture Contract

Query captures emit a common structure: definitions, references, calls, imports, feature flag/config usage, documentation comments, symbol spans, body spans, and chunk spans. Capture output is validated for scope, path, line/column, and content hash before write.

## 4. Full Build

```text
resolve snapshot
  -> enumerate authorized files
  -> batch parse and chunk
  -> write file/symbol/reference/feature-flag/chunk facts
  -> finalize cross-batch edges
  -> refresh scoped code search documents and software projection
  -> mark scope fresh
```

The old fresh scope continues serving queries during full builds; the new scope becomes fresh only after finalize succeeds.

## 5. Incremental Update

Incremental indexing first narrows the work set:

1. Use Git diff/status and blob hashes to find changed files.
2. Include deleted, renamed, and moved files.
3. Expand affected files through reverse dependencies and import/call/reference edges.
4. Refresh only affected code facts, chunks, and index families.

Manual `repo update` and resident commit reconciliation both resolve the selected base/head refs to immutable commits and submit the resulting `Incremental` request to the durable code-index task queue. Omitting the manual base chooses the last published clean snapshot, including the clean commit inside a worktree-overlay identity; omitting the head chooses `HEAD`. A repository without that base must complete a full index first. The Git diff is hard-capped at 512 changed paths across the commit pair before registered path filters. Exceeding the cap requires a full index; it is not permission to make the queue, parse set, or write transaction unbounded.

When an explicit `Full` Git initialization opts into historical reuse and targets a scope that is not fresh, it walks at most 10 ancestors on the immutable target commit's first-parent chain. The application queries commit-to-scope aliases from nearest to farthest and accepts only a published scope that is not stale or retiring, has compatible filters, and matches the current code-fact version. A match pins the request as a real `Incremental` task and exposes the base/head through task mode and the completion summary. An unfinished full or incremental task for the same target and requested scope is reused first. If the candidate-to-target diff exceeds the 100 changed-path initialization-reuse limit, the planner does not try older ancestors and falls back directly to the checkpointed full index; requests without the opt-in, filesystem sources, and histories without a matching base remain on the full path. Explicit incremental updates retain their separate 512 changed-path limit. Ancestor enumeration, diff preflight, and later parsing stay behind the blocking-worker boundary, and Git probe failures must not be silently treated as “no base.”

The resident FileWatcher treats `.git/HEAD`, ref, packed-ref, and HEAD-log events as low-latency hints. At startup and on a bounded periodic interval (default 5000 ms), it independently resolves the checked-out HEAD/tree; this reconciliation covers linked worktrees and missed/coalesced events. An advanced HEAD is pinned with the last published clean base into a durable task. A stable fingerprint per repository, checked-out ref, and filter set coalesces repeat hints while the slot is unfinished. Queue admission is transactional and capped at 32 unfinished tasks per repository and 256 globally. The attempt-scoped lease and a monotonically advancing publication generation remain the single-writer authority; every snapshot, batch, workspace, and software-projection transaction validates the live generation before commit, so commit events and detached expired attempts cannot bypass bounded retry/backoff, recovery, or dead-letter state. Full rebuilds retain their batch checkpoints; a bounded incremental attempt is an atomic snapshot transaction and does not claim per-path checkpoint progress.

Import dependency expansion prioritizes indexed code maps and versioned import edges. If an import points to an external dependency or cross-repository target without a code map, the indexer records only the unresolved target hint, resolution reason, and affected current-repository facts; it does not trigger an unauthorized full scan to fill that dependency. This coverage gap is not parser, file, scope, or response degradation. The query layer may use the hint inside the same scope to trigger bounded internal source fallback.

Local configuration relationships resolve only inside the same indexed source scope. Finalization may resolve deterministic local file references, template includes, and build-target references after all files in that scope have been written. Ambiguous local matches and external images, packages, remote labels, or templates remain unresolved or ambiguous metadata rather than degraded parser state.

Feature-flag extraction is an indexing-stage responsibility. Runtime config reads, boolean config declarations, and guarded-code relationships are written as versioned facts under the file scope; the query layer reads only those facts and their FTS documents. Boolean declarations in TOML, YAML, JSON, INI, Java properties, and related config formats reuse the configuration extractor's structured config-key facts instead of a separate feature-flag source. Changes to extractor rules, config files, or guarded branches require a full or incremental index refresh for the affected scope.

Successful publication runs bounded retention only after the new scope and software projection complete. The protected set is the union of active and a rolling window of the two latest successful publications (normally including active), the latest successful incremental predecessor, the clean base of any active worktree overlay, plus all unfinished task targets/bases and repository-set pins. Cleanup atomically marks one older scope `retiring`, removing it from read/base resolution, and persists a restart-safe job. Each subsequent maintenance transaction advances at most one scope-GC phase, whose physical deletion is capped at 512 rows in aggregate across affected application tables, including facts, code FTS/search documents, software projections, checkpoints, workspace state, or scope metadata. The same pass separately deletes at most 512 succeeded task-audit rows, 512 failure-class task-audit rows, and 512 commit-alias rows, capping primary cleanup at 2,048 physical rows plus at most one terminal GC-job bookkeeping row. Same-tree commits reuse the content scope through a bounded 256-row commit-alias window. Finished task history is bounded to 128 successful and 64 failed/dead-letter/cancelled rows per repository, preserving the newest success for each retained scope. Status reports pending/job phase/deleted rows/error, and the managed maintenance worker resumes after failure. Pruned commits require full indexing. This scoped code/software contract is not an atomic publication claim for the generic Knowledge Graph or independent semantic/vector generations.

Every explicit- or implicit-scope-resolving code read must use one deferred SQLite read snapshot from initial scope/`retiring` resolution through all dependent SELECTs. This includes code fact/search reads, repository status/list/scope-status/latest-scope-status, and repository-set status/cross-edge reads; it does not generalize to unrelated single-SELECT reporting. Concurrent phased cleanup may expose the complete old snapshot to that request or a clear retiring error to a later request, never a mixture of partially deleted scope state.

## 6. High-Performance Boundaries

Code indexing follows the shared principles behind Sourcegraph/Zoekt, GitHub Code Search, ripgrep, and Tree-sitter based systems: narrow candidates through path, language, trigram, symbol name, and blob hash before AST capture, edge resolution, or semantic/vector refresh. AST chunks should follow function, type, module, documentation comment, and import-block boundaries; fallback text chunks take over only when structural parsing is unavailable.

Cold full indexing, semantic embedding, cross-batch edge finalization, large-file skip/hash, and parser-heavy work belong behind master-supervised background worker or maintenance boundaries and do not block query hot paths. Code-index workers claim durable tasks through application services, hold attempt-scoped leases, and execute bounded parse/write batches; interface layers and query paths must not invoke tree-sitter full indexing directly. Incremental indexing records changed file count, affected file count, parse throughput, write batch count, candidate windows, and stale lag so hidden full scans are visible.

Full-index batches are bounded simultaneously by file count, byte count, and write-row count. Large-repository cold-index throughput may improve through larger bounded batches, parser-worker parallelism, removal of redundant SQLite probes on empty scopes, prepared-statement reuse, indexed FTS metadata cleanup, or phase-level finalization checkpoints, but it must not skip FTS/search-document writes, edge finalization, checkpoints, freshness checks, or degraded/status reporting. Any register-to-index performance optimization must leave a regression budget or guardrail for `index_ms`, `register_index_ms`, and post-finalization ref queryability in self-iteration `fast` or `--categories performance`.

Generated-source detection is a metadata step, not an indexing skip. The detector combines path patterns such as `.pb.go`, `.pulsar.go`, `.generated.ts`, `.auto.ts`, `.min.js`, `.min.css`, repository-root `dist/` and `build/` outputs, and `target/generated/` with bounded header heuristics such as `Code generated by`, `auto-generated`, `@generated`, and swagger/OpenAPI generator comments. Large text files without a path or header signal remain ordinary source metadata and are handled by the separate large-file degradation policy. Generated files still produce file, symbol, edge, chunk, FTS, and index facts; file rows carry `is_generated`, summaries and reports split handwritten versus generated symbols, representative query seeds ignore generated symbols, and query requests may set `exclude_generated` to remove generated files from structured retrieval and bounded source fallback.

Query-time source fallback follows the same blocking-worker boundary as Git blob reads. The product path uses an internal fixed-string scanner over a temporary tree of bounded indexed blobs, applies path/language/scope filters before search, and returns degraded reasons on candidate-path, candidate-file, or materialized-byte budget issues instead of turning a query hot path into a full repository scan. Developer or agent source inspection can use `rg` or `grep -RIn --exclude-dir=.git --exclude-dir=target ...`, but those commands must stay outside product runtime indexing and query loops.

## 7. Degradation Strategy

Unrecoverable parse errors, grammar panics, capture mismatches, and unsupported languages produce parse-status diagnostics and fall back to text chunks. C/C++ files with error nodes limited to macro expansion, bounded preprocessor directives, or decorator-like export macros may be recorded as parsed when symbol, reference, or import extraction succeeds. Degradation appears in repo status, health, and context pack metadata. Missing external dependency source remains unresolved edge metadata rather than `degraded_reason`. Query-time exact-text source fallback candidate-path or budget degradation appears in code query response metadata, not index state. Manual `rg`/`grep` fallback for agent inspection is documented operational behavior and must not be reported as product index health.

## 8. Acceptance Criteria

- Large repository indexing reports progress and does not replace the previous fresh scope early.
- Incremental updates process changed and affected files; they do not disguise full scans as incremental work.
- Commit advances survive missed watcher hints and restarts through bounded reconciliation and durable task replay.
- Long commit streams retain recovery bases and repository-set pins without unbounded scope or finished-task history growth.
- Files that fail parsing remain retrievable through text search.
- Indexing traces explain time spent in candidate narrowing, parsing, writing, and refresh phases.

---

Navigation: Previous: [11. Code Knowledge Graph Model](11-code-knowledge-graph-model.md) | Next: [13. Code Retrieval Ranking and Impact Analysis](13-code-retrieval-ranking-and-impact-analysis.md)
