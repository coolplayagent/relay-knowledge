# Installation, Release, and Upgrade

[English](../../en/03-architecture-specs/19-installation-release-and-upgrade.md) | [中文](../../zh/03-architecture-specs/19-installation-release-and-upgrade.md)

> Document version: 3.4
> Date: 2026-08-12
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Installation and release are part of product architecture. Stable releases are verifiable, rollbackable, uninstallable, and diagnosable. Binary install paths and runtime state are separate. Background services are managed by platform service managers.

## 2. Release Channels

- GitHub Releases publish cross-platform prebuilt archives, checksums, and release notes.
- crates.io keeps `cargo install relay-knowledge` working.
- Homebrew, Scoop, winget, or distro packages reference artifacts from the same release tag instead of rebuilding divergent snapshots.
- Release tags use `vX.Y.Z`, `X.Y.Z`, or matching prerelease forms such as `vX.Y.Z-rc.1`; the numeric version must match `Cargo.toml` and `Cargo.lock` before the tag is pushed. Manual dry-run dispatches validate the same version contract without publishing crates.io or GitHub release artifacts, and the workflow default dry-run tag must be updated with each release version bump.
- The v1.1.13 release preparation pins `Cargo.toml`, `Cargo.lock`, CLI skill metadata, and the release workflow dry-run default to `1.1.13`; publishing remains tag-driven and starts only after pushing `v1.1.13` or `1.1.13` to GitHub.
- macOS x64 release jobs must use an active Intel runner label, such as `macos-15-intel`, rather than retired `macos-13` images. Artifact upload/download and attestation actions must stay on Node 24-compatible releases so the release workflow remains runnable after GitHub-hosted runner runtime migrations.
- Linux GNU release jobs must build `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` artifacts on a glibc 2.31 baseline and fail the release if the resulting ELF requires any `GLIBC_*` symbol newer than 2.31. The CLI skill Linux x64 bundled asset must pass the same ABI check after packaging.
- OpenTelemetry dependencies form one release compatibility family: `opentelemetry`, `opentelemetry_sdk`, and `opentelemetry-otlp` use the same minor release, and `tracing-opentelemetry` uses the corresponding integration release. Dependency automation must update and validate the family together; a release candidate must not contain duplicate OpenTelemetry core or SDK major/minor lines. The current security floor is `opentelemetry_sdk` 0.32.1, which rejects W3C Baggage values above 8,192 bytes and stops after 64 list members as required by GHSA-w9wp-h8wv-79jx / CVE-2026-48504.
- The XML parser security floor is `quick-xml` 0.41.0, which keeps duplicate-attribute checking linear and caps namespace declarations per element as required by RUSTSEC-2026-0194 and RUSTSEC-2026-0195. Informational unsoundness warnings with an available patch are upgraded in the lockfile rather than ignored.
- Release archive attestations use the generated `checksums.txt` as their subject manifest, so GitHub artifact attestations cover the same archive digests that users verify locally.
- CLI version discovery uses configurable dual sources: GitHub Releases and crates.io. Detection must go through the `env`, `paths`, and `net::http` boundaries, inherit proxy, TLS, timeout, and runtime-cache policy, and ordinary commands may only notify about newer stable versions rather than silently replacing binaries.
- GitHub Releases include a `relay-knowledge-cli-skill-<tag>.tar.gz` skill artifact built from `skills/relay-knowledge-cli`; its version follows `Cargo.toml` and is written into generated `SKILL.md` metadata as numeric semver. The skill artifact includes a root-level `README.md`, Linux x64 and Windows x64 binaries under `assets/`, and the skill instructs agents to prefer the matching bundled asset whenever `version --format json` succeeds. Agents use `PATH` only as a fallback, when the host Linux glibc is older than the bundled asset baseline, or when the user explicitly requests the system install. The release workflow may also publish the same generated skill layout to ClawHub with `clawhub publish` when `CLAWHUB_TOKEN` is configured. This skill-over-CLI artifact is separate from MCP protocol packaging.
- The skill artifact includes `references/knowledge-map-workflows.md` and a policy-gated default prompt for joint knowledge-map/code-map bootstrap and pinned-ref spec development. Upgrading the skill changes agent instructions only; it does not mutate repository YAML or runtime index state until an authorized agent invokes the documented CLI workflow.

## 3. Installation Experience

Installers or install scripts support version selection, install directory selection, dry run, checksum verification, service-definition generation, failure rollback, and uninstall plans. Runtime data is not written to release extraction directories by default.

The service deployment installation experience must state the selected topology explicitly: `embedded_cli` installs no resident service, `resident_single_process` installs one platform service, and `resident_partitioned_sqlite` also includes the shard directory in backup/migration/uninstall confirmation. `service plan install|upgrade|rollback|uninstall --format json` must list the primary database, config/state/log/cache paths, service definition path, service name, permission requirements, failure rollback plan, and partitioned shard-directory coverage requirements in `runtime_state_paths`, `lifecycle_steps`, `rollback_steps`, `permission_requirements`, and `warnings`. `service lifecycle <action> --dry-run` is the default auditable output; only explicit `--execute` may write service definitions, checkpoints, or install directories and invoke systemd, launchd, or Windows Service commands. Future `split_worker_preview` generates separate control-service and worker-service definitions with each process's permissions, environment variables, logs, and shutdown behavior.

Installed resident services must also make the commit-loop policy explicit. `RELAY_KNOWLEDGE_WATCHER_ENABLED` controls both source watching and Git HEAD reconciliation; `RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS` defaults to `5000`. The service definition, lifecycle plan, and doctor output must preserve or explain these values so a shell-only export is not mistaken for installed configuration. The reconciler uses bounded periodic checks and durable code-index tasks under the platform service manager; installers must not add repository-specific Git hooks or unmanaged polling processes.

The implementation keeps this contract auditable through explicit ownership: lifecycle step policy remains in `application::service::lifecycle_plan`, while service-definition rendering, platform permissions, and systemd/launchd/Windows Service commands live in `lifecycle_plan::platform_service`. Changes to either boundary must preserve the same dry-run plan and execution contract across all supported platforms.

Exact code-source fallback is implemented inside the product and must not require `rg` at runtime. Agent-facing setup notes may mention bounded `rg` or `grep -RIn` as manual inspection tools, but installers must not make recursive grep a service dependency or a replacement for indexed query behavior.

## 4. Runtime State

Configuration, databases, indexes, logs, caches, temporary files, and dead-letter data live in platform directories owned by `paths`. Upgrades preserve runtime state and explicitly run schema/index migrations.
The commit-loop retention contract is runtime state. Each publication keeps the union of active and a rolling window of the two latest successes (normally including active), the latest successful incremental predecessor, the clean base of any active worktree overlay, plus unfinished task target/base scopes and repository-set pins. SQLite adds `retiring` scope state and durable GC jobs idempotently: logical retirement is atomic, while later maintenance transactions advance one scope-GC phase whose physical deletion is capped at 512 rows in aggregate across affected application tables of older facts, code FTS/search rows, software projections, checkpoints, workspace state, or scope metadata. Separate task-audit and commit-alias quotas cap primary cleanup at 2,048 physical rows per pass, plus at most one terminal GC-job bookkeeping row. Same-tree commits share content through a bounded 256-row commit-alias window. Finished task history is bounded to 128 succeeded and 64 failed/dead-letter/cancelled rows per repository, preserving the latest success for each retained scope. Upgrades resume persisted GC jobs; older binaries do not understand the retirement state and must not share the database or attempt to reconstruct pruned scopes from task rows. GC bounds live generations and makes freed SQLite pages reusable, but does not promise immediate OS-visible file shrink; physical high-water-mark recovery is a separate explicit, bounded compaction operation.
Repository-level retention adds `code_repository_retention_jobs` and its `cutoff_publication_generation` watermark idempotently during SQLite schema initialization; existing parent jobs receive the compatible default `0`. The default eligible indexed-repository limit is 10 and can be changed with `RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES`. Upgrade needs no manual data migration: existing indexed repositories are evaluated by the next successful publication or maintenance pass, and any selected repository is drained through the existing phased scope GC. Repository rows and aliases remain compatible and are not deleted. Rollback must not run concurrently with a newer binary; an older binary may ignore the additive parent-job table, but it cannot safely coordinate a parent job or reverse scopes already retired by its child GC jobs.
The repository-set migration adds the durable refresh-task queue plus its claim/capacity/audit indexes, and the overlay selector migration adds an idempotent virtual origin-path column with composite origin/target indexes during SQLite schema initialization. The managed service resumes eligible refresh tasks after upgrade. Manual refresh shares a 4,096-chunk, 16-MiB, 32,768-derived-item manifest budget across all members. Upgrade planning must budget for the one-time index build; rollback preserves the additive queue table, column, and indexes rather than deleting or rebuilding overlay facts. A legacy manual overlay above 8,192 edges is rejected unchanged before an unbounded delete, and whole-repository removal likewise rejects more than 64 affected sets or any affected over-cap overlay atomically. This release has no bounded repair command, so operators need a later upgrade-provided repair tool rather than assuming migration closes that cleanup path. These manual-set ceilings do not apply to the opt-in automatic-workspace cross-edge builder; scope GC bounds obsolete-state deletion but does not yet bound one such build.

SQLite graph-store schema marker v4 is an explicit forward migration of derived retrieval state. It recreates the global `graph_bm25` FTS5 table with an indexed, zero-weight `routing_key` containing a scope64 partition token plus a scope-qualified group token, and adds route state/document/group/term tables plus persisted global route-term document frequencies. Route documents contain document identity/kind/scope/path, `created_graph_version`, observable `label_gram_state`, group token, bounded term-count JSON, and an `fts_rowid NOT NULL UNIQUE` sidecar. Authoritative evidence, graph facts, code symbols, and code chunks do not change, so every v4 retrieval structure can be reconstructed from those sources.

The current document-write transaction updates `routing_key`, route sidecars, route-state document count, per-group collection frequencies, and persisted global document frequencies together. Fresh-open reconciliation checks schema, the `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4` route fingerprint, freshness/version state, persisted semantic/vector generation markers, and authoritative/active-global/route-document/group/semantic/vector/state population counts. It intentionally does not perform an unbounded identity, per-row tokenizer, or aggregate deep scan on every open. Canonical identity and tokenizer consistency are checked during a reconstruction already triggered by another stale/schema/count condition; equal-count per-row drift alone does not trigger rebuild on open.

A reconstruction acquires a durable owner/expiry lease and publishes `building` together with a phase/cursor checkpoint and fixed semantic/vector rebuild plan, creates `graph_bm25_rebuild`, and resumes from the persisted checkpoint after an expired-attempt takeover. Transactions admit at most 128 documents, 4 MiB of estimated authoritative source bytes, 8,192 labels, and 8,192 links. A single document that exceeds one or more work budgets is isolated in its own transaction and emits a bounded-identity warning; this guarantees progress and is not an absolute per-document byte bound. The previous flat `graph_bm25` remains readable, while semantic, vector, and fuzzy lexical fallback pause during `building`. Bounded rowid-keyset cleanup removes stale label/semantic/vector rows afterward. Current evidence/code writers use an `IMMEDIATE` transaction and reject a write while the rebuild is active. After completeness verification, one short transaction renames active `graph_bm25` to `graph_bm25_retired`, promotes the shadow, publishes route state `fresh`, and records schema marker v4. The retired table is dropped only after commit, so a crash or rollback cannot publish a partial FTS generation. Upgrade planning must reserve time, WAL capacity, and temporary disk headroom for simultaneous active and shadow FTS generations, sidecars, and short-lived retired cleanup.

The query hot path reads persisted version/count/DF values rather than running full-table `COUNT` or `SUM`. For every actual query term it compares the persisted global DF with a business-column-only `MATCH` probe bounded to `df + 1`; every term must be at or below 20% of the corpus, and all probes together reserve at most 65,536 postings. Scoped FTS also intersects the scope64 routing token, while the ordinary SQL scope predicate remains the hard authorization check. Its single-FTS reader orders a bounded identity window by the hidden rank column and hydrates through the `fts_rowid` sidecar; exact ties across that window's cutoff do not promise deterministic membership. Historical unscoped fallback uses version-leading global indexes for authorized-corpus, label-state, and `label_lower` probes, while scoped indexes remain available. One deferred read transaction spans the complete graph search, so concurrent FTS activation does not split its SQLite snapshot. Tables existing is never enough to report routing as fresh.

Although `routing_key` has zero weight in the v4 scorer, FTS5 still includes it in document length and corpus average document length. The v4 numerical BM25 baseline may therefore differ from v3. The supported parity invariant is only that a document common to routed and flat execution over the same v4 table has a bitwise-identical score.

Existing v1.1.13-era code indexes may contain Markdown source windows whose leading or trailing whitespace was trimmed. The one-time code-index migration atomically marks scopes containing Markdown stale and records its migration marker, but a database schema migration cannot recover bytes that were not persisted. Repository-graph materialization therefore also verifies contiguous chunk byte ranges against the indexed file length and reports an explicit lossless/re-index error for an affected scope. Before using `repo graph` on that scope, the operator must explicitly run full `repo index`; incremental `repo update` rejects a stale base and does not perform this recovery. The normal durable task, single-writer lease, checkpoint, bounded retry, and freshness publication workflow rebuilds the Markdown windows. Installation or binary replacement must not report that this data refresh completed merely because schema initialization succeeded.

Local file-location indexes store SQLite/FTS5 state in the same runtime data
area. Installers and service templates must not default to scanning a whole
disk, Linux `/opt`, mounted volumes, or non-system Windows drives; those roots
are indexed only when the user configures them or passes them to the CLI.

When `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite` is enabled, the main
database still stores control state and each code repository shard database
lives under `stores/repositories/` in the runtime data directory. Backup,
migration, doctor, uninstall confirmation, and rollback plans treat the main
database and shard directory as one runtime state set; they cannot move or
verify only the main database and then report upgrade success.
Shard catalog routes are relocatable and are resolved against the current
runtime data directory during restore, but this only works when the shard
directory is moved with the main database.

Future external graph/vector/storage backends or replicated SQLite backends are
also runtime state. Installers, doctor, and upgrade plans must record backend
kind, endpoint or local directory, authentication configuration source,
schema/index migration state, and rollback notes; replacing only the binary is
not enough to report data-plane upgrade success.

## 5. Upgrade and Rollback

Upgrade flow:

```text
preflight doctor
  -> operator stops every ad hoc CLI writer
  -> operator creates a transactionally consistent runtime-database backup
  -> lifecycle executor records its binary/service-definition rollback checkpoint
  -> lifecycle executor stops the managed service
     -> successful stop plus the absence of ad hoc writers establishes exclusive access
  -> copy/install the new binary and refresh the service definition
  -> start the new binary through the platform manager
     -> first synchronous database open runs schema/index migration and shadow rebuild
     -> service becomes available only after that open completes
  -> post-upgrade doctor
```

Stopping ad hoc CLI writers and creating a transactionally consistent runtime-database backup are operator preconditions. A successful lifecycle stop of the managed service, combined with the absence of ad hoc writers, establishes the exclusive database access required by migration. The lifecycle executor does not independently probe for exclusive access and does not create a runtime-database checkpoint; its rollback checkpoint covers only the binary and service definition. Operators that require an independent exclusive-access check must execute the documented stages through an external maintenance procedure rather than treating one-shot `--execute` as that verification.

On failure, the lifecycle executor rolls back the binary and service definition. Database rollback uses the operator-created runtime checkpoint; without that checkpoint, the v4 derived-index migration is forward-only as documented below.

For an upgrade that first enables commit reconciliation, stop the old service, back up the complete runtime state set, install the new service definition with the intended watcher switch and interval, and let startup recover leases before it reconciles HEAD. Post-upgrade verification must inspect `service status --format json` for watcher state, `total_commit_reconciliations`, `total_commit_tasks_queued`, `total_commit_reconcile_failures`, code-index queue/lease state, and retention. Rollback restores the previous binary/service configuration; it does not restore scopes already pruned after successful publication. Exact recovery of pruned historical scopes requires the runtime database backup or a new full index from the source repository.

Rolling back only the binary to a pre-v4 release does not undo the forward derived-index migration. The old binary can ignore routing sidecars and use its established flat query path, but the retained v4 `graph_bm25` table is not numerically equivalent to a v3 index. If the old binary writes derived documents, it does not maintain `routing_key` and v4 sidecars consistently; all hierarchical metadata must then be considered stale. The old writer also records its older schema marker, so a later v4 startup explicitly invalidates superficially compatible route state and rebuilds `routing_key` plus sidecars from authoritative documents before routing becomes eligible. Exact restoration of the old scoring baseline requires the pre-v4 runtime-database checkpoint, not just the old executable. The v4 `IMMEDIATE` application fence is not a cross-version lock protocol: an already running old binary does not check `building` and can write through it. Authoritative facts remain the recovery boundary; never treat route metadata as the only copy of user data, and require exclusive database access for upgrade, rebuild, and rollback instead of running old and new writers concurrently.

`service lifecycle upgrade --execute` follows the implemented dry-run stages: record a binary/service-definition rollback checkpoint, stop the managed service, copy the binary when requested, write the service definition, refresh the platform service manager, start the service, and retain execution reports around post-upgrade doctor. It has no separate exclusive-access verification, runtime-database checkpoint, or migration/rebuild stage. Before invoking it, the operator must stop ad hoc CLI writers and create the required transactionally consistent runtime-database checkpoint; the service manager cannot fence an older standalone process, and the lifecycle checkpoint does not cover runtime data. The command requires its managed-service stop step to succeed but does not otherwise verify exclusivity. When the platform manager starts the new binary, its first synchronous database open performs schema v4 migration and any shadow rebuild, and the service does not become available until the open completes. Installs that write an explicit `--install-dir` must not overwrite an existing target binary; upgrades must checkpoint an existing target binary and remove the copied target binary during rollback when no prior binary backup existed. If any stage fails after mutating work starts, the implementation must attempt the declared `rollback_steps` for completed work; failures before any mutating step must not stop, disable, or uninstall an existing service. A lifecycle report may mark rollback complete only when every selected rollback step succeeds, and external service-manager or doctor child processes must have bounded execution time. When an `--execute` run records a failed step, the API/CLI operation must return an error with the failed step id instead of wrapping the failed report in a successful response. `service lifecycle rollback --execute` restores checkpointed binary and service-definition backups, not the runtime database; when no lifecycle checkpoint exists, the gap must be reported through warnings or execution errors rather than silently reporting success.

`relay-knowledge version check` is a read-only diagnostic entry point that reports
the current version, newest stable version, source, release URL, and diagnostics.
Actual upgrades must still be performed explicitly by the user, installer, or
package manager and continue to follow the preflight, checkpoint, service
restart, and post-upgrade doctor flow.

## 6. Release Documentation Readiness

Before a release tag is pushed, the release owner checks the documentation
surface that users and operators will read first:

- Root `README.md` and `README.zh-CN.md` describe the current version's
  installation channels, bundled CLI skill artifact, and quality gates.
- `docs/README.md`, `docs/en/README.md`, and `docs/zh/README.md` list the
  current book structure, recent benchmark/verification records, and any
  Chinese-only records pending translation.
- Chapter 1 installation guidance and this Chapter 19 release contract agree on
  runtime directories, service-manager operation, version checks, rollback, and
  uninstall behavior.
- A dated record in `06-verification` captures the document inventory, local
  link check, file-length check, and the fact that the change is
  documentation-only when no product behavior is intentionally modified.

Documentation refreshes must not update release commands in a way that implies
unavailable artifacts, unsupported package managers, unmanaged service loops, or
automatic silent upgrades.

## 7. Acceptance Criteria

- Release artifacts, checksums, versions, and documentation match each other.
- Linux GNU release binaries and the skill Linux x64 bundled asset require no `GLIBC_*` symbol newer than 2.31.
- The GitHub Release includes the CLI skill archive in `checksums.txt`, the archive contains the skill `README.md` plus Linux x64 and Windows x64 asset binaries, and ClawHub publication uses the same crate version and generated asset layout when enabled.
- The CLI can explain when a newer stable version is available, JSON output remains machine-readable, and ordinary commands never auto-install an update.
- Release-facing documentation has a dated `06-verification` audit covering
  navigation, inventory, link checks, and documentation-only change boundaries.
- Service installation uses systemd, launchd, or Windows Service instead of unmanaged loops.
- `service lifecycle <action> --dry-run` reports the service name, definition path, install directory, runtime paths, permission requirements, rollback plan, and package-manifest verification chain; `--execute` runs only when explicitly requested, executes rollback steps on failure, and returns an operation error for failed executions.
- Uninstall removes service registration and service definitions while preserving runtime data unless the user explicitly confirms removal.
- Uninstalling the service stops commit reconciliation; preserving runtime data also preserves active/recent scopes, protected pins, bounded task history, and future full-reindex capability. Explicit data removal must include every code shard and cannot be described as reversible without a backup.
- Partitioned SQLite shard directories participate in backup, migration, doctor, and uninstall confirmation.
- SQLite graph-store upgrades recognize schema marker v4, rebuild `graph_bm25_rebuild` and rowid/version/label-state sidecars from authoritative facts through takeover-safe phase/cursor checkpoints and bounded document/source-byte/label/link batches while the old flat FTS remains readable, pause semantic/vector/fuzzy companion reads during `building`, atomically activate route/FTS/marker state, budget rebuild time/WAL/disk, and expose the v3-to-v4 score-baseline change. They require exclusive access because old binaries do not honor the application fence; binary-only rollback retains a flat path but not numerical v3 equivalence, while exact score rollback restores the pre-v4 database checkpoint.
- Control-service and split-worker service definitions, runtime directories, logs, environment variables, and permission boundaries are diagnosable and rollbackable in plan/install/uninstall flows.
- The release workflow or an equivalent gate must run a service lifecycle dry-run smoke so release binaries prove their service definition, rollback plan, and package-manifest checks do not drift from the release tag.

---

Navigation: Previous: [18. Observability, Diagnostics, and SLO](18-observability-diagnostics-and-slo.md) | Next: [20. Multi-Repository Code Graph Overlay](20-multi-repository-code-graph-overlay.md)
