# Tree-sitter Extraction and Incremental Indexing

[English](../../en/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md) | [中文](../../zh/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Tree-sitter is the entry point for code structure, not a complete semantic analyzer. The architecture connects grammar registration, query capture, error degradation, incremental candidate narrowing, and index refresh into a recoverable pipeline. Unsupported languages or parse errors degrade local capability only and do not break retrieval.

## 2. Language Registry

Each language registration includes language id, file extensions, tree-sitter grammar, capture queries, comment rules, identifier segmentation, and fallback chunker. When grammar is missing, files still enter text chunk and BM25 paths.

## 3. Capture Contract

Query captures emit a common structure: definitions, references, calls, imports, documentation comments, symbol spans, body spans, and chunk spans. Capture output is validated for scope, path, line/column, and content hash before write.

## 4. Full Build

```text
resolve snapshot
  -> enumerate authorized files
  -> batch parse and chunk
  -> write file/symbol/reference/chunk facts
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

## 6. Degradation Strategy

Parse errors, grammar panics, capture mismatches, and unsupported languages produce parse-status diagnostics and fall back to text chunks. Degradation appears in repo status, health, and context pack metadata.

## 7. Acceptance Criteria

- Large repository indexing reports progress and does not replace the previous fresh scope early.
- Incremental updates process changed and affected files; they do not disguise full scans as incremental work.
- Files that fail parsing remain retrievable through text search.

---

Navigation: Previous: [11. Code Knowledge Graph Model](11-code-knowledge-graph-model.md) | Next: [13. Code Retrieval Ranking and Impact Analysis](13-code-retrieval-ranking-and-impact-analysis.md)
