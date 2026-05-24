# Code Knowledge Graph Model

[English](../../en/03-architecture-specs/11-code-knowledge-graph-model.md) | [中文](../../zh/03-architecture-specs/11-code-knowledge-graph-model.md)

> Document version: 2.0
> Date: 2026-05-24
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

A code repository is not a plain text directory. Advanced code retrieval understands Git snapshots, paths, languages, symbols, references, calls, imports, documentation comments, chunks, and changeset evidence. The code knowledge graph turns these structures into versioned facts rather than stuffing code lines into a vector store.

## 2. Core Entities

| Entity | Meaning |
| --- | --- |
| `CodeRepository` | Stable local repository identity and authorization boundary |
| `CodeFile` | File instance at a tree hash |
| `CodeSymbol` | Snapshot-bound class, function, method, interface, variable, constant, or module |
| `CanonicalSymbol` | Candidate stable identity across snapshots |
| `CodeChunk` | Retrieval unit bound to line/column ranges and parent symbol |
| `CodeChangeSet` | Diff and impact evidence across base/head |

`symbol_snapshot_id` identifies a definition in one snapshot; `canonical_symbol_id` identifies a stable cross-snapshot candidate. They are not interchangeable.

## 3. Edge Types

Code edges include defines, references, calls, imports, implements, overrides, contains, documents, changed_by, tested_by, and affects. Each edge has a resolution state: resolved, unresolved, ambiguous, or inferred.

`imports` edges are the primary structural source for dependency sets. Dependency resolution first derives targets from import/include/module-import facts and indexed code maps. When a dependency target has no code map or code graph index, the system preserves the unresolved target hint and may use bounded query-time `rg` exact-text fallback to add current-repository source evidence. Fallback hits cannot create dependency graph facts, mark the edge as resolved, or masquerade as code-map evidence from the dependency library itself.

## 4. Confidence

Reference, call, and import resolution may be uncertain. Results expose target hints, confidence basis points, confidence tiers, and resolution reasons; inferred edges are not presented as certain calls.

## 5. Scope Binding

Code facts bind to repository snapshot or changeset scope. The same path at different tree hashes is a different fact instance; worktree overlays are explicitly marked.

## 6. Acceptance Criteria

- Retrieval results distinguish canonical symbols from snapshot symbols.
- Unresolved and ambiguous edges are visible in API, CLI, Web, and context packs.
- When a dependency code map is missing, import queries still expose the unresolved target hint and mark text fallback hits as current-repository lexical evidence.
- Code facts from the same path at different commits do not share fact keys.

---

Navigation: Previous: [10. Semantic/Vector Provider Architecture](10-semantic-vector-provider-architecture.md) | Next: [12. Tree-sitter Extraction and Incremental Indexing](12-tree-sitter-extraction-and-incremental-indexing.md)
