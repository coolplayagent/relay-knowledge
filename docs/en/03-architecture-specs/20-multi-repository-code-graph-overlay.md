# Multi-Repository Code Graph Overlay

[English](../../en/03-architecture-specs/20-multi-repository-code-graph-overlay.md) | [中文](../../zh/03-architecture-specs/20-multi-repository-code-graph-overlay.md)

> Document version: 1.0
> Date: 2026-05-19
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Decision

Multi-repository support must be a thin overlay, not a materialized large `source_scope` that copies facts from member repositories. A single repository `repository_snapshot` remains the smallest boundary for code facts, indexes, and query precision. The multi-repository layer only expands a user-selected repository set into real single-repository snapshot scopes, coordinates queries, merges ranking, and stores a small number of derived cross-repository edges.

This design preserves four constraints:

- Do not copy base facts such as `CodeFile`, `CodeSymbol`, `CodeChunk`, references, calls, or imports.
- Do not change existing single-repository query semantics; single-repository queries still push down to one `source_scope`.
- Every multi-repository result explains its repository, commit, tree hash, and scope.
- Cross-repository relationships are explainable overlay edges, not single-repository resolved edges.

## Implementation Status

The current implementation provides the initial product path across all three phases:

- `repo-set create/add/query/status/refresh` and the shared API/Web/MCP entry points use an explicit repository set selector.
- SQLite persists `code_repository_sets`, `code_repository_set_members`, `code_repository_cross_edges`, overlay status, and overlay refresh tasks. Repository sets do not copy rows into base code fact tables.
- Multi-repository query fans out at the application layer to each member's persisted `source_scope`, then merges by member priority, freshness, and overlay confidence. Request path/language filters narrow the member scope instead of widening or re-resolving it through current repository defaults. Deduplication includes repository, scope, path, line range, and excerpt.
- `repo-set refresh` builds import/module-level cross-repository overlay edges with resolved, ambiguous, and unresolved states plus evidence JSON.
- Scope retention preserves single-repository snapshots referenced by repository set members. Background overlay refresh tasks use durable leases, retries, and dead-letter state.

## 2. Current Baseline

The current implementation already has the foundation:

- `CodeRepository` has stable `repository_id`, alias, root path, and registered path/language scope.
- `repository_snapshot` scope is derived from `repository_id`, tree hash, path filters, and language filters.
- SQLite code fact tables are partitioned by `source_scope` and keep `repository_id`.
- `repo register/index/query/impact/status/report` currently accept one repository selector.
- Query candidate windows first constrain `source_scope`, so single-repository retrieval is not polluted by other repositories.

The missing pieces are first-class `RepositorySet` / workspace selectors, a coordinator that queries multiple real scopes, and separate cross-repository resolution edges.

## 3. Core Model

The model has three layers.

### 3.1 Repository

`CodeRepository` continues to represent one local Git worktree and its authorization boundary.

```text
CodeRepository {
  repository_id
  alias
  root_path
  allowed_path_filters
  allowed_language_filters
}
```

One Git root can have multiple aliases. One alias cannot point to different repository ids. Repository sets do not change this rule.

### 3.2 Repository Snapshot

`RepositorySnapshot` is the only partition that stores real code facts.

```text
RepositorySnapshot {
  source_scope
  repository_id
  resolved_commit_sha
  tree_hash
  path_filters
  language_filters
  freshness_state
}
```

Files, symbols, chunks, single-repository references, calls, imports, diagnostics, and tombstones are written only under this real snapshot scope. Repository sets must not copy those rows.

### 3.3 Repository Set

`RepositorySet` is the user-selected multi-repository query and authorization boundary. It stores member pointers, not code facts.

```text
RepositorySet {
  set_id
  alias
  description
  default_ref_policy
  created_at_ms
  updated_at_ms
}

RepositorySetMember {
  set_id
  repository_id
  repository_alias
  ref_selector
  resolved_commit_sha
  source_scope
  path_filters
  language_filters
  priority
}
```

`source_scope` must point to an existing single-repository `repository_snapshot`. If a member ref resolves to a snapshot that has not been indexed, multi-repository queries report missing or stale state instead of silently falling back to an old snapshot. Re-adding the same repository to a set replaces the previous member pointer so a moving ref such as `HEAD` does not fan out to both old and new snapshots.

## 4. Graph Model

The multi-repository graph adds a workspace overlay rather than merging repositories into one node space.

```text
RepositorySet
  contains -> CodeRepository
  resolves_to -> RepositorySnapshot

RepositorySnapshot
  contains -> CodeFile
  defines -> CodeSymbolSnapshot
  contains -> CodeChunk

CodeSymbolSnapshot
  belongs_to -> CanonicalSymbol
  references -> CodeSymbolSnapshot
  calls -> CodeSymbolSnapshot
  imports -> ModuleReference

CrossRepositoryEdge
  from -> CodeSymbolSnapshot | CodeFile | ModuleReference
  to -> CodeSymbolSnapshot | CodeFile | ExternalPackage | UnresolvedTarget
```

`CanonicalSymbol` still primarily represents stable identity across snapshots within one repository. Same-named symbols across repositories must not be automatically merged unless import, package/module metadata, or explicit configuration provides enough evidence.

## 5. Storage Design

The new multi-repository tables should stay thin:

```text
code_repository_sets
  set_id TEXT PRIMARY KEY
  alias TEXT NOT NULL UNIQUE
  description TEXT
  default_ref_policy_json TEXT NOT NULL
  created_at_ms INTEGER NOT NULL
  updated_at_ms INTEGER NOT NULL

code_repository_set_members
  set_id TEXT NOT NULL
  repository_id TEXT NOT NULL
  repository_alias TEXT NOT NULL
  ref_selector TEXT NOT NULL
  resolved_commit_sha TEXT NOT NULL
  source_scope TEXT NOT NULL
  path_filters_json TEXT NOT NULL
  language_filters_json TEXT NOT NULL
  priority INTEGER NOT NULL
  PRIMARY KEY (set_id, repository_id, source_scope)
```

The storage primary key keeps historical scope identity explicit, but the write path treats `(set_id, repository_id)` as the active member pointer and deletes the previous row before inserting a replacement member.

Derived cross-repository edges are stored separately:

```text
code_repository_cross_edges
  edge_id TEXT PRIMARY KEY
  set_id TEXT NOT NULL
  from_source_scope TEXT NOT NULL
  from_repository_id TEXT NOT NULL
  from_record_kind TEXT NOT NULL
  from_record_id TEXT NOT NULL
  to_source_scope TEXT
  to_repository_id TEXT
  to_record_kind TEXT NOT NULL
  to_record_id TEXT
  edge_kind TEXT NOT NULL
  resolution_state TEXT NOT NULL
  confidence_basis_points INTEGER NOT NULL
  confidence_tier TEXT NOT NULL
  evidence_json TEXT NOT NULL
  created_at_ms INTEGER NOT NULL
```

Do not add materialized virtual fact rows. For example, `code_repository_files` must not receive copied rows for a repository set. Small statistics or query caches are acceptable only when invalidated by member `source_scope` and index version.

## 6. Query Semantics

Single-repository query behavior is unchanged:

```text
CodeRepositorySelector
  -> resolve repository alias
  -> resolve indexed source_scope
  -> search source_scope = ?
  -> rank and return
```

Multi-repository queries add a new selector:

```text
CodeRepositorySetSelector {
  set_alias
  members
  ref_policy
  path_filters
  language_filters
}
```

Multi-repository query flow:

```text
set alias
  -> authorize set and members
  -> expand to member source scopes
  -> run bounded single-scope candidate queries
  -> merge candidates with repository metadata
  -> add cross_repository_edge evidence when available
  -> rerank and truncate
```

The first implementation should fan out at the application layer to existing `search_code` behavior, then merge and rerank. A SQL `source_scope IN (...)` optimization can come later after candidate windows and observability are well understood. Either way, every candidate must retain its original `source_scope`.

Repository-set queries must search the member's stored `source_scope` directly. Query-time path and language filters are applied as additional narrowing predicates against that stored scope; they are not merged into the member selector as OR alternatives, and they must not cause the query to resolve against the repository's current registration defaults.

## 7. Ranking and Precision Constraints

Single-repository queries must not use multi-repository ranking signals. Multi-repository queries may add:

- Repository member priority.
- Whether the exact symbol or path match came from an explicitly selected member.
- Cross-repository edge confidence.
- Matching repository alias, package/module name, or dependency evidence.
- Member snapshot freshness.

Multi-repository reranking must not globally deduplicate by path or symbol name. The dedupe key includes at least `repository_id`, `source_scope`, path, line range, and excerpt. `src/lib.rs`, `init`, or `main` in different repositories are different fact instances.

When a cross-repository edge is `ambiguous`, `unresolved`, or `inferred`, responses expose resolution state and confidence. Such edges must not be promoted into single-repository `resolved` calls or references.

## 8. Cross-Repository Resolution

Cross-repository resolution is a finalization stage after single-repository indexing:

```text
repository set members
  -> collect exported modules and public symbols per source_scope
  -> collect import/call/reference target hints
  -> match dependency and module evidence
  -> write cross_repository_edges
  -> mark repository set overlay freshness
```

The first version only needs import/module evidence with clear matches. Without package metadata or a unique module match, edges stay `ambiguous` or `unresolved`. Cross-repository calls and references can depend on later language-specific improvements and should not block basic multi-repository search.

## 9. Freshness, Retention, and Invalidation

`RepositorySet` freshness has member snapshot state and overlay edge state:

- If any member `source_scope` is missing, the set is incomplete.
- If any member snapshot is stale, the set is stale.
- For moving member refs such as `HEAD` or branch names, status re-resolves the ref through the repository worktree. If it points at a different commit than the stored member snapshot, the member and set are stale until the member pointer is refreshed.
- If member snapshots are fresh but cross-edge overlay is behind, basic multi-repository results can be returned with overlay-stale metadata.

Single-repository scope retention must not prune scopes still referenced by repository set members. Deleting a repository set removes those references; normal single-repository retention can then clean old scopes.

## 10. API and CLI Entrypoints

New APIs keep single-repository APIs compatible and add explicit multi-repository operations:

```text
repo-set create <alias>
repo-set add <set> <repo-alias> --ref <ref> [--path <filter>] [--language <id>]
repo-set query <set> --query <text> --kind <kind> --limit <n>
repo-set status <set>
repo-set refresh <set>
```

Responses include:

- Set alias and set id.
- Each member's repository id, alias, requested ref, resolved commit, source scope, and freshness.
- Each result's repository metadata and source scope.
- Overlay freshness, cross-edge evidence, truncation, and degraded reason.

MCP and Web use the same selector. A plain `source_scope` string must not silently represent a multi-repository set. MCP may promote a repository set at runtime only when the set alias is explicitly allowed without colliding with a registered repository alias, or every current member repository scope is already allowed by the static or runtime policy. Repository-set authorization is revalidated on every MCP call instead of being stored in the repository alias runtime cache, and MCP audit records the set alias for repository-set query responses.

## 11. Implementation Phases

Phase one implements thin sets and coordinated query:

- Add repository set tables, domain types, storage contract, and CLI/API registration entrypoints.
- `repo-set query` expands members and fans out to existing single-repository queries.
- Merge/rerank preserves repository metadata and does not resolve cross-repository edges yet.

Phase two adds cross-repository import overlay:

- Build a read-only exported module/symbol index for repository set members.
- Write `code_repository_cross_edges`.
- Attach cross-repository edge evidence to query responses.

Phase three optimizes performance and recovery:

- Add overlay freshness cursor, refresh queue, and status diagnostics.
- Add bounded parallelism, per-set budgets, and candidate window metrics.
- Add SQL `IN` queries or small materialized candidate caches only when real bottlenecks justify them.

## 12. Acceptance Criteria

- Creating a repository set does not increase row counts in `code_repository_files`, `code_repository_symbols`, or `code_repository_chunks`.
- Single-repository `repo query` candidate windows still contain one `source_scope`, and existing accuracy tests remain unchanged.
- Multi-repository `repo-set query` returns hits from multiple repositories and labels each hit with repository alias, repository id, commit, and source scope.
- Identical paths or symbols in two repositories do not overwrite or incorrectly deduplicate each other.
- After deleting or reindexing a member repository, repository set status reports missing, stale, or overlay-stale state.
- Cross-repository edges appear only in overlay storage and multi-repository responses, not in single-repository base edge tables.

---

Navigation: Previous: [19. Installation, Release, and Upgrade](19-installation-release-and-upgrade.md)
