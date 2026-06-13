# Hybrid Retrieval and Context Packing

[English](../../en/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md) | [中文](../../zh/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)

> Document version: 2.0
> Date: 2026-05-24
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Hybrid retrieval is the core algorithmic surface. Plain vector retrieval handles similarity; plain BM25 handles exact terms. `relay-knowledge` must answer terminology, concepts, multi-hop relations, time facts, code symbols, and impact analysis, so recall, structural expansion, fusion, rerank, and context packing form one algorithm.

## 2. Query Flow

```text
normalize query
  -> resolve source scope and freshness policy
  -> plan retriever families
  -> lexical / semantic / vector / graph / code / local file recall
  -> candidate normalization and dedup
  -> weighted reciprocal-rank fusion
  -> graph expansion and local rerank
  -> context pack budgeting
  -> response with provenance and freshness metadata
```

No retriever bypasses scope filters, authorization policy, or freshness policy.

The query planner first classifies intent: exact term, conceptual, multi-hop, temporal, code symbol, impact, file path, file content, or mixed agent context. Each intent selects retriever families and budgets. For example, filename/path queries prefer `local_file_path` and metadata, while content questions enter `local_file_content`, BM25, or semantic/vector paths.

For code intent, recall order is tree-sitter code graph, SQLite FTS/BM25, semantic/vector supplement, and only then bounded internal exact-text source fallback. Product runtime fallback inherits source scope, path/language filters, authorization, and freshness policy, and searches materialized indexed-commit candidates rather than a dirty worktree. It can produce source span evidence only; it cannot declare new graph edges or override edge confidence. Agent or maintainer inspection may use `rg` or `grep -RIn`, but that is a bounded development search technique, not a product query-path substitute.

## 3. Fusion Model

The baseline fusion uses weighted RRF:

```text
score(candidate) = sum(weight_i / (k + rank_i)) + structural_bonus - penalty
```

`structural_bonus` comes from source authority, direct graph paths, accepted lifecycle, exact symbol matches, exact file path/basename matches, fresh indexes, and evidence confidence. `penalty` comes from stale lag, degraded backends, ambiguous entities, low confidence, unauthorized candidate rejection, or duplicate parent evidence.

Multi-stage reranking is allowed after RRF, but it only processes bounded candidate windows and preserves each retriever's rank contribution. BM25, vector, graph path, code edge, and file path scores are not linearly added before calibration.

## 4. Graph Expansion

Graph expansion starts from high-confidence candidates and stays within budget:

- Entity neighborhoods.
- Direct relation/claim/event paths.
- Schema-guided paths.
- Temporal predecessor/successor links.
- Code symbol reference/call/import edges.
- Local file path/content evidence relations.

Expansion results carry path provenance; they are not returned as opaque related context.

## 5. Context Pack

A context pack is the stable evidence bundle for agents and UI. It includes query metadata, retriever sources, rank explanations, context items, source spans, graph paths, structured facts, code artifacts, local file artifacts, freshness, degraded state, budgets, and truncation reasons.

Packing favors diversity and citability. Duplicate hits from the same parent evidence, symbol, or source span merge; low-confidence expansions do not displace direct evidence.

The codegraph context pack is a specialized one-call orchestration for coding agents. It runs bounded hybrid, definition, and symbol entry queries, expands top seeds through references, callers, callees, and imports, then deduplicates by file, symbol, edge, and line span before enforcing `max_context_bytes`. Its response separates entry points, related symbols, graph paths, impact hints, and code excerpts, each with retrieval layer, score, line range, and provenance. It reuses the existing code graph read model and freshness policy; it does not add storage schema, start background refresh, or replace diff-based impact analysis.

## 6. Acceptance Criteria

- Exact-term, conceptual, multi-hop, temporal, and code-symbol queries have corresponding retriever signals.
- Filename/path and file-content queries distinguish path, metadata, content, and change-cursor freshness.
- Results explain item source, rank contribution, and freshness.
- Code exact-text fallback hits preserve `text_fallback` provenance and return degraded reasons when candidate-path or budget limits are hit; manual agent inspection documents the `rg`/`grep -RIn` fallback path separately.
- Broad-scope code exact-text fallback first narrows candidate paths through the indexed FTS read model using query, path filters, and language filters; it falls back to bounded scope enumeration only when the query has no indexed candidates.
- Degraded backends produce explicit degradation metadata instead of silent absence.

## 7. Smart Query Identifier Extraction

Query preprocessing (`retrieval/terms.rs`) recognizes and extracts code identifier patterns from natural language query text to improve FTS/BM25 recall:

| Pattern | Example | Extraction |
| --- | --- | --- |
| PascalCase / CamelCase | `UserService`, `signInWithGoogle` | Original + split parts |
| snake_case | `user_service`, `max_retries` | Original |
| SCREAMING_SNAKE_CASE | `MAX_RETRIES`, `API_KEY` | Original |
| dot.notation | `app.isPackaged` | Split segments |
| ALL_CAPS abbreviations | `REST`, `HTTP`, `LRU` | Original |
| Lowercase identifiers (3+ chars) | `render`, `parse`, `undo` | Original |

Stop-word filtering covers at least 80 common English words (the, and, for, with, from, how, what, etc.), excluding words that cannot correspond to code symbols during identifier extraction. Stem variant expansion generates matching candidates for English verb/noun inflections (connecting → connect, connected; renderer → render), broadening match coverage. Extracted PascalCase/CamelCase identifiers receive 1.5x weight in BM25/FTS queries, snake_case/SCREAMING_SNAKE identifiers receive 1.3x weight, and lowercase identifiers use a base weight of 0.8x.

---

Navigation: Previous: [8. Derived Indexes and Freshness](08-derived-indexes-and-freshness.md) | Next: [10. Semantic/Vector Provider Architecture](10-semantic-vector-provider-architecture.md)
