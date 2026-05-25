# Chapter 5: Code Repository Graph Workflow

[English](../../en/01-user-guide/05-code-repository-graph-workflow.md) | [中文](../../zh/01-user-guide/05-code-repository-graph-workflow.md)

The code repository graph brings Git trees, files, symbols, references, calls, and imports into one retrieval surface. It is not simple file search; queries and impact analysis depend on indexed code graph snapshots. Exact-text grep is only a bounded fallback layer over indexed snapshots, used to fill source-line gaps that AST and FTS leave explicit.

## 5.1 Register a Repository

Register a Git repository as a code retrieval source:

```bash
relay-knowledge repo register /path/to/repo \
  --alias core \
  --path src \
  --language rust \
  --format json
```

`--alias` is the short name used by later commands. `--path` and `--language` can be repeated. Registration scope limits indexing, queries, and impact analysis; later requests can narrow the scope but cannot widen it.

Registration records the repository root, alias, and allowed scope. It does not parse files immediately. The path must point to a readable local Git worktree; the target ref or worktree overlay is resolved during indexing. Registering the same Git root again adds an alias to the same repository id. If an alias already belongs to another repository id, registration fails.

## 5.2 Preview Scope

Preview the files covered by the current scope before indexing:

```bash
relay-knowledge repo scope preview core --ref HEAD --format json
```

`repo index --dry-run` uses the same preview path:

```bash
relay-knowledge repo index core --ref HEAD --dry-run --format json
```

Preview is useful after narrowing `--path` or `--language` so unrelated directories are not written into the code graph. The default source preset excludes dependency/cache/vendor/build/out/target directories, binary/media assets, `*.jsonl` dataset dumps, and lockfile snapshots such as `uv.lock`. Git-tracked source-language runtime subtrees under `dist`, such as `dist/js/core` or `dist/js/app`, are indexed, while minified files, CSS/assets, and other distribution subtrees remain excluded by default. Use a precise `--path` registration or request when other default-excluded files are intentionally needed.

## 5.3 Build the Code Graph Index

Index current `HEAD`:

```bash
relay-knowledge repo index core --ref HEAD --format json
```

Indexing an immutable commit is better for reproducible experiments:

```bash
relay-knowledge repo index core --ref <commit-sha> --format json
```

Full indexing reads ordinary blobs from a clean tree through Git and parses Rust, Python, JavaScript/JSX, TypeScript/TSX, Go, Java, Kotlin, Scala, C, C++, C#, Ruby, PHP, Swift, and Bash with tree-sitter. Gitlink submodules are skipped in the parent snapshot and should be registered as separate repositories when their contents need code graph coverage. Unsupported, invalid UTF-8, binary, oversized, or parser-failed files degrade to text-only or failed diagnostics without failing the whole batch.

When the requested full scope is not already fresh, `repo index` queues a durable background task and returns JSON with `task.state=queued` plus the target scope metadata instead of blocking on the entire cold parse. The CLI starts a bounded single-shot `repo index-worker` for that task; `relay-knowledge service run` also drains the same queue with one repository index worker. Running the same index request while a task is queued or running reuses the active task rather than starting parallel rebuilds for the same repository.

Fresh full indexes still return a completed `summary` immediately. Incremental `repo update` remains synchronous because it applies an explicit base-to-head diff and is already bounded by the changed path set.

## 5.4 Query Symbols and Relationships

Hybrid query:

```bash
relay-knowledge repo query core \
  --query retry_policy \
  --kind hybrid \
  --ref HEAD \
  --path src \
  --language rust \
  --freshness wait-until-fresh \
  --limit 10 \
  --format json
```

Narrow query kinds:

```bash
relay-knowledge repo query core --query RetryPolicy --kind symbol --format json
relay-knowledge repo query core --query retry_policy --kind definition --format json
relay-knowledge repo query core --query retry_policy --kind references --format json
relay-knowledge repo query core --query retry_policy --kind callers --format json
relay-knowledge repo query core --query retry_policy --kind callees --format json
relay-knowledge repo query core --query crate::retry_policy --kind imports --format json
```

Results include repository id, alias, `scope_id`, requested ref, resolved commit, tree hash, path, language, byte range, line range, symbol/file id, retrieval layer, index version, freshness, score, and excerpt.

Branches, tags, and `HEAD` first resolve to commit/tree. Multiple branches with the same tree hash reuse one scope, while the response keeps the requested ref for audit. After rebase or force-move, index the new head before querying; the query fails instead of returning old branch content.

Symbol hits also include `canonical_symbol_id` for expressing logical symbol identity across snapshots. Reference, call, and import hits return `edge_kind`, `edge_resolution_state`, `edge_target_hint`, `edge_confidence_basis_points`, and `edge_confidence_tier`. Uniquely unresolved targets are marked `unresolved` or `ambiguous` instead of being written as certain calls. If an import points at an unresolved external dependency that is not indexed as a code graph target, `repo query --kind imports` and repository-set import queries may run the bounded internal source fallback over the current indexed repository source. The fallback search is derived from the unresolved target hint and ranks after structured import-graph evidence, so agents can still inspect `edge_resolution_state` and `edge_target_hint` when limits are small. Fallback hits carry `text_fallback`, so agents should treat the result as local source-text evidence rather than dependency-library graph evidence. Missing dependency source remains unresolved edge coverage metadata and does not set `degraded_reason` unless the fallback itself fails.

`definition`, `references`, and `hybrid` queries run AST/FTS first and bounded internal exact-text source fallback last. The fallback starts only when the current structured results do not cover the requested identity or reference, or when a hybrid result window still has room. It searches materialized candidate files from the indexed commit after path, language, and scope filtering; it does not directly scan the current dirty worktree. Fallback hits include at least `lexical` and `text_fallback` in `retrieval_layers`, and definition fallback may also include `definition`. They do not carry resolved edge confidence because they are source-text evidence only.

If candidate-path lookup is unavailable, or if candidate-file, materialized-byte, or line-length budgets are exhausted, the query still returns existing code graph results and reports the source fallback diagnostic through `degraded_reason`. Narrowing `--path` or `--language`, and confirming that the target ref is fresh, is usually more useful than raising `--limit`.

### Feature-Flag Graph Queries

Existing repositories often spread feature flags across environment variables, config keys, settings objects, and guarded branches. `repo feature-flags` lists configuration-driven flags and their code relationships from facts extracted during indexing:

```bash
relay-knowledge repo feature-flags core --ref HEAD --format json
relay-knowledge repo feature-flags core --query checkout --path src --limit 20 --format json
```

Responses are grouped by feature flag and include configuration source, `defines_config`, `reads_config`, or `guards_code` relationships, source ranges, confidence, related symbols, and excerpts. The query reads only the feature-flag table and FTS documents for the selected indexed scope; it does not recursively grep the repository at query time. Re-run `repo index` or `repo update` after adding flags or changing extraction rules.

### Multi-Repository Repository Set Queries

Multi-repository query uses an explicit `repo-set` overlay. Index each member repository as a real single-repository snapshot first, then create a set and point members at those snapshots:

```bash
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace core --ref HEAD --priority 10 --format json
relay-knowledge repo-set add workspace sdk --ref HEAD --priority 0 --format json
relay-knowledge repo-set refresh workspace --format json
relay-knowledge repo-set remove workspace sdk --format json
```

`repo-set add` requires the target ref and path/language filters to have a matching single-repository indexed scope. If none exists, it fails instead of falling back to an older scope. Adding the same repository to the same set again replaces the previous member snapshot and invalidates the previous overlay edges. `repo-set remove` deletes a member pointer, invalidates the overlay, and lets normal code-scope retention reclaim that snapshot when nothing else references it. `repo-set refresh` rebuilds only cross-repository import/module overlay edges; it does not copy base facts into `code_repository_files`, `code_repository_symbols`, or `code_repository_chunks`. Async repository-set refreshes queued by CLI, Web, or MCP are drained by the resident `service run` repository-set overlay refresh worker.

Set queries fan out to each member’s real `source_scope`, then merge and rerank:

```bash
relay-knowledge repo-set query workspace \
  --query retry_policy \
  --kind definition \
  --freshness allow-stale \
  --limit 20 \
  --format json
```

Each result carries the member repository alias, repository id, resolved commit, tree hash, and original `source_scope`. Query `--path` and `--language` filters narrow the stored member scope; they do not widen it or switch to the repository's latest registration defaults. Same-named paths or symbols are not deduplicated across repositories; the dedupe key includes repository, scope, path, line range, and excerpt. `--freshness wait-until-fresh` requires every member snapshot to be fresh, moving refs such as `HEAD` to still resolve to the stored commit, and the overlay to be current. MCP uses the separate `relay_code_repository_set_query` tool, revalidates current set members for each call, records the set alias in audit entries, and requires the set alias or every member scope to be allowed by policy.

## 5.5 Incremental Updates

Index changes between two refs:

```bash
relay-knowledge repo update core --base main --head HEAD --format json
```

`repo update` applies the diff from `base` to `head` to the persisted `base` snapshot. `base` does not need to be the current active snapshot; it only needs to have been indexed for the same repository id, path filter, and language filter.

If the CLI reports that no matching indexed base scope exists, index the base first:

```bash
relay-knowledge repo index core --ref main --format json
relay-knowledge repo update core --base main --head HEAD --format json
```

The incremental path reads `git diff --name-status --find-renames -z` and rebuilds only added, modified, copied, renamed, or type-changed files. Deleted and renamed source paths are removed from the cloned base index, while rename lineage is kept as a tombstone.

## 5.6 Worktree Overlay

Use `--ref worktree` to index uncommitted work:

```bash
relay-knowledge repo index core --ref worktree --format json
relay-knowledge repo query core --query retry_policy --ref worktree --format json
```

The overlay is bound to the current checked-out `HEAD`, uses a synthetic snapshot identifier, and includes modified and untracked files. While an overlay is active, clean commit ref queries are rejected so uncommitted content is not mislabeled as a clean Git snapshot.

## 5.7 Impact Analysis

Analyze diff impact:

```bash
relay-knowledge repo impact core \
  --base main \
  --head HEAD \
  --limit 100 \
  --format json
```

Impact analysis verifies that `head_ref` has an indexed snapshot, filters changed paths through registration scope, then uses modules, symbols, callers, imports, and deleted symbol names to infer impacted locations.

## 5.8 Reports and Status

Generate a readable report:

```bash
relay-knowledge repo report core --format markdown
```

Use JSON for scripts:

```bash
relay-knowledge repo report core --format json
relay-knowledge repo status core --format json
```

Reports include repository id, root, indexed commit, tree hash, file/symbol/reference/chunk totals, scope, representative queries, latency samples, and degradation summary. Markdown reports fit PRs or release notes; JSON reports fit CI comparisons of index quality.

`repo status --format json` also includes `active_task` for queued/running/retrying cold indexes, `checkpoint` counters for the active or latest scope, and a `retention` summary. After a background full index succeeds, retention keeps the active scope, the two latest completed scopes, and unfinished task scopes; older scopes are pruned so large repositories do not accumulate unbounded SQLite rows.

`repo report --format markdown` also summarizes edge resolution counts for resolved, ambiguous, and unresolved edges. Use this to tell whether the graph is mostly deterministic AST extraction or still has many ambiguous edges requiring parser improvements.

## 5.9 Troubleshooting

When `repo query` returns no results, check in order:

1. Whether `repo status <alias>` shows an indexed clean commit or worktree overlay.
2. Whether query `--ref` matches the indexed snapshot.
3. Whether requested `--path` and `--language` only narrow the registered scope.
4. Whether `--kind` is too narrow; start with `--kind hybrid` when unsure.
5. Whether `degraded_reason` reports a source fallback candidate-path or budget issue; structured hits remain usable while exact-text fallback is degraded.
6. Whether files were diagnosed as unsupported, binary, oversized, invalid UTF-8, or parser failed.

`repo impact` requires an indexed snapshot for `--head`. Run `repo index core --ref <head>` or `repo update core --base <base> --head <head>` before impact analysis.
