# Code Graph Competitive Features

[English](./09-code-graph-competitive-features.md) | [中文](../../zh/02-capabilities/09-code-graph-competitive-features.md)

> Document version: 2.0
> Date: 2026-06-08
> Scope: Book 2 capability guide

## Capability Positioning

Code graph capability lifts code search from text matching to structural understanding. Users see symbols, references, calls, imports, chunks, canonical identity, and edge diagnostics instead of only paths and lines.

## User-visible Behavior

- Symbol hits include both `symbol_snapshot_id` and `canonical_symbol_id`.
- Reference, caller/callee, import, and impact hits expose `edge_kind`, `edge_resolution_state`, `edge_target_hint`, `edge_confidence_basis_points`, and `edge_confidence_tier`.
- Code queries return revision-scoped hits with path, line range, kind, score, freshness, symbol identity, edge diagnostics, and excerpt.
- Exact source fallback hits return `retrieval_layers` containing `lexical` and `text_fallback`; definition fallback may also include `definition`. Their edge diagnostic fields remain empty because they are source-text evidence, not resolved graph edges.

## Competitive Features

Ordinary code search cannot distinguish same-named symbols across snapshots and cannot explain whether call edges are resolved. The code graph models snapshot symbols and canonical symbols together and returns uncertainty as metadata.

Compared with pure grep, pure trigram, or pure embedding search, the code graph combines Sourcegraph/Zoekt-style lexical candidates, Tree-sitter structural captures, BM25 chunks, semantic/vector explanation recall, and revision scopes. Exact symbols and resolved edges take priority, while semantic similarity remains a supporting signal so natural-language relevance does not override structural facts. Internal source fallback is used as a bounded exact-text recovery layer when AST or indexed lexical read models leave a specific gap.

## Command/API Entry Points

```bash
relay-knowledge repo query core --query retry_policy --kind callers --ref HEAD --format json
relay-knowledge repo query core --query crate::retry_policy --kind imports --ref HEAD --format json
```

## Web Route Awareness

The code graph detects web framework route handler bindings during indexing. Supported frameworks include Express (JavaScript/TypeScript), Flask/FastAPI (Python), and Spring (Java). The detectors cover real Express app/router calls, router mount prefixes, `route()` chains, member-expression handler links, multiline route registrations, async inline callbacks recorded as anonymous handlers, Express middleware chains with a final endpoint handler, stacked and multiline Flask/FastAPI decorators, Flask Blueprint `url_prefix` values, Flask tuple or list `methods` declarations, FastAPI `APIRouter(prefix = ...)` prefixes, multiple Spring class-level `@RequestMapping` prefixes, multiline Spring mapping annotations, unannotated Java class prefix resets, order-independent `value`/`path` attributes, path arrays, `RequestMethod` arrays, and methodless Spring mappings recorded with `http_method = "any"`. Each detected route produces a `CodeRouteRecord` with HTTP method, URL path, handler name, framework identifier, source location, and a handler symbol link when the parser can match the route to a structured symbol.

Route records are carried through checkpointed batches, persisted in `code_repository_routes`, and indexed as route search documents so normal durable repository indexing can answer "which handler serves a given endpoint?" queries through hybrid code search. Symbols annotated as route handlers persist their `symbol_role` metadata in SQLite with `SymbolRole::RouteHandler`, enabling downstream retrieval to prioritize or filter by HTTP endpoint semantics after checkpoint replay or full snapshot import.

## Degradation and Diagnostics

Parser or query failure is isolated to affected files and does not abort the entire repository batch. Unresolved or ambiguous edges are not presented as certain calls.
Broad regex matches, unresolved or ambiguous edges, parser degradation, stale code indexes, and source fallback candidate-path or budget degradation are visible in responses. Unresolved external dependency edges are coverage metadata and are not degradation by themselves. `text_fallback` hits can fill recall windows but must not outrank exact symbols or resolved edges. Benchmark improvements must not rely on known path, query, or symbol special cases.

## Related Architecture Chapters

- [Code Knowledge Graph Model](../03-architecture-specs/11-code-knowledge-graph-model.md)
- [Code Retrieval Ranking and Impact Analysis](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

Navigation: Previous: [8. Code Repository Basics](08-code-repository-basics.md) | Next: [10. Code Impact and Reporting](10-code-impact-and-reporting.md)
