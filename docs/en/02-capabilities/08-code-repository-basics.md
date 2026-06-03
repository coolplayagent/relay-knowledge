# Code Repository Basics

[English](./08-code-repository-basics.md) | [中文](../../zh/02-capabilities/08-code-repository-basics.md)

> Document version: 2.0
> Date: 2026-05-30
> Scope: Book 2 capability guide

## Capability Positioning

Code repository basics let users register Git repositories or non-Git source directories as first-class sources, index clean snapshots or filesystem synthetic snapshots, and query code context through the same application service.

## User-visible Behavior

```bash
relay-knowledge repo register /path/to/relay-knowledge --path src
relay-knowledge repo index relay-knowledge --ref HEAD --format json
relay-knowledge repo query relay-knowledge --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo query relay-knowledge --query serde --kind sbom --ref HEAD --format json
relay-knowledge repo update relay-knowledge --base main --head HEAD --format json
relay-knowledge repo status relay-knowledge --format json
```

When `--alias` is omitted or blank, registration uses the resolved Git root or
filesystem root directory name as the stable repository alias. Agents should
prefer this default for first-time project registration so later sessions reuse
the same index; `--alias` remains available as an explicit override.

`repo query` supports `--limit`, `--ref`, repeatable `--path`, repeatable `--language`, and freshness policy. `repo register` rejects language filters so mixed-language repositories keep their full language surface; use query-time `--language` to narrow results.

`definition`, `references`, and `hybrid` queries use the indexed code graph and SQLite FTS first. When those layers leave a specific recall gap, the query may run bounded internal exact-text source fallback over candidate files from the indexed commit. Fallback results are exposed through `lexical` and `text_fallback` layers and do not replace resolved reference, call, or import edges.

Cold full `repo index` returns a queued task handle and lets the background code-index worker perform parsing and SQLite writes under a lease. `service run` recovers expired code-index leases at startup, and `repo index <alias> --reset` can requeue unfinished tasks without deleting completed indexed scopes or reviving terminal dead-letter history. `repo status` exposes the active task, checkpoint progress, finalization phase, and retention summary; successful workers keep the active scope, the two latest completed scopes, and unfinished task scopes. If a task is no longer active while the repository is still `indexing`, status reports the latest checkpoint so operators can distinguish a slow finalization phase from missing progress.

## Competitive Features

Repository indexing binds repository id, resolved commit, tree hash, and path filters. Query-time language filters narrow an indexed full-language scope. Equal trees can reuse scopes, rebased or force-moved heads require new indexes, and dirty worktrees are represented through explicit worktree overlays.

Code source layout detection is not limited to a top-level `src/` directory.
The indexer and import resolver also recognize real source under roots such as
`external_deps/`, `packages/`, `modules/`, `plugins/`, `extensions/`,
`Sources/`, and `lib/`, including nested JVM roots like
`modules/<name>/src/main/java`. When the registered scope covers the whole
repository, the Git tree decides which directories are source evidence:
tracked `.cloudbuild/`, `.cid/`, `.build_config/`, `build/`, `dist/`,
`vendor/`, and `third_party/` paths are eligible instead of being rejected by
name. File-level safeguards still skip binary/media assets and `*.jsonl`
dataset dumps unless an explicit path filter opts in. Dirty worktree overlays
still use Git status for untracked files and do not recursively expand
untracked broad dependency/cache/build directories unless an explicit path
filter opts in.

Non-Git source directories have no tracked tree authority, so their default scan
is whitelist based: supported source/config/docs files at the root and
source-like roots such as `src/`, `include/`, `lib/`, `Sources/`, `packages/`,
`modules/`, `plugins/`, `extensions/`, `docs/`, and `config/` are eligible.
`build/`, `dist/`, `target/`, `node_modules/`, `vendor/`, `third_party/`,
cache, virtualenv, and coverage directories are skipped by default and scanned
only when an explicit `--path` opts in. For non-Git sources, `HEAD` or any other
ref selector resolves to the current filesystem synthetic snapshot, and full,
incremental, and worktree-overlay modes share filesystem fingerprint semantics.

For example, a mixed-layout repository can register
`--path external_deps/python_sdk`,
`--path plugins/example.com/nonstandard/session`, or
`--path modules/payment/src/main/java` to index those authorized source trees.
If a registration is intentionally narrowed to `--path src`, paths under
`vendor/pkg`, `third_party/pkg`, or `build/` remain outside that registered
scope until the user widens the path filters.

## Command/API Entry Points

Narrow query kinds include `symbol`, `definition`, `references`, `callers`, `callees`, `imports`, and `sbom`. `--kind hybrid` searches symbols, definitions, references, imports, calls, and chunks together. `--kind sbom` searches dependency inventory facts extracted at index time from Cargo, npm, Go, Python, Maven effective `pom.xml`/BOM, Gradle, and Conan manifest or lock files; it is local inventory, not package-manager execution, transitive resolution, vulnerability analysis, or license compliance. Import resolution covers same-repository local import/use/module edges for the parser-supported languages, including JavaScript/JSX, Kotlin, Scala, C#, PHP, Rust, and Swift; package-manager or SDK imports without authorized indexed source remain unresolved edge metadata rather than parser degradation. Call-graph retrieval normalizes cross-language targets: C/C++ calls, Go cgo `C.*` calls, and Rust FFI/bindings paths resolve to C/C++ symbols in the same repository; when a header declaration, FFI scoped declaration, and implementation share a leaf name, the unique implementation is preferred as the resolved call target, and signature-only declarations do not block later implementation candidates. `C.*` leaf fallback is limited to Go cgo files, and call targets only resolve to callable symbols. Ordinary namespace calls are not collapsed to leaf names, so `module::connect` or `module::sys::connect` is not treated as an FFI alias for `connect`; resolved FFI calls keep the original scoped hint so both `rk_c_decode` and `ffi::rk_c_decode` queries can match the call edge.

Rust enum variants and C/C++ enumerators are indexed as structured `enum_member` symbols under their enum owner, so `--kind symbol` and `--kind definition` can resolve identities such as `Color.Red` or `Direction.kForward` without relying on text fallback. Other language enum-case forms should be added language by language with parser fixtures before they are treated as structured enum-member coverage.

`repo feature-flags` is a separate read-only entry point for enumerating or filtering configuration-driven feature-flag graph facts in an indexed scope. It returns flags grouped with configuration sources and `defines_config`, `reads_config`, and `guards_code` relationships instead of adding feature flags as a normal `repo query --kind` value.

General configuration and documentation files enter the same code graph rather than a separate documentation index. `.conf` reuses the INI/key-value surface and emits section, config, and boolean feature-flag facts; Markdown emits heading symbols and writes local inline links, image links, and reference link definitions as import facts; JSON emits stable dot-separated configuration paths with arrays normalized to `[]`. These files also keep file-level chunks so body text, config values, and local partial-parse content remain reachable through `hybrid` and BM25 retrieval.

## Degradation and Diagnostics

Unsupported, invalid UTF-8, binary, or oversized files degrade to text-only chunks. Syntax trees with unrecoverable error nodes are indexed as partial and record file diagnostics; C/C++ macro-heavy files may remain parsed when errors are isolated to macro expansions, bounded preprocessor directives, or decorator-bearing declarations and reliable symbols, references, or imports are still extracted. Missing external dependency source is exposed as unresolved edge coverage metadata, not `degraded_reason`. Source fallback candidate-path, candidate-file, materialized-byte, or line-length budget issues degrade only query-time exact-text fallback; responses keep `degraded_reason` while existing structured hits remain usable.

## Related Architecture Chapters

- [Source Scope Model](../03-architecture-specs/04-source-scope-model.md)
- [Tree-sitter Extraction and Incremental Indexing](../03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)
- [Code Retrieval Ranking and Impact Analysis](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

Navigation: Previous: [7. Multimodal Evidence Capability](07-multimodal-evidence-capability.md) | Next: [9. Code Graph Competitive Features](09-code-graph-competitive-features.md)
