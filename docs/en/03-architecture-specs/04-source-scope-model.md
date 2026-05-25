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
5. Merge path/language filters and produce a stable filter hash.
6. Look up or create the scoped index partition.

Queries must not automatically fall back from narrow filters to wider scopes. A new filter combination requires indexing that scope first.

Repository source normalization must treat source roots as a layout set rather
than a single `src/` convention. Supported source-root candidates include
`src/`, `lib/`, `Sources/`, `external_deps/`, `packages/`, `modules/`,
`plugins/`, `extensions/`, and nested JVM roots beneath those directories.
Default exclusion presets still protect high-volume dependency dumps such as
plain `vendor/` and `third_party/`; those paths require explicit path-filter
opt-in before they can enter a repository scope.

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
