# Code Repository Retrieval v2 Optimization

[English](../../en/specs/code-repository-retrieval-v2-optimization.md) | [中文](../../zh/specs/code-repository-retrieval-v2-optimization.md)

This is the English documentation page for `specs/code-repository-retrieval-v2-optimization.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> Status: implementation spec
> Scope: repository indexing ergonomics, code query performance, diagnostics,
> and local deterministic semantic/vector retrieval

## Summary

This phase turns the relay-teams E2E follow-up list into product behavior and
upgrades hybrid retrieval beyond the Phase 1 "semantic/vector unavailable"
state. The v2 backend is intentionally local and deterministic: SQLite stores
concept-token documents and hashed token/character n-gram vectors so tests,
offline installs, and CI do not require an embedding service.

Future embedding providers can replace the deterministic vector writer and
searcher behind the same retrieval source contract. The public response shape
already reports `semantic` and `vector` backend status, ranking signals, index
versions, freshness, and degradation.

## Key Changes

- `repo index --dry-run` and `repo scope preview <alias>` return a non-mutating
  scope preview with selected file count, byte count, language distribution,
  largest selected files, excluded paths, unsupported files, generated/heavy
  file estimates, and expected degraded files.
- The default source preset excludes common generated or heavyweight paths and
  assets such as `dist`, `build`, `target`, `node_modules`, caches, vendored
  directories, PDFs, archives, fonts, images, videos, source maps, wasm,
  line-oriented dataset dumps such as `*.jsonl`, and lockfile snapshots such as
  `uv.lock`.
- `.relay-knowledgeignore` at the Git repository root provides repeatable
  repository-local exclusions. Blank lines, comments, directory names, anchored
  paths, and `*.extension` patterns are supported. Ignore rules only narrow the
  effective scope.
- `repo index` summary now includes progress counters for Git enumeration/blob
  reads, parsed files, SQLite writes, skipped unchanged files, and degraded
  files.
- `repo impact` returns both the legacy `changed_paths` list and
  `path_groups.in_scope_changed_paths` /
  `path_groups.out_of_scope_changed_paths`.
- `repo report <alias> --format markdown|json` emits registration scope,
  resolved commit/tree, index totals, degradation summary, representative
  queries, latency samples, and freshness state.
- `graph inspect` and `health` include repository code totals in graph code
  counters so API consumers do not see a false empty code graph; the same
  repository-specific counts remain available under `repository_code_totals`.
- Code repository queries use SQLite candidate predicates and limits before
  in-memory scoring for symbols, references, calls, imports, and chunks.
- `repo query <alias> --query runtime tools role` accepts unquoted trailing
  query words until the next option flag.
- Duplicate-root registration preserves existing aliases and adds the new alias
  to the same repository id. Alias collisions across different repository ids
  are rejected.
- `repo update` can use a persisted matching base snapshot even when the active
  repository status already points at another head.

## Retrieval Model

The local v2 retrieval model writes derived documents whenever BM25 documents
are written:

- `retrieval_semantic_documents` stores normalized concept terms from content,
  entity labels, source paths, code symbols, and code chunks.
- `retrieval_vector_documents` stores vector document metadata and vector norm.
- `retrieval_vector_terms` stores deterministic FNV-hashed token and
  character-trigram weights.

Hybrid retrieval now fuses BM25, graph evidence, code graph, semantic, and
vector candidates with reciprocal-rank fusion. Context pack ranking metadata
records the source, rank, source score, and explanation for each contributing
retriever. `semantic` and `vector` backend statuses are `available` when the
local SQLite read model is used.

## Interfaces

- CLI:
  - `relay-knowledge repo index <alias> --dry-run [--ref <ref>]`
  - `relay-knowledge repo scope preview <alias> [--ref <ref>]`
  - `relay-knowledge repo report <alias> --format markdown|json`
  - `relay-knowledge repo query <alias> --query multi word query`
- API:
  - `CodeRepositoryScopePreviewResponse`
  - `CodeRepositoryReportResponse`
  - `CodeIndexProgressSummary`
  - `CodeImpactPathGroups`
  - `CodeRepositoryTotals`, including repository parse-status counts
- Storage:
  - repository totals and reports remain behind `CodeRepositoryStore`.
  - repository aliases remain behind the storage boundary; callers resolve
    aliases through the same repository status contract.
  - semantic/vector rows remain SQLite read models and are not domain facts.

## Testing

Required coverage:

- preview counts, language buckets, default preset exclusions, and
  `.relay-knowledgeignore` exclusions;
- impact in-scope/out-of-scope path grouping;
- CLI parsing for dry-run, scope preview, report, markdown format, and
  multi-word code queries;
- deterministic semantic/vector retrieval for identifier variants;
- duplicate-root alias preservation and alias collision handling;
- persisted-base incremental update after a different active head is indexed;
- benchmark regression gate for no-op index, persisted-base update, hybrid
  query, and impact latency budgets;
- repository totals in graph inspection and health responses;
- optimized code query paths preserving existing definition/reference/import
  results.

Quality gates remain:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --test benchmarks --all-features -- --nocapture
```

## Assumptions

- No external embedding provider is configured in this phase.
- Default source presets are active unless a user explicitly narrows the
  registered/requested path to a generated area or excluded asset such as a
  specific `*.jsonl` file or `uv.lock`.
- `.relay-knowledgeignore` exclusions cannot expand the registered scope and do
  not replace Git authorization or selector validation.
- Real embeddings, cross-repository retrieval, LLM reranking, and multimodal
  vectors remain future extensions behind the same retrieval source contract.
