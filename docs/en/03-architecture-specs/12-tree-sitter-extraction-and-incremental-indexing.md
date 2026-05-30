# Tree-sitter Extraction and Incremental Indexing

[English](../../en/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md) | [中文](../../zh/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)

> Document version: 2.0
> Date: 2026-05-24
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Tree-sitter is the entry point for code structure, not a complete semantic analyzer. The architecture connects grammar registration, query capture, error degradation, incremental candidate narrowing, and index refresh into a recoverable pipeline. Unsupported languages or unrecoverable parse errors degrade local capability only and do not break retrieval; recoverable C/C++ macro, preprocessor, and decorator parse errors should keep structured facts available when extraction remains reliable.

## 2. Language Registry

Each language registration includes language id, file extensions, tree-sitter grammar, capture queries, comment rules, identifier segmentation, and fallback chunker. When grammar is missing, files still enter text chunk and BM25 paths. Query-time source fallback is not a grammar substitute; it only adds exact-text evidence from indexed source candidates and cannot create graph facts.

Configuration, build, and template grammars are registered under the code configuration module rather than runtime configuration. The supported surface includes Markdown, XML, Bazel/Starlark, Make, CMake, Dockerfile/Containerfile, Java properties, TOML, INI, YAML, JSON, Go module files, Ninja, Jinja2, and Go templates. These formats emit ordinary file, symbol, reference, import, dependency, feature-flag, and chunk facts so query APIs do not need a separate schema for configuration search.

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
  -> refresh code/BM25/semantic/vector indexes
  -> mark scope fresh
```

The old fresh scope continues serving queries during full builds; the new scope becomes fresh only after finalize succeeds.

## 5. Incremental Update

Incremental indexing first narrows the work set:

1. Use Git diff/status and blob hashes to find changed files.
2. Include deleted, renamed, and moved files.
3. Expand affected files through reverse dependencies and import/call/reference edges.
4. Refresh only affected code facts, chunks, and index families.

Import dependency expansion prioritizes indexed code maps and versioned import edges. If an import points to an external dependency or cross-repository target without a code map, the indexer records only the unresolved target hint, resolution reason, and affected current-repository facts; it does not trigger an unauthorized full scan to fill that dependency. This coverage gap is not parser, file, scope, or response degradation. The query layer may use the hint inside the same scope to trigger bounded internal source fallback.

Local configuration relationships resolve only inside the same indexed source scope. Finalization may resolve deterministic local file references, template includes, and build-target references after all files in that scope have been written. Ambiguous local matches and external images, packages, remote labels, or templates remain unresolved or ambiguous metadata rather than degraded parser state.

Feature-flag extraction is an indexing-stage responsibility. Runtime config reads, boolean config declarations, and guarded-code relationships are written as versioned facts under the file scope; the query layer reads only those facts and their FTS documents. Boolean declarations in TOML, YAML, JSON, INI, Java properties, and related config formats reuse the configuration extractor's structured config-key facts instead of a separate feature-flag source. Changes to extractor rules, config files, or guarded branches require a full or incremental index refresh for the affected scope.

## 6. High-Performance Boundaries

Code indexing follows the shared principles behind Sourcegraph/Zoekt, GitHub Code Search, ripgrep, and Tree-sitter based systems: narrow candidates through path, language, trigram, symbol name, and blob hash before AST capture, edge resolution, or semantic/vector refresh. AST chunks should follow function, type, module, documentation comment, and import-block boundaries; fallback text chunks take over only when structural parsing is unavailable.

Cold full indexing, semantic embedding, cross-batch edge finalization, large-file skip/hash, and parser-heavy work belong behind background worker or maintenance boundaries and do not block query hot paths. Incremental indexing records changed file count, affected file count, parse throughput, write batch count, candidate windows, and stale lag so hidden full scans are visible.

Full-index batches are bounded simultaneously by file count, byte count, and write-row count. Large-repository cold-index throughput may improve through larger bounded batches, parser-worker parallelism, removal of redundant SQLite probes on empty scopes, or prepared-statement reuse, but it must not skip FTS/search-document writes, edge finalization, checkpoints, freshness checks, or degraded/status reporting. Any register-to-index performance optimization must leave a regression budget or guardrail for `index_ms` and `register_index_ms` in self-iteration `fast` or `--categories performance`.

Query-time source fallback follows the same blocking-worker boundary as Git blob reads. The product path uses an internal fixed-string scanner over a temporary tree of bounded indexed blobs, applies path/language/scope filters before search, and returns degraded reasons on candidate-path, candidate-file, or materialized-byte budget issues instead of turning a query hot path into a full repository scan. Developer or agent source inspection can use `rg` or `grep -RIn --exclude-dir=.git --exclude-dir=target ...`, but those commands must stay outside product runtime indexing and query loops.

## 7. Degradation Strategy

Unrecoverable parse errors, grammar panics, capture mismatches, and unsupported languages produce parse-status diagnostics and fall back to text chunks. C/C++ files with error nodes limited to macro expansion, bounded preprocessor directives, or decorator-like export macros may be recorded as parsed when symbol, reference, or import extraction succeeds. Degradation appears in repo status, health, and context pack metadata. Missing external dependency source remains unresolved edge metadata rather than `degraded_reason`. Query-time exact-text source fallback candidate-path or budget degradation appears in code query response metadata, not index state. Manual `rg`/`grep` fallback for agent inspection is documented operational behavior and must not be reported as product index health.

## 8. Acceptance Criteria

- Large repository indexing reports progress and does not replace the previous fresh scope early.
- Incremental updates process changed and affected files; they do not disguise full scans as incremental work.
- Files that fail parsing remain retrievable through text search.
- Indexing traces explain time spent in candidate narrowing, parsing, writing, and refresh phases.

---

Navigation: Previous: [11. Code Knowledge Graph Model](11-code-knowledge-graph-model.md) | Next: [13. Code Retrieval Ranking and Impact Analysis](13-code-retrieval-ranking-and-impact-analysis.md)
