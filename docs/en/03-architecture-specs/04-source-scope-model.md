# Source Scope Model

[English](../../en/03-architecture-specs/04-source-scope-model.md) | [中文](../../zh/03-architecture-specs/04-source-scope-model.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Source scope is the foundation for authorization, versioning, and index partitioning. Without scope, retrieval cannot explain its knowledge boundary; without snapshots, code and document retrieval drift under rebases, path filters, and dirty worktrees.

## 2. Scope Types

| Scope | Use | Source of truth |
| --- | --- | --- |
| `document_scope` | Document directories, files, import batches | Normalized source path + content hash |
| `repository_snapshot` | Clean Git snapshot retrieval | Repository id + commit sha + tree hash + filters |
| `git_changeset` | Review, impact, diff evidence | Base/head commits + changed path set |
| `worktree_overlay` | Explicit dirty-worktree analysis | Clean snapshot + overlay identity |
| `runtime_scope` | Service, worker, diagnostic events | Runtime instance + component identity |

Every fact, index cursor, audit event, and context item carries scope or can be traced to scope.
Local file-location indexes use explicit document scopes such as `user-documents`
or `local-files:<name>`. Roots can point at home document folders, Linux paths
such as `/opt`, mounted volumes, or Windows drive roots such as `D:\`, but the
scanner may not widen one authorized root into a whole-disk scan.

## 3. Normalization Algorithm

Scope construction follows a fixed flow:

1. Parse user input and authorization boundaries.
2. Normalize paths, languages, source kinds, and aliases.
3. Resolve branch, tag, HEAD, and PR refs to commit/tree identities.
4. Reject refs that could be parsed as command options, such as inputs beginning with `-`.
5. Merge path filters and any request-time language filters, then produce a stable filter hash. Repository registration never contributes language filters.
6. Look up or create the scoped index partition.

Queries must not automatically fall back from narrow filters to wider scopes. A new filter combination requires indexing that scope first.

Repository source normalization must treat source roots as a layout set rather
than a single `src/` convention. Supported source-root candidates include
`src/`, `lib/`, `Sources/`, `external_deps/`, `packages/`, `modules/`,
`plugins/`, `extensions/`, and nested JVM roots beneath those directories.
For clean Git snapshots, the tracked tree is the directory authority inside the
registered and requested path scope: tracked `.cloudbuild/`, `.cid/`,
`.build_config/`, `build/`, `dist/`, `vendor/`, and `third_party/` paths are
eligible instead of being rejected by name. The remaining default preset is
file-level protection for binary/media files and dataset dumps such as
`*.jsonl`. Source-root discovery still avoids widening a deliberately narrow
`--path src` registration into broad dependency trees, and worktree overlays do
not recursively expand untracked high-volume dependency/cache/build directories
unless an explicit path filter opts in.

Non-Git source directories use a filesystem synthetic snapshot instead of Git
history. Because there is no tracked tree authority, the default scan is
whitelist based: root-level supported files and source-like roots such as
`src/`, `include/`, `lib/`, `Sources/`, `packages/`, `modules/`, `plugins/`,
`extensions/`, `docs/`, and `config/` are eligible, while broad build,
dependency, cache, virtualenv, and coverage directories require explicit path
opt-in. The opt-in is path-specific: a narrow filter such as `--path src` does
not permit scanning sibling `node_modules/`, `target/`, or `vendor/`
directories, while `--path build` or a path beneath `build/` permits that broad
directory, and `--path .` explicitly opts into the whole non-Git root including
broad directories. Default scans without a path filter descend only into
directories that can contribute root-level whitelisted files or whitelisted
source/config/documentation roots. Filtered scans descend only into requested
paths and bounded discoverable source roots, so unrelated sibling directories
are skipped before `read_dir` can fail or spend time outside the requested scope.
If Git probing
finds Git metadata but cannot resolve the repository because of unsafe ownership,
missing Git support, or corrupt metadata, registration and ref resolution fail
instead of silently treating the directory as a non-Git filesystem source.

The `filesystem:` synthetic commit is bound to the effective indexed scope after
source-layout discovery, not to every scanned file. Unindexed files outside the
registered/requested path, language, and non-Git default whitelist do not move
that scoped commit. The pre-scope filesystem hash pass must also apply the file
preset before reading contents: media, binary, `*.jsonl`, and other default
excluded files are not read or hashed unless an explicit path filter opts into
that file. Non-Git moving-ref resolution for query, status, repository sets, and
impact must use the same effective path and language filters that identify the
indexed scope. Repository-set members with narrower filters must resolve through
the active compatible broader non-Git commit before requiring a narrower
synthetic commit to exist in storage, and repository-set freshness checks must
reuse the same compatible commit before marking a moving member stale. Full-index tasks that replay a queued `filesystem:` ref must
recompute the same scoped hash and fail or retry when the live indexed scope has
changed; they must not silently index a different live tree under the queued
commit. After a full-index plan or synchronous full-snapshot build has accepted
a non-Git synthetic scope, every later read must verify the planned per-path
content hashes before parsing live bytes, and incremental/worktree-overlay
filesystem deltas must reject bytes that no longer match the planned content
hashes before parsing. Non-Git file byte,
hash, and metadata materialization must recheck every selected path component
with `symlink_metadata` and reject final-path or ancestor-directory symlink
replacements before reading or sizing the file. Explicit stored `filesystem:`
refs resolve by identity before dynamic source-kind or Git probing, so old
indexed scopes remain queryable after local edits or after Git metadata is later
added to a source directory. Source byte materialization verifies the live tree
before reading, and a `filesystem:` ref must run that verification through the
filesystem snapshot path before any Git reprobe, using the same effective path
and language filters as the indexed scope.
Incremental non-Git updates must honor an explicit `base_ref` by resolving and
loading fingerprints for that stored scope instead of silently using the
currently active scope. They use both current and previous source-layout
discovery so deleting the last file in a discovered root still removes the old
indexed path. While a broader non-Git index task is active, narrower stale-read
requests may be served only after comparing both the task scope and the
requested selector scope with the task's effective path and language filters.
Impact path collection for non-Git sources must check explicit `filesystem:`
base/head refs before source-kind probing, resolve and compare scoped base/head
refs first with those path and language filters, return an empty changeset when
they match, and otherwise use the indexed effective filesystem filters,
including explicit broad-directory opt-ins. Impact path partitioning and
deleted-symbol extraction must also handle explicit `filesystem:` refs before
Git probing; empty filesystem changesets must not force a snapshot reprobe.
Query/status ref normalization and fresh full-index reuse checks for Git must
keep the cheap `rev-parse`/tree-id path and must not perform an index-scale tree
walk just to resolve a ref or prove the existing scope is fresh.

## 4. Git Snapshot Rules

- `repository_id` is the stable local identity and is not an alias.
- The same tree hash can reuse an index partition while preserving requested-ref audit metadata.
- Rebase or force-moved heads create new scopes.
- Dirty worktrees never mix into clean snapshots; they are represented as `worktree_overlay` or `git_changeset`.

## 5. Authorization and Isolation

Scope policy is enforced before retrieval, indexing, MCP tools, Web operations, and worker tasks. Adapters request only authorized scopes and cannot widen path, language, or repository boundaries during query execution.

## 6. Acceptance Criteria

- Every context item can state its `scope_id`, source kind, version, and filter.
- Code repository queries with different path filters at the same commit do not cross-contaminate.
- After rebase, old scopes are used only for historical audit or explicit queries.

---

Navigation: Previous: [3. Foundational Runtime](03-foundational-runtime.md) | Next: [5. Multimodal Evidence Ingestion](05-multimodal-evidence-ingestion.md)
