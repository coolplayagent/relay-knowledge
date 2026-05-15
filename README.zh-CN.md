[English](README.md) | [中文](README.zh-CN.md)

# relay-knowledge

`relay-knowledge` 是一个本地优先、基于图数据库能力的知识检索底座。它负责存储证据（evidence）、图事实、代码仓库结构、派生索引、新鲜度状态、诊断信息、worker 提案、审计记录，以及面向 agent 的上下文包（context pack）。它不是通用 agent 运行时，也不负责生成最终答案。

## 快速开始

默认本地配置不需要额外设置：运行时目录按平台默认位置解析，本地使用 SQLite，并启用确定性的本地 semantic/vector 读模型，不依赖外部服务。

```bash
cargo build
target/debug/relay-knowledge status
target/debug/relay-knowledge ingest --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust
target/debug/relay-knowledge query SQLite --source docs \
  --freshness wait-until-fresh
```

脚本集成时优先使用 JSON 输出：

```bash
target/debug/relay-knowledge status --format json
target/debug/relay-knowledge health --format json
target/debug/relay-knowledge help --format json
```

## 安装发布版

稳定版本通过 GitHub Releases 发布，包含 Linux x64/ARM64、macOS Intel/Apple Silicon、Windows x64/ARM64 的预构建压缩包。下载后先用 `checksums.txt` 校验，再将二进制文件放入 `PATH`。在原生 Windows ARM64 CI runner 可用之前，Windows ARM64 压缩包由 release workflow 交叉构建生成。

Rust 用户也可以从 crates.io 安装：

```bash
cargo install relay-knowledge
relay-knowledge --version
relay-knowledge service doctor
```

## 当前能力

- 混合 GraphRAG 上下文包：包含 BM25、本地语义签名、本地哈希向量检索、图证据回退、schema 路径、时间/社区上下文、新鲜度元数据、截断状态和排序解释。
- 结构化图事实：支持证据、实体、类型化关系、声明、事件、来源范围、置信度、图版本，以及已接受/提议的定位状态。
- 代码仓库能力：支持仓库注册、tree-sitter 索引、全量和增量刷新、工作树覆盖索引、符号/引用/代码块检索和影响分析。
- 有界索引刷新队列：支持持久租约、重试/死信、启动调和、过期诊断和作用域游标元数据。
- 运维工作流：支持 worker 队列、确定性回退提案、人工提案接受、持久审计事件、静默更新操作员状态，以及平台服务管理器的服务定义生成。
- Agent 接入：通过共享应用服务暴露 MCP Streamable HTTP 和本地 ACP 适配器，并带有作用域策略、QoS 准入、取消、资源/提示、持久审计元数据和 OTLP 准备的 agent 指标。
- 可观测性：常驻服务模式支持真实 OTLP HTTP/protobuf 跟踪和指标导出；Collector 导出失败时提供本地诊断。
- Web 工作区：Rust HTTP 服务可在同一端口提供静态 Web 诊断、操作组合器、`/api/*` 和可选 MCP 端点。
- 设置诊断：提供 local、只读 agent、平台服务、外部嵌入等命名设置配置文件。

## 文档

- [使用指南](docs/zh/01-user-guide/README.md)：安装与运行时目录、CLI 输出模式、GraphRAG、代码仓库索引/报告、Web 操作、MCP/ACP service 接入、排障和高级配置。
- [2026 行业能力快照](docs/zh/04-research/industry-capability-snapshot-2026.md)：当前 GraphRAG、MCP、A2A、托管检索和图 agent 生态信号，以及 relay-knowledge 的差距。
- [GraphRAG 功能文档](docs/zh/02-capabilities/graphrag-capability-guide.md)：context pack、新鲜度、backend、多模态、代码图、恢复、Web、MCP 和 ACP 行为。
- [混合检索上下文包](docs/zh/02-capabilities/hybrid-retrieval-context-pack.md)：检索器来源、RRF 融合、结构化图事实、图路径和后端状态。
- [Semantic/Vector Provider 后端](docs/zh/02-capabilities/semantic-vector-provider-backend.md)：外部 embedding provider 配置、脱敏诊断、Web provider 面板和降级行为。
- [代码仓库 Tree-sitter 检索](docs/zh/02-capabilities/code-repository-tree-sitter-retrieval.md)：仓库索引、检索、报告和影响分析。
- [文档刷新审计 2026-05-14](docs/zh/02-capabilities/documentation-refresh-audit-2026-05-14.md)：当前文档状态、已刷新内容和剩余实现工作。

关键规格：

- [工程硬约束](docs/zh/03-architecture-specs/engineering-hard-constraints.md)
- [GraphRAG 产品与实现路线规格](docs/zh/03-architecture-specs/graphrag-product-and-implementation-roadmap.md)
- [开放 Agent Runtime 与混合检索架构](docs/zh/03-architecture-specs/open-agent-runtime-and-hybrid-retrieval-architecture.md)
- [Semantic/Vector Provider Backend 规格](docs/zh/03-architecture-specs/semantic-vector-provider-backend.md)
- [常驻进程 Agent 图检索访问规格](docs/zh/03-architecture-specs/resident-agent-graph-retrieval-access.md)
- [安装部署与发布规格](docs/zh/03-architecture-specs/installation-and-release.md)

## 开发

按职责使用仓库脚本：

```bash
./setup.sh
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
./check.sh
```

底层质量门禁：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
cargo test --test benchmarks --all-features -- --nocapture
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

二进制启动 Tokio 运行时；从 CLI 边界向内，所有核心能力均通过共享应用服务的异步入口暴露。SQLite 存储通过存储边界打开，阻塞数据库操作被隔离到 Tokio 阻塞工作线程中。

存储契约包含 v1 代码图数据面：tree-sitter 输出的版本化代码文件、符号、引用、代码块和解析状态诊断均通过存储 trait 提交，而非直接访问 SQLite。当前代码仓库索引支持 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift 和 Bash；不支持或降级的文件会回退为文本代码块。Git 分支、标签和工作树选择器会解析为带作用域的提交/树快照；已索引作用域可按显式引用查询；rebase 或强制移动的 HEAD 需要重新索引；相同树的分支会复用同一作用域，同时保留请求引用的审计元数据。

代码图 v1 响应区分稳定的 `canonical_symbol_id` 和快照绑定的 `symbol_snapshot_id`。引用、调用和导入命中会暴露 `target_hint`、`resolution_state`、置信度基点和置信度等级，避免将未解析或有歧义的边误报为确定调用。

混合检索使用基于 SQLite 的 BM25、本地语义令牌签名、本地哈希向量近似最近邻、可配置的外部语义/向量后端元数据、图证据回退、schema 指导路径遍历、时间事件检索、社区摘要和代码图文档。候选结果通过互惠排名融合，最终返回包含检索器来源、排序解释、实体、来源范围、结构化图事实、直接图路径证据、代码工件、后端可用性、新鲜度、截断和预算元数据的上下文包。BM25 读模型会为实体标签和代码符号索引生成词汇别名，但不会将这些别名作为规范标签返回。

证据可携带多模态提取元数据，包括文本范围、图像资源、OCR 文本、标题、图像嵌入、表格和布局区域。派生的 OCR/标题/图像证据会引用父证据项；检索时按父项聚合这些命中，避免重复上下文项；后台或维护 worker 必须通过 `commit_multimodal_extraction` 提交 OCR/标题/表格/布局输出，不能阻塞查询热路径。

运维产品化能力会持久化 worker 任务、人工提案、审计事件和静默更新操作员状态。多模态摄取会排队 embedding/OCR/视觉/提取器工作；`worker run-once` 在配置 HTTP 端点时调用远端 worker，否则创建确定性回退提案；`proposal accept` 通过同一图变更路径提交；服务管理器命令仅生成平台服务定义，不执行特权安装。

`evaluation` 模块提供纯 GraphRAG 测试框架和 CI 夹具门控，覆盖精确事实、多跳、时间、负面拒绝、过期索引、歧义实体和代码影响观察。

图提交还会持久化第二阶段索引恢复元数据：变更日志条目记录受影响作用域、实体 ID、证据 ID 和来源哈希，包括作用域移动和结构化事实证据引用；作用域索引游标跟踪种类/作用域/模态新鲜度、来源哈希、后端游标，以及语义/向量 worker 可选的模型名称/维度元数据。`ingest`、`query --freshness wait-until-fresh`、`index refresh`、`health` 和 `service doctor` 共享有界刷新队列、活动租约/尝试保护、重试/死信和过期诊断路径。

当前 CLI 使用编译后的 `relay-knowledge` 二进制和 git 风格子命令：

```bash
relay-knowledge status --format json
relay-knowledge help repo query --format json
relay-knowledge ingest --source docs --content "Rust async services isolate blocking SQLite work" --entity Rust
relay-knowledge query SQLite --freshness wait-until-fresh --format json
relay-knowledge repo register /path/to/repo --alias core --path src --language rust --format json
relay-knowledge repo index core --ref main --format json
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo query core --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo impact core --base main --head HEAD --format json
relay-knowledge repo status core --format json
relay-knowledge graph inspect --format json
relay-knowledge index refresh --kind bm25 --format json
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
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs relay-knowledge service run --web --mcp streamable-http
relay-knowledge query --help
relay-knowledge query -- --help
```

CLI 参数含义是公开契约的一部分。Skills 和其它 LLM 工具在发出命令前应先读取 `relay-knowledge help --format json`；该输出会描述每条 command path、operation、读写影响、必填参数、默认值、允许值、可重复性、示例和注意事项。

Semantic/vector 读模型 backend 元数据只能通过 `env` 边界配置。默认模式是本地确定性读模型；可以用以下变量选择外部 worker metadata：

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

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` 和 `RELAY_KNOWLEDGE_VECTOR_BACKEND` 也接受 `local` 与 `disabled`。禁用的 read-model backend 不参与 semantic/vector 检索执行和刷新调度；空 embedding model name 会在运行时配置阶段失败。

CLI `ingest` 命令会写入 evidence 和 entity label。共享 API 还接受面向 adapter 的更丰富 Phase 1 graph fact：evidence `source_path`、source `span`、confidence、lifecycle status、类型化 relation、claim，以及引用 evidence id 的 event。结构化事实必须引用 supporting evidence；反序列化后会重新校验 supplied confidence、span 和 version-range 字段；检索只使用 `accepted` 或 `proposed` evidence 作为上下文。Context pack item 现在会暴露从这些结构化事实派生的直接 `graph_paths`，方便 agent caller 在 raw fact provenance 旁边引用一跳 relation、claim 或 event path。

`service run --web --mcp streamable-http` 会在同一端口启动 Web 诊断、`/api/*` 和常驻 MCP Streamable HTTP adapter。默认绑定为 `http://127.0.0.1:8791/` 和 `http://127.0.0.1:8791/mcp`。除非通过命令或 `RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED=true` 显式启用，MCP 默认关闭；graph tool 需要 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES`，除非显式配置 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true`。

Adapter 会校验 `initialize` 参数，然后签发不可预测的 `Mcp-Session-Id`。客户端必须发送 `notifications/initialized`，之后调用需要携带该 session header 和 `MCP-Protocol-Version`，确保 `ping`、工具请求和 `notifications/cancelled` 绑定到已签发的 session。缺少 session header 会返回 HTTP 400；未知或已驱逐的 session ID 会返回 HTTP 404。

MCP 工具界面包含图检索、图检查、健康状况、服务状态、索引状态、授权代码图查询和授权代码影响分析。MCP 不暴露 index refresh 或 repository indexing；仓库索引需要用户主动运行 `relay-knowledge repo index` 或 `relay-knowledge repo update`。MCP 服务器也会发布资源和提示：资源暴露服务状态、健康状况、索引状态和 Prometheus 文本指标；只有在 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true` 时才发布全图摘要资源。提示提供检索和代码影响规划模板。`/mcp/metrics` 暴露 Prometheus 文本指标；MCP 客户端只使用原生 Streamable HTTP `/mcp` 入口。

Agent 请求会写入有界进程内审计事件，包含运行时身份、作用域、新鲜度、QoS 决策、预算、截断、结果数和状态。设置 `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true` 后，这些事件会镜像到由 `paths` 管理的 JSONL 文件 `logs/agent-audit.jsonl`；sink 使用由 `RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH` 控制的有界异步队列，最多允许 65536 条。

本地 ACP session adapter 通过 agent-client session 暴露同一检索契约，包括进度更新、取消和上下文工件。前台服务启动时会先执行恢复流程，刷新过期的索引游标，然后再接受常驻 adapter 工作。

Web diagnostics、operation workspace 和浏览器集成检查：

```bash
./build.sh
./run.sh start --port 8791 --daemon
curl http://127.0.0.1:8791/api/health
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

静态 Web 工作区会渲染项目健康状况、GraphRAG 准备度、图计数、用于 evidence/code/index/worker 拓扑的紧凑 SVG graph overview、交互式 Graph canvas、作用域索引新鲜度、刷新队列诊断、过期原因、运行时预算，以及检索、摄取、图检查、代码仓库工作流、索引刷新、提供者探测、worker/提案/审计操作和服务运行时命令的交互式操作编排器。同一个 Rust HTTP 服务会在本地端口提供静态 Web 资源，以及 `/api/project/status`、`/api/health`、`/api/service/status` 和 `/api/web/operations/execute`。execute 端点接收当前编排器快照，调用共享应用服务，并返回操作元数据和结果 JSON 供页面展示。Web `service run` 只返回服务运行时快照，不从浏览器启动常驻循环。Web execute 请求受 `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` 限制；非 loopback HTTP 绑定必须显式启用远程客户端访问策略。

可选本地 hooks：

```bash
pre-commit install
pre-commit run --all-files
```
