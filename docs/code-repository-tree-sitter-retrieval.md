# Code Repository Tree-sitter Retrieval

`relay-knowledge` now treats Git repositories as first-class code sources. The
CLI and application service share the same async API for repository
registration, tree-sitter indexing, code retrieval, and impact analysis.

## Commands

```bash
relay-knowledge repo register /path/to/repo --alias core --path src --language rust
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo query core --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo query core --query retry_policy --kind references --ref HEAD --format json
relay-knowledge repo query core --query crate::retry_policy --kind imports --ref HEAD --format json
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo impact core --base main --head HEAD --format json
relay-knowledge repo status core --format json
```

`--kind hybrid` searches symbols, definitions, references, imports, calls, and
chunks. Narrow kinds are `symbol`, `definition`, `references`, `callers`,
`callees`, and `imports`. Diff-based impact analysis is served by
`repo impact`; `impact` is rejected as a plain query kind so changeset results
cannot be confused with hybrid search.
`repo query` also accepts `--limit`, `--ref`, repeated `--path`, repeated
`--language`, and `--freshness allow-stale|wait-until-fresh|graph-only`.

## Implementation

- Git registration resolves the repository root and derives a stable
  `repository_id` from both `remote.origin.url` and the local repository root,
  falling back to the absolute root path when no origin is configured. Status
  lookup checks `repository_id` first and then falls back to alias lookup, so
  `repo:` aliases remain reachable when they do not collide with a repository
  id.
- Full indexing reads a clean Git tree using `git ls-tree` and `git show`.
- Incremental indexing reads `git diff --name-status --find-renames -z` and
  only reparses changed, copied, renamed, or type-changed paths. Selected
  deleted and renamed paths are removed from the active index, copy sources do
  not seed impact analysis, and rename lineage is kept as tombstones. The
  incremental base ref must resolve to the currently indexed snapshot before
  previous file fingerprints are reused.
- Worktree overlay mode indexes changed worktree files under a synthetic
  `worktree:<hash>` tree id and `worktree:<commit>:<hash>` resolved snapshot
  identity. Queries must use `--ref worktree` to read overlay rows; clean commit
  refs are rejected while an overlay is active so uncommitted content is not
  mislabeled as a clean Git snapshot. Clean or out-of-scope-only worktree status
  falls back to a clean full snapshot. Overlay indexing is bound to the
  checked-out `HEAD`, forces untracked-file visibility, expands untracked
  directories to file-level changes, skips non-file status entries, and removes
  selected rename sources from the active overlay index.
- Parser work runs behind application-level `spawn_blocking` boundaries.
  SQLite writes also remain behind the storage blocking worker.
- Rust, Python, JavaScript/JSX, TypeScript/TSX, Go, Java, Kotlin, Scala, C,
  C++, C#, Ruby, PHP, Swift, and Bash files use tree-sitter grammars. Syntax
  trees containing error nodes are indexed as `partial` with file diagnostics
  while retaining reliable symbols, references, imports, calls, and chunks.
  Unsupported, invalid UTF-8, binary, or oversized files degrade to text-only
  chunks where possible. Parser or query failures are isolated to the affected
  file as `failed` diagnostics so one bad file cannot abort a repository batch.
- Revision-scoped queries are served only when the requested ref resolves to the
  currently indexed commit or to the explicit `worktree` overlay ref; callers
  must index another ref before querying it. Refs beginning with `-` are
  rejected before invoking Git so user-supplied ref names cannot be parsed as
  Git options.
- Request path/language filters are intersected with the registered repository
  scope and cannot widen ingestion, retrieval, or impact analysis. `.` and `./`
  path filters select the repository root, and leading `./` is normalized for
  relative filters such as `./src`.
- `wait-until-fresh` code queries reject stale repository status. `graph-only`
  returns no repository-index rows and reports that the graph-only policy was
  selected.
- Impact analysis validates that `head_ref` resolves to the indexed snapshot,
  filters changed paths before deriving module/symbol seeds, matches callers by
  resolved symbol identity, and carries deleted symbol names so callers of
  removed APIs remain visible. Deleted paths that no longer have file rows fall
  back to extension-based language inference for path/language filtering.
- Import impact seeds include path modules, Rust `crate::...` module keys,
  symbol qualified names, and symbol names. Import matches require module
  boundaries such as punctuation or whitespace; `_` and `-` remain part of a
  module token.
- Retrieval hits include repository id, scope alias, resolved commit, tree hash,
  path, language id, byte and line ranges, symbol/file identifiers, retrieval
  layers, index version, stale flag, degraded reason, score, and excerpt.

## Storage Model

SQLite stores the active code index in dedicated tables:

- `code_repositories`
- `code_repository_files`
- `code_repository_symbols`
- `code_repository_references`
- `code_repository_imports`
- `code_repository_calls`
- `code_repository_chunks`
- `code_repository_file_diagnostics`
- `code_repository_path_tombstones`

The storage boundary exposes code repository methods through
`CodeRepositoryStore`; CLI and application code do not access SQLite directly.

## Testing

The Rust test suite covers:

- selector validation and query limits,
- Git name-status parsing for add/modify/delete/rename/copy/type-change,
- tree-sitter symbol, reference, import, chunk extraction, mainstream language
  grammar coverage, and partial-parse diagnostics,
- text-only and failed-file degradation for unsupported, malformed, or parser
  failure cases,
- SQLite code repository persistence and query retrieval,
- worktree overlay scope, freshness policy, provenance, and impact-analysis
  edge cases,
- end-to-end Git fixture registration, full indexing, definition/reference/import
  query, incremental update, and impact analysis through
  `RelayKnowledgeService`.
