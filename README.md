[English](README.md) | [中文](README.zh-CN.md)

# relay-knowledge

`relay-knowledge` is a local-first knowledge substrate for graph-backed
retrieval. It stores evidence, graph facts, code-repository structure, derived
indexes, freshness state, diagnostics, worker proposals, audit records, and
agent-facing context packs. It does not try to be a general agent runtime or
final-answer generator.

## Quick Start

The default local profile is zero configuration: runtime directories are
resolved from platform defaults, SQLite is used locally, and deterministic
semantic/vector read models are enabled without external services.

```bash
cargo build
target/debug/relay-knowledge status
target/debug/relay-knowledge ingest --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust
target/debug/relay-knowledge query SQLite --source docs \
  --freshness wait-until-fresh
```

Use JSON output when scripting:

```bash
target/debug/relay-knowledge status --format json
target/debug/relay-knowledge health --format json
target/debug/relay-knowledge help --format json
```

## Installing Releases

Stable releases are distributed through GitHub Releases with prebuilt archives
for Linux x64/ARM64, macOS Intel/Apple Silicon, and Windows x64/ARM64. Verify
the downloaded archive with `checksums.txt` before placing the binary on your
PATH. GitHub artifact attestations cover the same archive digests and can be
verified with `gh attestation verify <artifact> -R coolplayagent/relay-knowledge`.
Linux GNU archives are built and checked against a glibc 2.31 baseline so they
run on Ubuntu 20.04-class hosts and newer GNU/Linux distributions.
Windows ARM64 archives are produced by the release workflow as
cross-built artifacts until native Windows ARM64 CI runners are available.

Rust users can install the crate from crates.io:

```bash
cargo install relay-knowledge
relay-knowledge --version
relay-knowledge service doctor
```

Each GitHub Release also includes
`relay-knowledge-cli-skill-<tag>.tar.gz`, a ClawHub-compatible skill that
teaches LLM agents to use the `relay-knowledge` CLI for local graph and
code-repository workflows. The skill package includes Linux x64 and Windows x64
binaries under `assets/`; agents prefer the matching bundled asset for the
current operating system, CPU, and active command runner when it passes
`version --format json`, and use `PATH` only as a fallback, when Linux glibc is
older than 2.31, or when the user explicitly requests the system install.
Windows `.exe` asset examples stay in PowerShell or cmd.exe instructions, not
bash/POSIX command blocks. The generated `SKILL.md` metadata
records the same numeric version as `Cargo.toml`. The skill package also
carries a root-level `README.md` for registry and package consumers. The
release workflow can publish the same generated skill layout to ClawHub when
`CLAWHUB_TOKEN` is configured:

```bash
clawhub publish skills/relay-knowledge-cli \
  --slug relay-knowledge-cli \
  --name "Relay Knowledge CLI" \
  --version <version>
```

This skill-over-CLI path is separate from MCP/ACP protocol access.

### Release Readiness Notes

Before tagging a new release, verify that user-facing entry points, installation
guidance, release constraints, checksums, generated skill metadata, and version
numbers still agree. The release-focused reading path is:

- [Documentation Bookshelf](docs/en/README.md)
- [Installation and Runtime Directories](docs/en/01-user-guide/01-install-and-runtime.md)
- [Installation, Release, and Upgrade](docs/en/03-architecture-specs/19-installation-release-and-upgrade.md)
- [Documentation Release Readiness Audit 2026-06-05](docs/en/06-verification/11-documentation-release-readiness-2026-06-05.md)

This documentation refresh is intentionally documentation-only; it does not
change CLI, service, Web, indexing, retrieval, or release workflow behavior.

## What Works Today

- Hybrid GraphRAG context packs with BM25, local semantic signatures,
  local hashed-vector retrieval, graph evidence fallback, schema paths,
  temporal/community context, freshness metadata, truncation state, and ranking
  explanations.
- Structured graph facts for evidence, entities, typed relations, claims,
  events, source spans, confidence, graph versions, and accepted/proposed
  grounding status.
- Code repository registration, tree-sitter indexing, full and incremental
  refresh, worktree overlay indexing, symbol/reference/chunk retrieval, impact
  analysis, and thin multi-repository `repo-set` overlay queries without
  copying base facts.
- Optional monorepo workspace detection for pnpm workspaces, Go workspaces
  (`go.work`), and Cargo workspace members. When `CodeIndexRequest`
  enables workspace detection, cross-repository import resolution maps
  unresolved imports to sibling packages via a workspace package-mapping
  table, providing `target_hint` metadata instead of silently dropping
  cross-repo references. CLI indexing keeps the default disabled, so
  single-repository indexing paths are completely unaffected.
- Software global projection for repository-scoped files, documentation topics,
  config/code relationships, dependencies, and unresolved SDK/API usage, exposed
  through `repo software` without query-time repository scans.
- Local file-location indexing without Everything, Spotlight, Windows Search,
  locate, or other external search software: explicitly scan authorized roots
  and use SQLite/FTS5 to quickly find files by name, path, extension, and
  directory.
- Bounded index refresh queues, persistent leases, retry/dead-letter handling,
  startup reconciliation, stale diagnostics, and scoped cursor metadata.
- Worker queues, deterministic fallback proposals, manual proposal acceptance,
  persistent audit events, silent-update operator state, and service definition
  generation for platform service managers.
- Service deployment topologies documenting `embedded_cli`,
  `resident_single_process`, `resident_partitioned_sqlite`, and future split
  worker control-plane/data-plane boundaries.
- MCP Streamable HTTP and local ACP adapter access through the shared
  application service, with scope policy, QoS admission, cancellation,
  resources/prompts, durable audit metadata, and OTLP-ready agent metrics.
- Real OTLP HTTP/protobuf traces and metrics export for resident service mode,
  with local diagnostics when Collector export fails.
- Static Web diagnostics, categorized agent/model settings, persisted model
  provider profiles, and operation composers served by the Rust HTTP service on
  the same local port as `/api/*` and MCP when enabled.
- Setup diagnostics and named setup profiles for local, read-only agent,
  platform service, and external embedding configurations.

## Documentation

- [Documentation Bookshelf](docs/en/README.md): book-style entry point for the
  user guide, implemented capabilities, architecture specs, research,
  benchmarks, and verification records.
- [Book 1, Chapter 0: User Guide](docs/en/01-user-guide/README.md): executable local workflows for
  install/runtime directories, CLI output modes, knowledge graphs, code
  repository graphs, Web operations, MCP/ACP access, resident services,
  troubleshooting, and advanced configuration.
- [Book 4, Chapter 1: 2026 Industry Capability Snapshot](docs/en/04-research/01-industry-capability-snapshot-2026.md):
  current GraphRAG, MCP, A2A, hosted retrieval, and graph-agent ecosystem
  signals, plus relay-knowledge gaps.
- [Book 4, Chapter 4: ai-knowledge-graph Reference Analysis](docs/en/04-research/04-ai-knowledge-graph-reference-analysis.md):
  architecture, algorithm, performance, and reliability lessons from an
  external LLM-extracted knowledge graph project.
- [Book 4, Chapter 8: Competitive, High-Performance, and Local File Retrieval Research](docs/en/04-research/08-competitive-performance-research-2026.md):
  broader paper and industry-system references for GraphRAG, hybrid search,
  vector indexes, code search, fast local file retrieval, graph storage, and SRE.
- [Book 4, Chapter 9: GitNexus Feature and UI Implementation Research](docs/en/04-research/09-gitnexus-reference-analysis-2026.md):
  GitNexus CLI/MCP/HTTP backend, code graph, Web graph UI, agent workflows, and
  product improvement points for relay-knowledge.
- [Book 2, Chapter 1: Capability Overview](docs/en/02-capabilities/01-capability-overview.md): foundational behaviors and competitive differentiators.
- [Book 2, Chapter 4: Query and Context Pack Basics](docs/en/02-capabilities/04-query-and-context-pack-basics.md): query metadata, context items, budgets, truncation, and source spans.
- [Book 2, Chapter 5: Hybrid Retrieval Advantage](docs/en/02-capabilities/05-hybrid-retrieval-advantage.md): BM25, semantic, vector, graph evidence, code graph, RRF, and ranking explanations.
- [Book 2, Chapter 9: Code Graph Competitive Features](docs/en/02-capabilities/09-code-graph-competitive-features.md): symbols, references, calls, imports, chunks, identities, and edge diagnostics.
- [Book 2, Chapter 13: Agent Access Capabilities](docs/en/02-capabilities/13-agent-access-capabilities.md): MCP Streamable HTTP, resources, prompts, ACP session access, scope policy, and audit.
- [Appendix A.5: Competitive and High-Performance Benchmark Targets](docs/en/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md):
  metric targets for graph, code, local file, indexing, and worker performance gates.
- [Appendix B.1: Documentation Refresh Audits](docs/en/06-verification/01-documentation-book-refresh-2026-05-17.md):
  dated verification records for documentation freshness and implemented
  capability closures.
- [Appendix B.11: Documentation Release Readiness Audit 2026-06-05](docs/en/06-verification/11-documentation-release-readiness-2026-06-05.md):
  latest release-navigation, inventory, and link-check record for this
  documentation-only refresh.

Key specs:

- [Book 3, Chapter 1: Architecture Vision and Algorithm Map](docs/en/03-architecture-specs/01-architecture-vision-and-algorithm-map.md)
- [Book 3, Chapter 2: Engineering Hard Constraints](docs/en/03-architecture-specs/02-engineering-hard-constraints.md)
- [Book 3, Chapter 9: Hybrid Retrieval and Context Packing](docs/en/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Book 3, Chapter 13: Code Retrieval Ranking and Impact Analysis](docs/en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)
- [Book 3, Chapter 15: Resident Agent Graph Access Protocol](docs/en/03-architecture-specs/15-resident-agent-graph-access-protocol.md)
- [Book 3, Chapter 19: Installation, Release, and Upgrade](docs/en/03-architecture-specs/19-installation-release-and-upgrade.md)
- [Book 3, Chapter 20: Multi-Repository Code Graph Overlay](docs/en/03-architecture-specs/20-multi-repository-code-graph-overlay.md)

## Development

Use the repository scripts by responsibility:

```bash
./setup.sh
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
./check.sh
```

### Self-Iteration Harness

For unattended code and semantic/vector retrieval optimization experiments,
start the independent Rust self-iteration harness documented in
[tools/self_iteration](tools/self_iteration/README.md) through the stable
launcher:

```bash
./self-iterate.sh
./self-iterate.sh once
./self-iterate.sh loop --strategy unattended-layered
./self-iterate.sh chart
```

The launcher auto-builds the harness binary when needed. It stores v2 run
history, progressive memory, reports, patches, and score curves under
`.git/relay-knowledge-self-iteration/` and only commits candidates that improve
the configured score.

The research judge supports OpenAI-compatible HTTP or an open coding-agent CLI,
defaulting to `opencode` when no backend is configured. The semantic/vector
fixture inherits the same `RELAY_KNOWLEDGE_*` embedding environment as normal
runtime commands and does not persist secrets in benchmark cases.

The `unattended-layered` strategy is tuned for 1-2 day runs. It performs short
smoke-level Codex explores, validates promising candidates with the fast
profile, persists resume state in
`.git/relay-knowledge-self-iteration/unattended-state-v2.json`, and escalates to
longer competitive-capability macro exploration when short attempts stall.

External repositories in the self-iteration evaluation set are pinned to
documented commits. C/C++ adds tree-sitter-oriented generated syntax fixtures,
and multilingual generated fixtures extend the same evaluation set. See
[Book 5, Chapter 6: C/C++ Syntax Self-Iteration Evaluation Set](docs/en/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md)
and [Book 5, Chapter 7: Multilingual Syntax Self-Iteration Evaluation Set](docs/en/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md).

### Quality Gates

The underlying quality gates are:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
cargo test --test benchmarks --all-features -- --nocapture
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

The self-iteration harness runs its own product and harness quality checks in
parallel dependency stages and defaults `--jobs auto` to the local CPU count.

The default `fast` profile also includes targeted `code_index_recovery_cases`,
`code_index_sqlite_lock_cases`, and `code_index_health_isolation_cases`, plus a
registration-language guardrail. These keep expired code-index leases, stale
worker completions, dead-letter recovery, checkpoint lease renewal,
duplicate-process SQLite lock avoidance, service health liveness, concurrent
code-index task claiming, committed-scope code queries, and mixed-language
registration safety from regressing without exhaustive large-repository
workloads.

### Runtime and Storage

The binary starts a Tokio runtime, and the shared application service exposes
async entrypoints from the CLI boundary inward. SQLite storage is opened through
the storage boundary, and blocking database work is isolated behind Tokio
blocking workers.

The default storage topology is `single_sqlite`. Set
`RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite` to keep global control
state in the main runtime database while routing repository code facts,
checkpoints, and scoped code queries into per-repository SQLite shard files
under the runtime data directory. Repository-set overlay refresh still requires
`single_sqlite` until cross-shard import/export aggregation is implemented.
After partitioned shard catalog rows become active, startup with
`single_sqlite` fails fast; keep the partitioned topology enabled or perform an
explicit rollback that clears the shard catalog and files.
Shard routes are resolved from the current runtime data directory, so backups
and restores must keep the main database and `stores/repositories/` together.
The control plane continues to own task leases, audit, operator state, topology
catalogs, and diagnostics; data-plane shards only execute reads and writes
authorized and budgeted by shared application services. See
[Service Deployment, Control Plane, and Data Plane](docs/en/03-architecture-specs/22-service-deployment-control-data-plane.md)
for the full contract.

The storage contract includes the v1 code graph data surface for tree-sitter
output. Versioned code files, symbols, references, chunks, and parse-status
diagnostics are committed through storage traits rather than direct SQLite
access.

SQLite writes stay on a single writer lane. Code queries, reports, graph reads,
file queries, and health diagnostics use bounded read-only connections where
SQLite WAL permits concurrent committed-snapshot reads.

After bulk code-index snapshot apply or checkpointed finalize, SQLite storage
runs best-effort `PRAGMA optimize` and `PRAGMA wal_checkpoint(PASSIVE)`.
`health --format json` and graph inspection expose `graph.sqlite` diagnostics
with journal mode, WAL size, last maintenance time, and any maintenance error.
Maintenance timestamps and errors are persisted in SQLite so service restarts
and one-shot worker exits do not erase the last attempt. Under
`partitioned_sqlite`, those fields aggregate the control database and all active
repository shard databases through read-only shard diagnostics. If any active
shard cannot be inspected, the aggregate keeps the shard error and reports an
unknown WAL size instead of presenting a partial total.

Public health is a liveness-safe read. It does not enqueue index refresh work,
and under storage pressure it returns stale/degraded `storage_busy` diagnostics
instead of waiting for indexing to finish.

### Code Indexing

Code repository indexing currently parses Rust, Python, JavaScript/JSX,
TypeScript/TSX, Go, Java, Kotlin, Scala, C, C++, C#, Ruby, PHP, Swift, Bash,
SQL, Markdown, XML, Bazel/Starlark, Make, CMake, Dockerfile/Containerfile, Java
properties, TOML, INI, YAML, JSON, Go module files, Ninja, Jinja2, and Go
templates with tree-sitter grammars. Unsupported or degraded files fall back to
text chunks.

SQL files contribute schema object symbols such as tables, views and
materialized views, functions and procedures, triggers, and types, plus SQL
object references and function/procedure-call edges.

Same-scope local file, template, and build-target references are resolved during
finalize when unambiguous. External or ambiguous configuration relationships
stay unresolved metadata.

Registration rejects language filters so mixed-language repositories keep their
full language surface; use query-time `--language` to narrow results.

C/C++ macro-heavy files can be conservatively recovered as parsed when errors
are isolated to macro expansions, typedef-style external-header declarations
such as Nginx/Kong module tables, GCC/Clang-style declaration attributes and
inline extensions such as `__attribute__((always_inline))`,
`attribute((always_inline))`, and `__always_inline`, bounded preprocessor
directives, or recognized decorator-bearing declarations with
declaration-shaped bodies. Recovery still requires reliable structured facts
such as symbols, references, or imports.

Full Git repository indexing first discovers the tracked source layout. It then
uses resource-bounded SQLite batches with durable checkpoints and a finalize
phase for cross-batch references, includes, and call edges, so large scopes
expose `indexing` progress without replacing the previous fresh scope until
finalization succeeds.

Non-Git batch plans and delta snapshots recheck planned file content hashes
before accepting live bytes, so a live edit cannot be committed under an older
filesystem synthetic snapshot.

Non-Git source directories use filesystem synthetic snapshots. When no path
filter is supplied, they default to source/config/documentation whitelisted
roots and do not walk unrelated directories. Explicit path filters opt into
matching broad build, cache, and dependency directories, and `--path .` opts
into the whole root.

Incremental updates use the same source-layout policy when new files appear
outside `src/`.

A cold full `repo index` queues a durable code-index task and returns a `task`
handle immediately. The CLI starts a bounded single-shot worker,
non-interactive agents can call
`repo index-worker --task-id <id> --format json` for one explicit drain attempt,
and `service run` drains the same queue with a bounded code-index worker pool
controlled by `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` plus one repository-set
overlay refresh worker.

Local CLIs can query a deployed resident service with `--remote <base-url>` or
`RELAY_KNOWLEDGE_REMOTE_BASE_URL`. Remote repository index commands submit
durable tasks to the service and return task/status/checkpoint JSON; the remote
`service run --web` worker pool drains those tasks rather than the local CLI
running `repo index-worker`. Remote read-only repository graph commands
(`repo query`, `repo feature-flags`, `repo impact`, `repo report`, and
`repo software`) read service-host index state and preserve their CLI `--kind`
arguments. Remote maintenance commands such as
`repo index --reset` and `repo index-worker` are rejected by a remote-selected
CLI and must be run on the service host. Remote dispatch validates the remote
URL and outbound network settings before HTTP; unrelated local runtime and
retrieval settings are validated only when a command falls back to local state.

Distinct task fingerprints are queued and leased independently, while identical
full-index fingerprints reuse the active task.

`repo query` and `repo feature-flags` with `allow-stale` continue serving the
latest compatible completed scope when the requested ref and filters are still
being indexed, marking the response stale rather than blocking behind the
writer.

`repo status` reports `active_task`, checkpoint counters, and scope retention.
Successful background tasks retain the active scope, the two latest completed
scopes, and unfinished task scopes while pruning older repository scopes.

Code-index task leases are attempt-scoped. Expired running leases are recovered
to retry or dead-letter before claim/status paths report them, stale workers
cannot complete or fail a reclaimed task, and active workers renew the lease
before expensive batch parsing, after each committed checkpoint batch, around
finalization, and before task completion.

Stores that do not implement the optional lease recovery/renewal hooks keep
status and indexing reads compatible by treating those hooks as no-ops.
Checkpoints expose `updated_at_ms` in JSON status so operators can distinguish
slow progress from a stuck task.

### Source Scopes and Overlays

Repository source scope is not limited to a top-level `src/` layout. Index
planning inspects tracked paths before parsing, so real source under
`external_deps/`, `packages/`, `modules/`, `plugins/`, `extensions/`,
`Sources/`, `lib/`, and nested JVM source roots is indexed by default.

Clean Git snapshots treat the tracked tree as the directory authority inside
the registered and requested path scope. Tracked `.cloudbuild/`, `.cid/`,
`.build_config/`, `build/`, `dist/`, `vendor/`, and `third_party/` paths are
eligible instead of being rejected by name.

Non-Git source directories do not have a tracked tree authority, so the default
scan is whitelist based. Root-level supported source/config/docs files and
source-like roots such as `src/`, `include/`, `lib/`, `Sources/`, `packages/`,
`modules/`, `plugins/`, `extensions/`, `docs/`, and `config/` are eligible,
while `build/`, `dist/`, `target/`, `node_modules/`, `vendor/`, `third_party/`,
cache, virtualenv, and coverage directories require explicit path opt-in.

That opt-in is path-specific: `--path src` must not hash `node_modules/` or
`target/`, while `--path build` or `--path build/generated.rs` opts into the
matching broad directory and `--path .` opts into the whole root.

`--path` is the CLI flag for a path filter. Use it during `repo register` to
store the indexed scope, and during `repo query` or `repo feature-flags` to
narrow reads inside that scope. `repo index` does not accept `--path`; it
indexes the registered scope for the selected `--ref`.

Default non-Git scans descend only into directories that can contribute
whitelist content. Filtered non-Git scans descend only into requested paths and
bounded discoverable source roots, so unrelated siblings such as `private/` are
skipped instead of being read before selector filtering.

Git probe failures on a directory with Git metadata, such as unsafe ownership
or corrupt `.git`, fail loudly instead of falling back to non-Git filesystem
indexing.

A default `--path src` registration is expanded for discovered source roots
during indexing, while precise selector path filters still narrow queries and
avoid widening into unrelated dependency trees.

Non-Git `filesystem:` snapshot ids are computed from the effective indexed
scope after discovery, so unindexed file edits do not move the scoped ref.
Queued synthetic refs, full-index batch reads, and incremental delta bytes are
verified before live bytes are accepted, and incremental deletes still remove
files from previously discovered roots.

Non-Git moving-ref resolution uses the effective path and language filters for
the indexed scope. Non-Git impact path collection uses the same effective
indexed filesystem filters, including explicit broad-directory opt-ins.
For the normal non-Git workflow, register with the desired `--path` filter,
then use `repo index <alias> --ref HEAD` and query `--ref HEAD`; the indexed
commit recorded in status is the resulting `filesystem:<hash>` snapshot.

Git ref normalization for query/status paths uses cheap ref/tree resolution
instead of walking the full tracked tree. Git branch, tag, and worktree
selectors resolve to scoped commit/tree snapshots; indexed scopes remain
queryable by explicit ref, rebase or force-moved heads require a new index
before query, and same-tree branches reuse the same scope while preserving
requested-ref audit metadata.

Worktree overlays use Git status for Git repositories. `.gitignore`-ignored
untracked files are skipped, and untracked broad dependency/cache/build
directories require explicit path opt-in before recursive expansion.

Registering the same repository root with an additional alias preserves prior
aliases and resolves all aliases to the same repository id.
`repo remove <alias>` deletes that repository's runtime registration, aliases,
index scopes, tasks, repository-set membership and overlays, and software
projections without deleting source files on disk; after removal, the same path
or alias can be registered again.

### Code Retrieval

Code graph v1 responses distinguish stable `canonical_symbol_id` values from
snapshot-bound `symbol_snapshot_id` values. Reference, call, import, and SBOM
dependency hits expose `target_hint`, `resolution_state`, confidence basis
points, and confidence tier so unresolved, ambiguous, declared, or locked edges
are visible instead of being reported as certain calls.

Import resolution covers local same-repository imports for every tree-sitter
language listed above, including JavaScript/JSX, Kotlin, Scala, C#, PHP, Rust,
and Swift. Package-manager or SDK imports without authorized indexed source
remain unresolved edge metadata rather than parser degradation.

Call graph retrieval resolves static same-repository cross-language edges for
C/C++, Go cgo `C.*`, and Rust FFI/bindings paths. This is code-graph evidence,
not full build-system or linker analysis.

Code repository lexical retrieval uses a SQLite FTS candidate table for
symbols, references, calls, imports, SBOM dependencies, and chunks. Effective
path filters are applied inside the FTS candidate window before bounded
scoring, graph-edge candidates are ordered by BM25 before truncation, fuzzy
symbol recall can match any query term while typed graph edge queries keep their
narrower semantics, and Rust scoring recognizes snake_case/CamelCase identifier
parts, multi-part symbol names, call-direction context, and declaration-shaped
API chunks.

`repo query --kind sbom` returns dependency inventory facts extracted during
indexing from Cargo, npm, Go, Python, Maven effective `pom.xml`/BOM, Gradle,
and Conan manifest or lock files. It does not execute package managers, contact
registries, or provide vulnerability/license analysis.

Maven effective POM handling resolves repository-local parent POMs, properties,
dependency management, profiles, plugin management, modules, and imported BOM
declarations from indexed evidence only.

Call excerpts use a `source_scope + symbol_snapshot_id` chunk lookup and line
containment so high fan-out caller/callee queries do not multiply one call edge
across unrelated chunks.

Code repository queries also use bounded internal exact-text source fallback.
AST and indexed lexical layers run first; then the product scans materialized
indexed-commit candidate content when definition/reference/hybrid recall has a
specific gap or an import points at an unresolved external dependency target
that is not indexed as a code graph target.

For non-Git filesystem commits, fallback first verifies that the current
synthetic snapshot still equals the indexed `filesystem:` commit. If it has
moved, fallback reports degradation instead of reading live files from a
different snapshot.

Definition fallback chooses the last identifier-like query target, so command
words in natural-language prompts are not searched as symbols.

If the FTS read model is unavailable, candidate-path lookup first uses indexed
path and chunk terms to keep source fallback query-aware. When no query-aware
candidates can be produced, it reports the read-model degradation instead of
scanning lexicographic file prefixes.

Only source-plannable definition, reference, and single-identifier hybrid
queries may return empty indexed results for source fallback. Import, symbol,
caller, callee, and non-plannable hybrid queries surface the read-model error
instead of silently returning false negatives.

When earlier lexical layers already produced usable hits, later FTS-layer
outages preserve those partial hits and mark them degraded instead of clearing
them or hiding the outage.

Missing external dependency source is reported as unresolved edge coverage
metadata, not as `degraded_reason`. External dependency fallback searches use
the unresolved target hint rather than arbitrary user query text, stay below
structured import-graph evidence in ranking, and are marked with
`text_fallback` so agents treat them as current-repository source evidence, not
dependency-library graph evidence.

Candidate lookup, candidate-file, materialized-byte, and line-length budget
failures degrade only the fallback layer. Structured code graph results remain
available and report diagnostics.

For manual agent or maintainer inspection, prefer `rg`. If it is not installed,
use bounded `grep -RIn` searches with VCS and build directories excluded rather
than stopping source analysis.

### GraphRAG, Workers, and Recovery

Hybrid retrieval uses SQLite-backed BM25, local semantic token signatures,
local hashed-vector ANN, configurable external semantic/vector backend metadata,
graph evidence fallback, schema-guided path traversal, temporal event
retrieval, community summaries, and code graph documents.

It fuses candidates with reciprocal-rank fusion, applies a deterministic local
rerank before final truncation, and returns a context pack with retriever
sources, ranking and rerank explanations, entities, source spans, structured
graph facts, direct graph path evidence, code artifacts, backend availability,
freshness, truncation, and budget metadata. The BM25 read model indexes
generated lexical aliases for entity labels and code symbols without returning
those aliases as canonical labels.

Evidence can carry multimodal extraction metadata for text spans, image assets,
OCR text, captions, image embeddings, tables, and layout regions. Derived
OCR/caption/image evidence references a parent evidence item, retrieval groups
those hits by parent to avoid duplicate context items, and background or
maintenance workers commit OCR/caption/table/layout outputs through
`commit_multimodal_extraction` rather than query hot paths.

Operational productization persists worker tasks, manual proposals, audit
events, and silent-update operator state. Multimodal ingest queues
embedding/OCR/vision/extractor work; `worker run-once` calls a configured HTTP
endpoint when available or creates a deterministic fallback proposal;
`proposal accept` commits through the same graph mutation path; and service
manager commands now expose staged install, upgrade, rollback, and uninstall
lifecycle plans. Dry-run is the default; explicit `service lifecycle ... --execute`
runs local file steps and platform service-manager commands with rollback steps
if a later stage fails, and failed executions return an operation error with the
failed step id instead of a successful response.

The `evaluation` module provides a pure GraphRAG harness plus a CI fixture gate
for exact fact, multi-hop, temporal, negative rejection, stale index, ambiguous
entity, and code impact observations.

Graph commits also persist Phase 2 index recovery metadata. Mutation log
entries record affected scopes, entity ids, evidence ids, and source hashes,
including scope moves and structured-fact evidence references. Scoped index
cursors track kind/scope/modality freshness plus source hash, backend cursor,
and optional model name/dimension metadata for semantic/vector workers.

`ingest`, `query --freshness wait-until-fresh`, `index refresh`, `health`, and
`service doctor` share the bounded refresh queue, active lease/attempt guards,
retry/dead-letter, and stale diagnostics path. Diagnostic reconcilers preserve
dead-letter isolation, explicit refresh paths surface queue-cap failures instead
of reporting false freshness, and `index_refresh.stale_reasons` explains
index-family and scoped-cursor lag or failure by kind, scope, modality, lag
versions, and last error.

### CLI Contract

Current CLI commands use the compiled `relay-knowledge` binary with git-style
subcommands:

```bash
relay-knowledge status --format json
relay-knowledge help repo query --format json
relay-knowledge ingest --source docs --content "Rust async services isolate blocking SQLite work" --entity Rust
relay-knowledge query SQLite --freshness wait-until-fresh --format json
relay-knowledge repo register /path/to/relay-knowledge --path src --format json
relay-knowledge repo index relay-knowledge --ref main --format json
relay-knowledge repo index-worker --task-id <task-id> --format json
relay-knowledge repo update relay-knowledge --base main --head HEAD --format json
relay-knowledge repo query relay-knowledge --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge --remote http://127.0.0.1:8791 repo query relay-knowledge --query retry_policy --kind definition --freshness wait-until-fresh --format json
relay-knowledge --remote http://127.0.0.1:8791 repo software relay-knowledge --kind relationships --ref HEAD --format json
relay-knowledge repo query relay-knowledge --query serde --kind sbom --ref HEAD --format json
relay-knowledge repo feature-flags relay-knowledge --query checkout --ref HEAD --format json
relay-knowledge repo software relay-knowledge --kind relationships --ref HEAD --format json
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace relay-knowledge --ref HEAD --priority 10 --format json
relay-knowledge repo-set remove workspace relay-knowledge --format json
relay-knowledge repo-set query workspace --query retry_policy --kind definition --format json
relay-knowledge repo impact relay-knowledge --base main --head HEAD --format json
relay-knowledge repo status relay-knowledge --format json
relay-knowledge graph inspect --format json
relay-knowledge index refresh --kind bm25 --format json
RELAY_KNOWLEDGE_FILE_INDEX_ROOTS=/opt/docs relay-knowledge files index --root /opt/docs --source local-files --format json
relay-knowledge files query "quarterly design pdf" --source local-files --freshness wait-until-fresh --format json
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
relay-knowledge proposal accept <proposal-id> --by reviewer --reason reviewed
relay-knowledge audit query --limit 50 --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
relay-knowledge service plan install --format json
relay-knowledge service plan upgrade --target-version 1.2.3 --format json
relay-knowledge service lifecycle install --dry-run --format json
relay-knowledge service definition write --format json
relay-knowledge service operator pause
relay-knowledge setup doctor --format json
relay-knowledge setup profile agent-readonly --format json
relay-knowledge version check --format json
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs relay-knowledge service run --web --mcp streamable-http
relay-knowledge query --help
relay-knowledge query -- --help
```

`repo-set refresh` rebuilds cross-repository import overlay edges from the
indexed member snapshots. The overlay understands Go workspace/module manifests
(`go.work`, `go.mod`) and pnpm workspaces (`pnpm-workspace.yaml` plus package
`package.json` names, entry points, and exports). Nested `go.work` files only
scope `go.mod` filtering to their own directory tree, pnpm package globs only
match paths under the workspace root, and package keys are parsed from complete
workspace/package manifest content retained during indexing. pnpm root packages
are always included, a workspace without `packages` includes only the root
package, and `exports` takes precedence over `main`/`module` entry aliases.
Declared package `exports` entries also bind package subpath keys: conditional export
objects select a single preferred runtime target, wildcard subpath exports map
matching file patterns, and private files outside declared exports do not receive
synthetic package subpath aliases.
Imports that still cannot be matched to a member package are retained as
`unresolved` cross edges with target hint evidence.

#### Kind Reference

`--kind` values are command-local. Do not reuse a value from one command family
in another command just because the flag name is the same:

- `repo query --kind` and `repo-set query --kind` select code retrieval intent:
  `hybrid`, `symbol`, `definition`, `references`, `callers`, `callees`,
  `imports`, or `sbom`. Use `repo impact` for impact analysis and
  `repo feature-flags` for feature flags instead of inventing query kinds.
- `repo software --kind` selects repository-wide software graph slices:
  `dependencies`, `sdks`, `files`, `topics`, `relationships`, `build`, `iac`,
  `design`, or `all`.
- `index refresh --kind` selects derived retrieval index families: `bm25`,
  `semantic`, or `vector`. Omitting `--kind` requests every supported index
  family.
- `worker status|run-once --kind` selects background worker families:
  `embedding`, `ocr`, `vision`, or `extractor`.
- `map source add|update --kind` labels knowledge-map source categories:
  `repo`, `file`, `doc`, `config`, `db`, `ci`, `runtime`, `wiki`, or
  `monitoring`.

Knowledge-map commands that read or write `.knowledge/knowledge-map.yaml`
discover the repository root from the process start directory. Discovery walks
up to the first `.git` or `.knowledge` marker and falls back to the nearest
`AGENTS.md` for compatibility. If no repository marker is found, the command
fails with a stable error instead of writing runtime state into the current
directory. `map agent-snippet` does not require repository-root discovery.

CLI parameter meaning is part of the public contract. Skills and other LLM tools
should inspect `relay-knowledge help --format json` before issuing commands. It
describes each command path, operation, read/write effect, required parameters,
defaults, allowed values, repeatability, examples, and notes.

Local file indexing roots must be absolute and present in
`RELAY_KNOWLEDGE_FILE_INDEX_ROOTS`; relative entries are rejected before a
background or explicit scan starts. `RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS`
sets the per-root scan timeout budget. `files query --format json` returns a
top-level `freshness` object with root cursors, index lag, stale/degraded
reasons, bounded-rescan state, and direct-source-read instructions. Use
`--freshness wait-until-fresh` to suppress pending, degraded, or overflowed
file-index answers until a bounded scan has completed.

### File Watcher (fs.watch)

The resident `service run` process starts the file watcher for registered code
repositories and pushes source-code changes into the durable code-index task
queue automatically. It is enabled by default on supported platforms.

```bash
RELAY_KNOWLEDGE_WATCHER_ENABLED=true
RELAY_KNOWLEDGE_WATCHER_DEBOUNCE_MS=3000
RELAY_KNOWLEDGE_WATCHER_MAX_WATCH_DIRS=1024
RELAY_KNOWLEDGE_WATCHER_HASH_CACHE_CAPACITY=4096
```

The watcher uses the `notify` crate for cross-platform file system events
(Linux inotify, macOS FSEvents, Windows ReadDirectoryChangesW). Events are
debounced, content-hash filtered, and path-filtered before generating
`WorktreeOverlay` task payloads that existing code-index workers claim through
leases, retry, and dead-letter handling. Watcher diagnostics (state, watched
repository count, event/drop counts, queued task count, degraded reason) appear
in `service status --format json`.

### Semantic and Vector Backends

Semantic/vector read-model backend metadata is configured only through the
`env` boundary. The default mode is local deterministic read models; external
worker metadata can be selected with:

```bash
RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external
RELAY_KNOWLEDGE_VECTOR_BACKEND=external
RELAY_KNOWLEDGE_LLM_PROVIDER=openai_compatible
RELAY_KNOWLEDGE_EMBEDDING_BASE_URL=https://api.example.com/v1
RELAY_KNOWLEDGE_EMBEDDING_API_KEY=...
RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL=text-embed-3-small
RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL=clip-vit-b32
RELAY_KNOWLEDGE_EMBEDDING_DIMENSION=1536
```

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` and
`RELAY_KNOWLEDGE_VECTOR_BACKEND` also accept `local` and `disabled`. Disabled
read-model backends are excluded from semantic/vector retrieval execution and
refresh scheduling; blank embedding model names fail during runtime
configuration.

### Settings and Graph Facts

The Web Settings page groups agent interoperability, retrieval defaults, and
model providers. Agent/retrieval settings read the same redacted runtime and
service diagnostics to prepare MCP exposure, scope policy, audit, and external
model environment variables, including the configured MCP origin allow-list.

Model provider settings manage named chat/completion profiles, fallback
policies, catalog refresh from `models.dev`, endpoint probes, and model
discovery through `/api/configs/model/*`. Profile and fallback files live under
the resolved config directory as `model-profiles.json` and
`model-fallback.json`; the public catalog cache lives under the resolved cache
directory as `model-catalog-cache.json`. Secret values are accepted only on save
and are returned to the browser as configured booleans or redacted headers.

Profile updates preserve redacted stored header secrets unless a replacement
value is supplied, and API callers can set `clear_api_key=true` to explicitly
remove a stored API key during header-only migrations.

The CLI ingest command writes evidence plus entity labels. The shared API also
accepts richer Phase 1 graph facts for adapters: evidence `source_path`, source
`span`, confidence, lifecycle status, typed relations, claims, and events that
reference evidence ids. Structured facts must cite supporting evidence, supplied
confidence, span, and version-range fields are revalidated after deserialization,
and retrieval only uses `accepted` or `proposed` evidence as context. Context
pack items now expose direct `graph_paths` derived from those structured facts
so agent callers can cite one-hop relation, claim, or event paths alongside raw
fact provenance.

### Web, MCP, and ACP

`service run --web --mcp streamable-http` starts the same-port Web diagnostics,
`/api/*`, and resident MCP Streamable HTTP adapters on the configured local HTTP
bind, defaulting to `http://127.0.0.1:8791/` and
`http://127.0.0.1:8791/mcp`. MCP is disabled unless requested by the command or
`RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED=true`; graph tools require
`RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` unless
`RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true` is explicitly configured or
the requested scope matches a code repository alias already registered in this
runtime. Registered repository aliases are promoted into a process-local MCP
allow-list on first use; unknown scopes are still rejected with the missing
scope and the exact `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=<scope>` repair hint.

The adapter validates `initialize` params, then issues an unpredictable
`Mcp-Session-Id`. Clients must send `notifications/initialized`, then include
that session header and `MCP-Protocol-Version` on later calls so `ping`, tool
requests and `notifications/cancelled` stay bound to the issued session.
Missing session headers are rejected with HTTP 400; unknown or evicted session
IDs are rejected with HTTP 404.

MCP discovery is storage-cold: `initialize`, `notifications/initialized`, and
`tools/list` register and return static schemas without opening SQLite. Storage
opens lazily on the first storage-backed tool call, and the first `tools/list`
per session records an initialize-to-tools-list cold-start metric. Code query
tools return an `explore_budget` based on indexed file count, cap oversized
result sets for agent context, reject free-text queries over 10,000 characters
and path filters over 4,096 characters, and return compact outlines for
container types when `include_code=true`.

The MCP tool surface includes graph retrieval, graph inspection, health,
service status, index status, authorized code graph queries, authorized
software global-model queries, repository-set code graph queries, and
authorized code impact analysis. Agent-facing kind selection reuses existing
product kinds: `relay_code_query` handles code graph kinds,
`relay_software_query` handles software global-model kinds, and
`relay_code_feature_flags` handles configuration-driven feature flags. Common
agent aliases such as `dependency`, `configuration`, and `models` normalize to
the existing `dependencies`, `relationships`, and `design` kinds instead of
creating duplicate kinds. MCP does not expose index refresh or repository indexing;
run `relay-knowledge repo index`, `relay-knowledge repo update`, or
`relay-knowledge index refresh` from an explicit CLI/Web workflow before MCP
queries depend on fresh indexes.

The MCP server also advertises resources and prompts: resources expose service
status, health, index status, and Prometheus text metrics; the graph-wide
summary resource is advertised only when
`RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true`. Prompts provide retrieval
and code-impact planning templates. `/mcp/metrics` exports a
small Prometheus-compatible snapshot for graph version, index refresh backlog,
dead letters, QoS request counts, and per-index stale state.

Agent requests write bounded in-process audit events with runtime identity,
scope, freshness, QoS decision, budget, truncation, result count, and status.
Set `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true` to mirror those events to
the path-owned JSONL file `logs/agent-audit.jsonl`; the sink uses a bounded
async queue controlled by `RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH` and capped
at 65536 entries.

The local ACP session adapter exposes the same retrieval contract for
agent-client sessions, including progress updates, cancellation, and context
artifacts. Foreground service startup runs a recovery pass that refreshes stale
index cursors before accepting resident adapter work.

### Browser Checks

Web diagnostics, operation workspace, and browser integration checks:

```bash
./build.sh
./run.sh start --port 8791 --daemon
curl http://127.0.0.1:8791/api/health
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

The static Web workspace renders project health, GraphRAG readiness, graph
counts, a compact SVG graph overview for evidence/code/index/worker topology,
the interactive Graph canvas, scoped index freshness, refresh queue diagnostics,
stale reasons, runtime budgets, and interactive operation composers for
retrieval, ingestion, graph inspection, code repository workflows, index refresh,
provider probes, worker/proposal/audit operations, service runtime commands,
agent interoperability settings, retrieval defaults, and model provider profile
management. The same Rust HTTP service serves static Web assets plus
`/api/project/status`, `/api/health`, `/api/service/status`, and
`/api/web/operations/execute` on one local port. The execute endpoint accepts
the current composer snapshot, calls the shared application service, and returns
operation metadata plus result JSON for the page to display. Web `service run`
returns a service runtime snapshot rather than starting a resident loop from the
browser. Web execute requests are bounded by
`RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES`, and non-loopback HTTP binds require the
remote-client access policy to be enabled explicitly.

### Optional Hooks

Optional local hooks:

```bash
pre-commit install
pre-commit run --all-files
```
