# relay-knowledge

基于图数据库的知识图谱。

## Documentation

文档按用途归档在 [`docs/`](docs/README.md):

- `docs/research/`: 知识图谱、GraphRAG、代码仓库检索和 arXiv 论文研究总结。
- `docs/specs/`: 能力规格、参考实现分析和后续接口规格。
- [混合检索 Context Pack 功能文档](docs/hybrid-retrieval-context-pack.md): 当前 BM25 read model、RRF 融合、结构化图事实、context pack 响应字段和 freshness/truncation 行为说明。
- [代码仓库 Tree-sitter 检索功能文档](docs/code-repository-tree-sitter-retrieval.md): 注册 Git 仓库、tree-sitter 索引、代码图查询、增量更新和影响分析的当前实现说明。

重点架构文档:

- [工程硬约束](docs/specs/engineering-hard-constraints.md): 禁止浅函数、死代码和循环依赖，要求文档完整、文件不超过 1000 行、UT 覆盖率大于 90%，并规定 `env`、`paths`、`net`、事件驱动 HTTP、QoS、UT+集成测试分层与 Playwright Chromium 浏览器集成测试门禁。
- [基础运行时层规格](docs/specs/foundational-runtime.md): `env`、`paths`、`net::http` 和 `net::qos` 的环境变量、路径默认值、网络预算、失败模式和测试策略。
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
Hybrid retrieval uses the SQLite-backed BM25 read model plus graph evidence fallback, fuses candidates with reciprocal-rank fusion, and returns a context pack with retriever sources, ranking explanations, freshness, truncation, and budget metadata.

Current CLI commands use the compiled `relay-knowledge` binary with git-style subcommands:

```bash
relay-knowledge status --format json
relay-knowledge ingest --source docs --content "Rust async services isolate blocking SQLite work" --entity Rust
relay-knowledge query SQLite --freshness wait-until-fresh --format json
relay-knowledge repo register /path/to/repo --alias core --path src --language rust --format json
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo query core --query retry_policy --kind definition --format json
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

Web diagnostics, operation workspace, and browser integration checks:

```bash
npm install --prefix web
npm run build --prefix web
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

The static Web workspace renders project health, graph counts, index freshness,
runtime budgets, and interactive operation composers for retrieval, ingestion,
graph inspection, code repository workflows, index refresh, and service runtime
commands. The current Web client still reads live diagnostics only from
`/api/project/status` and `/api/health`; operation composers stage typed command
and request previews until a Rust HTTP adapter exposes executable Web endpoints.

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
