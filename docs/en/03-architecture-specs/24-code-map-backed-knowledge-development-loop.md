# Code-Map-Backed Knowledge Development Loop

[English](../../en/03-architecture-specs/24-code-map-backed-knowledge-development-loop.md) | [中文](../../zh/03-architecture-specs/24-code-map-backed-knowledge-development-loop.md)

> Document version: 1.0
> Prepared: 2026-08-12
> Requirements: [issue #351](https://github.com/coolplayagent/relay-knowledge/issues/351), [issue #352](https://github.com/coolplayagent/relay-knowledge/issues/352)

## 1. Decision and Scope

This specification consolidates the two issues into one executable Knowledge development loop:

1. Repository bootstrap must establish `codespec/codespec-map.yaml`, `knowledge/knowledge-map.yaml`, and a versioned code map. Completing only one surface is not a successful initialization.
2. A Git commit is authoritative for tracked source facts. The code map is the published symbol, call, dependency, and retrieval-evidence view for one exact commit/source scope. The whole-software model is a derived read model published for the same code-map scope.
3. YAML stores stable knowledge routes and a model entry point. It does not copy commit-varying architecture narratives, build targets, or deployment facts. The actual `design`, `build`, `iac`, and `relationships` facts come from `repo software` with ref, source-scope, freshness, and evidence metadata.
4. Before producing a spec or code, an agent must consume the `business-knowledge` route, business terms/mappings, software model, architecture/business-domain views, and code context for one pinned ref. After a commit, it must refresh the same fenced projections and validate YAML again.

This specification does not introduce a second copy of code facts, persist LLM narratives as authoritative facts, scan repositories on query hot paths, or replace durable tasks and leases with shell polling loops.

This chapter answers how to implement the loop through CLI coordination,
durable tasks, leases, freshness, and evidence gates. For why commits are the
fact anchors and how humans and agents organize decision context, read
[Chapter 26: Git Commit + Knowledge Development Philosophy and Iteration
Loop](26-git-commit-knowledge-development-loop.md).

## 2. Authoritative State and Ownership

| Surface | Authoritative content | Owner | Consistency identity |
| --- | --- | --- | --- |
| Git repository | Source, documents, manifests, CI, deployment configuration | Git | Immutable commit or explicit worktree overlay |
| Repository maps | Typed CodeSpec/Knowledge directories plus topics, sources, routes, bounded history, and software-model entry point | `codespec/codespec-map.yaml`, `knowledge/knowledge-map.yaml`, `knowledge/topics/`, `knowledge/history/` | `schema_version`, `map_type`, `map_version`, SHA-256 digest |
| Code map | Files, symbols, references, calls, imports, chunks, and change facts | Code repository index | Repository id, resolved commit, tree hash, source scope |
| Software model | Dependency, SDK, file, topic, relationship, build, IaC, and design projections | Software global projection | Same source scope and graph version as the code map |
| Business model | Domains, canonical terms, aliases, semantics, definition conflicts, and technical mappings | Fenced projection of the Git-authored glossary | Same resolved commit, source scope, and graph version as the code map |
| Agent context | Bounded map-route, software/view, context, and impact evidence | Skill workflow | Pinned base/head, freshness, evidence ids |

The default stable entry in `knowledge/knowledge-map.yaml` is:

- topic id: `software-model`
- source id: `repository-software-model`
- source kind: `repo`
- URI: `.`
- source scope: `repo`

This source denotes the current repository's code-map-backed software-model entry point; it is not a generated-result cache. `map init` must idempotently ensure the entry for both new and existing maps. If the reserved id is already attached to an incompatible topic, kind, URI, or scope, initialization must report a conflict instead of overwriting the user's contract.

`map init` also ensures the `business-knowledge` topic, `repository-business-glossary` file source, `knowledge/glossary/business-glossary.yaml` URI, and `repo` scope. That route grants authorization only. The glossary stores authored business facts, which become commit-, scope-, freshness-, and evidence-bound graph facts only after indexing. An existing glossary is preserved, and incompatible reserved route/source fields fail closed.

Repository Map v3 adds a strongly typed `directories` contract to both visible roots. `codespec` requires `requirements`, `design`, `api`, `test`, and `decisions`; `knowledge` requires `domain`, `guides`, `ops`, `glossary`, and `best-practices`, while custom confined directories remain extensible. Entries carry purpose, content scope, key files, load policy, typed qualified relations, and update policy. Knowledge Map still keeps topic summaries, ordered source ids, content-addressed shard refs, map version, and at most 16 recent history entries in its root. Topic sources/routes live under `knowledge/topics/`; older history lives under `knowledge/history/`. `map route <topic> --type knowledge` loads one shard; aggregate reads default to both maps, while targeted writes require a concrete type. Cross-file digests, directory existence, relationship targets and cycles, history continuity, path confinement, and reserved routes are enforced by `map validate` rather than JSON Schema alone.

All generated refs are restricted to designated real directories under the selected map root; absolute paths, `..`, symlink/reparse escape, and mismatched map type/path are rejected. Multi-file mutations publish immutable artifacts first and the root last. V2 migration copies retained assets before publishing `knowledge/knowledge-map.yaml`, then replaces `.knowledge/knowledge-map.yaml` with a v3 redirect that old readers reject explicitly. The CLI retains a verified v2 root for `map migrate --type knowledge --rollback`; uninstall never deletes repository-owned map content.

## 3. Why YAML Does Not Copy the Derived Model

Writing a resolved commit, architecture narrative, build targets, and deployment resources back into the indexed YAML creates a self-reference cycle: changing YAML changes the Git tree, which creates another snapshot identity and demands another YAML rewrite. It also creates a stale fact copy outside the durable publication fence.

The contract therefore separates responsibilities:

- YAML fixes where knowledge is read and which repository is the model root.
- The code map fixes which source-fact views are derived and served for a ref.
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
7. Only after status identifies the exact resolved target, the checkpoint is complete, and the scope is not stale, read same-ref `repo business --kind all`, `repo software --kind all`, architecture/business-domain views, and business-driven `repo context`.
8. Run `map validate` once more and include map version, resolved ref, source scope, freshness, and degraded diagnostics in the initialization result.

Bootstrap is not a fictitious cross-YAML/SQLite transaction. A partial failure retains the recoverable map, durable task, checkpoint, and diagnostics. A later run resumes from state instead of deleting valid work or starting unbounded retries.

## 5. Incremental Development Protocol

### 5.1 Commit Events

For a registered Git repository, one normal commit event invokes `repo update <alias>`. The service resolves and pins base/head before queueing. The agent captures the same immutable pair from a completed response's `summary.base_resolved_commit_sha` and `summary.resolved_commit_sha`, or from a queued task's immutable base/head.

It must then:

1. Wait until `repo status` reports the exact head as published and not stale.
2. Run `repo impact` on the pinned base/head.
3. Run `repo business --kind all`, `repo context`, `repo software --kind all`, and architecture/business-domain views at the pinned head.
4. When Markdown, specifications, or the knowledge map changed, also read `repo software --kind topics|relationships` and the affected OKF neighborhood.
5. Run `map validate`. When authoritative document, config, CI, or runtime sources were added, moved, or removed, maintain routes only through `map source add/update/remove` and retain history.

Code-index publication refreshes business and software projections under the same task lease, attempt, and publication fence. A staged scope cannot publish while business projection is incomplete. A second writer, query-time glossary/repository scan, or unmanaged background loop is not an acceptable synchronization mechanism.

### 5.2 Worktree Iteration

When an agent needs uncommitted edits before a commit, it first ensures that a clean `HEAD` baseline exists and then runs `repo index <alias> --ref worktree`. Every subsequent query, software, view, and context command must also select `worktree`; a clean-commit result must not be described as containing uncommitted files.

A map mutation changes the worktree. If the current spec or coding decision must see the new route immediately, refresh the worktree overlay. Otherwise commit the map together with its related source or documentation and let the next commit update publish it. In both cases the handoff states which ref was served.

## 6. Spec and Coding Context Contract

Before writing a specification, an agent reads at least:

- `map route business-knowledge` and same-ref `repo business --kind all`;
- the relevant `map route`, including architecture, build, deployment, or repository-specific topics;
- `repo software --kind all` at a pinned ref, with particular attention to `design`, `build`, `iac`, and `relationships`;
- `repo view --kind architecture-layers`;
- `repo view --kind business-domains`, distinguishing authored from inferred evidence kinds;
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
- `repo business`, `repo software`, `repo view`, and `repo context` read committed projection/graph facts and do not read glossary YAML or recursively scan the repository on query hot paths.
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
| KDL-06 | Architecture, build, and deployment model share the code scope | An isolated-runtime CLI end-to-end test chains `map init`, register/index, `repo software all`, the architecture view, and final validation while asserting the resolved commit, source scope, freshness, and evidence; software projection tests retain focused boundary coverage |
| KDL-07 | Spec/coding entry consumes map, model, impact, and context | Skill default prompt, reference workflow, and package validation |
| KDL-08 | Documentation and release packages cannot regress to the old prompt | Shared skill policy self-test, PR gate, and release bundle gate |
| KDL-09 | Repository delivery passes complete quality gates | fmt, clippy, all-target tests, coverage, package, publish dry-run, and relevant self-iteration cases |
| KDL-10 | Business and code/software models share one publication identity | End-to-end coverage chains the business route, glossary authoring, index, business/view/context reads and asserts identical commit, scope, freshness, evidence, and publication fence |

---

Navigation: Previous: [22. Service Deployment, Control Plane, and Data Plane](22-service-deployment-control-data-plane.md) | Next: [25. Code Index Retention](25-code-index-retention.md)
