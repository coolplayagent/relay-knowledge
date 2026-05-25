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

稳定版本通过 GitHub Releases 发布，包含 Linux x64/ARM64、macOS Intel/Apple Silicon、Windows x64/ARM64 的预构建压缩包。下载后先用 `checksums.txt` 校验，再将二进制文件放入 `PATH`。Linux GNU 压缩包以 glibc 2.31 为 ABI baseline 构建和检查，可运行在 Ubuntu 20.04 同级或更新的 GNU/Linux 发行版上。在原生 Windows ARM64 CI runner 可用之前，Windows ARM64 压缩包由 release workflow 交叉构建生成。

Rust 用户也可以从 crates.io 安装：

```bash
cargo install relay-knowledge
relay-knowledge --version
relay-knowledge service doctor
```

每个 GitHub Release 还会包含
`relay-knowledge-cli-skill-<tag>.tar.gz`，这是一个兼容 ClawHub
的 skill，用于引导 LLM agent 通过 `relay-knowledge` CLI 使用本地图谱和代码仓库工作流。skill
包会在 `assets/` 下内置 Linux x64 和 Windows x64 二进制；当匹配平台的内置二进制通过
`version --format json` 校验时，agent 会优先使用它，只有内置二进制不可用、Linux glibc
低于 2.31，或用户明确要求系统安装版本时才回退到 `PATH`。生成后的 `SKILL.md` metadata 会记录与 `Cargo.toml`
相同的数字版本。配置 `CLAWHUB_TOKEN` 后，release workflow 可以把同一个生成后的 skill
布局发布到 ClawHub。skill 包还会携带根目录 `README.md`，供 registry 和包使用者查看：

```bash
clawhub publish skills/relay-knowledge-cli \
  --slug relay-knowledge-cli \
  --name "Relay Knowledge CLI" \
  --version <version>
```

这条 skill-over-CLI 路径与 MCP/ACP 协议接入是分离的。

## 当前能力

- 混合 GraphRAG 上下文包：包含 BM25、本地语义签名、本地哈希向量检索、图证据回退、schema 路径、时间/社区上下文、新鲜度元数据、截断状态和排序解释。
- 结构化图事实：支持证据、实体、类型化关系、声明、事件、来源范围、置信度、图版本，以及已接受/提议的定位状态。
- 代码仓库能力：支持仓库注册、tree-sitter 索引、全量和增量刷新、工作树覆盖索引、符号/引用/代码块检索、影响分析，以及不复制基础事实的多仓库 `repo-set` 薄覆盖查询。
- 本地文件定位索引：不依赖 Everything 等外部检索软件，显式扫描授权 roots，并用 SQLite/FTS5 快速按文件名、路径、扩展名和目录定位文件。
- 有界索引刷新队列：支持持久租约、重试/死信、启动调和、过期诊断和作用域游标元数据。
- 运维工作流：支持 worker 队列、确定性回退提案、人工提案接受、持久审计事件、静默更新操作员状态，以及平台服务管理器的服务定义生成。
- Agent 接入：通过共享应用服务暴露 MCP Streamable HTTP 和本地 ACP 适配器，并带有作用域策略、QoS 准入、取消、资源/提示、持久审计元数据和 OTLP 准备的 agent 指标。
- 可观测性：常驻服务模式支持真实 OTLP HTTP/protobuf 跟踪和指标导出；Collector 导出失败时提供本地诊断。
- Web 工作区：Rust HTTP 服务可在同一端口提供静态 Web 诊断、分类后的 agent/model 设置、持久化模型 provider profile、操作组合器、`/api/*` 和可选 MCP 端点。
- 设置诊断：提供 local、只读 agent、平台服务、外部嵌入等命名设置配置文件。

## 文档

- [文档书架](docs/zh/README.md)：用户手册、已实现能力、架构规格、研究资料、基准记录和验证记录的书籍式入口。
- [第一卷第 0 章：使用指南](docs/zh/01-user-guide/README.md)：安装与运行时目录、CLI 输出模式、GraphRAG、代码仓库索引/报告、Web 操作、MCP/ACP service 接入、排障和高级配置。
- [第四卷第 1 章：2026 行业能力快照](docs/zh/04-research/01-industry-capability-snapshot-2026.md)：当前 GraphRAG、MCP、A2A、托管检索和图 agent 生态信号，以及 relay-knowledge 的差距。
- [第四卷第 4 章：ai-knowledge-graph 参考项目分析](docs/zh/04-research/04-ai-knowledge-graph-reference-analysis.md)：对外部 LLM 抽取型知识图谱项目的架构、算法、性能和可靠性借鉴分析。
- [第四卷第 8 章：竞争力、高性能与本机文件检索研究](docs/zh/04-research/08-competitive-performance-research-2026.md)：GraphRAG、混合搜索、向量索引、代码搜索、本机文件检索、图存储和 SRE 的系统参考。
- [第四卷第 9 章：GitNexus 功能与界面实现研究](docs/zh/04-research/09-gitnexus-reference-analysis-2026.md)：GitNexus CLI/MCP/HTTP 后端、代码图谱、Web 图谱界面、Agent 工作流和后续改进点。
- [第二卷第 1 章：能力版图总览](docs/zh/02-capabilities/01-capability-overview.md)：基础功能与竞争力特性的阅读导览。
- [第二卷第 4 章：查询与 Context Pack 基础](docs/zh/02-capabilities/04-query-and-context-pack-basics.md)：查询元数据、上下文项、预算、截断和来源范围。
- [第二卷第 5 章：混合检索竞争力](docs/zh/02-capabilities/05-hybrid-retrieval-advantage.md)：BM25、semantic、vector、图证据、代码图、RRF 和排序解释。
- [第二卷第 9 章：代码图竞争力特性](docs/zh/02-capabilities/09-code-graph-competitive-features.md)：符号、引用、调用、导入、代码块、身份和边诊断。
- [第二卷第 13 章：Agent 接入能力](docs/zh/02-capabilities/13-agent-access-capabilities.md)：MCP Streamable HTTP、资源、提示、ACP session、作用域策略和审计。
- [附录 B.1：文档刷新审计](docs/zh/06-verification/01-documentation-book-refresh-2026-05-17.md)：文档新鲜度和已实现能力关闭状态的带日期验证记录。

关键规格：

- [第三卷第 1 章：架构愿景与算法版图](docs/zh/03-architecture-specs/01-architecture-vision-and-algorithm-map.md)
- [第三卷第 2 章：工程硬约束](docs/zh/03-architecture-specs/02-engineering-hard-constraints.md)
- [第三卷第 9 章：混合检索与 Context Packing](docs/zh/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [第三卷第 13 章：代码检索排序与影响分析](docs/zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)
- [第三卷第 15 章：常驻 Agent 图访问协议](docs/zh/03-architecture-specs/15-resident-agent-graph-access-protocol.md)
- [第三卷第 19 章：安装、发布与升级](docs/zh/03-architecture-specs/19-installation-release-and-upgrade.md)
- [第三卷第 20 章：多仓库代码图谱薄覆盖层](docs/zh/03-architecture-specs/20-multi-repository-code-graph-overlay.md)

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

面向代码检索和 semantic/vector 检索优化实验的 `tools/self_iteration` 独立 Rust
harness 可以通过稳定启动脚本直接运行：

```bash
./self-iterate.sh
./self-iterate.sh once
./self-iterate.sh chart
```

启动脚本会在需要时自动构建 debug harness binary，默认 `fast` profile 不跑产品 release build、全量 clippy、全量 test、文件 fixture、semantic/vector fixture 或 research judge，并保留一个轻量 repo-set 跨仓门槛护栏；需要完整门禁和 workload 时使用 `./self-iterate.sh once --profile full`。v2 运行历史、渐进式记忆、报告、patch 和评分曲线保存在
`.git/relay-knowledge-self-iteration/`，只有评分严格改进的候选修改才会被提交。research judge 支持 OpenAI-compatible HTTP 或开放 coding-agent CLI；未配置 backend 时默认使用 `opencode`。semantic/vector fixture 会继承普通运行时使用的
`RELAY_KNOWLEDGE_*` embedding 环境变量，不会把 provider URL、API key、模型名或维度写入 benchmark cases。
自迭代测评集中的外部仓库已固定到文档记录的 commit，C/C++ 还包含基于 tree-sitter 语法能力生成的专用 fixture；复现清单见
[第五卷第 6 章：C/C++ 语法型自迭代测评集](docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md)
和 [第五卷第 7 章：多语言语法型自迭代测评集](docs/zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md)。

底层质量门禁：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
cargo test --test benchmarks --all-features -- --nocapture
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

自迭代 harness 默认只执行轻量 fast 门禁；完整 profile 的产品与 harness 质量检查会按依赖阶段并行执行，`--jobs auto` 默认使用本机 CPU 数。

二进制启动 Tokio 运行时；从 CLI 边界向内，所有核心能力均通过共享应用服务的异步入口暴露。SQLite 存储通过存储边界打开，阻塞数据库操作被隔离到 Tokio 阻塞工作线程中。

存储契约包含 v1 代码图数据面：tree-sitter 输出的版本化代码文件、符号、引用、代码块和解析状态诊断均通过存储 trait 提交，而非直接访问 SQLite。当前代码仓库索引支持 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift 和 Bash；不支持或降级的文件会回退为文本代码块。C/C++ 宏密集文件如果 error node 局限在宏、预处理器或已识别 decorator 声明区域，decorator 类型体仍保持声明形态，并且仍能抽取可靠结构化事实，会被保守恢复为 parsed。代码仓库 full index 使用受资源预算约束的 SQLite 批次和持久 checkpoint；大 scope 索引过程中 `repo status` 会显示 `indexing` 和已提交计数，旧的 fresh scope 在 finalize 成功前继续服务查询，finalize 阶段再基于同一 scope 的完整已落库事实解析跨 batch reference、include 和 call edge。冷启动 full `repo index` 会落持久化 code-index task 并立即返回 `task` handle；CLI 会启动有界单次 worker，`service run` 则用同一队列上的单个仓库索引 worker 消费任务。`repo status` 会报告 `active_task`、checkpoint 计数和 scope retention；后台任务成功后保留 active scope、最近两个完成 scope 以及未完成任务 scope，并淘汰更旧的仓库 scope。Git 分支、标签和工作树选择器会解析为带作用域的提交/树快照；已索引作用域可按显式引用查询；rebase 或强制移动的 HEAD 需要重新索引；相同树的分支会复用同一作用域，同时保留请求引用的审计元数据。

代码图 v1 响应区分稳定的 `canonical_symbol_id` 和快照绑定的 `symbol_snapshot_id`。引用、调用和导入命中会暴露 `target_hint`、`resolution_state`、置信度基点和置信度等级，避免将未解析或有歧义的边误报为确定调用。

代码仓库 source scope 不再局限于顶层 `src/` 布局：`external_deps/`、`packages/`、`modules/`、`plugins/`、`extensions/`、`Sources/`、`lib/` 和嵌套 JVM source root 下的真实源码会默认纳入索引，普通 `vendor/` 和 `third_party/` 这类大容量依赖转储仍需要显式 path opt-in。调用图检索也支持同仓静态跨语言边：C/C++ 互调、Go cgo `C.*` 和 Rust FFI/bindings 路径可解析为代码图证据，但这不等同于完整 build-system 或 linker 分析。

代码仓库词法检索使用 SQLite FTS 候选表覆盖 symbol、reference、call、import 和 chunk。有效 path filter 会在 FTS 候选窗口内先过滤再进入有界评分；graph edge 候选在截断前按 BM25 排序；fuzzy symbol 召回可以命中任一查询词，而 typed graph edge 查询保持更窄语义；Rust 评分会识别 snake_case/CamelCase identifier 片段、多段符号名、调用方向上下文和声明形态 API chunk。Call excerpt 通过 `source_scope + symbol_snapshot_id` chunk lookup 与调用行包含条件定位，避免高 fan-out caller/callee 查询把一条 call edge 放大成多个无关 chunk 候选。代码仓库查询还会使用可选 ripgrep 兜底恢复精确源码文本：AST 和已索引词法层先执行，当 definition/reference/hybrid 存在具体召回缺口，或 import 指向未作为代码图 target 索引的 unresolved external dependency 时，再用有界 `rg` 搜索已索引 commit 内容。Definition 兜底会选择最后一个 identifier-like 查询目标，因此自然语言提示里的命令词不会被当成 symbol 搜索。如果 FTS read model 不可用，候选文件路径会先使用已索引 path 和 chunk 词项保持源码兜底 query-aware；如果无法产生 query-aware 候选，则暴露 read-model 降级，而不是扫描按字典序截断的文件前缀。只有可规划源码兜底的 definition、reference 和单 identifier hybrid 查询可以把索引结果视为空；import、symbol、caller、callee 以及不可规划的 hybrid 查询会暴露 read-model 错误，不能静默返回假阴性空结果。当前面的词法层已经产生可用命中时，后续 FTS 层 outage 会保留这些部分命中，而不是清空结果。外部依赖源码缺失会作为 unresolved edge coverage metadata 暴露，不写入 `degraded_reason`。外部依赖兜底使用 unresolved target hint 而非任意用户查询文本，排序低于结构化 import-graph 证据，并标记 `text_fallback` 与诊断，提醒 agent 这是当前仓库源码证据，不是依赖库图谱证据。`rg` 缺失或超时只降级兜底层；结构化代码图结果仍可用，并会返回诊断信息。人工 agent 或维护者检查源码时优先使用 `rg`；如果本机未安装，可用排除 VCS 和 build 目录的有界 `grep -RIn` 继续搜索，不能因为缺少 ripgrep 就停止源码分析。

混合检索使用基于 SQLite 的 BM25、本地语义令牌签名、本地哈希向量近似最近邻、可配置的外部语义/向量后端元数据、图证据回退、schema 指导路径遍历、时间事件检索、社区摘要和代码图文档。候选结果先通过互惠排名融合，再在最终截断前执行确定性本地 rerank，最终返回包含检索器来源、排序和 rerank 解释、实体、来源范围、结构化图事实、直接图路径证据、代码工件、后端可用性、新鲜度、截断和预算元数据的上下文包。BM25 读模型会为实体标签和代码符号索引生成词汇别名，但不会将这些别名作为规范标签返回。

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
relay-knowledge repo feature-flags core --query checkout --ref HEAD --format json
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace core --ref HEAD --priority 10 --format json
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

CLI 参数含义是公开契约的一部分。Skills 和其它 LLM 工具在发出命令前应先读取 `relay-knowledge help --format json`；该输出会描述每条 command path、operation、读写影响、必填参数、默认值、允许值、可重复性、示例和注意事项。
本地文件索引 root 必须是绝对路径，并且必须出现在
`RELAY_KNOWLEDGE_FILE_INDEX_ROOTS` 中；`RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS`
用于设置每个 root 的扫描 timeout 预算。

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

Web Settings 页面按 agent 互操作性、检索默认值和模型 provider 分类展示。Agent/检索设置会读取同一套脱敏 runtime 与 service diagnostics，用于生成 MCP 暴露、origin allow-list、作用域策略、审计和外部模型相关环境变量。模型 provider 设置通过 `/api/configs/model/*` 管理命名 chat/completion profile、fallback policy、`models.dev` catalog 刷新、endpoint probe 和模型发现。Profile 与 fallback 文件位于解析后的配置目录，文件名为 `model-profiles.json` 和 `model-fallback.json`；公共 catalog cache 位于解析后的缓存目录，文件名为 `model-catalog-cache.json`。Secret 只在保存时接收，回传给浏览器时只显示 configured boolean 或脱敏 header；更新 profile 时会保留已脱敏的 header secret，API 调用方可设置 `clear_api_key=true` 显式清除已保存的 API key，便于迁移到 header-only 认证。

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

静态 Web 工作区会渲染项目健康状况、GraphRAG 准备度、图计数、用于 evidence/code/index/worker 拓扑的紧凑 SVG graph overview、交互式 Graph canvas、作用域索引新鲜度、刷新队列诊断、过期原因、运行时预算，以及检索、摄取、图检查、代码仓库图谱工作流、索引刷新、提供者探测、worker/提案/审计操作、服务运行时命令、agent 互操作性设置、检索默认值和模型 provider profile 管理的交互式工作区。同一个 Rust HTTP 服务会在本地端口提供静态 Web 资源，以及 `/api/project/status`、`/api/health`、`/api/service/status` 和 `/api/web/operations/execute`。execute 端点接收当前编排器快照，调用共享应用服务，并返回操作元数据和结果 JSON 供页面展示。Web `service run` 只返回服务运行时快照，不从浏览器启动常驻循环。Web execute 请求受 `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` 限制；非 loopback HTTP 绑定必须显式启用远程客户端访问策略。

可选本地 hooks：

```bash
pre-commit install
pre-commit run --all-files
```
