# Code Repository Basics

[English](./08-code-repository-basics.md) | [中文](../../zh/02-capabilities/08-code-repository-basics.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Code repository basics let users register Git repositories as first-class sources, index clean snapshots, and query code context through the same application service.

## User-visible Behavior

```bash
relay-knowledge repo register /path/to/repo --alias core --path src --language rust
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo query core --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo status core --format json
```

`repo query` supports `--limit`, `--ref`, repeatable `--path`, repeatable `--language`, and freshness policy.

`definition`, `references`, and `hybrid` queries use the indexed code graph and SQLite FTS first. When those layers leave a specific recall gap, the query may run bounded `ripgrep` fallback over candidate files from the indexed commit. Fallback results are exposed through `lexical` and `text_fallback` layers and do not replace resolved reference, call, or import edges.

Cold full `repo index` returns a queued task handle and lets the background code-index worker perform parsing and SQLite writes under a lease. `repo status` exposes the active task, checkpoint progress, and retention summary; successful workers keep the active scope, the two latest completed scopes, and unfinished task scopes.

## Competitive Features

Repository indexing binds repository id, resolved commit, tree hash, path filters, and language filters. Equal trees can reuse scopes, rebased or force-moved heads require new indexes, and dirty worktrees are represented through explicit worktree overlays.

Code source layout detection is not limited to a top-level `src/` directory.
The indexer and import resolver also recognize real source under roots such as
`external_deps/`, `packages/`, `modules/`, `plugins/`, `extensions/`,
`Sources/`, and `lib/`, including nested JVM roots like
`modules/<name>/src/main/java`. Heavy dependency dumps such as plain `vendor/`
and `third_party/` remain excluded by the source preset unless the user
explicitly opts in with a path filter.

## Command/API Entry Points

Narrow query kinds include `symbol`, `definition`, `references`, `callers`, `callees`, and `imports`. `--kind hybrid` searches symbols, definitions, references, imports, calls, and chunks together. Call-graph retrieval normalizes cross-language targets: C/C++ calls, Go cgo `C.*` calls, and Rust FFI/bindings paths resolve to C/C++ symbols in the same repository; when a header declaration and implementation share a name, the unique implementation is preferred as the resolved call target. Ordinary namespace calls are not collapsed to leaf names, so `module::connect` is not treated as an FFI alias for `connect`.

`repo feature-flags` is a separate read-only entry point for enumerating or filtering configuration-driven feature-flag graph facts in an indexed scope. It returns flags grouped with configuration sources and `defines_config`, `reads_config`, and `guards_code` relationships instead of adding feature flags as a normal `repo query --kind` value.

## Degradation and Diagnostics

Unsupported, invalid UTF-8, binary, or oversized files degrade to text-only chunks. Syntax trees with error nodes are indexed as partial and record file diagnostics. Missing `rg`, timeouts, or exhausted candidate budgets degrade only query-time exact-text fallback; responses keep `degraded_reason` while existing structured hits remain usable.

## Related Architecture Chapters

- [Source Scope Model](../03-architecture-specs/04-source-scope-model.md)
- [Tree-sitter Extraction and Incremental Indexing](../03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)
- [Code Retrieval Ranking and Impact Analysis](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

Navigation: Previous: [7. Multimodal Evidence Capability](07-multimodal-evidence-capability.md) | Next: [9. Code Graph Competitive Features](09-code-graph-competitive-features.md)
