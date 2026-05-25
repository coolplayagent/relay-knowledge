# Code Retrieval Ranking and Impact Analysis

[English](../../en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md) | [中文](../../zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

> Document version: 2.0
> Date: 2026-05-24
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
| feature-flags | Config source, guards_code/read edge, source range, confidence |
| explanation | Doc comment, body chunk, semantic/vector similarity |
| impact | Changeset diff, reverse dependency, test edge, risk score |

## 3. Ranking Signals

Signals include BM25, identifier-part matches, CamelCase/snake_case segmentation, normalized query-to-symbol name overlap, symbol-kind priors, path proximity, language filters, graph-edge confidence, call direction, non-test source path priority for caller/callee queries, a small symbol test/benchmark path penalty when the query has no test intent, class-member excerpt context for qualified method hits, import surface/re-export files, declaration surface priority for header chunks that already match declaration-shape evidence, chunk quality, freshness, semantic/vector rank, and rerank explanations. Ranking preserves original query casing for identifier segmentation and intent checks before lowercased lexical scoring. Test/benchmark path adjustments are disabled when the query itself asks for tests or benchmarks.

Industry code-search practice requires lexical, structural, and semantic layering: Zoekt/Google Code Search style trigram candidates are good for substring and regex screening, BM25 for natural language and documentation chunks, Tree-sitter captures for symbols and edges, and semantic/vector retrieval for conceptual explanation queries. Ranking must not let semantic scores override exact symbols or resolved edges, and broad regex results cannot bypass scope, path, language, or revision filters.

Code repository queries use AST-first cooperation with exact grep fallback. Definition, reference, and hybrid queries first consult the versioned tree-sitter graph and SQLite FTS read model. When that structured path has a specific recall gap, a bounded `rg` pass may search the same indexed revision scope for exact text evidence. Import/dependency queries first derive the dependency set from import edges. If the dependency target has a code map or code graph target, retrieval uses structured dependency evidence; only when the dependency library is not indexed as a code graph target may the import query use the unresolved external dependency target hint as the grep term. Missing external dependency source is unresolved edge coverage metadata, not `degraded_reason`. Grep fallback is a lexical layer only: it can recover precise source lines and improve recall, but it cannot report resolved graph edges or confidence that the AST graph did not establish.

Agent-facing and maintainer-facing source inspection follows a separate tool policy. Use `rg` first for local repository search. If the local machine does not have `rg`, continue with bounded `grep -RIn --exclude-dir=.git --exclude-dir=target --exclude-dir=node_modules --exclude-dir=dist <pattern> <authorized-root>` commands. This manual grep path is allowed for investigation and self-iteration prompts, but it must not be wired into product query hot paths or used to bypass indexed scope, freshness, or authorization.

## 4. Candidate Window

FTS and grep candidate windows apply scope/path/language filters before bounded scoring. High fan-out caller/callee queries are truncated by edge score and line containment so one call edge is not multiplied across unrelated chunks.

Ripgrep fallback runs behind the same blocking-worker boundary as Git snapshot reads. It is constrained by storage-side candidate-path limits, candidate-file, match, line-length, and timeout budgets. The current fallback budget is 256 candidate files, 8 MiB of materialized blobs, 4096 bytes per line, and a 3 second `rg` timeout. It searches indexed commit content rather than the dirty working tree. When fallback needs broad scope paths, storage first narrows candidate files through the indexed FTS read model using the query plus path and language filters, then falls back to bounded scope enumeration when the query has no indexed candidate or the search read model is unavailable. If the FTS read model is temporarily unavailable during structured code search, the structured lexical layer may return an empty candidate set after retry so bounded source-text fallback can continue instead of failing the whole query. This keeps late-sorted relevant files reachable without increasing the global candidate-file budget. The materialized search tree must include indexed dot-path candidates, and JSON decoding must accept both ripgrep `text` and base64 `bytes` fields. Oversized blobs are skipped without stopping later candidates that still fit the materialization budget, and definition fallback filters candidate lines before enforcing the returned-hit limit. If `rg` is unavailable, times out, exhausts its budget, or cannot obtain bounded candidate paths from the storage backend, the query remains valid and surfaces a degraded reason for the fallback layer instead of bypassing freshness or authorization. The product runtime does not shell out to recursive `grep`; recursive `grep -RIn` is documented only as a bounded agent/maintainer search fallback when investigating the workspace.

Candidate windows expose observability fields: pre-filter count, post-filter count, scored count, truncation reason, and elapsed time for each layer. Impact, caller/callee, and import queries expand with changed paths, seed symbols, module hints, and edge confidence rather than full scope table size.

Feature-flag queries enumerate structured flag facts in the indexed scope by default. When `--query` is present, they filter only indexed flag names, source keys, paths, and excerpts. Ranking prioritizes guarded-code relationships, configuration definitions, ordinary reads, then query matches and path/language filters. The query must not use query-time grep to enumerate unknown flags or hardcode known product, repository, or benchmark flag names to improve ranking.

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
- Grep fallback hits are marked with lexical/text-fallback provenance and never include resolved edge confidence; hybrid grep fallback fills result windows without outranking existing structured hits.
- Unresolved external dependency coverage is reported through edge resolution metadata and does not set `degraded_reason` unless the bounded fallback itself fails.
- Self-iteration and maintainer prompts cover both product `rg` fallback behavior and the manual `grep -RIn` inspection fallback so missing local ripgrep does not block source analysis.
- Impact output explains whether each result came from diff, call, reference, import, or test signals.
- Benchmarks are not improved by enumerating known queries, paths, or symbols; improvements come from general ranking signals, index structures, or candidate pushdown.

---

Navigation: Previous: [12. Tree-sitter Extraction and Incremental Indexing](12-tree-sitter-extraction-and-incremental-indexing.md) | Next: [14. Open Agent Runtime Adapter Architecture](14-open-agent-runtime-adapter-architecture.md)
