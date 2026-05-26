# Code Repository Basics

[English](./08-code-repository-basics.md) | [中文](../../zh/02-capabilities/08-code-repository-basics.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Code repository basics let users register Git repositories as first-class sources, index clean snapshots, and query code context through the same application service.

## User-visible Behavior

```bash
relay-knowledge repo register /path/to/relay-knowledge --path src
relay-knowledge repo index relay-knowledge --ref HEAD --format json
relay-knowledge repo query relay-knowledge --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo query relay-knowledge --query serde --kind sbom --ref HEAD --format json
relay-knowledge repo update relay-knowledge --base main --head HEAD --format json
relay-knowledge repo status relay-knowledge --format json
```

When `--alias` is omitted or blank, registration uses the resolved Git root
directory name as the stable repository alias. Agents should prefer this default
for first-time project registration so later sessions reuse the same index;
`--alias` remains available as an explicit override.

`repo query` supports `--limit`, `--ref`, repeatable `--path`, repeatable `--language`, and freshness policy. `repo register` rejects language filters so mixed-language repositories keep their full language surface; use query-time `--language` to narrow results.

`definition`, `references`, and `hybrid` queries use the indexed code graph and SQLite FTS first. When those layers leave a specific recall gap, the query may run bounded internal exact-text source fallback over candidate files from the indexed commit. Fallback results are exposed through `lexical` and `text_fallback` layers and do not replace resolved reference, call, or import edges.

Cold full `repo index` returns a queued task handle and lets the background code-index worker perform parsing and SQLite writes under a lease. `repo status` exposes the active task, checkpoint progress, and retention summary; successful workers keep the active scope, the two latest completed scopes, and unfinished task scopes.

## Competitive Features

Repository indexing binds repository id, resolved commit, tree hash, and path filters. Query-time language filters narrow an indexed full-language scope. Equal trees can reuse scopes, rebased or force-moved heads require new indexes, and dirty worktrees are represented through explicit worktree overlays.

Code source layout detection is not limited to a top-level `src/` directory.
The indexer and import resolver also recognize real source under roots such as
`external_deps/`, `packages/`, `modules/`, `plugins/`, `extensions/`,
`Sources/`, and `lib/`, including nested JVM roots like
`modules/<name>/src/main/java`. Heavy dependency dumps such as plain `vendor/`
and `third_party/` remain excluded by the source preset unless the user
explicitly opts in with a path filter.

For example, a mixed-layout repository can register
`--path external_deps/python_sdk`,
`--path plugins/example.com/nonstandard/session`, or
`--path modules/payment/src/main/java` to index those authorized source trees.
If source under `vendor/pkg` or `third_party/pkg` is intentionally needed, that
path must be passed explicitly so high-volume dependency dumps do not enter the
default scope by accident.

## Command/API Entry Points

Narrow query kinds include `symbol`, `definition`, `references`, `callers`, `callees`, `imports`, and `sbom`. `--kind hybrid` searches symbols, definitions, references, imports, calls, and chunks together. `--kind sbom` searches dependency inventory facts extracted at index time from Cargo, npm, Go, Python, Maven BOM, Gradle, and Conan manifest or lock files; it is local inventory, not package-manager execution, transitive resolution, vulnerability analysis, or license compliance. Call-graph retrieval normalizes cross-language targets: C/C++ calls, Go cgo `C.*` calls, and Rust FFI/bindings paths resolve to C/C++ symbols in the same repository; when a header declaration, FFI scoped declaration, and implementation share a leaf name, the unique implementation is preferred as the resolved call target, and signature-only declarations do not block later implementation candidates. `C.*` leaf fallback is limited to Go cgo files, and call targets only resolve to callable symbols. Ordinary namespace calls are not collapsed to leaf names, so `module::connect` or `module::sys::connect` is not treated as an FFI alias for `connect`; resolved FFI calls keep the original scoped hint so both `rk_c_decode` and `ffi::rk_c_decode` queries can match the call edge.

Rust enum variants and C/C++ enumerators are indexed as structured `enum_member` symbols under their enum owner, so `--kind symbol` and `--kind definition` can resolve identities such as `Color.Red` or `Direction.kForward` without relying on text fallback. Other language enum-case forms should be added language by language with parser fixtures before they are treated as structured enum-member coverage.

`repo feature-flags` is a separate read-only entry point for enumerating or filtering configuration-driven feature-flag graph facts in an indexed scope. It returns flags grouped with configuration sources and `defines_config`, `reads_config`, and `guards_code` relationships instead of adding feature flags as a normal `repo query --kind` value.

## Degradation and Diagnostics

Unsupported, invalid UTF-8, binary, or oversized files degrade to text-only chunks. Syntax trees with unrecoverable error nodes are indexed as partial and record file diagnostics; C/C++ macro-heavy files may remain parsed when errors are isolated to macro expansions, bounded preprocessor directives, or decorator-bearing declarations and reliable symbols, references, or imports are still extracted. Missing external dependency source is exposed as unresolved edge coverage metadata, not `degraded_reason`. Source fallback candidate-path, candidate-file, materialized-byte, or line-length budget issues degrade only query-time exact-text fallback; responses keep `degraded_reason` while existing structured hits remain usable.

## Related Architecture Chapters

- [Source Scope Model](../03-architecture-specs/04-source-scope-model.md)
- [Tree-sitter Extraction and Incremental Indexing](../03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)
- [Code Retrieval Ranking and Impact Analysis](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

Navigation: Previous: [7. Multimodal Evidence Capability](07-multimodal-evidence-capability.md) | Next: [9. Code Graph Competitive Features](09-code-graph-competitive-features.md)
