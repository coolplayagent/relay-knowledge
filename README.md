[English](README.md) | [中文](README.zh-CN.md)

# relay-knowledge

`relay-knowledge` is a local-first knowledge substrate for graph-backed
retrieval. It stores evidence, graph facts, code-repository structure, derived
indexes, freshness state, diagnostics, audit records, and agent-facing context
packs. It is not a general agent runtime or a final-answer generator.

## Quick Start

The default local profile needs no external service: platform defaults select
the runtime directories, SQLite stores local state, and deterministic local
semantic/vector read models are enabled.

```bash
cargo build
target/debug/relay-knowledge status
target/debug/relay-knowledge ingest --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust
target/debug/relay-knowledge query SQLite --source docs \
  --freshness wait-until-fresh
```

Use JSON for scripts and agent integrations:

```bash
target/debug/relay-knowledge status --format json
target/debug/relay-knowledge health --format json
target/debug/relay-knowledge help --format json
```

## Installing Releases

[GitHub Releases](https://github.com/coolplayagent/relay-knowledge/releases)
provide prebuilt archives for Linux x64/ARM64, macOS Intel/Apple Silicon, and
Windows x64/ARM64. Verify the selected archive with `checksums.txt` before
putting the binary on `PATH`; GitHub artifact attestations cover the same
archive digests. Linux GNU archives target a glibc 2.31 baseline.

Rust users can install from crates.io:

```bash
cargo install relay-knowledge
relay-knowledge --version
relay-knowledge service doctor
```

Each release also publishes `relay-knowledge-cli-skill-<tag>.tar.gz` for agents
that use the CLI instead of MCP/ACP. See the
[CLI skill package](skills/relay-knowledge-cli/README.md) and the
[installation, release, and upgrade contract](docs/en/03-architecture-specs/19-installation-release-and-upgrade.md)
for platform details, verification, service installation, upgrade, rollback,
and uninstall behavior.

## Capability Snapshot

- Hybrid GraphRAG context packs combine BM25, local or external semantic/vector
  retrieval, graph evidence, freshness, bounded context, and ranking
  explanations.
- Structured evidence, entities, relations, claims, events, source spans,
  confidence, graph versions, and accepted/proposed grounding remain
  traceable.
- Repository workflows cover registration, tree-sitter indexing, full and
  incremental refresh, worktree overlays, symbols, references, calls, imports,
  context, impact, feature flags, SBOM evidence, and multi-repository sets.
- Durable bounded queues, leases, checkpoints, backpressure, recovery, and
  observable maintenance protect long-running indexing and background work.
- Software-wide projections and authorized local-file indexing expose
  dependencies, SDKs, files, topics, build/IaC/design evidence, and
  relationships without query-time repository scans.
- CLI, Web, MCP Streamable HTTP, and local ACP modes share the same application
  behavior, scope policy, QoS, cancellation, audit, and diagnostics.

Detailed behavior, limits, and implementation ownership belong in the linked
responsibility-specific documentation, not in this navigation page.

## Documentation

| Area | Entry point |
| --- | --- |
| Complete bookshelf | [English documentation](docs/en/README.md) |
| User workflows | [User Guide](docs/en/01-user-guide/README.md) |
| Implemented behavior | [Capabilities](docs/en/02-capabilities/README.md) |
| Architecture contracts | [Architecture Specifications](docs/en/03-architecture-specs/README.md) |
| Mandatory engineering rules | [Engineering Hard Constraints](docs/en/03-architecture-specs/02-engineering-hard-constraints.md) |
| Research and external evidence | [Research](docs/en/04-research/README.md) |
| Performance and self-iteration contracts | [Benchmarks](docs/en/05-benchmarks/README.md) |
| Auditable verification records | [Verification](docs/en/06-verification/README.md) |

Two development-loop chapters have distinct responsibilities:

- [Chapter 24: Code-Map-Backed Knowledge Development Loop](docs/en/03-architecture-specs/24-code-map-backed-knowledge-development-loop.md)
  is the executable operating contract.
- [Chapter 26: Git Commit + Knowledge Development Philosophy and Iteration Loop](docs/en/03-architecture-specs/26-git-commit-knowledge-development-loop.md)
  explains the commit fact boundary, derived knowledge, decision context,
  recovery model, and human-agent handoff philosophy.

## Essential CLI Workflows

The machine-readable help surface is the command contract:

```bash
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

Create and query knowledge:

```bash
relay-knowledge ingest --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust
relay-knowledge query SQLite --freshness wait-until-fresh --format json
relay-knowledge graph inspect --format json
```

Register, index, and query a code repository:

```bash
relay-knowledge repo register /path/to/repository --path src --format json
relay-knowledge repo index repository --ref HEAD --format json
relay-knowledge repo status repository --format json
relay-knowledge repo query repository --query retry_policy \
  --kind definition --ref HEAD --path src --freshness wait-until-fresh \
  --limit 10 --format json
relay-knowledge repo software repository --kind relationships \
  --ref HEAD --format json
```

Indexing returns a durable task and makes progress observable through
`repo status`. If a one-shot CLI cannot finish a large cold index before the
caller times out, inspect status and use the bounded task worker or managed
service recovery path documented in
[Code Repository Graph Workflow](docs/en/01-user-guide/05-code-repository-graph-workflow.md).
Do not start unmanaged loops or competing writers.

Query a resident service without opening unrelated local state:

```bash
relay-knowledge --remote http://127.0.0.1:8791 \
  repo query repository --query retry_policy --kind definition \
  --freshness wait-until-fresh --format json
```

The full grammar, command-local `--kind` values, JSON schemas, read/write
effects, and environment precedence are in the
[CLI Command Reference](docs/en/01-user-guide/03-cli-command-reference.md).

## Resident Service and Agent Access

Start the shared Web/API service and opt into MCP Streamable HTTP:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --web --mcp streamable-http
```

The default Web endpoint is `http://127.0.0.1:8791/`; the MCP endpoint is
`http://127.0.0.1:8791/mcp`. MCP is disabled unless requested, and graph tools
require an allowed scope or an explicitly registered repository alias.

See [Web Workspace](docs/en/01-user-guide/06-web-workspace.md),
[MCP and Agent Access](docs/en/01-user-guide/07-mcp-agent-access.md), and
[Resident Service](docs/en/01-user-guide/09-resident-service.md) for session,
authorization, cancellation, audit, service-manager, and diagnostics guidance.

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

The principal local quality gates are:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
python3 tools/docs/check_docs.py --self-test-and-check
```

Architecture boundaries, async and resource-budget requirements, unit-test
coverage, documentation completeness, and the requirement that hand-written
files stay below 1,000 lines are mandatory in
[Engineering Hard Constraints](docs/en/03-architecture-specs/02-engineering-hard-constraints.md).

### Self-Iteration Harness

The independent Rust harness for retrieval and indexing optimization is
documented in [tools/self_iteration](tools/self_iteration/README.md):

```bash
./self-iterate.sh
./self-iterate.sh once
./self-iterate.sh loop --strategy unattended-layered
./self-iterate.sh chart
```

The default `fast` profile builds and evaluates the release product binary with
focused gates and workload guardrails. Use
`./self-iterate.sh once --profile full` for the complete rails and workloads.
Run history, reports, patches, and resume state stay under
`.git/relay-knowledge-self-iteration/`. The harness documentation also records
the exact pinned commits and reproducible detached-checkout preparation for
external repositories.

### Browser Checks

```bash
./build.sh
./run.sh start --port 8791 --daemon
curl http://127.0.0.1:8791/api/health
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

Runtime data, configuration, indexes, logs, and caches belong in the documented
platform directories, not in the repository. Do not commit secrets, local
databases, private datasets, or generated build output. See
[Installation and Runtime Directories](docs/en/01-user-guide/01-install-and-runtime.md).

Optional local hooks: `pre-commit install` and
`pre-commit run --all-files`.
