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

`expected_degraded_file_count` is validated by the same parser and bounded batches as full indexing, against the preview resolved snapshot. It includes syntax errors, invalid UTF-8, binary content, unsupported grammars, and oversized files; each diagnostic file is counted once. Missing external dependency source alone is not degradation. Preview reads and parses selected source files, so it costs more than a metadata-only listing, but writes no index facts, tasks, or checkpoints. The application admits at most two preview workers, waits up to five seconds for admission, and applies a 120-second response deadline. Cancellation or timeout stops further batches after the current blocking batch finishes; its worker retains its permit until it exits. Incomplete counts are never returned as successful previews. Narrow the registered scope if a preview times out. Compare this count with a full index of the same resolved ref and scope; incremental summaries and worktree overlays may cover a different file set.

Preview is useful after narrowing registered `--path` values so unrelated directories are not written into the code graph. Clean Git indexing reads the tracked tree as the authority: tracked directories such as `.cloudbuild/`, `.cid/`, `.build_config/`, `build/`, `dist/`, `vendor/`, and `third_party/` are eligible when they are inside the registered and requested path scope. Non-Git source directories default to whitelist scanning for root-level supported files and source-like roots such as `src/`, `include/`, `lib/`, `Sources/`, `packages/`, `modules/`, `plugins/`, `extensions/`, `docs/`, and `config/`; `build/`, `dist/`, `target/`, `node_modules/`, `vendor/`, `third_party/`, cache, virtualenv, and coverage directories enter only when an explicit `--path` opts in. The opt-in is path-specific: `--path src` does not scan sibling `node_modules/` or `target/`, while `--path build` or a path inside `build/` permits that broad directory and `--path .` permits the whole root. Default non-Git scans skip directories that cannot contribute whitelist content, and filtered non-Git scans skip unrelated sibling directories before reading them. If a directory contains Git metadata but Git cannot resolve it because of unsafe ownership or corrupt metadata, registration fails instead of falling back to non-Git indexing. A default `--path src` registration still expands only to discovered source roots such as `external_deps/`, `packages/`, `modules/`, `plugins/`, `extensions/`, `Sources/`, `lib/`, and nested JVM source roots; precise request path filters still narrow queries. The `filesystem:` snapshot id is scoped to the files that are actually indexed after that discovery step, so edits to unindexed files do not invalidate the scoped ref, queued synthetic refs are verified before background workers replay them, full-index batches and incremental deltas verify planned file hashes before accepting live bytes, and moving-ref resolution uses the same path and language filters as the indexed scope. Explicit stored `filesystem:` refs remain queryable after local edits; only source fallback reads require the live tree to still match. The remaining default preset is file-level protection for binary/media assets and `*.jsonl` dataset dumps. Lockfile snapshots such as `uv.lock` can contribute SBOM dependency facts without being expanded into source chunks or configuration symbols. Git worktree overlays use Git status, so untracked files ignored by `.gitignore` are not indexed unless Git reports them, untracked broad dependency/cache/build directories are not recursively expanded unless an explicit path filter opts in, and dirty submodule worktrees are not read until the submodule commit and parent gitlink are updated.

`--path` is registered or query-time scope, not an index-time option. Use
`repo register <path> --path <filter>` to select the source scope, then run
`repo index <alias> --ref HEAD` without `--path`. For non-Git directories,
`HEAD` is the normal moving filesystem selector and resolves to a
`filesystem:<hash>` snapshot; `worktree` is only for Git worktree overlays.

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

When the requested full scope is not already fresh, `repo index` queues a durable background task and returns JSON with `task.state=queued` plus the target scope metadata instead of blocking on the entire cold parse. With the explicit `--reuse-historical` option, Git full initialization checks at most 10 ancestors from nearest to farthest on the target commit's first-parent chain before constructing a cold plan. The nearest published scope that is not stale or retiring, has compatible filters, and uses the current fact version becomes the incremental base. This initialization-reuse diff is capped at 100 changed paths; an oversized diff or missing base continues through the checkpointed full index. Without the option, initialization remains a full index. Incremental tasks converted from full requests retain the existing lease, publication fence, atomic snapshot, and retention workflow. The CLI starts a bounded single-shot `repo index-worker` for that task; non-interactive agents can also call `repo index-worker --task-id <id> --format json` explicitly when they need to drain a queued or retrying task without holding a foreground `service run` process open. `relay-knowledge service run` acts as the resident master: it recovers expired code-index leases during startup, emits a startup status line on stderr, and drains the same queue with a bounded code-index worker pool, defaulting to 2 workers and configurable with `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` up to the documented cap of 8. Distinct fingerprints queue, lease, and checkpoint independently; an unfinished full or incremental task for the same target and requested scope is reused to avoid cross-mode duplicate builds. `relay-knowledge service status --format json` reports `code_index_workers` with configured workers, active worker slots, queue depth, queued/running/retrying/dead-letter task counts, running leases, and last error. During cross-batch finalization, `checkpoint.state` reports concrete phases such as `finalizing:resolve_references`, `finalizing:rebuild_reference_search`, `finalizing:rebuild_calls`, and `finalizing:publish_scope`; queries become fresh only after the checkpoint reaches `completed`.

The CLI-shaped Web index request accepts the optional boolean `reuse_historical`; omission and `null` keep the default full-index behavior. The 100-path reuse budget counts both the old and new path of each rename or copy, and a historical base retired before task admission safely falls back to a full task.

In remote service mode, register the repository on the service host, start `service run --web`, and point the local CLI at the resident HTTP API with `--remote http://host:8791` or `RELAY_KNOWLEDGE_REMOTE_BASE_URL`. Remote `repo index` and `repo update` only submit durable tasks and return task/status/checkpoint data; they do not run `repo index-worker` in the local CLI process. The remote resident master drains the task through its code-index worker pool. Remote mode supports `repo list`, `repo index`, `repo update`, `repo scope preview`, `repo status`, `repo query`, `repo context`, `repo framework`, `repo feature-flags`, `repo impact`, `repo report`, `repo software` including standard exports, and `repo view`; it does not register local paths into a remote service. Run `repo index --reset` and `repo index-worker` on the service host because remote-selected CLIs reject those maintenance commands instead of falling back to local state.

```bash
RELAY_KNOWLEDGE_REMOTE_BASE_URL=http://127.0.0.1:8791 \
  relay-knowledge repo index repo --ref HEAD --format json
relay-knowledge --remote http://127.0.0.1:8791 repo list --format json
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

If `repo index` already completed the single-shot worker, the later `repo index-worker` returns JSON with `claimed=false` and `task=null`, but it still advances one bounded retention pass. Repeat it while `maintenance_active=true` or `repo status` reports `maintenance_pending=true`. If optional `maintenance_error` is present, report and resolve it instead of interpreting `maintenance_active=false` as completion; status remains the source of truth for checkpoint progress, GC errors, and freshness.

If an old service process died while holding a task lease and the task remains stuck, run `relay-knowledge repo index repo --reset --format json` to requeue unfinished tasks for that repository. Reset does not delete completed indexed scopes or revive historical dead-letter tasks; old workers cannot complete reset tasks because completion still requires the active lease owner and attempt token.

Fresh full indexes still return a completed `summary` immediately. Freshness checks compare the code-fact version embedded in the `scope_id`, so extraction-surface changes such as SBOM dependency facts or web route facts require a rebuild even when the Git tree hash is unchanged. For Git scopes with submodules, the freshness key also records whether scoped gitlinks expanded from available submodule objects or were skipped as unavailable, so initializing a submodule after an earlier skipped index invalidates the old scope. Scoped Git freshness probes inspect only gitlinks that overlap the requested path filters before falling back to whole-tree submodule state for unscoped scopes. Incremental `repo update` now enters the same durable task, lease, retry, and publication path as a full index; only full rebuilds expose batch checkpoints, while the bounded incremental snapshot publishes atomically. The local CLI performs a bounded drain attempt, while remote or watcher-triggered calls may remain queued for resident workers. New files under non-`src` roots such as `external_deps/` or `modules/` use the same source-layout policy.

## 5.4 Query Symbols and Relationships

Short type-name call queries use indexed type and direct-member ownership across the languages in the capability matrix below. `--query B --kind callers` aggregates incoming edges to B, its constructors and direct methods; `--kind callees` aggregates their outgoing edges. Results retain the actual caller/callee methods and call sites. For `A.main` calling `B.process()`, B callers returns `main calls process`; A callers does not incorrectly return that outgoing edge merely because its text mentions A. A matched class with no edges in the requested direction returns empty without full-text broadening.

This aggregation applies to short type names with `callers` or `callees`; names match case-sensitively. Hybrid and qualified-name queries retain their existing behavior. Persisted ownership separates language and module identities; nested types and local functions do not contribute to their enclosing type. Inherited members and dynamic dispatch are not guessed. Unresolved outgoing edges retain their status. `--query B.process` continues to query a specific method. Path, language, generated-file and inline filters apply before call-candidate limits; paths constrain call sites, so callers may be outside the selected type file.

Selection admits at most 64 candidate classes and 1,024 class/member records, retaining the existing maximum of 200 call candidates and the requested result limit. Class resolution and call reads share approximately 4.1 million SQLite instructions. Exhausting identity or execution budgets explicitly returns a `class call query incomplete` capacity error instead of silently truncating identities. Query a member method to reduce expansion. This read-only change reuses existing facts, with no schema, fact-version or installation configuration change; completed older indexes can be queried directly.

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

Agents can also put structured filters inside `--query`, for example
`--query "kind:function,method lang:rust path:storage name:query search_code"`.
Recognized labels are `kind:`, `lang:` or `language:`, `path:`, and `name:`.
Unknown `prefix:value` tokens stay in the search text. Inline language filters
intersect explicit `--language` values and are pushed into SQL candidate
selection; inline symbol kind filters are pushed into symbol SQL. Inline `path:`
and `name:` filters are applied after scoring and before result truncation, so
they narrow returned paths, symbol names, or SBOM package identities without
changing the indexed scope.

Results include repository id, alias, `scope_id`, requested ref, resolved commit, tree hash, path, language, byte range, line range, symbol/file id, retrieval layer, index version, freshness, score, and excerpt.

The JSON response also includes a top-level `freshness` object for graph governance. It reports `state` (`fresh`, `pending`, `stale`, or `degraded`), graph version, served source scope, requested-versus-served ref lag, checkpoint cursor counts, pending code-index task and queue state, stale/degraded reasons, and whether direct source reads are required. If `--freshness allow-stale` serves the last completed index while a newer ref is queued or running, `metadata.stale`, `scope.stale`, and `freshness.direct_source_read_required` are all true; agents must read the returned `freshness.direct_source_read_paths` from source before editing or citing changed files. `--freshness wait-until-fresh` suppresses stale code graph answers and returns an error until the requested scope is indexed.

Branches, tags, and `HEAD` first resolve to commit/tree. Multiple branches with the same tree hash reuse one scope, while the response keeps the requested ref for audit. After rebase or force-move, index the new head before querying; the query fails instead of returning old branch content.

Workspace import resolution is an opt-in indexing feature. API callers can set `CodeIndexRequest.workspace_detection.enabled` with pnpm, Go, or Cargo workspace formats to record package mappings and derive `cross_repo_import` edges for unresolved sibling-package imports during snapshot apply or checkpoint finalization. Web operation payloads for code repository indexing accept the same `workspace_detection` object. CLI indexing leaves this detection disabled by default, preserving the existing single-repository path unless a caller explicitly opts in.

Symbol hits also include `canonical_symbol_id` for expressing logical symbol identity across snapshots. Reference, call, import, and SBOM hits return `edge_kind`, `edge_resolution_state`, `edge_target_hint`, `edge_confidence_basis_points`, and `edge_confidence_tier`. Uniquely unresolved targets are marked `unresolved` or `ambiguous` instead of being written as certain calls. `repo query --kind sbom` returns dependency declarations and locked packages extracted during indexing from `Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`, `go.mod`, `go.sum`, `pyproject.toml`, `uv.lock`, `requirements*.txt`, files under `requirements/`, `constraints.txt`, Maven effective `pom.xml` dependencies and BOM imports, Gradle dependency blocks, CMake `CMakeLists.txt`, Conan `conanfile.txt` or common `conanfile.py` declarations, and allowlisted IaC YAML such as GitHub Actions workflows, GitLab CI, Docker Compose, Helm `Chart.yaml`, and Ansible `requirements.yml`. YAML, JSON, TOML, INI, and Java properties files are also indexed as code languages for configuration-key search, so `--language yaml|json|toml|ini|properties` can retrieve nested configuration keys, sections, and their evidence lines; dependency-only lockfiles such as `package-lock.json` and `uv.lock` contribute SBOM facts without expanding every locked key into symbols or source chunks. Shared npm, JVM, CMake, Conan, and IaC manifests preserve compatible language scope for TypeScript/JSX, Kotlin/Scala, C/C++, and YAML queries. It handles common Python PEP 508 markers, editable Python direct references, uv dependency groups, Cargo rename syntax, CMake package declarations, Gradle map-style notation, and Maven repository-local parent POM/property/dependencyManagement resolution; it de-duplicates `go.sum` module and `/go.mod` pairs, skips local Cargo path/workspace packages, local npm `file:`/`link:`/`workspace:` specs, local npm package-lock v1/v2 workspace rows, local Python/Poetry/uv path dependencies, local CMake subdirectories, and local workflow actions, and treats Maven imported BOMs as SBOM records. It does not execute package managers, CI workflows, Maven, CMake, Helm, Docker, or Kubernetes tooling, resolve transitives, contact registries, or provide vulnerability/license analysis. An unresolved external import whose structured import-graph excerpt already contains a source-like statement with the same parsed specifier is complete local source evidence; `repo query --kind imports` and repository-set import queries do not add redundant `text_fallback` hits for that surface. Relative imports, dynamic-import intent, incomplete excerpts, and mixed complete/incomplete result sets may still run bounded internal source fallback over the current indexed repository source. That fallback is derived from the unresolved target hint and ranks after structured import-graph evidence; its hits carry `text_fallback` and are local source-text evidence, not dependency-library graph evidence. Missing dependency source remains unresolved edge coverage metadata and does not set `degraded_reason` unless a required fallback itself fails.

`definition`, `references`, and `hybrid` queries run AST/FTS first and bounded internal exact-text source fallback last. The fallback starts when the current structured results do not cover the requested identity or reference, when a hybrid result window still has room, or when a fresh scope reports parser-degraded files whose missing reference facts must not be hidden by healthy files that did produce structured hits. It searches materialized candidate files from the indexed commit after path, language, and scope filtering; it does not directly scan the current dirty worktree. For non-Git `filesystem:` commits, fallback first verifies that the live tree still resolves to the same synthetic snapshot and reports degradation instead of reading changed live files. Fallback hits include at least `lexical` and `text_fallback` in `retrieval_layers`, and definition fallback may also include `definition`. They do not carry resolved edge confidence because they are source-text evidence only.

If candidate-path lookup is unavailable, or if candidate-file, materialized-byte, or line-length budgets are exhausted, the query still returns existing code graph results and reports the source fallback diagnostic through `degraded_reason`. Narrowing `--path` or `--language`, and confirming that the target ref is fresh, is usually more useful than raising `--limit`.

### Angular and Vue Framework Graph Queries

`repo framework` exposes component/template semantics as an independent graph instead of mixing framework facts into ordinary symbol hits:

```bash
relay-knowledge repo framework repo --framework angular --kind component --path src/app --format json
relay-knowledge repo framework repo --framework vue --kind prop --query modelValue --limit 20 --format json
```

Angular indexing reads decorators plus inline or external HTML templates. Vue SFC indexing records props, emits, models, slots, template variables, and control flow while still sending embedded script content through ordinary TypeScript/JavaScript extraction. The response separates typed `nodes` and `edges`, carries resolution state and target hints, and marks result truncation explicitly. Queries use only committed, bounded framework tables; they do not parse templates or read the worktree on demand.

### Feature-Flag Graph Queries

Existing repositories often spread feature flags across environment variables, config keys, settings objects, SDK clients, and guarded branches. `repo feature-flags` lists configuration-driven flags and their code relationships from facts extracted during indexing:

```bash
relay-knowledge repo feature-flags repo --ref HEAD --format json
relay-knowledge repo feature-flags repo --query checkout --path src --limit 20 --format json
```

Responses are grouped by feature flag and include configuration source, `defines_config`, `reads_config`, or `guards_code` relationships, source ranges, confidence, related symbols, and excerpts. The indexer recognizes static code/config evidence from environment access, config/settings reads, boolean config facts from supported configuration formats, and common OpenFeature, LaunchDarkly, and Unleash evaluation calls. Provider control-plane state such as rollout strategies, segments, and variants is not synchronized in this path. The query reads only the feature-flag table and FTS documents for the selected indexed scope; it does not recursively grep the repository at query time. Re-run `repo index` or `repo update` after adding flags or changing extraction rules.

### Software Global Ontology and Compatibility Projections

`repo software` exposes compatibility projections, typed ontology entities, provenance statements, and conflict diagnostics for one repository scope:

```bash
relay-knowledge repo software repo --kind files --ref HEAD --format json
relay-knowledge repo software repo --kind topics --ref HEAD --format json
relay-knowledge repo software repo --kind relationships --ref HEAD --format json
relay-knowledge repo software repo --kind systems --ref HEAD --format json
relay-knowledge repo software repo --kind statements --ref HEAD --format json
relay-knowledge repo software repo --kind conflicts --ref HEAD --format json
relay-knowledge repo software export repo --profile cyclonedx-1.7 --ref HEAD --format json
```

An `entity_key` remains stable across commits while `occurrence_id` binds a snapshot and evidence. Ordinary Markdown/spec headings become only documentation units or topics. Explicit frontmatter, API traits or schemas, test symbols, Dockerfiles and build files, Compose/Kubernetes/Terraform, and service definitions project into their corresponding controlled kinds. Dockerfiles and CI jobs no longer become IaC resources. Statements retain source kind, evidence, extractor version, assertion/resolution/fact state, time, and confidence. A statement without evidence or violating a shape returns a `rejected` diagnostic and does not become an accepted fact. Every slice and SPDX 3.0.1, CycloneDX 1.7, or PROV-O export reads committed tables for the selected indexed scope and does not scan package caches, SDK directories, unindexed external source, or whole-repository docs at query time.

### Multi-Repository Repository Set Queries

Multi-repository query uses an explicit `repo-set` overlay. Index each member repository as a real single-repository snapshot first, then create a set and point members at those snapshots:

```bash
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace repo --ref HEAD --priority 10 --format json
relay-knowledge repo-set add workspace sdk --ref HEAD --priority 0 --format json
relay-knowledge repo-set refresh workspace --format json
relay-knowledge repo-set remove workspace sdk --format json
```

`repo-set add` requires the target ref and path/language filters to have a matching single-repository indexed scope. If none exists, it fails instead of falling back to an older scope. Adding the same repository to the same set again replaces the previous member snapshot and invalidates the previous overlay edges. `repo-set remove` deletes a member pointer, invalidates the overlay, and lets normal code-scope retention reclaim that snapshot when nothing else references it. `repo-set refresh` rebuilds only cross-repository import/module overlay edges; it does not copy base facts into `code_repository_files`, `code_repository_symbols`, or `code_repository_chunks`. CLI/Web default-synchronous and async refreshes first enter the same bounded durable queue. A local default-synchronous request drains only when its exact task is claimable; otherwise it returns queued, and the resident `service run` worker drains it. Overlay edges and member replacements publish together in one attempt-scoped live-lease transaction, so takeover rolls back a stale attempt. This overlay capability still requires `single_sqlite`; `partitioned_sqlite` reports it as unsupported until cross-shard import/export aggregation exists.

A manual set admits at most 64 members, and one publication replaces at most 64 member fact versions. One whole refresh shares manifest ceilings of 4,096 chunks, 16 MiB of path/content bytes, and 32,768 derived items across every member. Overlay refresh scans immutable import rows with a `(source_scope, import_id)` primary-key cursor in pages of 512, admits at most 262,144 scanned rows across the set, and filters unresolved external candidates in memory; resolved and local rows consume the same scan budget. It keeps at most 131,072 file/symbol export targets and 8,192 cross-member candidate edges. An external import with no member export candidate remains authoritative unresolved metadata in its source scope and is not duplicated as a set edge. Selector requests admit 512 combined origin/target keys. Matching observes at most 11 exports per import and retains at most 10 candidate IDs, so an ambiguous `candidate_count` is bounded rather than exhaustive. Scan or collection cap-plus-one overflow returns retryable `qos_rejected` instead of publishing a truncated `fresh` overlay. Direct and selector reads inspect at most 8,193 edges and exclude edges whose origin or target scope is retiring. Refresh, add, and member removal reject a legacy overlay above 8,192 edges before an unbounded delete. Removing a repository also atomically rejects more than 64 affected sets or any affected over-cap overlay. The current release has no bounded legacy-overlay repair command, so rejected data remains unchanged until an upgrade supplies a repair tool. These ceilings cover manually managed repository sets, not the opt-in automatic-workspace cross-edge builder; phased scope GC bounds deletion of obsolete workspace state but not a single automatic build, so workspace detection remains disabled by default and this build-path bound is future work.

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

Update from the last published clean snapshot to the checked-out `HEAD`:

```bash
relay-knowledge repo update repo --format json
```

Omitting `--base` selects the last successfully published clean Git commit. If the active identity is a worktree overlay, the service unwraps its clean base. Omitting `--head` selects `HEAD`. Both refs are resolved and pinned to immutable commit ids and the target tree before the durable task is queued, so later ref movement cannot change that task's input. A completed response includes `summary.base_resolved_commit_sha`; a queued response exposes the pinned base/head in `task.mode`. Use `repo status --format json` to inspect task and checkpoint state.

Select an explicit pair when required:

```bash
relay-knowledge repo update repo --base <base-commit> --head <head-commit> --format json
```

`repo update` applies the diff from `base` to `head` to the persisted `base` snapshot. `base` does not need to be the current active snapshot; it only needs to have been indexed for the same repository id, path filter, and language filter. For non-Git scopes, delta parsing rejects live bytes that no longer match the planned filesystem content hashes. The Git changed-path set is capped at 512 across the commit pair before registered path filters; use a full index instead of turning an oversized delta into an unbounded task.

If the CLI reports that no matching indexed base scope exists, index the base first:

```bash
relay-knowledge repo index repo --ref main --format json
relay-knowledge repo update repo --base main --head HEAD --format json
```

The incremental path reads `git diff --name-status --find-renames -z` and rebuilds only added, modified, copied, renamed, or type-changed files. Deleted and renamed source paths are removed from the cloned base index, while rename lineage is kept as a tombstone.

After successful publication, retention keeps the union of the active scope and a rolling window of the two latest successful publications (normally including active), the latest successful incremental predecessor, the clean base of any active worktree overlay, plus every unfinished task target/base and repository-set member pin. One older scope is atomically marked `retiring` and excluded from reads. Each maintenance transaction then advances one durable scope-GC phase, whose physical deletion of code graph facts, FTS/search documents, software projections, checkpoints, workspace state, or scope metadata is capped at 512 rows in aggregate across affected application tables. Same-tree commits share content under a bounded 256-row commit-alias window. Finished task history is bounded to 128 successful and 64 failed/dead-letter/cancelled rows per repository, preserving the latest success for each retained scope. `repo status` reports pending GC phase/progress/errors. A pruned historical ref requires full `repo index` before query or incremental reuse.

The runtime also caps indexed repositories outside user-managed repository sets at 10 by default. `RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES` changes this positive limit. The oldest eligible repository is cleaned through the same durable phased GC while its registration, aliases, unfinished tasks, and publications made after cleanup scheduling remain available. Automatic-workspace sets do not exempt repositories from the cap.

## 5.6 Worktree Overlay

Use `--ref worktree` to index uncommitted Git work:

```bash
relay-knowledge repo index repo --ref worktree --format json
relay-knowledge repo query repo --query retry_policy --ref worktree --format json
```

The overlay is bound to the current checked-out `HEAD`, uses a synthetic snapshot identifier, and includes modified files, untracked files, staged submodule gitlink updates, and unstaged submodule worktree commits when the submodule `HEAD` differs from the parent gitlink. If a submodule has both a staged gitlink and a different checked-out submodule `HEAD`, the overlay indexes the checked-out worktree commit. Staged submodule commits remain readable from cached gitdirs after deinit. Staged submodule additions, removals, renames, and file/submodule replacements clean up the old indexed paths by expanded child path rather than by the gitlink path alone. While an overlay is active, clean commit ref queries are rejected so uncommitted content is not mislabeled as a clean Git snapshot.

At query admission, the user-facing `worktree` selector is pinned to the active immutable `worktree:<base>:<overlay-hash>` identity. Multi-step operations such as `repo context` reuse that resolved identity for every internal graph query; they do not pass the synthetic identity back to Git as though it were a branch or commit. Editing the live worktree after publication therefore does not silently change an in-flight context pack; run `repo index ... --ref worktree` again to publish a new overlay.

For the CLI's default workspace-detection-disabled path, indexing first uses the direct overlay transaction only when its complete clone/delete/insert surface fits the task's frozen writer budget. If it does not fit, the same durable task stages the overlay bytes, clones the immutable clean base in bounded pages, and applies dirty files in deterministic bounded batches. A worker commits no more than one dirty batch per step, so an expired lease can be reclaimed without replaying committed batches. The final handoff also budgets owner cleanup, tombstones, checkpoint control rows, and the multi-batch receipt together. The operation retains its worktree identity and scope throughout; it is not reported as a clean full index and cannot return success before finalization and publication complete. API/Web worktree requests with auto-workspace detection currently fail closed when durable staging is required; they do not drop workspace metadata or convert the overlay to a clean snapshot.

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
relay-knowledge repo list --format json
relay-knowledge repo report repo --format json
relay-knowledge repo status repo --format json
```

`repo list` returns only repositories with at least one completed indexed scope. Each item includes the current alias, root, indexed commit, state, stale flag, and file/symbol/reference/chunk counts. A repository that has only been registered and has not completed its first index is omitted.

Reports include repository id, root, indexed commit, tree hash, file/symbol/reference/chunk totals, scope, representative queries, latency samples, and degradation summary. Markdown reports fit PRs or release notes; JSON reports fit CI comparisons of index quality.

`repo status --format json` also includes `active_task` for queued/running/retrying cold indexes, `checkpoint` counters for the active or latest scope, and a `retention` summary. If a repository is still marked `indexing` but no task is active, status falls back to the repository's latest checkpoint. After publication, retention keeps the union of active and the latest-two-success window (normally overlapping), plus the latest incremental predecessor, active-worktree clean base, unfinished-task scopes, and repository-set pins; older scopes are pruned.

`repo report --format markdown` also summarizes edge resolution counts for resolved, ambiguous, and unresolved edges. Use this to tell whether the graph is mostly deterministic AST extraction or still has many ambiguous edges requiring parser improvements.

## 5.9 Troubleshooting

Version freshness and content integrity are separate. A query can be `fresh`
with `content_integrity.state=partial`; `unknown` does not establish complete
content. Legacy `degraded_reason` alone is not a reason to reindex a fresh scope.
Read-model or query-capability failures can still make a query degraded.
Reports summarize at most 20 file diagnostics and expose truncation plus a
pinned diagnostics command. Follow that command for the report's snapshot:

```bash
relay-knowledge repo diagnostics repo --ref <pinned-ref> --limit 50 --format json
relay-knowledge repo diagnostics repo --ref <pinned-ref> --limit 50 --cursor <next-cursor> --format json
```

Use the returned `next_cursor` until null, preserving ref and path filters.
The cursor pins the served scope even if HEAD moves; removed snapshots fail
explicitly. The page maximum is 200. Multiple diagnostics can describe one file;
the integrity count is a distinct file count. Fix or explicitly scope out the
relevant source before reindexing to change content completeness.

When `repo query` returns no results, check in order:

1. Whether `repo status <alias>` shows an indexed clean commit or worktree overlay.
2. Whether query `--ref` matches the indexed snapshot.
3. Whether requested `--path` and `--language` only narrow the registered scope.
4. Whether `--kind` is too narrow; start with `--kind hybrid` when unsure.
5. Whether `degraded_reason` reports a source fallback candidate-path or budget issue; structured hits remain usable while exact-text fallback is degraded.
6. Whether files were diagnosed as unsupported, binary, oversized, invalid UTF-8, or parser failed.

`repo impact` requires an indexed snapshot for `--head`. Run `repo index repo --ref <head>` or `repo update repo --base <base> --head <head>` before impact analysis.

### Configuration keys, reads and guarded code

`repo feature-flags` combines Java system-property and environment reads, constant keys and zero-argument configuration getters with properties, INI, Consul-template (`.ctmpl`) and exported shell variables. Configuration symbols are resolved inside the served repository snapshot. This capability is independent of canonical callers/callees queries and does not alter Python or C++ parsing. Runtime production switch values are outside this static registry.

A `defines_config` usage supplies a file definition. `declares_config_key` identifies a Java constant or a template output key. `reads_config` identifies a concrete read. A `guards_code` usage contains `metadata.read_usage_id`, linking its condition to the read that supplied it. Java local bindings stop propagating after a write; deferred class/method/lambda bodies do not overwrite the enclosing binding. Field, parameter and local getter receivers use lexical type evidence. Anonymous receivers and unknown dynamic values are not assumed to use a default implementation. Conflicting configuration-symbol targets remain unresolved.

Each usage carries source format and optional default value, value type, owning domain and hot-reload support. Unknown values stay absent. An adjacent comment such as `# @config domain=business hot-reload=true` supplies explicit ownership metadata. Properties continuation/Unicode escapes and INI sections preserve their key/value identity; section keys use `section.key`. Environment variables use a separate namespace from system properties.

```powershell
relay-knowledge repo feature-flags demo --query feature_x --domain business --source properties --hot-reload true --format json
relay-knowledge repo feature-flags demo --query feature_y --consistency --format json
```

Source filtering selects matching groups and retains their linked Java usages. Consistency compares observed formats in the served snapshot within the registered authorized scope and reports `read_without_definition`, `missing_from_format` and `conflicting_defaults`. A missing key is a diagnostic, not an assertion about production configuration. Stale or unresolved analysis cannot prove absence. Query limits apply to returned groups; completeness budgets remain separate. Registering with `--path src` is only a scope example, not a required repository layout.

Remote CLI and the Web repository endpoint accept the same domain request, with a `filters` object containing `domain`, `source`, `hot_reload` and `consistency`. MCP exposes those four fields directly in `relay_code_feature_flags` arguments.

Java getter flow follows proven java.lang Boolean/Integer/Long/Double parsing and boxing conversions. Unsupported getter result flow sets `metadata.flow_incomplete` and prevents complete consistency claims. String constants become public configuration declarations only when a visible configuration read references them, they have explicit `@config domain=...` / `hot-reload=...` metadata, or follow the declaration convention of a containing type ending in `Keys` or a field ending in `_KEY`. Other strings remain internal symbol candidates, excluded from registry results and general configuration views.

Consistency format coverage comes from the scoped indexed file inventory, including empty/comment-only templates, and respects registration path/language restrictions. Query-time path/language filters project returned `usages`; connected binding and consistency evidence remain within the registered authorized snapshot. Thus `conflicting_default_sources` can identify an authorized definition outside the displayed query path. `conflicting_default_sources` contains the usage records for conflicting defaults: `metadata.default_value`, `path`, `line_range`, `excerpt`, and `usage_id` directly identify each source. The compact `conflicting_defaults` diagnostic remains available.

Java SDK feature flags continue through the existing SDK extractor alongside configuration reads. Static platform imports ignore unrelated sibling/nested classes and inapplicable method overloads. Shell assignments exported by a later unconditional command retain their source defaults; properties escape decoding does not alter INI/template backslashes. Expanded and consistency usages retain containing-symbol evidence. The result limit applies after symbolic keys are resolved and ranked: candidates are bounded by the 10,000-usage budget, with an explicit incomplete-analysis error on overflow or SQLite time/step interruption. A served stale snapshot cannot emit definitive consistency diagnostics, even when its stored status originally recorded a completed fresh index.

Consistency queries apply query terms before the fact budget and expand only connected bindings; the file-format inventory remains bounded by the registered scope independently of query projection. Referenced constant bindings are collected once instead of rescanning all records per declaration. Java receiver names erase generic arguments, simple assignments to existing local variables propagate to subsequent conditions until reassignment, and explicit static imports take precedence over wildcard imports. Shell `set -a` / `set -o allexport` applies to subsequent assignments; disabling allexport does not remove an existing variable's export attribute. General codebase and software views exclude raw symbolic getter rows; resolved configuration usage remains available through `feature-flags`.

### Configuration registry acceptance matrix

| Contract | Required result | Verification |
| --- | --- | --- |
| Java reads, constants and getters (#389/#394) | Real key, located read, linked guard; respect imports, overloads, visibility and nonvirtual dispatch | Java receiver matrix and snapshot binding regressions |
| Properties, INI, ctmpl, Shell and dotenv (#394) | Format-correct definitions, defaults and locations; preserve quoted text and continuations | Format and execution matrices |
| Metadata and filters (#394) | Default/type/domain/source/hot-reload; CLI, Web and MCP use the same request | Domain, interface and real index-service acceptance tests |
| Consistency (#394) | Located conflicting defaults and missing-format/read diagnostics from authorized evidence | Scoped, stale, ambiguous and incremental snapshot tests |
| Unknown or conditional behavior | Retain evidence and uncertainty; never infer runtime state or definite absence from incomplete analysis | Conditional export/template and unresolved-binding regressions |
| Resource bounds | Explicit errors before exceeding file facts, metadata, expansion or query budgets | Boundary and overflow tests |

This is bounded static analysis, not execution of arbitrary Java, Shell or templates. The expected behaviors above must not be removed merely to obtain a clean review. Findings are assessed against this contract and reproducible behavior; an inaccurate premise does not by itself invalidate a demonstrated bug.

The Java platform-reader inventory for this change is System.getProperty/getenv, direct System.getenv().get/getOrDefault and System.getProperties().getProperty, Boolean.getBoolean, Integer.getInteger and Long.getLong, with supported literal/constant keys and proven getter forwarding/conversions. Equivalent qualified/static-import forms share the same rules. Additional arbitrary APIs are not implicitly promised by the registry label; demonstrated misbinding, lost evidence or wrong defaults within this inventory remain defects.


Enclosing getter fallback requires indexed inheritance evidence; deferred template definitions do not publish root defaults. Quoted Shell options obey quote removal, and static interface methods are not inherited. Hexadecimal Double fallback strings remain explicitly unsupported: raw evidence is retained, the default is unknown and consistency is incomplete; this change does not evaluate arbitrary Java numeric syntax. Fact version: `config-registry-v49`.

Software ontology configuration projection excludes internal constants, type/getter markers and unresolved symbolic rows while retaining their indexed evidence. Shell set options use the same static quote removal as export options, including enable/disable and the option terminator. Fact version: `config-registry-v50`.

Explicit unevaluated Java fallbacks make consistency incomplete; known String constant expressions participate in overload applicability. Inline non-output template control actions preserve static/conditional text. Edge-kind query terms remain searchable after grouping. Metadata-only queries without path/language projections seed matching groups before bounded symbol expansion. Collection containsKey presence APIs and executing named template bodies are outside the finite extraction inventory; missing-definition diagnostics describe observed static evidence, not runtime rendering or values. Fact version: `config-registry-v51`.

Statically decoded Shell builtin names use the same export classification for quoted, concatenated and escaped forms; generic assignment operands retain defaults. A for/select loop variable shadows inherited environment values in its body, while loop input expansions remain reads and possible later overrides remain uncertain. Java Boolean conversion of an explicit null property fallback yields false; other unevaluated fallbacks remain incomplete. Catch parameters bind receivers within their body and shadow outer fields; multi-catch receivers remain unresolved when no unique static type is proven. Fact version: `config-registry-v52`.

Free-text configuration queries match persisted metadata as well as keys and located usages; final group matching and row scoring preserve the SQL metadata search contract. An explicitly supplied query containing no alphanumeric character or underscore is rejected before loading rows; omit the query for an unfiltered registry. Java try-with-resources declarations bind receivers in the try body and later resource initializers, not catch/finally blocks. Shell assignments preceding a recognized export builtin define configuration when that builtin exports the same variable; unrelated ordinary-command prefixes do not define the parent environment. Fact version: `config-registry-v53`.

Proven getter conversions canonicalize explicit environment fallbacks as well as property fallbacks; property-specific nullable handling remains separate. Known platform wildcard imports contribute only supported members they actually expose, and final var keys require a proven String initializer. Named local Java types have lexical identities so unrelated methods or blocks cannot share getter providers. Asynchronous Shell commands cannot define or mutate the parent configuration/export state. Nameref alias tracking and command/builtin dispatch wrappers are outside the finite Shell extraction inventory; recognizing direct builtin names and quoted equivalents does not execute wrappers or indirect variable writes. Absence diagnostics describe observed static evidence within this inventory. Fact version: `config-registry-v54`.

## 5.10 Language capability matrix

Type queries use ownership facts stored during indexing. They aggregate the type and its direct callable members; inherited methods, unknown receivers and nested functions are excluded. The short-name entry point remains unchanged. A call-site path filter constrains the actual call location. Type selection admits at most 64 types and 1,024 type/member records, followed by at most 200 call candidates and the existing SQLite work budget. Exhaustion is an explicit incomplete-query error.

Configuration analysis reuses the indexing syntax tree. The following is the finite reader inventory, together with the language structures that supply ownership. `config`/`settings` readers recognized by the existing configuration API inventory remain available. Literal keys, proven constant expressions, zero-argument read getters and local condition uses contribute evidence; runtime-dependent expressions retain unknown values.

| Source | Type ownership | Environment/property reader inventory |
| --- | --- | --- |
| Java | Classes, constructors and direct methods | Existing `System` environment/property APIs and documented configuration readers |
| Python | Classes and direct methods | `os.getenv`, `os.environ.get`, `os.environ[key]` |
| JavaScript / JSX | Classes and direct methods | `process.env`, `Deno.env.get`, `Bun.env`, `import.meta.env` |
| TypeScript / TSX | Classes, interfaces and direct methods | Same JS APIs with TypeScript syntax |
| C | Not applicable; ordinary function queries remain available | `getenv` |
| C++ | Classes and scoped member implementations | `getenv`, `std::getenv` |
| C# | Classes/structs and direct methods | `Environment.GetEnvironmentVariable`, `System.Environment.GetEnvironmentVariable` |
| Rust | Types and inherent/trait `impl` methods | `std::env::var`, `std::env::var_os`, `env::var`, `env::var_os` |
| Go | Named types and receiver methods | `os.Getenv`, `os.LookupEnv` |
| Kotlin | Classes and objects | `System.getenv`, `System.getProperty` |
| Scala | Classes, traits and objects | `System.getenv`, `System.getProperty`, `sys.env.get`, `sys.env.getOrElse` |
| Ruby | Classes/modules and direct methods | `ENV[key]`, `ENV.fetch` |
| PHP | Classes and direct methods | `getenv`, `$_ENV[key]`, `$_SERVER[key]` |
| Swift | Types and extensions | `ProcessInfo.processInfo.environment[key]` |
| Bash (`--source shell`) | Not applicable | Parameter expansion, existing export/default evidence, direct output getters |
| Starlark | Not applicable | `ctx.getenv(key, default)` and documented configuration readers; unknown receiver provenance remains incomplete; `load` provides explicit bindings |
| Vue | Embedded JS/TS ownership | The embedded script uses the matching JS/TS reader rules |
| SQL, build scripts and templates | Not applicable where the grammar has no callable type | Existing structured definition, reference and condition evidence; no synthetic class relationships |

Cross-file bindings are confined to the authorized repository snapshot and require explicit import, type or native module evidence. Environment variables and property keys have independent namespaces. A common spelling alone never joins symbols from different code languages. Dynamic keys, external providers, reassignment, shadowing, unsupported wrappers and exhausted resolution depth must be treated as unresolved or incomplete evidence. `analysis_complete=false` prevents absence conclusions. Defaults describe observed static evidence; they do not predict runtime values. For example, converting the string `"false"` with Python `bool` or JavaScript `Boolean` yields `true`, while C# `bool.Parse` yields `false`.

Code fact version `config-registry-v56-portable-evidence` requires rebuilding older indexes through the durable repository indexing task. The deferred query-index plan is version 5; it preserves the v4 ownership and language/file indexes and appends configuration identity/source-key and caller/callee identity indexes (ordinals 19–22). Existing v1–v4 checkpoints retain their prefix checks during recovery. CLI, HTTP and MCP continue to use the same service contract and configuration budgets.

### Cross-file evidence and limits

| Language group | Required evidence and retained limits |
| --- | --- |
| Python | Explicit module import or relative `from` import; lexical rebinding terminates the connection. |
| JS/JSX and TS/TSX; Vue scripts | Explicit relative import with the source extension and a matching named/default export. Extensionless resolution, package loaders and re-export chains remain unresolved. |
| C/C++ | Quoted repository-relative includes. C++ ownership preserves qualified scope; headers retain their detected grammar (`.h` defaults to C and uses C++ when declaration evidence proves that dialect; `.hpp` is C++). |
| Rust | Indexed `mod` declarations prove module membership, including static `#[path]` redirection. Imports retain original names through aliases. Missing/conditional modules and macro-controlled membership remain unresolved. |
| Go | The declared package and directory bind receiver methods and package configuration providers. |
| Kotlin, Scala and C# | Exact native package/namespace/type identities; private providers cannot satisfy cross-file imports. Companion objects remain separate direct owners. |
| Swift | Configuration providers share the indexed directory module; detached cross-file type extensions require an explicit typed import under the existing unique module-directory contract. Unknown targets remain unresolved. |
| Ruby / PHP | Ruby `require_relative`; PHP `require`/`include` anchored at `__DIR__`. Namespace/name equality alone does not prove a PHP file was loaded. |
| Bash / Starlark | Shell script-directory `source` using `dirname` of `BASH_SOURCE[0]`, or Starlark `load`. Plain relative Shell sources retain unknown working-directory evidence; `source`, `eval` and `unset` can invalidate an earlier function binding. |

The analyzer records non-configuration getter declarations as internal blockers so an unrelated same-name getter cannot inherit another provider's configuration result. Native parameter syntax, local declarations and imports constrain scope. Explicit fallbacks and supported conversions carry their evidence; unevaluated defaults remain unknown. SQL/build/template rows retain the existing grammar's definition/reference coverage and do not imply general program-flow evaluation.

Rust `crate` imports require a conventional Cargo root and indexed `mod`
reachability from that root to the importing file. `src/bin`, `tests` and
`examples` roots remain separate from `src/lib.rs`; shared roots, inline modules,
conditional module declarations and custom manifest roots remain unresolved when
membership cannot be proved. Swift typed imports require one physical module
directory across all admitted Swift files; the language/file index excludes other
languages from that check, with a 1,024-file evidence ceiling.

Getter provider reassignment revokes its exported proof while preserving the
original body read. C# explicit aliases constrain the provider identity; unsupported
native import aliases stay unresolved. Non-nullable Boolean/numeric conversions
do not activate nullish fallbacks. A fallback around an unresolved imported getter
retains an incomplete state until its return semantics can be proved.

Java zero-argument configuration getters may use any method name; the existing
visibility, inheritance and shadowing checks still apply. Local method/property
expansion shares the provider stability checks, including known member rewrites.
C# file-scoped namespaces and relative namespace aliases retain their declaration
scope; `global::` explicitly selects the global namespace. Unsupported Scala
selector imports block an unproved package fallback.

C++ primary class-template parameters use declaration slots, including the outer
parameter layer of a member template, while concrete specializations keep distinct
identities. Complex template arguments remain bounded by the extractor's grammar
and static-identity limits. Swift constructor, protocol requirement and subscript
definitions retain their own member ranges. Dockerfile coverage here is the
existing stage/import graph; it does not evaluate arbitrary `RUN` commands.

Detached C++ implementations with named concrete template arguments require
type-binding evidence; a bare argument such as `V` remains unresolved because
different namespaces can define different `V` types. Parameter slots, built-in
types and proven literal arguments retain their bounded identity support.

The persisted type-call regression matrix also exercises both JavaScript and
TypeScript Vue scripts, including `vue` language and call-site path filters.


Type-call aggregation preserves actual call byte and line ranges; ordinary function queries retain their existing context line ranges. Anonymous callbacks keep their own call owner; a matching member name on an unknown receiver never proves a target. JS static and instance `this` are separate; Java permits instance-qualified static calls. Class headers and computed member names do not establish the new class's `this` binding.

Ruby bracket reads and Rust `env::var_os` contribute environment evidence; pure writes and Python/JS method selectors do not become keys. Rust import provenance follows the nearest lexical scope and distinguishes local `std` modules from `::std`. Go package constants can compose keys across files using at most 32 string components, four snapshot resolution levels and 4,096 result bytes. Mutable values, non-string providers, getters used as constants and cycles remain unresolved; different unknown expressions retain separate identities.

Extensionless scripts require a supported interpreter shebang within the first 256 bytes; watcher admission preserves their update/delete events. A leading Flow pragma selects the existing typed JSX grammar while preserving JS/JSX identity and ordinary partial diagnostics for unsupported syntax. This is limited syntax recovery, not complete Flow type analysis. Vue counts only top-level SFC regions against the 16-region budget and reuses its parsed HTML tree.

Language selection must satisfy both repository registration and the current request. Shared manifests can satisfy more than one language rule. Different effective selections have different scope identities and cannot reuse each other's checkpoint. A shebang candidate excluded after content inspection remains visible as `excluded` progress, contributes no code/configuration evidence, and does not make the repository degraded. Source-fallback candidate exhaustion is reported as incomplete rather than an authoritative empty result.
