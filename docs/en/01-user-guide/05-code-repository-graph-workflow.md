# Chapter 5: Code Repository Graph Workflow

[English](../../en/01-user-guide/05-code-repository-graph-workflow.md) | [中文](../../zh/01-user-guide/05-code-repository-graph-workflow.md)

The code repository graph brings Git trees or filesystem synthetic snapshots, files, symbols, references, calls, imports, and dependency inventory into one retrieval surface. It is not simple file search; queries and impact analysis depend on indexed code graph snapshots. Exact-text grep is only a bounded fallback layer over indexed snapshots, used to fill source-line gaps that AST and FTS leave explicit.

## 5.1 Register a Repository

Register a Git repository or non-Git source directory as a code retrieval source:

```bash
relay-knowledge repo register /path/to/repo \
  --path src \
  --format json
```

When `--alias` is omitted, the short name used by later commands defaults to the resolved Git root or filesystem root directory name. For `/path/to/repo`, later commands use `repo` unless an explicit `--alias` override is supplied. `--path` can be repeated. Registration rejects `--language` so mixed-language repositories keep their full language surface; later `repo query --language` requests can narrow results without shrinking the indexed snapshot.

Registration records the repository root, alias, and allowed scope. It does not parse files immediately. The path can point to a readable local Git worktree or ordinary source directory; the target ref, worktree overlay, or filesystem synthetic snapshot is resolved during indexing. Registering the same root again adds an alias to the same repository id. If an alias already belongs to another repository id, registration fails.

Remove a registered repository when its runtime state should be rebuilt from scratch:

```bash
relay-knowledge repo remove repo --format json
```

Removal deletes the registration, all aliases for that repository id, indexed scopes, code-index tasks, repository-set membership and overlays, and software projection rows. It does not delete files from the source repository on disk. Removal is rejected while a code-index task for that repository is still running; after removal, the same path or alias can be registered again.

## 5.2 Preview Scope

Preview the files covered by the current scope before indexing:

```bash
relay-knowledge repo scope preview repo --ref HEAD --format json
```

`repo index --dry-run` uses the same preview path:

```bash
relay-knowledge repo index repo --ref HEAD --dry-run --format json
```

Preview is useful after narrowing registered `--path` values so unrelated directories are not written into the code graph. Clean Git indexing reads the tracked tree as the authority: tracked directories such as `.cloudbuild/`, `.cid/`, `.build_config/`, `build/`, `dist/`, `vendor/`, and `third_party/` are eligible when they are inside the registered and requested path scope. Non-Git source directories default to whitelist scanning for root-level supported files and source-like roots such as `src/`, `include/`, `lib/`, `Sources/`, `packages/`, `modules/`, `plugins/`, `extensions/`, `docs/`, and `config/`; `build/`, `dist/`, `target/`, `node_modules/`, `vendor/`, `third_party/`, cache, virtualenv, and coverage directories enter only when an explicit `--path` opts in. The opt-in is path-specific: `--path src` does not scan sibling `node_modules/` or `target/`, while `--path build` or a path inside `build/` permits that broad directory and `--path .` permits the whole root. Default non-Git scans skip directories that cannot contribute whitelist content, and filtered non-Git scans skip unrelated sibling directories before reading them. If a directory contains Git metadata but Git cannot resolve it because of unsafe ownership or corrupt metadata, registration fails instead of falling back to non-Git indexing. A default `--path src` registration still expands only to discovered source roots such as `external_deps/`, `packages/`, `modules/`, `plugins/`, `extensions/`, `Sources/`, `lib/`, and nested JVM source roots; precise request path filters still narrow queries. The `filesystem:` snapshot id is scoped to the files that are actually indexed after that discovery step, so edits to unindexed files do not invalidate the scoped ref, queued synthetic refs are verified before background workers replay them, full-index batches and incremental deltas verify planned file hashes before accepting live bytes, and moving-ref resolution uses the same path and language filters as the indexed scope. Explicit stored `filesystem:` refs remain queryable after local edits; only source fallback reads require the live tree to still match. The remaining default preset is file-level protection for binary/media assets and `*.jsonl` dataset dumps. Lockfile snapshots such as `uv.lock` can contribute SBOM dependency facts without being expanded into source chunks or configuration symbols. Git worktree overlays use Git status, so untracked files ignored by `.gitignore` are not indexed unless Git reports them, untracked broad dependency/cache/build directories are not recursively expanded unless an explicit path filter opts in, and dirty submodule worktrees are not read until the submodule commit and parent gitlink are updated.

For `--ref worktree`, a committed submodule update is included when either the parent gitlink is staged or the submodule worktree `HEAD` has moved while the parent gitlink is still unstaged. When both states exist, the overlay reflects the checked-out submodule worktree `HEAD` so the worktree snapshot matches the files on disk. Staged submodule commits remain readable after deinit when the committed objects are available through `.git/modules`; uncommitted dirty content inside the submodule is still ignored.

## 5.3 Build the Code Graph Index

Index current `HEAD`:

```bash
relay-knowledge repo index repo --ref HEAD --format json
```

Indexing an immutable commit is better for reproducible experiments:

```bash
relay-knowledge repo index repo --ref <commit-sha> --format json
```

Full indexing reads ordinary blobs from a clean tree through Git or reads a filesystem synthetic snapshot from a non-Git source directory, performs bounded source-layout discovery, and parses Rust, Python, JavaScript/JSX, TypeScript/TSX, Go, Java, Kotlin, Scala, C, C++, C#, Ruby, PHP, Swift, Bash, SQL, and common project configuration/build/template files with tree-sitter. SQL files contribute table, view/materialized view, function/procedure, trigger, and type symbols, plus SQL object references and function/procedure-call edges. The configuration surface includes Markdown, XML, Bazel/Starlark, Make, CMake, Dockerfile/Containerfile, Java properties, TOML, INI, YAML, JSON, Go module files, Ninja, Jinja2, and Go templates; hierarchical configuration writes stable paths such as `server.port`, `containers[].name`, and `bin[].name`. Same-scope local file, template, and build-target references are resolved during finalize when the target is unambiguous; external or ambiguous references stay unresolved metadata. Gitlink submodules inside the requested path scope are expanded into the parent snapshot under paths such as `vendor/module/src/lib.rs` when their committed blobs are readable from the checked-out worktree or cached `.git/modules` gitdir, including custom submodule names and nested submodules. Uninitialized or inaccessible submodules are skipped until `git submodule update --init --recursive` or an available cached gitdir makes their committed blobs readable. Incremental updates expand bounded submodule gitlink changes for indexing and impact analysis, readable submodule commit bumps use the nested submodule diff so unchanged child files are not reparsed or seeded for impact, nested gitlink bumps expand to the nested child files instead of the gitlink path, and deleted gitlinks expand the base submodule tree so stale child paths are removed. Incremental indexing, worktree overlays, and impact analysis apply path scope before gitlink expansion and before enforcing expansion budgets, so out-of-scope submodule bumps remain ordinary changed paths instead of forcing large submodule scans. If a gitlink update expands beyond the incremental file budget inside the requested scope, run a full index so the work is checkpointed and batched. Submodules can still be registered separately when independent repository identity is required. Unsupported, invalid UTF-8, binary, oversized, or parser-failed files degrade to text-only or failed diagnostics without failing the whole batch.

When the requested full scope is not already fresh, `repo index` queues a durable background task and returns JSON with `task.state=queued` plus the target scope metadata instead of blocking on the entire cold parse. The CLI starts a bounded single-shot `repo index-worker` for that task; non-interactive agents can also call `repo index-worker --task-id <id> --format json` explicitly when they need to drain a queued or retrying task without holding a foreground `service run` process open. `relay-knowledge service run` acts as the resident master: it recovers expired code-index leases during startup, emits a startup status line on stderr, and drains the same queue with a bounded code-index worker pool, defaulting to 2 workers and configurable with `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` up to the documented cap of 8. Distinct fingerprints queue, lease, and checkpoint independently; identical full-index fingerprints reuse the active task instead of starting duplicate rebuilds. `relay-knowledge service status --format json` reports `code_index_workers` with configured workers, active worker slots, queue depth, queued/running/retrying/dead-letter task counts, running leases, and last error. During cross-batch finalization, `checkpoint.state` reports concrete phases such as `finalizing:resolve_references`, `finalizing:rebuild_reference_search`, `finalizing:rebuild_calls`, and `finalizing:publish_scope`; queries become fresh only after the checkpoint reaches `completed`.

In remote service mode, register the repository on the service host, start `service run --web`, and point the local CLI at the resident HTTP API with `--remote http://host:8791` or `RELAY_KNOWLEDGE_REMOTE_BASE_URL`. Remote `repo index` only submits a durable task and returns task/status/checkpoint data; it does not run `repo index-worker` in the local CLI process. The remote resident master drains the task through its code-index worker pool. Remote mode supports `repo index`, `repo scope preview`, `repo status`, `repo query`, `repo feature-flags`, `repo impact`, `repo report`, and `repo software`; it does not register local paths into a remote service. Run `repo index --reset` and `repo index-worker` on the service host because remote-selected CLIs reject those maintenance commands instead of falling back to local state.

```bash
RELAY_KNOWLEDGE_REMOTE_BASE_URL=http://127.0.0.1:8791 \
  relay-knowledge repo index repo --ref HEAD --format json
relay-knowledge --remote http://127.0.0.1:8791 repo query repo --query retry_policy --kind definition --freshness wait-until-fresh --format json
relay-knowledge --remote http://127.0.0.1:8791 repo software repo --kind relationships --ref HEAD --format json
```

Agent-oriented initialization should keep each command finite:

```bash
relay-knowledge repo register /path/to/repo --format json
relay-knowledge repo index repo --ref HEAD --format json
relay-knowledge repo status repo --format json
relay-knowledge repo index-worker --task-id <task-id-from-repo-index> --format json
relay-knowledge repo status repo --format json
```

If `repo index` already completed the single-shot worker, the later `repo index-worker` returns JSON with `claimed=false` and `task=null`; `repo status` remains the source of truth for checkpoint progress and freshness.

If an old service process died while holding a task lease and the task remains stuck, run `relay-knowledge repo index repo --reset --format json` to requeue unfinished tasks for that repository. Reset does not delete completed indexed scopes or revive historical dead-letter tasks; old workers cannot complete reset tasks because completion still requires the active lease owner and attempt token.

Fresh full indexes still return a completed `summary` immediately. Freshness checks compare the code-fact version embedded in the `scope_id`, so extraction-surface changes such as SBOM dependency facts require a rebuild even when the Git tree hash is unchanged. For Git scopes with submodules, the freshness key also records whether scoped gitlinks expanded from available submodule objects or were skipped as unavailable, so initializing a submodule after an earlier skipped index invalidates the old scope. Scoped Git freshness probes inspect only gitlinks that overlap the requested path filters before falling back to whole-tree submodule state for unscoped scopes. Incremental `repo update` remains synchronous because it applies an explicit base-to-head diff and is already bounded by the changed path set; new files under non-`src` roots such as `external_deps/` or `modules/` use the same source-layout policy.

## 5.4 Query Symbols and Relationships

Hybrid query:

```bash
relay-knowledge repo query repo \
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
relay-knowledge repo query repo --query RetryPolicy --kind symbol --format json
relay-knowledge repo query repo --query retry_policy --kind definition --format json
relay-knowledge repo query repo --query retry_policy --kind references --format json
relay-knowledge repo query repo --query retry_policy --kind callers --format json
relay-knowledge repo query repo --query retry_policy --kind callees --format json
relay-knowledge repo query repo --query crate::retry_policy --kind imports --format json
relay-knowledge repo query repo --query serde --kind sbom --format json
```

Results include repository id, alias, `scope_id`, requested ref, resolved commit, tree hash, path, language, byte range, line range, symbol/file id, retrieval layer, index version, freshness, score, and excerpt.

The JSON response also includes a top-level `freshness` object for graph governance. It reports `state` (`fresh`, `pending`, `stale`, or `degraded`), graph version, served source scope, requested-versus-served ref lag, checkpoint cursor counts, pending code-index task and queue state, stale/degraded reasons, and whether direct source reads are required. If `--freshness allow-stale` serves the last completed index while a newer ref is queued or running, `metadata.stale`, `scope.stale`, and `freshness.direct_source_read_required` are all true; agents must read the returned `freshness.direct_source_read_paths` from source before editing or citing changed files. `--freshness wait-until-fresh` suppresses stale code graph answers and returns an error until the requested scope is indexed.

Branches, tags, and `HEAD` first resolve to commit/tree. Multiple branches with the same tree hash reuse one scope, while the response keeps the requested ref for audit. After rebase or force-move, index the new head before querying; the query fails instead of returning old branch content.

Workspace import resolution is an opt-in indexing feature. API callers can set `CodeIndexRequest.workspace_detection.enabled` with pnpm, Go, or Cargo workspace formats to record package mappings and derive `cross_repo_import` edges for unresolved sibling-package imports during snapshot apply or checkpoint finalization. Web operation payloads for code repository indexing accept the same `workspace_detection` object. CLI indexing leaves this detection disabled by default, preserving the existing single-repository path unless a caller explicitly opts in.

Symbol hits also include `canonical_symbol_id` for expressing logical symbol identity across snapshots. Reference, call, import, and SBOM hits return `edge_kind`, `edge_resolution_state`, `edge_target_hint`, `edge_confidence_basis_points`, and `edge_confidence_tier`. Uniquely unresolved targets are marked `unresolved` or `ambiguous` instead of being written as certain calls. `repo query --kind sbom` returns dependency declarations and locked packages extracted during indexing from `Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`, `go.mod`, `go.sum`, `pyproject.toml`, `uv.lock`, `requirements*.txt`, files under `requirements/`, `constraints.txt`, Maven effective `pom.xml` dependencies and BOM imports, Gradle dependency blocks, CMake `CMakeLists.txt`, Conan `conanfile.txt` or common `conanfile.py` declarations, and allowlisted IaC YAML such as GitHub Actions workflows, GitLab CI, Docker Compose, Helm `Chart.yaml`, and Ansible `requirements.yml`. YAML, JSON, TOML, INI, and Java properties files are also indexed as code languages for configuration-key search, so `--language yaml|json|toml|ini|properties` can retrieve nested configuration keys, sections, and their evidence lines; dependency-only lockfiles such as `package-lock.json` and `uv.lock` contribute SBOM facts without expanding every locked key into symbols or source chunks. Shared npm, JVM, CMake, Conan, and IaC manifests preserve compatible language scope for TypeScript/JSX, Kotlin/Scala, C/C++, and YAML queries. It handles common Python PEP 508 markers, editable Python direct references, uv dependency groups, Cargo rename syntax, CMake package declarations, Gradle map-style notation, and Maven repository-local parent POM/property/dependencyManagement resolution; it de-duplicates `go.sum` module and `/go.mod` pairs, skips local Cargo path/workspace packages, local npm `file:`/`link:`/`workspace:` specs, local npm package-lock v1/v2 workspace rows, local Python/Poetry/uv path dependencies, local CMake subdirectories, and local workflow actions, and treats Maven imported BOMs as SBOM records. It does not execute package managers, CI workflows, Maven, CMake, Helm, Docker, or Kubernetes tooling, resolve transitives, contact registries, or provide vulnerability/license analysis. If an import points at an unresolved external dependency that is not indexed as a code graph target, `repo query --kind imports` and repository-set import queries may run the bounded internal source fallback over the current indexed repository source. The fallback search is derived from the unresolved target hint and ranks after structured import-graph evidence, so agents can still inspect `edge_resolution_state` and `edge_target_hint` when limits are small. Fallback hits carry `text_fallback`, so agents should treat the result as local source-text evidence rather than dependency-library graph evidence. Missing dependency source remains unresolved edge coverage metadata and does not set `degraded_reason` unless the fallback itself fails.

`definition`, `references`, and `hybrid` queries run AST/FTS first and bounded internal exact-text source fallback last. The fallback starts only when the current structured results do not cover the requested identity or reference, or when a hybrid result window still has room. It searches materialized candidate files from the indexed commit after path, language, and scope filtering; it does not directly scan the current dirty worktree. For non-Git `filesystem:` commits, fallback first verifies that the live tree still resolves to the same synthetic snapshot and reports degradation instead of reading changed live files. Fallback hits include at least `lexical` and `text_fallback` in `retrieval_layers`, and definition fallback may also include `definition`. They do not carry resolved edge confidence because they are source-text evidence only.

If candidate-path lookup is unavailable, or if candidate-file, materialized-byte, or line-length budgets are exhausted, the query still returns existing code graph results and reports the source fallback diagnostic through `degraded_reason`. Narrowing `--path` or `--language`, and confirming that the target ref is fresh, is usually more useful than raising `--limit`.

### Feature-Flag Graph Queries

Existing repositories often spread feature flags across environment variables, config keys, settings objects, SDK clients, and guarded branches. `repo feature-flags` lists configuration-driven flags and their code relationships from facts extracted during indexing:

```bash
relay-knowledge repo feature-flags repo --ref HEAD --format json
relay-knowledge repo feature-flags repo --query checkout --path src --limit 20 --format json
```

Responses are grouped by feature flag and include configuration source, `defines_config`, `reads_config`, or `guards_code` relationships, source ranges, confidence, related symbols, and excerpts. The indexer recognizes static code/config evidence from environment access, config/settings reads, boolean config facts from supported configuration formats, and common OpenFeature, LaunchDarkly, and Unleash evaluation calls. Provider control-plane state such as rollout strategies, segments, and variants is not synchronized in this path. The query reads only the feature-flag table and FTS documents for the selected indexed scope; it does not recursively grep the repository at query time. Re-run `repo index` or `repo update` after adding flags or changing extraction rules.

### Software Global Projection

`repo software` exposes repository-scoped software graph projections for dependencies, unresolved SDK/API usage, whole-file nodes, documentation topics, and cross-domain relationships:

```bash
relay-knowledge repo software repo --kind files --ref HEAD --format json
relay-knowledge repo software repo --kind topics --ref HEAD --format json
relay-knowledge repo software repo --kind relationships --ref HEAD --format json
```

The projection connects Markdown/spec headings and `.knowledge/knowledge-map.yaml` topics with documentation files, dependency manifests with package components, unresolved imports with SDK/API usage candidates, and config/feature-flag facts with code or config files. It reads committed projection tables for the selected indexed scope and does not scan package caches, SDK directories, unindexed external source, or whole-repository docs at query time.

### Multi-Repository Repository Set Queries

Multi-repository query uses an explicit `repo-set` overlay. Index each member repository as a real single-repository snapshot first, then create a set and point members at those snapshots:

```bash
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace repo --ref HEAD --priority 10 --format json
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
relay-knowledge repo update repo --base main --head HEAD --format json
```

`repo update` applies the diff from `base` to `head` to the persisted `base` snapshot. `base` does not need to be the current active snapshot; it only needs to have been indexed for the same repository id, path filter, and language filter. For non-Git scopes, delta parsing rejects live bytes that no longer match the planned filesystem content hashes.

If the CLI reports that no matching indexed base scope exists, index the base first:

```bash
relay-knowledge repo index repo --ref main --format json
relay-knowledge repo update repo --base main --head HEAD --format json
```

The incremental path reads `git diff --name-status --find-renames -z` and rebuilds only added, modified, copied, renamed, or type-changed files. Deleted and renamed source paths are removed from the cloned base index, while rename lineage is kept as a tombstone.

## 5.6 Worktree Overlay

Use `--ref worktree` to index uncommitted work:

```bash
relay-knowledge repo index repo --ref worktree --format json
relay-knowledge repo query repo --query retry_policy --ref worktree --format json
```

The overlay is bound to the current checked-out `HEAD`, uses a synthetic snapshot identifier, and includes modified files, untracked files, staged submodule gitlink updates, and unstaged submodule worktree commits when the submodule `HEAD` differs from the parent gitlink. If a submodule has both a staged gitlink and a different checked-out submodule `HEAD`, the overlay indexes the checked-out worktree commit. Staged submodule commits remain readable from cached gitdirs after deinit. Staged submodule additions, removals, renames, and file/submodule replacements clean up the old indexed paths by expanded child path rather than by the gitlink path alone. While an overlay is active, clean commit ref queries are rejected so uncommitted content is not mislabeled as a clean Git snapshot.

## 5.7 Impact Analysis

Analyze diff impact:

```bash
relay-knowledge repo impact repo \
  --base main \
  --head HEAD \
  --limit 100 \
  --format json
```

Impact analysis verifies that `head_ref` has an indexed snapshot, filters changed paths through registration scope, then uses modules, symbols, callers, imports, and deleted symbol names to infer impacted locations. Non-Git impact requests use the same indexed filesystem scope filters, so explicitly indexed `build/` or `vendor/` paths are not dropped by the default non-Git scan policy.

## 5.8 Reports and Status

Generate a readable report:

```bash
relay-knowledge repo report repo --format markdown
```

Use JSON for scripts:

```bash
relay-knowledge repo report repo --format json
relay-knowledge repo status repo --format json
```

Reports include repository id, root, indexed commit, tree hash, file/symbol/reference/chunk totals, scope, representative queries, latency samples, and degradation summary. Markdown reports fit PRs or release notes; JSON reports fit CI comparisons of index quality.

`repo status --format json` also includes `active_task` for queued/running/retrying cold indexes, `checkpoint` counters for the active or latest scope, and a `retention` summary. If a repository is still marked `indexing` but no task is active, status falls back to the repository's latest checkpoint so operators can see the last durable phase instead of a blank progress report. After a background full index succeeds, retention keeps the active scope, the two latest completed scopes, and unfinished task scopes; older scopes are pruned so large repositories do not accumulate unbounded SQLite rows.

`repo report --format markdown` also summarizes edge resolution counts for resolved, ambiguous, and unresolved edges. Use this to tell whether the graph is mostly deterministic AST extraction or still has many ambiguous edges requiring parser improvements.

## 5.9 Troubleshooting

When `repo query` returns no results, check in order:

1. Whether `repo status <alias>` shows an indexed clean commit or worktree overlay.
2. Whether query `--ref` matches the indexed snapshot.
3. Whether requested `--path` and `--language` only narrow the registered scope.
4. Whether `--kind` is too narrow; start with `--kind hybrid` when unsure.
5. Whether `degraded_reason` reports a source fallback candidate-path or budget issue; structured hits remain usable while exact-text fallback is degraded.
6. Whether files were diagnosed as unsupported, binary, oversized, invalid UTF-8, or parser failed.

`repo impact` requires an indexed snapshot for `--head`. Run `repo index repo --ref <head>` or `repo update repo --base <base> --head <head>` before impact analysis.
