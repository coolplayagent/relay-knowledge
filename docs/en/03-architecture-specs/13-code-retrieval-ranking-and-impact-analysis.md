# Code Retrieval Ranking and Impact Analysis

[English](../../en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md) | [中文](../../zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Advanced code retrieval comes from fusing structural signals with lexical and semantic signals. Grep misses call relations, vector search weakens exact symbols, and pure AST search lacks natural-language intent. Ranking considers symbols, chunks, edges, paths, languages, query intent, and freshness together.

## 2. Query Types

| Type | Primary signals |
| --- | --- |
| definition | Exact symbol, identifier segmentation, path/language filters |
| reference | Reference edge, target hint, confidence, callsite excerpt |
| caller/callee | Call edge, line containment, fan-out budget |
| import/dependency | Import edge, module path, resolution state |
| explanation | Doc comment, body chunk, semantic/vector similarity |
| impact | Changeset diff, reverse dependency, test edge, risk score |

## 3. Ranking Signals

Signals include BM25, identifier-part matches, CamelCase/snake_case segmentation, normalized query-to-symbol name overlap, symbol-kind priors, path proximity, language filters, graph-edge confidence, call direction, non-test source path priority for caller/callee queries, a small symbol test/benchmark path penalty when the query has no test intent, class-member excerpt context for qualified method hits, import surface/re-export files, declaration surface priority for header chunks that already match declaration-shape evidence, chunk quality, freshness, semantic/vector rank, and rerank explanations. Test/benchmark path adjustments are disabled when the query itself asks for tests or benchmarks.

## 4. Candidate Window

FTS candidate windows apply scope/path/language filters before bounded scoring. High fan-out caller/callee queries are truncated by edge score and line containment so one call edge is not multiplied across unrelated chunks.

## 5. Impact Analysis

Impact analysis starts from changeset scope:

```text
changed files
  -> changed symbols
  -> direct references/calls/imports
  -> reverse dependency expansion
  -> tests/docs/config affected candidates
  -> risk groups with evidence
```

Impact output is not an absolute conclusion; it is a risk grouping with evidence, paths, edge confidence, and budget truncation.

## 6. Acceptance Criteria

- Query `foo_bar` can match `fooBar`, `FooBar`, and multipart symbol names, while typed edge queries stay narrower.
- Caller/callee results point to chunks containing the call line.
- Impact output explains whether each result came from diff, call, reference, import, or test signals.

---

Navigation: Previous: [12. Tree-sitter Extraction and Incremental Indexing](12-tree-sitter-extraction-and-incremental-indexing.md) | Next: [14. Open Agent Runtime Adapter Architecture](14-open-agent-runtime-adapter-architecture.md)
