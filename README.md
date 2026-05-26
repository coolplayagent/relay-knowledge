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
binaries under `assets/`; agents prefer the matching bundled asset when it
passes `version --format json`, and use `PATH` only as a fallback, when Linux
glibc is older than 2.31, or when the user explicitly requests the system
install. The generated `SKILL.md` metadata
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
- Local file-location indexing without Everything, Spotlight, Windows Search,
  locate, or other external search software: explicitly scan authorized roots
  and use SQLite/FTS5 to quickly find files by name, path, extension, and
  directory.
- Bounded index refresh queues, persistent leases, retry/dead-letter handling,
  startup reconciliation, stale diagnostics, and scoped cursor metadata.
- Worker queues, deterministic fallback proposals, manual proposal acceptance,
  persistent audit events, silent-update operator state, and service definition
  generation for platform service managers.
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

For unattended code and semantic/vector retrieval optimization experiments, the
independent Rust self-iteration harness under `tools/self_iteration` can be
started directly through the stable launcher:

```bash
./self-iterate.sh
./self-iterate.sh once
./self-iterate.sh loop --strategy unattended-layered
./self-iterate.sh chart
```

The launcher auto-builds the harness binary when needed. It stores v2 run
history, progressive memory, reports, patches, and score curves under
`.git/relay-knowledge-self-iteration/` and only commits candidates that improve
the configured score. The research judge supports OpenAI-compatible HTTP or an
open coding-agent CLI, defaulting to `opencode` when no backend is configured.
The semantic/vector fixture inherits the same `RELAY_KNOWLEDGE_*` embedding
environment as normal runtime commands and does not persist secrets in
benchmark cases. The `unattended-layered` strategy is tuned for 1-2 day runs:
it performs short smoke-level Codex explores, validates promising candidates
with the fast profile, persists resume state in
`.git/relay-knowledge-self-iteration/unattended-state-v2.json`, and escalates
to longer competitive-capability macro exploration when short attempts stall.
External repositories in the self-iteration evaluation set are pinned to
documented commits, and C/C++ adds tree-sitter-oriented generated syntax
fixtures. Multilingual generated fixtures extend the same evaluation set. See
[Book 5, Chapter 6: C/C++ Syntax Self-Iteration Evaluation Set](docs/en/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md)
and [Book 5, Chapter 7: Multilingual Syntax Self-Iteration Evaluation Set](docs/en/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md).

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
The default `fast` profile also includes targeted `code_index_recovery_cases`
so expired code-index leases, stale worker completions, dead-letter recovery,
and checkpoint lease renewal cannot regress without running exhaustive
large-repository workloads.

The binary starts a Tokio runtime, and the shared application service exposes async entrypoints from the CLI boundary inward.
SQLite storage is opened through the storage boundary, and blocking database work is isolated behind Tokio blocking workers.
The storage contract also includes the v1 code graph data surface for tree-sitter output: versioned code files, symbols, references, chunks, and parse-status diagnostics are committed through storage traits rather than direct SQLite access.
Code repository indexing currently parses Rust, Python, JavaScript/JSX, TypeScript/TSX, Go, Java, Kotlin, Scala, C, C++, C#, Ruby, PHP, Swift, and Bash with tree-sitter grammars, falling back to text chunks for unsupported or degraded files. C/C++ macro-heavy files can be conservatively recovered as parsed when errors are isolated to macro expansions, bounded preprocessor directives, or recognized decorator-bearing declarations with declaration-shaped bodies, and reliable structured facts such as symbols, references, or imports are still extracted. Full repository indexing uses resource-bounded SQLite batches with durable checkpoints and a finalize phase for cross-batch references, includes, and call edges, so large scopes expose `indexing` progress without replacing the previous fresh scope until finalization succeeds. A cold full `repo index` queues a durable code-index task and returns a `task` handle immediately; the CLI starts a bounded single-shot worker, while `service run` drains the same queue with one repository index worker and one repository-set overlay refresh worker. `repo status` reports `active_task`, checkpoint counters, and scope retention; successful background tasks retain the active scope, the two latest completed scopes, and unfinished task scopes while pruning older repository scopes. Git branch, tag, and worktree selectors resolve to scoped commit/tree snapshots; indexed scopes remain queryable by explicit ref, rebase or force-moved heads require a new index before query, and same-tree branches reuse the same scope while preserving requested-ref audit metadata. Registering the same repository root with an additional alias preserves prior aliases and resolves all aliases to the same repository id.
Code-index task leases are attempt-scoped: expired running leases are recovered to retry or dead-letter before claim/status paths report them, stale workers cannot complete or fail a reclaimed task, and active workers renew the lease before expensive batch parsing, after each committed checkpoint batch, around finalization, and before task completion. Stores that do not implement the optional lease recovery/renewal hooks keep status and indexing reads compatible by treating those hooks as no-ops. Checkpoints expose `updated_at_ms` in JSON status so operators can distinguish slow progress from a stuck task.
Code graph v1 responses distinguish stable `canonical_symbol_id` values from snapshot-bound `symbol_snapshot_id` values. Reference, call, import, and SBOM dependency hits expose `target_hint`, `resolution_state`, confidence basis points, and confidence tier so unresolved, ambiguous, declared, or locked edges are visible instead of being reported as certain calls.
Repository source scope is not limited to a top-level `src/` layout: real source under `external_deps/`, `packages/`, `modules/`, `plugins/`, `extensions/`, `Sources/`, `lib/`, and nested JVM source roots is indexed by default, while broad `vendor/` and `third_party/` dependency dumps still require explicit path opt-in. Call graph retrieval also resolves static same-repository cross-language edges for C/C++, Go cgo `C.*`, and Rust FFI/bindings paths; this is code-graph evidence, not full build-system or linker analysis.
Code repository lexical retrieval uses a SQLite FTS candidate table for symbols, references, calls, imports, SBOM dependencies, and chunks. Effective path filters are applied inside the FTS candidate window before bounded scoring, graph-edge candidates are ordered by BM25 before truncation, fuzzy symbol recall can match any query term while typed graph edge queries keep their narrower semantics, and Rust scoring recognizes snake_case/CamelCase identifier parts, multi-part symbol names, call-direction context, and declaration-shaped API chunks. `repo query --kind sbom` returns dependency inventory facts extracted during indexing from Cargo, npm, Go, Python, Maven BOM, Gradle, and Conan manifest or lock files; it does not execute package managers, contact registries, or provide vulnerability/license analysis. Call excerpts use a `source_scope + symbol_snapshot_id` chunk lookup and line containment so high fan-out caller/callee queries do not multiply one call edge across unrelated chunks. Code repository queries also use bounded internal exact-text source fallback: AST and indexed lexical layers run first, then the product scans materialized indexed-commit candidate content when definition/reference/hybrid recall has a specific gap or an import points at an unresolved external dependency target that is not indexed as a code graph target. Definition fallback chooses the last identifier-like query target, so command words in natural-language prompts are not searched as symbols. If the FTS read model is unavailable, candidate-path lookup first uses indexed path and chunk terms to keep source fallback query-aware; when no query-aware candidates can be produced, it reports the read-model degradation instead of scanning lexicographic file prefixes. Only source-plannable definition, reference, and single-identifier hybrid queries may return empty indexed results for source fallback; import, symbol, caller, callee, and non-plannable hybrid queries surface the read-model error instead of silently returning false negatives. When earlier lexical layers already produced usable hits, later FTS-layer outages preserve those partial hits and mark them degraded instead of clearing them or hiding the outage. Missing external dependency source is reported as unresolved edge coverage metadata, not as `degraded_reason`. External dependency fallback searches use the unresolved target hint rather than arbitrary user query text, stay below structured import-graph evidence in ranking, and are marked with `text_fallback` so agents treat them as current-repository source evidence, not dependency-library graph evidence. Candidate lookup, candidate-file, materialized-byte, and line-length budget failures degrade only the fallback layer; structured code graph results remain available and report diagnostics. For manual agent or maintainer inspection, prefer `rg`; if it is not installed, use bounded `grep -RIn` searches with VCS and build directories excluded rather than stopping source analysis.
Hybrid retrieval uses SQLite-backed BM25, local semantic token signatures, local hashed-vector ANN, configurable external semantic/vector backend metadata, graph evidence fallback, schema-guided path traversal, temporal event retrieval, community summaries, and code graph documents. It fuses candidates with reciprocal-rank fusion, applies a deterministic local rerank before final truncation, and returns a context pack with retriever sources, ranking and rerank explanations, entities, source spans, structured graph facts, direct graph path evidence, code artifacts, backend availability, freshness, truncation, and budget metadata. The BM25 read model indexes generated lexical aliases for entity labels and code symbols without returning those aliases as canonical labels.
Evidence can carry multimodal extraction metadata for text spans, image assets, OCR text, captions, image embeddings, tables, and layout regions. Derived OCR/caption/image evidence references a parent evidence item, retrieval groups those hits by parent to avoid duplicate context items, and background or maintenance workers commit OCR/caption/table/layout outputs through `commit_multimodal_extraction` rather than query hot paths.
Operational productization persists worker tasks, manual proposals, audit events, and silent-update operator state. Multimodal ingest queues embedding/OCR/vision/extractor work; `worker run-once` calls a configured HTTP endpoint when available or creates a deterministic fallback proposal; `proposal accept` commits through the same graph mutation path; and service manager commands generate platform service definitions without running privileged installation.
The `evaluation` module provides a pure GraphRAG harness plus a CI fixture gate for exact fact, multi-hop, temporal, negative rejection, stale index, ambiguous entity, and code impact observations.
Graph commits also persist Phase 2 index recovery metadata: mutation log entries record affected scopes, entity ids, evidence ids, and source hashes, including scope moves and structured-fact evidence references; scoped index cursors track kind/scope/modality freshness plus source hash, backend cursor, and optional model name/dimension metadata for semantic/vector workers; and `ingest`, `query --freshness wait-until-fresh`, `index refresh`, `health`, and `service doctor` share the bounded refresh queue, active lease/attempt guards, retry/dead-letter, and stale diagnostics path. Diagnostic reconcilers preserve dead-letter isolation, explicit refresh paths surface queue-cap failures instead of reporting false freshness, and `index_refresh.stale_reasons` explains index-family and scoped-cursor lag or failure by kind, scope, modality, lag versions, and last error.

Current CLI commands use the compiled `relay-knowledge` binary with git-style subcommands:

```bash
relay-knowledge status --format json
relay-knowledge help repo query --format json
relay-knowledge ingest --source docs --content "Rust async services isolate blocking SQLite work" --entity Rust
relay-knowledge query SQLite --freshness wait-until-fresh --format json
relay-knowledge repo register /path/to/relay-knowledge --path src --language rust --format json
relay-knowledge repo index relay-knowledge --ref main --format json
relay-knowledge repo update relay-knowledge --base main --head HEAD --format json
relay-knowledge repo query relay-knowledge --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo query relay-knowledge --query serde --kind sbom --ref HEAD --format json
relay-knowledge repo feature-flags relay-knowledge --query checkout --ref HEAD --format json
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace relay-knowledge --ref HEAD --priority 10 --format json
relay-knowledge repo-set remove workspace core --format json
relay-knowledge repo-set query workspace --query retry_policy --kind definition --format json
relay-knowledge repo impact core --base main --head HEAD --format json
relay-knowledge repo status core --format json
relay-knowledge graph inspect --format json
relay-knowledge index refresh --kind bm25 --format json
RELAY_KNOWLEDGE_FILE_INDEX_ROOTS=/opt/docs relay-knowledge files index --root /opt/docs --source local-files --format json
relay-knowledge files query "quarterly design pdf" --source local-files --format json
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
relay-knowledge proposal accept <proposal-id> --by reviewer --reason reviewed
relay-knowledge audit query --limit 50 --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
relay-knowledge service plan install --format json
relay-knowledge service definition write --format json
relay-knowledge service operator pause
relay-knowledge setup doctor --format json
relay-knowledge setup profile agent-readonly --format json
relay-knowledge version check --format json
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs relay-knowledge service run --web --mcp streamable-http
relay-knowledge query --help
relay-knowledge query -- --help
```

CLI parameter meaning is part of the public contract. Skills and other LLM tools
should inspect `relay-knowledge help --format json` before issuing commands; it
describes each command path, operation, read/write effect, required parameters,
defaults, allowed values, repeatability, examples, and notes.
Local file indexing roots must be absolute and present in
`RELAY_KNOWLEDGE_FILE_INDEX_ROOTS`; relative entries are rejected before a
background or explicit scan starts. `RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS`
sets the per-root scan timeout budget.

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
The MCP tool surface includes graph retrieval, graph inspection, health,
service status, index status, authorized code graph queries, and authorized
code impact analysis. MCP does not expose index refresh or repository indexing;
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

Optional local hooks:

```bash
pre-commit install
pre-commit run --all-files
```
