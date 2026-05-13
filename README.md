# relay-knowledge

基于图数据库的知识图谱。

## Documentation

文档按用途归档在 [`docs/`](docs/README.md):

- [使用指南](docs/user-guide/README.md): 按章节拆分的安装、CLI、知识图谱、代码仓库、Web、MCP/Agent 和运维排障说明。
- `docs/research/`: 知识图谱、GraphRAG、代码仓库检索和 arXiv 论文研究总结。
- `docs/specs/`: 能力规格、参考实现分析和后续接口规格。
- [GraphRAG 功能文档](docs/graphrag-capability-guide.md): 当前 evidence ingest、hybrid retrieval、local semantic/vector、schema path、temporal/community、多模态 evidence、代码图、index recovery、Web readiness、MCP/ACP 接入和 freshness/truncation 行为说明。
- [混合检索 Context Pack 功能文档](docs/hybrid-retrieval-context-pack.md): 当前 BM25、semantic/vector、path/temporal/community、RRF 融合、结构化图事实、多模态 grouping、context pack 响应字段、backend 状态和 freshness/truncation 行为说明。
- [代码仓库 Tree-sitter 检索功能文档](docs/code-repository-tree-sitter-retrieval.md): 注册 Git 仓库、tree-sitter 索引、代码图查询、增量更新和影响分析的当前实现说明。

重点架构文档:

- [工程硬约束](docs/specs/engineering-hard-constraints.md): 禁止浅函数、死代码和循环依赖，要求文档完整、文件不超过 1000 行、UT 覆盖率大于 90%，并规定 `env`、`paths`、`net`、事件驱动 HTTP、QoS、UT+集成测试分层与 Playwright Chromium 浏览器集成测试门禁。
- [基础运行时层规格](docs/specs/foundational-runtime.md): `env`、`paths`、`net::http` 和 `net::qos` 的环境变量、路径默认值、网络预算、失败模式和测试策略。
- [GraphRAG 产品与实现路线规格](docs/specs/graphrag-product-and-implementation-roadmap.md): 产品边界、当前实现基线、优化措施、分阶段路线和验收要求。
- [安装部署与发布规格](docs/specs/installation-and-release.md): GitHub Releases、crates.io、包管理器、服务安装、升级卸载和 release CI 的交付要求。
- [统一 API 层与交互层架构](docs/specs/unified-api-and-interface-architecture.md): CLI/Web 收口到统一 API、React/Vite Web 交互层和 `streaming-json` 输出协议。
- [先进架构与可观测性设计](docs/specs/advanced-architecture-observability.md): 本地优先、异步优先、模块解耦和 telemetry 设计。
- [Source Scope 与多模态摄取规格](docs/specs/source-scope-and-multimodal-ingestion.md): Git 分支/rebase 快照隔离、检索 scope 和文档文字/图片多模态 evidence 设计。
- [代码仓库 Tree-sitter 检索规格](docs/specs/code-repository-tree-sitter-retrieval.md): Git 代码仓库基于 tree-sitter 的结构化解析、全量/增量更新、高并发检索、代码图和影响分析设计。
- [开放 Agent Runtime 与混合检索架构](docs/specs/open-agent-runtime-and-hybrid-retrieval-architecture.md): 支持外部 agent runtime 驱动 LLM 知识处理，但 core 不实现 runtime，并定义混合检索、mutation proposal 和 adapter 边界。
- [常驻进程 Agent 图检索访问规格](docs/specs/resident-agent-graph-retrieval-access.md): 常驻进程通过 MCP server 和 Agent Client Protocol adapter 向其它 agent 暴露图检索能力，并统一权限、QoS、新鲜度、审计和测试要求。

## Development

This is a Rust project. Install Rust through `rustup`, then run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
cargo run
cargo run -- --format json
cargo build
target/debug/relay-knowledge version
target/debug/relay-knowledge --version
```

The binary starts a Tokio runtime, and the shared application service exposes async entrypoints from the CLI boundary inward.
SQLite storage is opened through the storage boundary, and blocking database work is isolated behind Tokio blocking workers.
The storage contract also includes the v1 code graph data surface for tree-sitter output: versioned code files, symbols, references, chunks, and parse-status diagnostics are committed through storage traits rather than direct SQLite access.
Code repository indexing currently parses Rust, Python, JavaScript/JSX, TypeScript/TSX, Go, Java, Kotlin, Scala, C, C++, C#, Ruby, PHP, Swift, and Bash with tree-sitter grammars, falling back to text chunks for unsupported or degraded files.
Hybrid retrieval uses SQLite-backed BM25, local semantic token signatures, local hashed-vector ANN, graph evidence fallback, schema-guided path traversal, temporal event retrieval, community summaries, and code graph documents. It fuses candidates with reciprocal-rank fusion and returns a context pack with retriever sources, ranking explanations, entities, source spans, structured graph facts, direct graph path evidence, code artifacts, backend availability, freshness, truncation, and budget metadata. The BM25 read model indexes generated lexical aliases for entity labels and code symbols without returning those aliases as canonical labels.
Evidence can carry multimodal extraction metadata for text spans, image assets, OCR text, captions, image embeddings, tables, and layout regions. Derived OCR/caption/image evidence references a parent evidence item, and retrieval groups those hits by parent to avoid duplicate context items.
The `evaluation` module provides a pure GraphRAG harness for exact fact, multi-hop, temporal, negative rejection, stale index, ambiguous entity, and code impact observations.
Graph commits also persist Phase 2 index recovery metadata: mutation log entries record affected scopes, entity ids, evidence ids, and source hashes, including scope moves and structured-fact evidence references; scoped index cursors track kind/scope/modality freshness plus source hash, backend cursor, and optional model name/dimension metadata for semantic/vector workers; and `ingest`, `query --freshness wait-until-fresh`, `index refresh`, `health`, and `service doctor` share the bounded refresh queue, active lease/attempt guards, retry/dead-letter, and stale diagnostics path. Diagnostic reconcilers preserve dead-letter isolation, explicit refresh paths surface queue-cap failures instead of reporting false freshness, and `index_refresh.stale_reasons` now explains index-family and scoped-cursor lag or failure by kind, scope, modality, lag versions, and last error.

Current CLI commands use the compiled `relay-knowledge` binary with git-style subcommands:

```bash
relay-knowledge status --format json
relay-knowledge ingest --source docs --content "Rust async services isolate blocking SQLite work" --entity Rust
relay-knowledge query SQLite --freshness wait-until-fresh --format json
relay-knowledge repo register /path/to/repo --alias core --path src --language rust --format json
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo query core --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo impact core --base main --head HEAD --format json
relay-knowledge repo status core --format json
relay-knowledge graph inspect --format json
relay-knowledge index refresh --kind bm25 --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs relay-knowledge service run --mcp streamable-http
relay-knowledge query -- --help
```

The CLI ingest command writes evidence plus entity labels. The shared API also
accepts richer Phase 1 graph facts for adapters: evidence `source_path`, source
`span`, confidence, lifecycle status, typed relations, claims, and events that
reference evidence ids. Structured facts must cite supporting evidence, supplied
confidence, span, and version-range fields are revalidated after deserialization,
and retrieval only uses `accepted` or `proposed` evidence as context. Context
pack items now expose direct `graph_paths` derived from those structured facts
so agent callers can cite one-hop relation, claim, or event paths alongside raw
fact provenance.

`service run --mcp streamable-http` starts the resident MCP Streamable HTTP
adapter on the configured local HTTP bind, defaulting to
`http://127.0.0.1:8791/mcp`. It is disabled unless requested by the command or
`RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED=true`; graph tools require
`RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` unless
`RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true` is explicitly configured.
The adapter validates `initialize` params, then issues an unpredictable
`Mcp-Session-Id`. Clients must send `notifications/initialized`, then include
that session header and `MCP-Protocol-Version` on later calls so `ping`, tool
requests and `notifications/cancelled` stay bound to the issued session.
Missing session headers are rejected with HTTP 400; unknown or evicted session
IDs are rejected with HTTP 404.
The MCP tool surface includes graph retrieval, graph inspection, health,
service status, index status, authorized code graph queries, and authorized
code impact analysis. `relay.refresh_indexes` remains hidden unless
`RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH=true` is explicitly configured.
Agent requests write bounded in-process audit events with runtime identity,
scope, freshness, QoS decision, budget, truncation, result count, and status.
The local ACP session adapter exposes the same retrieval contract for
agent-client sessions, including progress updates, cancellation, and context
artifacts. Foreground service startup runs a recovery pass that refreshes stale
index cursors before accepting resident adapter work.

Web diagnostics, operation workspace, and browser integration checks:

```bash
npm install --prefix web
npm run build --prefix web
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

The static Web workspace renders project health, GraphRAG readiness, graph
counts, scoped index freshness, refresh queue diagnostics, stale reasons, runtime budgets, and interactive operation composers
for retrieval, ingestion, graph inspection, code repository workflows, index
refresh, and service runtime commands. The current Web client still reads live
diagnostics only from `/api/project/status` and `/api/health`; operation
composers stage typed command and request previews until a Rust HTTP adapter
exposes executable Web endpoints.

Optional local pre-commit checks:

```bash
pre-commit install
pre-commit run --all-files
```

Setup helpers are also available:

```bash
./setup.sh
```

On Windows, run `setup.bat` from Command Prompt.
