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

## relay-teams E2E Findings

The `relay-teams` repository was exercised through the CLI as an end-to-end
code retrieval source. The successful interactive baseline used an immutable
commit ref so the exact index totals remain reproducible:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-src-e2e \
  relay-knowledge repo register /opt/workspace/relay-teams \
  --alias relay-teams-src \
  --path src/relay_teams \
  --language python \
  --format json

RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-src-e2e \
  relay-knowledge repo index relay-teams-src \
  --ref a6063949f4c526ce0e4eddf09d627f5f26c69df7 \
  --format json
```

That run indexed commit `a6063949f4c526ce0e4eddf09d627f5f26c69df7` for the
Python production source scope: 691 files, 13,399 symbols, 82,460 references,
and 13,402 chunks with no degraded files. Definition, reference, import, caller,
and hybrid queries all returned revision-scoped hits with resolved commit, tree
hash, path, line range, retrieval layer, index version, freshness, score, and
excerpt metadata.

The wider trial scope, `src`, `tests`, `docs`, and `frontend`, was not suitable
for interactive CLI indexing because it pulled in generated frontend assets,
PDFs, large documentation files, and large UI test fixtures. The current CLI
does not expose enough preflight or progress information for users to understand
that cost before starting a long full-index operation.

Follow-up improvements from this run:

- Add `repo index --dry-run` or `repo scope preview` so users can see selected
  file count, byte count, language distribution, largest files, unsupported
  files, generated assets, and expected degraded files before indexing.
- Add progress and budget reporting during full indexing, including Git file
  enumeration, blob reads, parser work, SQLite writes, elapsed time, skipped
  files, degraded files, and active scope.
- Add practical source presets and exclusion support. A source preset should
  exclude common generated or heavyweight paths such as `dist`, build outputs,
  cache directories, PDFs, and vendored assets unless users explicitly opt in.
  A repository-local ignore file, such as `.relay-knowledgeignore`, should make
  these exclusions repeatable.
- Make `graph inspect` and `health` code counts either include code repository
  index totals or clearly label those fields as graph-evidence counts only.
  During the E2E run, `repo status` reported code index totals while
  `graph inspect` reported zero code files and symbols, which is easy to
  misread as a failed code index.
- Split `repo impact` path reporting into in-scope and out-of-scope changes, or
  default the visible `changed_paths` list to the registered scope. The impact
  hits respected the registered source scope, but the path report still included
  unrelated docs, frontend, and test changes.
- Optimize query execution for larger code indexes. The measured Python source
  baseline was acceptable for focused use, but hybrid retrieval was materially
  slower than definition and reference lookup. Symbol, reference, call, import,
  and chunk search should use indexed SQLite predicates or FTS-backed candidate
  selection before in-memory scoring.
- Improve CLI argument ergonomics for multi-word queries. A command such as
  `repo query relay-teams-src --query runtime tools role` currently fails after
  `runtime`; the error should explain quoting, or the CLI should accept the
  remaining words as the query when doing so is unambiguous.
- Add `repo report <alias> --format markdown|json` to emit a reusable
  operations report with registration scope, resolved commit, tree hash, index
  totals, degradation summary, representative queries, latency samples, and
  freshness state.

The v2 implementation specification for these follow-ups and the local
deterministic semantic/vector retrieval baseline is maintained in
[`docs/specs/code-repository-retrieval-v2-optimization.md`](specs/code-repository-retrieval-v2-optimization.md).

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
  relative filters such as `./src`. After that normalization, the resolved
  request filters must match an indexed snapshot scope exactly; querying or
  impact-analyzing a narrower or broader filter set requires indexing that
  scope first.
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
