# Self-Iteration Accepted Optimization Records

[English](../../en/05-benchmarks/04-self-iteration-accepted-optimizations.md) | [中文](../../zh/05-benchmarks/04-self-iteration-accepted-optimizations.md)

This page is the compact English companion for the self-iteration optimization log. The Chinese primary log keeps the full rolling record and archives old detailed entries before they exceed the repository file-length cap.

## Issue #147: Cross-Language Call Graph

- Algorithm and architecture: call-target resolution keeps the original target hint and adds only constrained same-repository leaf candidates for cross-language boundaries. C/C++ calls keep direct symbol names, Go cgo maps `C.<name>` to `<name>` only from `.go` files, and Rust FFI/bindings paths add a leaf candidate only for `ffi`, `bindings`, `libc`, or `*_sys` prefixes.
- Invariants and limits: no SQLite schema, parser facts, FTS content, ranking weights, semantic/vector read model, CLI/API, or installation behavior changed. The capability is a static same-repository code-graph feature; it does not claim full build-system, linker, dynamic-loading, macro-generated call, external prebuilt SDK, or unindexed bindgen coverage.
- Guardrails: resolution only targets callable symbols, prefers a unique implementation over header or signature-only declarations, keeps ordinary namespace calls from collapsing to broad leaf aliases, and is covered by the default-fast `cross_language_syntax_fixture`.

## Issue #154: Query-Aware Source Fallback Candidates

- Algorithm and architecture: when exact source fallback needs broad scope paths, storage first narrows candidate files through indexed `code_repository_search` FTS with the query plus path and language filters. It falls back to bounded scope enumeration only when the query has no indexed candidate.
- Current runtime: product fallback now materializes Git blobs and searches them with the internal fixed-string scanner behind the blocking-worker boundary. It keeps the same 256 candidate-file, 8 MiB blob, 4096-byte line, result-limit, safe-path, path-filter, language-filter, and `text_fallback` provenance budgets. The product hot path no longer depends on an external `rg` process.
- Diagnostics: the historical issue text used `ripgrep candidate file budget exhausted`; the current diagnostic uses `source fallback candidate file budget exhausted` for the same bounded candidate exhaustion state. Existing structured symbol and definition hits remain valid when fallback is degraded.

## Issue #146: Nonstandard Source Layouts

- Algorithm and architecture: repository source normalization treats source roots as a layout set rather than a single top-level `src/` convention. The indexer and import/module resolver recognize `external_deps/`, `packages/`, `modules/`, `plugins/`, `extensions/`, `Sources/`, `lib/`, nested JVM roots, and C/C++ `include/` segments.
- Invariants and limits: plain `vendor/` and `third_party/` remain excluded by the source preset and require explicit path-filter opt-in. TypeScript bare specifiers resolve only when a local indexed module candidate exists, and ambiguous local matches stay protected.
- Guardrails: `nonstandard_layout_fixture` is included in the default fast profile and covers Python, TypeScript, Go, Java, C++, and Swift source outside a top-level `src/` directory without repository, path, query, symbol, or case-id special casing.

## Issue #166: Registration Language Filters

- Algorithm and architecture: repository registration rejects non-empty language filters so mixed-language repositories keep their complete indexed language surface. Query-time `--language` remains the supported narrowing mechanism.
- Guardrails: the default fast self-iteration profile includes a generated cross-language registration case that expects `repo register --language cpp` to fail with the stable registration-language error.

## Issue #167: C External Header Macro Recovery

- Algorithm and architecture: C parser recovery now treats isolated typedef-style external-header declarations, module tables, and uppercase macro calls with declaration bodies as recoverable when structured symbols, references, imports, or calls are still extracted. Macro-generated C function symbols expand to the following compound body so call ownership remains available.
- Invariants and limits: missing Nginx/Kong-style headers stay unresolved import metadata with `target_hint`; they are not file degradation. Broken assignments, preprocessor-branch syntax errors, registration macros, and non-body data macros still surface diagnostics or stay out of the call graph.
- Guardrails: the default fast `c_syntax_fixture` includes unresolved `ngx_*` headers, a `KONG_ACCESS_PHASE` handler, typedef-style module tables, symbol/definition/callee/import cases, and no repository/path/query special casing.

## Documentation Maintenance

- The primary Chinese accepted-optimization log is kept below the 1000-line hard cap by moving late detailed records to dated archive files.
- Capability and architecture pages in both languages document the current source-root, cross-language call-target, and internal source-fallback behavior.
