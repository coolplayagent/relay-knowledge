# Code-Map-Backed Knowledge Development Loop

[English](../../en/03-architecture-specs/24-code-map-backed-knowledge-development-loop.md) | [中文](../../zh/03-architecture-specs/24-code-map-backed-knowledge-development-loop.md)

> Document version: 1.0
> Prepared: 2026-08-12
> Requirements: [issue #351](https://github.com/coolplayagent/relay-knowledge/issues/351), [issue #352](https://github.com/coolplayagent/relay-knowledge/issues/352)

## 1. Decision and Scope

This specification consolidates the two issues into one executable Knowledge development loop:

1. Repository bootstrap must establish both the `.knowledge/knowledge-map.yaml` navigation contract and a versioned code map. Completing only one is not a successful initialization.
2. The code map is the primary source of truth for source, symbols, calls, dependencies, and source scopes. The whole-software model is a derived read model published for the same code-map scope.
3. YAML stores stable knowledge routes and a model entry point. It does not copy commit-varying architecture narratives, build targets, or deployment facts. The actual `design`, `build`, `iac`, and `relationships` facts come from `repo software` with ref, source-scope, freshness, and evidence metadata.
4. Before producing a spec or code, an agent must consume the knowledge routes, software model, architecture view, and code context for one pinned ref. After a commit, it must refresh the code map and software model and validate YAML again.

This specification does not introduce a second copy of code facts, persist LLM narratives as authoritative facts, scan repositories on query hot paths, or replace durable tasks and leases with shell polling loops.

## 2. Authoritative State and Ownership

| Surface | Authoritative content | Owner | Consistency identity |
| --- | --- | --- | --- |
| Git repository | Source, documents, manifests, CI, deployment configuration | Git | Immutable commit or explicit worktree overlay |
| Knowledge map | Topics, sources, routes, bounded recent history, and stable software-model entry point | `.knowledge/knowledge-map.yaml` root manifest, `.knowledge/topics/` shards, `.knowledge/history/` archive | `schema_version`, `map_version`, SHA-256 digest |
| Code map | Files, symbols, references, calls, imports, chunks, and change facts | Code repository index | Repository id, resolved commit, tree hash, source scope |
| Software model | Dependency, SDK, file, topic, relationship, build, IaC, and design projections | Software global projection | Same source scope and graph version as the code map |
| Agent context | Bounded map-route, software/view, context, and impact evidence | Skill workflow | Pinned base/head, freshness, evidence ids |

The default stable entry in `.knowledge/knowledge-map.yaml` is:

- topic id: `software-model`
- source id: `repository-software-model`
- source kind: `repo`
- URI: `.`
- source scope: `repo`

This source denotes the current repository's code-map-backed software-model entry point; it is not a generated-result cache. `map init` must idempotently ensure the entry for both new and existing maps. If the reserved id is already attached to an incompatible topic, kind, URI, or scope, initialization must report a conflict instead of overwriting the user's contract.

Knowledge Map v2 keeps only topic summaries, each topic's ordered source-id summary, content-addressed shard refs, map version, and at most 16 recent history entries in the root manifest. Persistent artifact schema v2 and the bounded `KnowledgeMapView` used by `map show` have distinct type identities, so a partial history view cannot be written back as a storage contract. Before any shard is loaded, the root summary rejects source ids duplicated across topics, history-version overflow, and invalid topic/archive ref or digest. Topic sources/routes live under `.knowledge/topics/`; complete history beyond that window lives in content-addressed archives under `.knowledge/history/`. `map route <topic>` loads only the root and requested shard; `map show` loads current shards but not history archives and reports `archived_through`, `complete`, and recent entries; `map history` exposes bounded pages up to 256 entries. The root manifest's optional `history.index` references a content-addressed B+ tree with fanout 64 and maximum height 10. Node ranges must be contiguous, non-overlapping, and cover `1..=archived_through`, so locating one archive reads at most 11 nodes plus that archive independently of the total archive count. Early v2 maps without an index remain available to show, route, and complete validation, but require a writer-locked `map init` representation migration before paging; reads never fall back to walking the reverse chain. `map validate` and every mutation still verify the complete archive chain and its index. Mutations use an OS advisory repository lock with a ten-second bounded wait, so live writers remain exclusive and abnormal process exit releases ownership automatically. The code parser emits separate root authorization and digest-verified shard facts; software projection joins those facts and therefore accepts only current root-referenced shards, while v1 root topics remain compatible.

All shard/archive refs are restricted to designated real directories under `.knowledge/`; absolute paths, `..`, root/backup or artifact leaf symlinks, designated artifact-directory symlinks, and repository escape are rejected. Multi-file mutations share one repository writer lock, append only newly completed history chunks, publish immutable content-addressed artifacts first, and publish the root manifest last. Artifact temporaries are removed when write or rename fails, while a root-publication failure restores the previous valid root. A successful publication retains the preceding recovery manifest and its refs. The first observation that an older shard is unreferenced creates a retirement marker, and a minimum 60-second grace period is measured from that retirement time before best-effort cleanup so a concurrent reader that already loaded an older root can finish loading its shards. Cleanup admits only files that strictly match the content-addressed shard naming contract and preserves README, `.gitkeep`, and other user-managed files under `.knowledge/topics/`. The map remains stable navigation only and must never contain snapshot-bound code, build, IaC, framework-scan, or design projection facts.

## 3. Why YAML Does Not Copy the Derived Model

Writing a resolved commit, architecture narrative, build targets, and deployment resources back into the indexed YAML creates a self-reference cycle: changing YAML changes the Git tree, which creates another snapshot identity and demands another YAML rewrite. It also creates a stale fact copy outside the durable publication fence.

The contract therefore separates responsibilities:

- YAML fixes where knowledge is read and which repository is the model root.
- The code map fixes the source facts for a ref.
- The software projection fixes what architecture, build, and deployment facts can be derived deterministically from that same source scope.
- Short narratives may appear only as response sections backed by evidence ids; they are not persisted authoritative facts.

This keeps YAML reviewable and reversible, keeps derived models refreshable and diagnosable, and prevents dual-write drift.

## 4. Bootstrap Protocol

When initializing repository knowledge, the skill must coordinate the existing CLI in this order:

1. Resolve a published `relay-knowledge` executable and read command metadata.
2. Run `map validate --format json`. Create only when the map is missing; report an existing invalid map instead of replacing it.
3. Run `map init --format json` to create or idempotently ensure the default `software-model` route, then validate again.
4. Run `repo list --format json` and reuse a completed alias whose normalized root and registered scope match. Otherwise run `repo register` and capture the returned alias.
5. Establish a clean `HEAD` baseline for a Git repository. If the map was created or upgraded, or other authorized uncommitted files must be visible, then establish a `worktree` overlay. Non-Git source directories continue to use a `HEAD` filesystem snapshot.
6. Treat `repo index` as a durable, bounded, single-writer task. Recover a command timeout through `repo status`; do not start a competing worker when a managed service is active; without a service, run only bounded single-shot `repo index-worker` attempts for queued or retrying work.
7. Only after status identifies the exact resolved target, the checkpoint is complete, and the scope is not stale, read `repo software --kind all` and `repo view --kind architecture-layers` at the same ref.
8. Run `map validate` once more and include map version, resolved ref, source scope, freshness, and degraded diagnostics in the initialization result.

Bootstrap is not a fictitious cross-YAML/SQLite transaction. A partial failure retains the recoverable map, durable task, checkpoint, and diagnostics. A later run resumes from state instead of deleting valid work or starting unbounded retries.

## 5. Incremental Development Protocol

### 5.1 Commit Events

For a registered Git repository, one normal commit event invokes `repo update <alias>`. The service resolves and pins base/head before queueing. The agent captures the same immutable pair from a completed response's `summary.base_resolved_commit_sha` and `summary.resolved_commit_sha`, or from a queued task's immutable base/head.

It must then:

1. Wait until `repo status` reports the exact head as published and not stale.
2. Run `repo impact` on the pinned base/head.
3. Run `repo context`, `repo software --kind all`, and relevant `repo view` kinds at the pinned head.
4. When Markdown, specifications, or the knowledge map changed, also read `repo software --kind topics|relationships` and the affected OKF neighborhood.
5. Run `map validate`. When authoritative document, config, CI, or runtime sources were added, moved, or removed, maintain routes only through `map source add/update/remove` and retain history.

Code-index publication already refreshes the software projection under the same task lease and publication fence. A second unleased writer, query-time repository scan, or unmanaged background loop is not an acceptable synchronization mechanism.

### 5.2 Worktree Iteration

When an agent needs uncommitted edits before a commit, it first ensures that a clean `HEAD` baseline exists and then runs `repo index <alias> --ref worktree`. Every subsequent query, software, view, and context command must also select `worktree`; a clean-commit result must not be described as containing uncommitted files.

A map mutation changes the worktree. If the current spec or coding decision must see the new route immediately, refresh the worktree overlay. Otherwise commit the map together with its related source or documentation and let the next commit update publish it. In both cases the handoff states which ref was served.

## 6. Spec and Coding Context Contract

Before writing a specification, an agent reads at least:

- the relevant `map route`, including architecture, build, deployment, or repository-specific topics;
- `repo software --kind all` at a pinned ref, with particular attention to `design`, `build`, `iac`, and `relationships`;
- `repo view --kind architecture-layers`;
- a requirement-specific `repo context` or definition/references/callers/callees query;
- freshness, unresolved-edge, direct-source-read, and degraded diagnostics.

Before coding, the agent maps every requirement to code symbols, call/dependency edges, configuration, build/deployment evidence, and test entry points. Missing evidence is reported as a gap or unresolved target; it is not replaced with guesses, fixture special cases, or arbitrary grep matches.

Acceptance includes a requirement-to-authoritative-evidence-to-test/gate matrix. A focused unit test proves only its covered behavior; repository quality, skill-package behavior, and release surfaces each require their corresponding gate.

## 7. Freshness, Failure, and Degradation Semantics

| State | Allowed behavior | Forbidden claim |
| --- | --- | --- |
| Map missing | Create and validate the map | Repository knowledge is initialized |
| Map invalid or conflicting | Report diagnostics and stop map mutation | Automatically repaired or synchronized |
| Task queued, running, or retrying | Report task/checkpoint and recover via service or bounded worker | Exact target is queryable |
| Scope stale | Continue recovery or explicitly use stale diagnostics | Spec/code uses the latest graph |
| Scope fresh, projection degraded | Use unaffected evidence with disclosure; directly verify affected paths | Unconditionally complete model |
| Exact scope fresh, map valid | Produce provenance-backed spec/code context | Evidence-free architecture facts |

Missing external dependency source remains unresolved metadata, not repository degradation. Only parsing, persistence, or projection failures inside the authorized scope may create a degraded reason.

## 8. Safety, Resource, and Recovery Constraints

- Every index/update preserves bounded queues, leases, checkpoints, backoff, dead letters, and one active repository writer.
- The skill does not kill competing processes, increase an unbounded busy timeout, or delete runtime state to manufacture success.
- `repo software`, `repo view`, and `repo context` read committed projection/graph facts and do not recursively scan the repository on query hot paths.
- Map mutation uses a file lock, atomic rename, contiguous history versions, and CLI validation. YAML is edited manually only when the CLI is unavailable and the user explicitly requests repair.
- Silent background updates remain hosted by a platform service manager and remain pausable, observable, and recoverable.

## 9. Acceptance Matrix

| ID | Requirement | Authoritative evidence |
| --- | --- | --- |
| KDL-01 | A new map contains the default software-model route | Domain/application unit tests read YAML and verify topic/source/route |
| KDL-02 | An old map is upgraded idempotently and repeat init does not bump the version | Application unit test compares the first upgrade with the second init |
| KDL-03 | A reserved source-id conflict is not overwritten | Domain unit test asserts an explicit conflict error |
| KDL-04 | Skill bootstrap covers both map and code map | Skill contract gate checks ordered validate/init/list/register/index/status/model/view/validate workflow |
| KDL-05 | Incremental loop pins base/head and refreshes the model | Skill contract gate and existing update/index integration tests |
| KDL-06 | Architecture, build, and deployment model share the code scope | `repo software all`/`repo view` scope, freshness, evidence, and software projection tests |
| KDL-07 | Spec/coding entry consumes map, model, impact, and context | Skill default prompt, reference workflow, and package validation |
| KDL-08 | Documentation and release packages cannot regress to the old prompt | Shared skill policy self-test, PR gate, and release bundle gate |
| KDL-09 | Repository delivery passes complete quality gates | fmt, clippy, all-target tests, coverage, package, publish dry-run, and relevant self-iteration cases |

---

Navigation: Previous: [22. Service Deployment, Control Plane, and Data Plane](22-service-deployment-control-data-plane.md) | Next: To be planned
