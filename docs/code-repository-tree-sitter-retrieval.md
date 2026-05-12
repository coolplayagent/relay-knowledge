# Code Repository Tree-sitter Retrieval

`relay-knowledge` now treats Git repositories as first-class code sources. The
CLI and application service share the same async API for repository
registration, tree-sitter indexing, code retrieval, and impact analysis.

## Commands

```bash
relay-knowledge repo register /path/to/repo --alias core --path src --language rust
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo query core --query retry_policy --kind definition --format json
relay-knowledge repo query core --query retry_policy --kind references --format json
relay-knowledge repo query core --query crate::retry_policy --kind imports --format json
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo impact core --base main --head HEAD --format json
relay-knowledge repo status core --format json
```

`--kind hybrid` searches symbols, definitions, references, imports, calls, and
chunks. Narrow kinds are `symbol`, `definition`, `references`, `callers`,
`callees`, `imports`, and `impact`.

## Implementation

- Git registration resolves the repository root and derives a stable
  `repository_id` from `remote.origin.url`, falling back to the absolute root
  path when no origin is configured.
- Full indexing reads a clean Git tree using `git ls-tree` and `git show`.
- Incremental indexing reads `git diff --name-status --find-renames -z` and
  only reparses changed, copied, renamed, or type-changed paths. Deleted and
  renamed paths are removed from the active index, with rename lineage kept as
  tombstones.
- Worktree overlay mode indexes changed worktree files under a synthetic
  `worktree:<hash>` tree id that includes selected changed file content hashes
  without mutating a clean commit snapshot. Non-file status entries are skipped.
- Parser work runs behind application-level `spawn_blocking` boundaries.
  SQLite writes also remain behind the storage blocking worker.
- Rust, Python, TypeScript, and TSX files use tree-sitter grammars. Unsupported,
  invalid UTF-8, binary, or oversized files degrade to text-only chunks where
  possible.
- Revision-scoped queries are served only when the requested ref resolves to the
  currently indexed commit; callers must index another ref before querying it.
- Retrieval hits include repository id, scope alias, resolved commit, tree hash,
  path, language id, byte and line ranges, symbol/file identifiers, retrieval
  layers, index version, stale flag, degraded reason, score, and excerpt.

## Storage Model

SQLite stores the active code index in dedicated tables:

- `code_repositories`
- `code_files`
- `code_symbols`
- `code_references`
- `code_imports`
- `code_calls`
- `code_chunks`
- `code_file_diagnostics`
- `code_path_tombstones`

The storage boundary exposes code repository methods through
`CodeRepositoryStore`; CLI and application code do not access SQLite directly.

## Testing

The Rust test suite covers:

- selector validation and query limits,
- Git name-status parsing for add/modify/delete/rename/copy/type-change,
- tree-sitter symbol, reference, import, and chunk extraction,
- text-only degradation for unsupported files,
- SQLite code repository persistence and query retrieval,
- end-to-end Git fixture registration, full indexing, definition/reference/import
  query, incremental update, and impact analysis through
  `RelayKnowledgeService`.
