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

For `callers` and `callees`, pass a definition result’s complete `canonical_symbol_id` (`repo://...`) or `symbol_snapshot_id` (`symbol:...`) as `--query`. Canonical selectors are case-sensitive and retain repository/module identity. If a canonical ID has multiple definitions in the served scope, such as Java/C++ overloads, the query returns an ambiguity error instead of combining their call chains. Select the desired structured definition (its `retrieval_layers` includes `symbol`), copy its `symbol_snapshot_id`, and reuse the same indexed ref. Snapshot selectors never cross source scopes. An unknown exact selector returns no hits and never falls back to same-name methods or text search. Class IDs do not aggregate all methods. Inline `path:` and `name:` filters apply before bounded call candidate selection. Upgrading to the `canonical-call-selectors-v1` read model makes earlier scopes stale; run `repo index <alias> --ref <ref>` to rebuild through the durable indexing workflow. Exact selectors reject missing indexes until that work completes. Copy snapshot IDs again from the rebuilt scope; canonical IDs are not rewritten. `repo index --reset` resets unfinished task state and does not force a completed scope to rebuild.

Canonical call selectors prefer callable definitions over C/C++ prototypes and signature-only declarations according to the indexed call-target policy. If no definition exists, a unique callable declaration remains queryable; multiple declarations are explicitly ambiguous. They inspect at most 1024 symbols sharing that canonical ID; exceeding this budget returns an explicit error directing you to a definition `symbol_snapshot_id`, rather than reporting an empty or falsely unique result.

Direct call queries scan non-generated and generated candidates separately in path/line order, retaining non-generated priority without sorting every matching edge before the candidate limit. Selector resolution, both scans, and excerpt hydration share a roughly 4.1-million SQLite instruction budget. Exhaustion reports `call query incomplete` with `error_kind=timeout` (HTTP 408 through the repository API) and does not fall back to empty or partial success; narrow path/language filters or select a more specific snapshot. The bounded query algorithm uses the durable directional indexes. Upgrading to this version still requires the ordinary durable reindex described above to create required indexes and refresh versioned facts; do not skip that upgrade step.

The `cpp-callable-declarations-v1` extraction version also requires ordinary `repo index <alias> --ref <ref>` to rebuild older scopes, including scopes that already have canonical-call indexes. It preserves C++ prototype declarations separately from executable definitions; snapshot IDs must be copied again after rebuilding.

```sh
relay-knowledge repo query repo --query dispatch --kind definition --ref HEAD --format json
# Copy symbol_snapshot_id from the desired overload definition into the next query.
relay-knowledge repo query repo --query "<symbol_snapshot_id>" --kind callees --ref HEAD --format json
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

Responses are grouped by feature flag and include configuration source, `defines_config`, `reads_config`, or `guards_code` relationships, source ranges, confidence, related symbols, and excerpts. The indexer recognizes static code/config evidence from environment access, config/settings reads, boolean config facts from supported configuration formats, and common OpenFeature, LaunchDarkly, and Unleash evaluation calls. Provider control-plane state such as rollout strategies, segments, and variants is not synchronized in this path. The query reads only persisted feature-flag and symbol facts for the selected indexed scope; it does not recursively grep the repository at query time. Re-run `repo index` or `repo update` after adding flags or changing extraction rules.

Metadata filters and consistency analysis use the same CLI, Web, and MCP request contract:

```bash
relay-knowledge repo feature-flags repo --domain task --source properties --hot-reload true --format json
relay-knowledge repo feature-flags repo --query feature_y --consistency --format json
```

`--source` matches the persisted source format, while `--domain` and `--hot-reload true|false` match explicit metadata. Unknown values never match a boolean filter. A matching flag retains all its selected-scope usages, so definitions and guarded reads remain visible together. Each usage carries `metadata` (default value, value type, source format, explicitly declared domain/hot-reload policy, symbol bindings and originating read-site id) and `resolution_state`; conflicting defaults remain separate source evidence, never an invented runtime value. Java constant/getter bindings resolve through at most two exact symbol hops inside the selected persisted scope. Ambiguous or unresolved bindings remain explicit; queries never consult live source or another historical snapshot.

Constant bindings preserve the read API namespace: `System.getenv(Keys.KEY)` remains `env_var`, while `System.getProperty(Keys.KEY)` remains `config_key`. If both APIs read the same string, they produce separate groups and namespace-specific identifiers; getter aliases carry the originating namespace through resolution. Metadata stores the typed `read_source_kind` when the API proves it. Shared identifier encoding lives in the dependency-free identity owner and remains byte-compatible with existing persisted IDs.

`--consistency` adds per-flag `consistency_diagnostics`: reads without definitions, keys missing from a source format observed in the selected indexed facts, and conflicting declared defaults. A Java key declaration counts as a declaration, not a runtime default. These are scope-relative comparisons, not typo inference or a claim that every format must contain every key. Absence becomes `unknown` and `analysis_complete` is false when the scope is stale/degraded or the key cannot be resolved. Ordinary queries select ranked groups in SQL before loading usage records, then follow a snapshot-local binding closure (at most four load rounds and 1,000 identities); a unique-key query remains available even with more than 10,000 unrelated usages. Consistency analysis examines all selected-scope facts: template reads count as format presence without becoming definitions. Each analysis or candidate closure admits at most 10,000 usages and 16 MiB of persisted fact text (at most 64 KiB metadata per usage); exceeding either scope budget returns an explicit incomplete-analysis error instead of false missing-key claims. Narrow `--path` or `--language` to choose a smaller comparison scope. `--limit` limits returned groups after this analysis. Changed/deleted definitions and metadata follow the ordinary durable incremental snapshot copy/deletion workflow, and historical refs retain their own facts.

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

When `repo query` returns no results, check in order:

1. Whether `repo status <alias>` shows an indexed clean commit or worktree overlay.
2. Whether query `--ref` matches the indexed snapshot.
3. Whether requested `--path` and `--language` only narrow the registered scope.
4. Whether `--kind` is too narrow; start with `--kind hybrid` when unsure.
5. Whether `degraded_reason` reports a source fallback candidate-path or budget issue; structured hits remain usable while exact-text fallback is degraded.
6. Whether files were diagnosed as unsupported, binary, oversized, invalid UTF-8, or parser failed.

`repo impact` requires an indexed snapshot for `--head`. Run `repo index repo --ref <head>` or `repo update repo --base <base> --head <head>` before impact analysis.

Configuration extraction follows detected INI languages, including `.conf`, `.cfg`, and case-insensitive suffixes. An `@config` annotation applies only to the immediately following definition; blank lines, other comments, and section headers break adjacency. Java bindings preserve all enclosing type names and erase structured generic type arguments consistently. Guard flow tracks unqualified locals: copying a value does not overwrite it, and unrelated member names do not create dependencies. Visible class/interface/enum/record shadows suppress platform-API inference. Bash parameter operators retain the parameter read; only static operands of `-`, `:-`, `=`, and `:=` supply defaults, while alternatives, errors, lengths, removals, replacements, and transformations do not.

Consistency compares observed formats only within the same `source_kind` namespace. Query terms may be distributed across a resolved group's usage records; combined metadata predicates must match the same usage. Ordinary SQL seeds remain a bounded superset, expanding geometrically up to 1,000 raw identities when alias collapse leaves fewer than the requested result groups. The final `--limit` applies after alias resolution; remaining candidates at an exhausted budget produce an explicit incomplete-analysis error. The existing 10,000-usage and 16 MiB analysis limits still apply.

Java getter bindings require a return in the getter's own callable scope; reads inside returned expression/block lambdas remain read evidence but do not bind the getter returning that callback. Interface constant declarations use Java's implicit static/final semantics and complete enclosing type identities. Local flag dependencies include ternary conditions nested in return statements and variable initializers, while writes, nested blocks, and callable boundaries stop that local analysis. A configuration getter proven to have implementations for different keys retains ambiguous read/guard references; consistency is unknown with `analysis_complete=false` and no concrete destination is guessed.

Python overload declarations proven through visible `typing` or `typing_extensions` imports (including aliases) are distinguished from the runtime implementation. Custom or shadowed decorators are not assumed to be typing declarations. Decorators directly in a class body may use that class namespace; nested functions or classes skip enclosing class namespaces and continue through function/module bindings. Global/nonlocal directives resolve the declared module/enclosing function namespace. A possible later write in a namespace accessed across a delayed function boundary prevents assuming the earlier typing import still applies; attribute/subscript targets do not rebind an ordinary decorator name. Unrelated module members remain independent; actual overload-member writes and unknown namespace keys invalidate the import proof. A bounded try/except import merge requires every completing branch to prove a typing binding, while mixed or unknown writes remain conservative. Delayed lookup accepts later proven imports in linear binding order before a call, return, or unknown control path; subsequent custom writes invalidate that proof. Direct setattr/delattr calls invalidate the selected overload member; unrelated members remain independent. Repeated import aliases use the last binding in statement order. The `python-overload-declarations-v11` fact component invalidates old completed scopes; run ordinary `repo index <alias> --ref <ref>` and copy snapshot selectors again after rebuilding.

Configuration getter bindings require zero parameters, matching supported zero-argument getter calls. Platform receiver checks include interface fields, and direct reads in ordinary for-loop conditions produce guard relationships. Shell extraction excludes lexically visible unexported assignments and function-local bindings; explicit exports and unshadowed external environment reads remain evidence. The lexical binding scan is bounded to 1,024 ancestor/preceding nodes and does not guess environment origin when that budget is exhausted. Template `keyOrDefault` reads preserve a literal fallback (including spaces) and its inferred scalar type; dynamic fallback expressions remain unknown. Configuration query SQLite work has a shared execution budget across candidate sorting, alias expansion, and symbol enrichment; exhaustion returns a timeout/incomplete result with guidance to narrow query terms or path/language filters.

For exact call selectors, `name:` filters the returned call identity before candidate admission. Callees use the resolved canonical identity, or the persisted target hint/name when no local target is resolved; unresolved edges retain their resolution state and metadata. Callers filter the caller identity, so a callee name cannot admit an unrelated caller. The existing shared execution and result budgets still apply.

Zero-argument getter bindings use the nearest declared Java type, including records, enums, and default interface methods. Local flag propagation retains conditions completed before the first lexical write, then stops at the writing statement; conditions after a body write do not inherit the earlier read. Bash configuration definitions and lexical bindings share parsed declaration options, covering whitespace-independent `export` and `declare`/`typeset`/`local -x` while excluding explicit unexports. Constant declarations are promoted only for the constant symbols traversed by a proven read binding; equal string values alone are not declaration evidence.

Java configuration reads also recognize explicit and wildcard static imports of `java.lang.System.getenv`, `java.lang.System.getProperty`, and `java.lang.Boolean.getBoolean`. Explicit imports take precedence over wildcards; visible methods with the same name, competing imports, or unproven inherited method sets prevent platform inference. Variable names use Java's separate value namespace and do not shadow an imported method. Import and method-scope proof has a 1024-node inspection budget; exhausted proof produces no guessed configuration read. The same structured extraction retains namespaces, literal defaults, and guard links without restoring lexical Java environment fallback.

Python overload proof inspects every target of chained assignment within one shared target budget, excluding ordinary right-hand and standalone boolean reads. Transparent parentheses (including comments) around a decorator or its module receiver retain the same binding identity; custom or rebound decorators remain runtime definitions.

Static-import inheritance proof does not assume that an unresolved `Object` superclass belongs to `java.lang`: a fully qualified name or explicit platform import is required when package-wide type evidence is unavailable. Private parent methods and static interface methods do not shadow imported calls, while locally declared methods and genuinely inherited methods retain their normal shadowing behavior. Java fallback strings follow [Java lexical translation](https://docs.oracle.com/javase/specs/jls/se21/html/jls-3.html#jls-3.3): eligible Unicode escapes are translated before standard/octal string escapes. Literal backslashes remain distinct from Unicode escapes. Text blocks, malformed strings, isolated UTF-16 surrogates, and sources exceeding 64 KiB keep an unknown default rather than persisting an inaccurate value.

Java configuration key literals and referenced string constants use the same bounded runtime string decoding: Unicode or octal escapes resolve to the actual key in configuration files, while escaped backslashes remain literal and are not decoded as Unicode a second time. An unreferenced constant with the same string value does not prove a configuration declaration.

`.ctmpl` `key`, `keyOrDefault`, and `env` reads are recognized from complete template actions, including function and literal arguments split across lines, preserving original UTF-8 byte spans, start/end lines, and static defaults. The scan advances forward; template comments and `}}` inside quoted or raw strings do not terminate an action. Literal decoding is limited to 65,536 bytes per action; exceeding the bound reports an extraction error instead of truncated facts. String literals follow Go escape rules and remove carriage returns from raw strings; byte escapes that produce invalid UTF-8 leave the default unknown without lossy replacement.

Expression write proofs include nested assignment expressions in immediately inspected expressions and exclude deferred lambda bodies. Decorator and mutation receivers share bounded transparent-parenthesis normalization. Comment-only statements do not create a delayed-execution barrier; calls, returns and unknown control flow still do.

The expression budget follows evaluation timing: lambda defaults and a generator’s outer iterable are eager, while lambda and unconsumed generator bodies are deferred. Annotation-only module/class statements do not replace values; function-local annotation bindings remain local. A proven unchanged zero-argument direct invocation fixes that invocation’s decorator binding before later namespace writes. Other calls, changed callable identities, and argument evaluation remain conservative boundaries.

The `config-registry-v4` extraction component invalidates completed scopes created with earlier configuration semantics, independently of Python or query-index versions. Ordinary `repo index <alias> --ref <ref>` on the same commit refreshes these facts through the durable leased pipeline; `--reset` is not required. Java receiver/constant proof, template action boundaries and static shell defaults are refreshed together. This is a fact refresh, not a new SQL schema migration. Preserve the runtime database, WAL and task checkpoints during upgrade; exact rollback restores the pre-upgrade database/shards with the matching binary.

Shell parameter fallback metadata decodes supported static quoting and concatenated quoted/unquoted segments before type inference. Empty quoted operands remain empty. Single quotes inside an enclosing double-quoted expansion retain their literal characters, following Bash evaluation. Dynamic substitutions, escapes outside the supported static subset, unmatched quotes, or operands above 64 KiB keep an unknown default; a 1,024-node ancestor budget bounds quote-context inspection. Original read ranges and environment-binding checks remain intact. Equivalent quoted/unquoted defaults no longer create false consistency conflicts.

`.ctmpl` configuration declarations come only from static `KEY=value` text outside template actions. Text inside actions, template comments, or strings does not declare a key, even when a raw string would render that text. Equal-width masking preserves original byte and line evidence; assignment lines containing actions do not imply a static default. Read detection covers pipeline commands in control actions such as `if`, `with`, and `range`, as well as variable assignments, nested parentheses, and pipeline calls to `key`, `keyOrDefault`, and `env`. A directly preceding literal command may supply the last argument, as in `"flag" | key` or `"true" | keyOrDefault "flag"`; intervening functions or dynamic values keep that argument unknown. Strings, comments, fields, variable names, and dynamic key arguments do not prove static reads; repeated calls in one action retain separate usage identities. Scanning and tokenization remain bounded by the existing 65,536-byte per-action decoding limit, without changing parser diagnostics or freshness rules.

Explicit Java `java.lang.System` / `java.lang.Boolean` receivers also require the leading `java` name to be free of visible value or type shadows; an unqualified statically imported method is unaffected by an ordinary value with that name. Java text fallback, including oversized, invalid-UTF-8 and syntax-initialization-failure paths, does not retain lexical environment reads without AST proof, and original file diagnostics remain visible. Unqualified constants may resolve through proven parent types in the same syntax tree. Inheritance and type lookup share a 1,024-node budget and a visited set, preserving declaring-type identity, diamond deduplication and local/parameter shadows; private or non-static/final fields, conflicting interfaces, unknown parents and budget exhaustion do not guess a concrete key.

Python overload proofs distinguish eager definition defaults/decorators/class bases from deferred function bodies. Explicit `from __future__ import annotations` suppresses annotation evaluation; without that directive, module/class annotations retain the eager contract and function-local annotations do not execute. Function-local binding names cover the entire function body, control-flow targets use actual assigned names rather than attribute text, and a completing `finally` binding overrides preceding branches. An unproven prior eager call invalidates import proof; an explicit write to another member of the proven typing module remains distinct from replacing `overload`.

Implicit decorator calls and immediate class-body effects also participate in this proof. Class-local assignments and comprehension loop targets do not overwrite outer bindings; explicit class `global` writes and comprehension walrus writes remain visible. The safe unrelated-member mutator exception requires an unshadowed builtin name; user-defined `setattr`/`delattr` calls are unknown.

Configuration `--query` uses the same Unicode letter/number/underscore word boundaries for SQL candidate selection and final group matching, so Chinese path or excerpt terms are retained even with a small result limit or consistency analysis. ASCII case matching remains insensitive; non-ASCII characters retain their spelling and case. A supplied query containing no searchable terms returns `error_kind=invalid_argument` (HTTP 400) instead of listing unrelated flags or reporting a storage outage. This uses a dedicated query-validation error; existing backend conditions such as missing repository shards retain their storage-failure classification (HTTP 503), and query-budget exhaustion remains a timeout (HTTP 408). Omitting `--query` still lists the selected authorized scope. Existing alias grouping, metadata predicates, candidate limits and shared SQL budgets remain unchanged. This query-only correction does not itself require a schema migration or fact rebuild.

Java configuration reads check visible inherited fields in parent types proven in the same syntax tree, preventing inherited `java`, `System`, or `Boolean` receiver fields from being treated as platform APIs. Parent resolution retains complete nested owner paths such as `Outer.Base` and does not enter method-local types. Zero-argument instance configuration getter overrides also bind to parent types that actually declare the contract; private, static, and different-arity methods do not supply an override contract. Inheritance traversal shares a node budget and a visited set. Unknown external parents do not produce guessed member contracts, and exhausted budgets do not admit additional proofs.

Static Go template pipelines retain the result of literal-only parenthesized expressions, including nested parentheses; calls and dynamic arguments do not prove a static result. Exported shell assignments use the same bounded quote-concatenation decoder as parameter defaults before value-type inference, retaining unknown values for dynamic expansion and unsupported escapes. Recovery and configuration extraction share a literal-aware action scanner in config_files, so }} inside quoted/raw strings or comments does not end an action. Recovery requires complete actions, literals, and paired control blocks, with a 64 KiB action limit and a nesting limit of 128; incomplete quotes, comments, parentheses, and control blocks retain partial diagnostics. Valid non-UTF-8 Go byte literals can pass syntax validation without fabricating a UTF-8 configuration default.

Recovery conservatively retains parser diagnostics when variable declarations and scope cannot be proven. Confirmed invalid comma arguments, define arity, missing block pipelines, literal pipeline commands and unspaced trim markers cannot pass recovery. The native error-free AST path is unchanged; this recovery boundary is not a full Go type or execution validator.

Python overload-member write proofs follow bounded direct module aliases, including every assigned name in a chained assignment. Unrelated member writes retain their existing meaning. Class construction is an eager boundary unless its shape is proven safe: no explicit base or metaclass, known non-descriptor values, ordinary function definitions, module imports, or a from-import proven to be the standard overload function. Unproven metaclass, subclass and descriptor construction hooks prevent declaration inference. Decorator effects retain their node-specific proof; method bodies remain deferred.

Provider identity uses the authorized snapshot inventory. Repository-top-level `typing.py`, `typing/__init__.py`, `typing_extensions.py` and `typing_extensions/__init__.py` independently prevent treating the corresponding local import as the standard provider. Path/language filters that cannot cover the provider candidates, or an exhausted inventory budget, leave the origin unknown. The index does not inspect the host Python environment, unauthorized files, script-entry search paths or custom `sys.path` changes. This is a static repository-root import model. A local or unknown origin supplies no typing declaration proof, so queries may retain multiple executable candidates and require a specific snapshot selector. External dependency coverage and file degradation semantics are unchanged.

Provider changes preserve the original classification in retained historical scopes. The existing retention policy still applies: after three unpinned full publications, the earliest scope can retire outside the latest-two window. A retired ref returns an explicit missing-index error and needs a full reindex; it is not silently interpreted with current provider identity.

When provider changes force a worktree Python file to be parsed again, it is removed from the unchanged-skip count. Other unchanged files remain counted as skipped, and the changed-path count continues to describe Git status.

Java parent types may use nested paths relative to the nearest visible type scope, such as `Inner.Base` inside `Outer`. Resolution first binds the leading segment lexically and then follows member types, preserving complete owner identity. A nearer shadowing type, missing member, ambiguity, or exhausted budget does not fall back to another type with the same suffix.

Bounded manual Go-template recovery distinguishes identifiers, field chains and complete numeric literals. Malformed tokens such as `123abc` retain syntax diagnostics; the native parser path for an error-free tree is unchanged. An unquoted leading `~` leaves a Shell default unknown because its value depends on runtime expansion. Quoted tildes and supported static concatenations retain their literal values; indexing does not read the host home directory to guess a default.
