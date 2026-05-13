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
```

## What Works Today

- Hybrid GraphRAG context packs with BM25, local semantic signatures,
  local hashed-vector retrieval, graph evidence fallback, schema paths,
  temporal/community context, freshness metadata, truncation state, and ranking
  explanations.
- Structured graph facts for evidence, entities, typed relations, claims,
  events, source spans, confidence, graph versions, and accepted/proposed
  grounding status.
- Code repository registration, tree-sitter indexing, full and incremental
  refresh, worktree overlay indexing, symbol/reference/chunk retrieval, and
  impact analysis.
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
- Static Web diagnostics and operation composers served by the Rust HTTP
  service on the same local port as `/api/*` and MCP when enabled.

## Documentation

- [使用指南](docs/user-guide/README.md): default local usage first, then CLI,
  knowledge graph, code repository, Web, agent service, troubleshooting, and
  advanced configuration.
- [2026 行业能力快照](docs/research/industry-capability-snapshot-2026.md):
  current GraphRAG, MCP, A2A, hosted retrieval, and graph-agent ecosystem
  signals, plus relay-knowledge gaps.
- [GraphRAG 功能文档](docs/graphrag-capability-guide.md): current context-pack,
  freshness, backend, multimodal, code graph, recovery, Web, MCP, and ACP
  behavior.
- [混合检索 Context Pack 功能文档](docs/hybrid-retrieval-context-pack.md):
  retriever sources, RRF fusion, structured graph facts, graph paths, and
  backend status.
- [Semantic/Vector Provider Backend](docs/semantic-vector-provider-backend.md):
  external embedding provider setup, redacted diagnostics, Web provider panels,
  and degradation behavior.
- [代码仓库 Tree-sitter 检索功能文档](docs/code-repository-tree-sitter-retrieval.md):
  repository indexing, retrieval, reports, and impact analysis.

Key specs:

- [工程硬约束](docs/specs/engineering-hard-constraints.md)
- [GraphRAG 产品与实现路线规格](docs/specs/graphrag-product-and-implementation-roadmap.md)
- [开放 Agent Runtime 与混合检索架构](docs/specs/open-agent-runtime-and-hybrid-retrieval-architecture.md)
- [Semantic/Vector Provider Backend 规格](docs/specs/semantic-vector-provider-backend.md)
- [常驻进程 Agent 图检索访问规格](docs/specs/resident-agent-graph-retrieval-access.md)
- [安装部署与发布规格](docs/specs/installation-and-release.md)

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

The underlying quality gates are:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

Web diagnostics and browser integration checks:

```bash
./build.sh
./run.sh start --port 8791 --daemon
curl http://127.0.0.1:8791/api/health
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

Optional local hooks:

```bash
pre-commit install
pre-commit run --all-files
```
