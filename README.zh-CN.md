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

### 发版准备说明

打新 release tag 前，先确认用户入口、安装说明、发布约束、checksum、生成后的
skill metadata 和版本号仍然一致。发版相关阅读路径如下：

- [文档书架](docs/zh/README.md)
- [安装与运行时目录](docs/zh/01-user-guide/01-install-and-runtime.md)
- [安装、发布与升级](docs/zh/03-architecture-specs/19-installation-release-and-upgrade.md)
- [文档发版准备审计 2026-06-05](docs/zh/06-verification/11-documentation-release-readiness-2026-06-05.md)

本轮文档刷新保持 documentation-only，不改变 CLI、service、Web、索引、检索或 release
workflow 行为。

## 当前能力

- 混合 GraphRAG 上下文包：包含 BM25、本地语义签名、本地哈希向量检索、图证据回退、schema 路径、时间/社区上下文、新鲜度元数据、截断状态和排序解释。
- 结构化图事实：支持证据、实体、类型化关系、声明、事件、来源范围、置信度、图版本，以及已接受/提议的定位状态；`domain/graph/{multimodal,mutation,retrieval}/` 让每组 contract 与直接 UT 保持同域。
- 代码仓库能力：支持仓库注册、tree-sitter 索引、全量和增量刷新、工作树覆盖索引、符号/引用/代码块检索、影响分析，以及不复制基础事实的多仓库 `repo-set` 薄覆盖查询；应用层把 query、context、freshness、scope、impact 与 software projection 收敛到具名物理子域，repo-set 的 membership、member freshness、refresh、status 与 query 也分别归物理 owner；`interfaces/code_index_mode/` 在 CLI/Web 间共享 worktree 语义，content/language/generated 原语、符号/引用身份与源码快照、校验式 blob 读取、安全文件系统访问和哈希分别归带直接 UT 的物理 owner。
- 可选大仓工作区检测：支持 pnpm workspace、Go workspace（`go.work`）和 Cargo workspace 成员检测。当 `CodeIndexRequest` 显式启用 workspace detection 时，跨仓库导入解析会通过工作区包映射表将未解析的导入映射到兄弟包，提供 `target_hint` 元数据，而非静默丢弃跨仓库引用。CLI 索引保持默认关闭，因此单仓库索引路径完全不受影响。生态、workspace format、manifest、package prefix 与 import statement 归一化规则归 `code::workspace::ecosystem`；自动 set 生命周期/状态、package mapping、cross-edge resolution 与 target-file selection 分属带配对 UT 的 owner，workspace facade 只协调无环持久化工作流。
- 软件全域投影：按 repository scope 暴露文件整体节点、文档主题、配置/代码关系、依赖和 unresolved SDK/API 使用，`repo software` 读取投影表而不是查询时扫描仓库。knowledge-map、文档、dependency/build manifest、部署、测试、模板、配置与源码的确定性分类统一归 SQLite software `file_role` owner 及其同级 UT；software 根仅作声明 facade，物理 `projection/`、`query_scope/` 与 `schema/` owner 分别收敛 refresh/read 编排、有界 scope/filter SQL 和投影 schema 生命周期及直属测试；`software::graph` 把 file、topic 与 relationship 的物化/查询/映射交给直接配对测试的 owner；`software::dependency_usage::python` 直接由匹配的物理 `python.rs` owner 声明，不使用生产路径重定向；`software/lifecycle/` 目录在小型编排 facade 后分离 build、IaC、design、indexed-document 与共享 syntax owner。
- 本地文件定位索引：不依赖 Everything 等外部检索软件，显式扫描授权 roots；物理 `file_index/scanner/` owner 隔离有界文件系统工作并共置其 UT contract，SQLite/FTS5 快速按文件名、路径、扩展名和目录定位文件。
- 有界索引刷新队列：SQLite `indexing/` 根只包含物理 `metadata/`、`cursor_metadata/`、`diagnostics/`、`schema/`、`status/`、`task_queue/` 与 facade；前三者分别独占持久 codec/identity、backend cursor/model metadata 和 lag/queue 诊断并直接挂载 UT，`task_queue/` 把规划、入队/upsert、租约恢复、完成、失败/死信、持久记录身份与解码交给独立 owner。
- 运维工作流：支持 worker 队列、确定性回退提案、人工提案接受、持久审计事件、静默更新操作员状态，以及平台服务管理器的服务定义生成；version check 以物理子域分离 config、cache、SemVer、release aggregation 与 notice workflow，并为每个 owner 共置直属 UT，再通过技术无关 release-metadata port 调用外层有界 QoS-aware HTTP adapter。
- 服务化部署：文档化 `embedded_cli`、`resident_single_process`、`resident_partitioned_sqlite` 和未来 split worker 的控制面/数据面边界。
- Agent 接入：通过共享应用服务暴露 MCP Streamable HTTP 和本地 ACP 适配器，并带有作用域策略、QoS 准入、取消、资源/提示、持久审计元数据和 OTLP 准备的 agent 指标；HTTP 与 QoS 基础 owner 分别物理收敛在 `net/http/`、`net/qos/`，并在各自子域内共置 UT contract。
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
- [附录 B.11：文档发版准备审计 2026-06-05](docs/zh/06-verification/11-documentation-release-readiness-2026-06-05.md)：本次 documentation-only 刷新的发版导航、清单和链接检查记录。

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

可复用领域模型保留公开 `domain::*` facade，物理职责归属五个无环子域。
`domain/operations/runtime/` 独占 worker、proposal、audit、service operator 与 lifecycle
contract 并直接挂载 UT；`software/` 再按 request、dependency、graph、lifecycle、projection
与 validation 分组，operations facade 只做稳定重导出。其他 repository、scope、retrieval、
status 与 index-summary owner 进一步物理分组为 `domain/code` 下直接挂载 UT 的 call-target、context、dependency、graph-record、repository、index、repository-set、staleness、view 与 workspace owner，不再共用平铺源码或测试模块。

### 自迭代 Harness

面向代码检索和 semantic/vector 检索优化实验，可以通过稳定启动脚本运行
`tools/self_iteration` 下的独立 Rust harness：

```bash
./self-iterate.sh
./self-iterate.sh once
./self-iterate.sh loop --strategy unattended-layered
./self-iterate.sh chart
```

启动脚本会在需要时自动构建 debug harness binary。默认 `fast` profile
不跑产品 release build、全量 clippy、全量 test、文件 fixture、
semantic/vector fixture 或 research judge，并保留一个轻量 repo-set
跨仓门槛护栏；需要完整门禁和 workload 时使用
`./self-iterate.sh once --profile full`。

v2 运行历史、渐进式记忆、报告、patch 和评分曲线保存在
`.git/relay-knowledge-self-iteration/`，只有评分严格改进的候选修改才会被提交。

research judge 支持 OpenAI-compatible HTTP 或开放 coding-agent CLI；未配置
backend 时默认使用 `opencode`。semantic/vector fixture 会继承普通运行时使用的
`RELAY_KNOWLEDGE_*` embedding 环境变量，不会把 provider URL、API key、模型名
或维度写入 benchmark cases。

`unattended-layered` 策略面向 1-2 天运行：先执行短 smoke 级 Codex
探索，用 fast profile 验证有希望的候选，保存 resume state 到
`.git/relay-knowledge-self-iteration/unattended-state-v2.json`，并在短尝试停滞时升级到更长的
competitive-capability macro 探索。

自迭代测评集中的外部仓库已固定到文档记录的 commit。C/C++ 还包含基于
tree-sitter 语法能力生成的专用 fixture；多语言生成 fixture 扩展同一测评集。复现清单见
[第五卷第 6 章：C/C++ 语法型自迭代测评集](docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md)
和 [第五卷第 7 章：多语言语法型自迭代测评集](docs/zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md)。
大仓库索引的弹性长预算模型、180 秒历史基线、吞吐率计算和上限见
[第五卷第 12 章：大仓库索引弹性长预算模型](docs/zh/05-benchmarks/12-elastic-index-budgets.md)。

### 质量门禁

底层质量门禁：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
cargo test --test benchmarks --all-features -- --nocapture
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

CI 安装当前最新的 Rust stable 工具链。最终执行 Clippy 门禁前应先更新本地
`stable`，避免新稳定的 lint 在本地旧版本中未触发、直到推送后才失败。

OpenTelemetry 依赖必须作为一个兼容族整体升级：`opentelemetry`、
`opentelemetry_sdk` 与 `opentelemetry-otlp` 保持相同 minor 版本，
`tracing-opentelemetry` 使用与其匹配的集成版本。只升级其中一部分会引入重复的
telemetry trait 和不兼容的 provider 类型，因此不能进入发版候选。

自迭代 harness 默认只执行轻量 fast 门禁。完整 profile 的产品与 harness
质量检查会按依赖阶段并行执行，`--jobs auto` 默认使用本机 CPU 数。

默认 `fast` profile 还包含 targeted `code_index_recovery_cases`、
`code_index_sqlite_lock_cases` 和注册期 language guardrail。这样可以在不跑
exhaustive 大仓 workload 的情况下，持续验证 code-index 过期租约恢复、旧
worker 完成拒绝、dead-letter、checkpoint 续租、重复进程 SQLite 锁规避、并发
task claim 和混合语言注册安全性。

### 运行时与存储边界

二进制启动 Tokio 运行时；从 CLI 边界向内，所有核心能力均通过共享应用服务的异步入口暴露；`application::runtime::status` 独占 runtime/API 诊断投影，并直接挂载同级 UT。
SQLite 存储通过存储边界打开；物理 `storage/contracts/` 子域把异步错误、拓扑、graph、search、health、index、运维请求、code graph、canvas、repository code 与 file-index contract 分配给具名 owner，根 `storage/mod.rs` 只保留稳定 facade。SQLite adapter 内部由 `store/` 独占 schema-aware connection lifecycle 与有界 blocking worker，storage-port 映射归 `store/implementations.rs`；`evidence_identity/`、`mutation_log/`、`scope_filters/` 与 `table_stats/` 也是直接测试的物理 owner，跨 owner 持久化场景统一位于 `sqlite/tests/`。真实 `sqlite::graph/` 域把 graph-fact transaction/evidence invariant 交给 `mutation`，inspection/version read 交给 `mod.rs` facade，commit-time validity normalization 交给 `version`，每个行为 owner 都有直接配对测试。物理 `sqlite/schema/` 模块树独占 columns、initialization、compatibility marker 与 migration，不使用根 path redirect；`sqlite/mod.rs` 只组合具名 adapter owner。Graph canvas 将 request context/budget、稳定 node 格式、knowledge/evidence、structured facts 与 code facts 分别交给同名 owner 及其同级测试，根模块只保留预算校验和组合。`net::http::outbound` 独占 client 构造、有界 raw JSON transport 与响应校验，`qos_client` 保留 reqwest permit/body 计费；两者直接挂载同级 UT，HTTP facade 只保留配置、server runtime 与跨边界 integration contract。
应用服务以物理子域分离 health、retrieval、service status、storage diagnostics/provider、watcher 与 lifecycle 工作流；`retrieval` owner 负责 source-scope 校验、freshness reconciliation、有界 search/rerank 和响应预算，service facade 只保留构造与跨 owner 组合。`lifecycle_plan` 再将 plan 装配、install/upgrade/uninstall 正向步骤、checkpoint 回滚步骤、step/rollback execution、attempt-scoped checkpoint 文件、timeout/有界输出 process runner、共享 step policy 和平台 service definition 分配给直接挂载配对 UT 的 owner，不保留含混的 review 测试桶。
领域层 `graph::retrieval` facade 将 policy、backend status、diagnostics、evidence、hit、traversal provenance 与 context-pack contract 分别交给具名 owner；公开 `domain::*` API 保持不变，provenance、policy、backend 和 graph-path 行为由各自同级 UT 直接保护。
共享 API 的 `operations` facade 按 graph canvas、ingestion、retrieval、graph maintenance、service runtime、worker、proposal、audit、code repository 与 repository set 划分物理 owner；稳定的请求、响应、状态与流合同集中在 `api/contracts/`，因此 API 根只保留两个具名子域。`api::*` 导出保持稳定，解析、转换、默认值和 filter 合并行为由 owner 同级 UT 保护。
批量代码索引的 snapshot apply 或 checkpointed finalize 成功后，SQLite 存储会 best-effort 执行 `PRAGMA optimize` 和 `PRAGMA wal_checkpoint(PASSIVE)`；`health --format json` 与 graph inspection 会在 `graph.sqlite` 中暴露 journal mode、WAL 大小、最近维护时间和维护错误。最近维护时间和错误会持久化到 SQLite，因此服务重启或一次性 worker 退出后仍可诊断上一轮维护结果。`partitioned_sqlite` 拓扑下这些字段会通过只读 shard 诊断聚合 control 数据库和所有 active repository shard 数据库；任一 active shard 无法检查时，聚合结果会保留 shard 错误并把 WAL 大小标记为未知，避免把部分总量误报为完整状态。

默认存储拓扑是 `single_sqlite`；`application::runtime::storage` 独占该选择的校验。设置
`RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite` 后，全局控制状态仍写入主运行时数据库；物理 `repository/` 独占注册、别名状态、scope fallback 与删除一致性，`indexing/{checkpoint,file_index,lifecycle,retention}` 独占 checkpoint 路由、有界文件候选、durable 发布与 scope 清理，`catalog/`、`control_plane/`、`routing/`、`diagnostics/`、`status/` 和 `totals/` 协调运行时数据目录下的每仓库 SQLite shard。多仓 `repo-set` overlay refresh 在跨 shard import/export 聚合实现前仍要求 `single_sqlite`。
控制面继续拥有 task lease、audit、operator、topology catalog 和诊断，数据面 shard 只执行被共享应用服务授权和预算约束的读写；每个 partitioned 行为 owner 直接挂载 UT，跨 shard contract 留在 facade 测试。完整方案见 [服务化部署、控制面与数据面分离](docs/zh/03-architecture-specs/22-service-deployment-control-data-plane.md)。

存储契约包含 v1 代码图数据面。SQLite `code_graph` 根只保留 `mod.rs` 与物理 `batch/`、`schema/`、`query/`、`tests/` 目录：`schema` 独占 DDL，`batch` 独占事务性 fact replacement，`query/{symbols,references,chunks,status,common}` 独占有界读取、行解码、状态映射与输入校验；每个有行为 owner 直接挂载同级测试，跨 owner fixture 只放在 `tests/support.rs`。

### 代码索引

当前代码仓库索引支持 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift、Bash、SQL、Markdown、XML、Bazel/Starlark、Make、CMake、Dockerfile/Containerfile、Java properties、TOML、INI、YAML、JSON、Go module、Ninja、Jinja2 和 Go template。物理 `code::index` 子域分离 filesystem delta、full/incremental snapshot、plan、impact path 与 deleted symbol；worktree overlay 再分离 change recording、directories、scope、plan、snapshot 与 untracked policy，并直接共置 UT。不支持或降级的文件会回退为文本代码块。

SQL 文件会贡献 table、view/materialized view、function/procedure、trigger、type 等 schema object 符号，以及 SQL 对象引用和函数/过程调用边。

同一 source scope 内的本地文件、模板和构建目标引用会在 finalize 阶段解析；外部或有歧义的配置关系保留为 unresolved metadata。

Tracked-entry 路径准入、submodule 展开判定和 Git pathspec 投影归直接挂载 UT 的
`code/source` 物理根把 change-status parsing、有界 declaration fallback、非 Git filesystem policy、filter 常量、Git 执行、ref/snapshot resolution 与 source-root candidate 分离到直接测试的 owner 目录；tracked-entry 准入归 `changes/scope`，有界递归 tree 枚举和 submodule 状态报告归配对 UT 的 `changes/tracked_entries`，经 ref 校验的 name-status 加载归配对 UT 的 `changes/diff`，`changes/submodule_repository` 独占安全 worktree/gitdir 解析。Gitlink/submodule 源码访问
统一收敛在 `code/source/gitlink/`。tree commit 判定、child-filtered entry 发现、
初始化或反初始化 submodule blob 读取及 worktree root 校验由 `entries` owner 负责
并直接挂载 UT；incremental 与 worktree overlay 复用该边界。双侧 submodule change
分类、nested gitlink 有界递归、worktree/git-dir diff fallback、scope-aware entry
expansion 与预算检查归独立 `diff` owner；`impact` owner 独占影响编排、双侧 fallback
合并、稳定去重与最终预算校验，`gitlink::mod` 只保留模块声明和边界重导出。

仓库注册会拒绝 language filter，确保混合语言仓库保留完整语言面；需要收窄结果时在查询期使用 `--language`。`code::parser` 根只保留 facade、跨域测试与具名物理子域；共享 chunk、manual extraction、node/range primitive、record materialization、syntax capture 和 text validation 分别由专属 owner 管理。其 `languages` 根把 C-family reference、configuration definition、enum member 与 Markdown import 分别收敛到具名且直接测试的共享子域，并与语言专属目录并列。结构化配置解析把 call aggregation、language detection、key/value fact、knowledge-map fact、normalized record 与 source-line primitive 分别收敛到 `code::config_files` 下具名且直接测试的物理 owner。文件级解析编排物理收敛在 `code::parser::file/`，共享 parse contracts、解析状态诊断、text-only topic、route 与 feature-flag 投影分别由直接挂载 UT 的具名 owner 独占；feature-flag comment shielding、boolean config 扫描与 extractor 同样位于具名且直接测试的子域。

C/C++ 宏密集文件如果 error node 局限在宏、Nginx/Kong 这类外部头文件 typedef/module table 声明、GCC/Clang 风格声明属性与 inline 扩展（如 `__attribute__((always_inline))`、`attribute((always_inline))`、`__always_inline`）、预处理器或已识别 decorator 声明区域，decorator 类型体仍保持声明形态，并且仍能抽取可靠结构化事实，会被保守恢复为 parsed。

C/C++ recovery 把 declaration-head normalization 与 token/type/qualifier 识别放在 `declaration` owner，把 function signature、parameter boundary、operator、method suffix、postfix attribute 与 recovery decorator 判定放在 `signature` owner；依赖保持 `signature -> declaration/scan` 单向，两个 owner 都直接挂载 accepted/rejected shape UT。C-language adapter 把 declaration symbol、GCC recovery、lexical predicate、macro function、node kind 与 preprocessor lifecycle 分别收敛到物理且直接测试的 owner 目录，parser-wide C/GCC 场景归 `languages/c/tests/`；C++ adapter 把确定性 tree-sitter 分类收敛到直接挂载 UT 的 `node_kinds/`，把共享 head rule、decorated type recovery 与 structured/GCC-decorated function recovery 收敛到直接挂载 UT 的 `manual::{lexical,type_definitions,function_definitions}` owner；手工 header recovery 把 byte-stable source text、top-level scan、class/member name recognition 与 nested member collection 分别独占在直接挂载 UT 的 `cpp_header_recovery::{source_text,top_level_scan,declarators,member_collection}` owner。这些 owner 都是真实目录模块并直接共置 contract，不再用 facade 文件和生产路径重定向模拟子树。

Python type-reference parsing 将 literal-aware 函数签名 annotation 扫描与
tree-sitter node 分类分离。`languages/python/annotations` owner 负责跨行参数/
返回值 annotation、default-expression 边界和文件内 type parameter，并直接挂载
同级 UT；Python module facade 保留 node-context 与 local-type-reference 解析。

Web 路由检测把 Express orchestration、import/factory 与 application/router alias
发现、call/path syntax、参数/handler 解析、有界多行 statement aggregation、直接与
链式 registration recording、mount discovery 和 prefix materialization 统一收敛在
`detect/express/` 子域；各阶段分属明确 owner，并由每个 owner
直接挂载 UT。物理 `detect/lexical/` 边界把 Python 静态路由字符串与 JavaScript
注释/字符串/正则词法状态保留为独立语言 owner，不得用笼统 shared 实现跨越这些
语言边界。Spring annotation
与 Java type-scope 检测统一收敛在 `detect/spring/` 子域；Java comment/text-block
过滤和 declaration 识别、annotation path/method attribute 解析分别使用独立
owner；有界多行 annotation aggregation 与两者隔离，mapping kind 和
RequestMapping 语义也由独立 owner 负责；每个 owner 都直接挂载 UT。
class-prefix 派生、method 合并、URL 拼接与 route fact 去重收敛在 Spring
materialization owner。
Flask/FastAPI decorator、router mount 与 Python route materialization 统一收敛
在 `detect/flask/`；Python triple-quoted-string 与 comment 词法状态使用独立
owner，有界多行 statement aggregation 与 call-argument parsing 各自使用独立
owner；argument owner 负责顶层边界、keyword value、route path、method
collection、具名 handler、router identifier 与静态/动态 mount prefix 分类，
router-state owner 负责 declaration、late merge、include/register mount 记录与
framework 解析；materialization owner 负责 receiver URL 展开、mount-prefix
合并、动态 prefix 过滤、route fact 生成与去重；registration owner 负责 route
decorator、`add_url_rule`、methods override、receiver/handler 识别与 Python
function 绑定，所有 owner 都直接挂载 UT。

代码仓库 full index 会先发现 tracked source layout；有界 source-root 推导和 effective filter 归 `source/layout/discovery` 所有，归一化 filter 交集与 submodule child 投影归 `source/layout/path_scope` 所有，Git/filesystem 准入原因归 `source/layout/selection` 所有，source 解析、已选 entry、effective filter、内容 hash 与 filesystem-ref 一致性归 `source/layout/scoped_snapshot` 所有，受界 exclusion/largest-file 样本和 preview 计数归 `source/layout/preview` 所有，稳定的 scope 内外变更分组归 `source/layout/impact_partition` 所有。SQLite snapshot 持久化把 repository attachment、metadata/scope import 及其直接 legacy regression 收敛到 `code/snapshot/repository_import`，`scope_tables` 独占与增量 scope clone 共享的 table-copy contract。随后使用受资源预算约束的 SQLite 批次和持久 checkpoint。大 scope 索引过程中 `repo status` 会显示 `indexing` 和已提交计数，旧的 fresh scope 在 finalize 成功前继续服务查询，finalize 阶段再基于同一 scope 的完整已落库事实解析跨 batch reference、include 和 call edge。

增量 `repo update` 在新增文件出现在 `src/` 之外时复用同一套 source-layout 策略。

冷启动 full `repo index` 通过 `storage::sqlite::code::tasks::queue` 落持久化 code-index task 并立即返回 `task` handle。CLI 会启动有界单次 worker；非交互式 agent 可调用 `repo index-worker --task-id <id> --format json` 显式 drain 一次；`application::runtime::worker` 独占 endpoint 校验与 `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` 并发上限，`service run` 依此运行有界 code-index worker pool，并继续运行单个 repository-set overlay refresh worker。

本地 CLI 可以用 `--remote <base-url>` 或 `RELAY_KNOWLEDGE_REMOTE_BASE_URL` 访问已部署常驻服务。远端 `repo index` 只向服务提交 durable task 并返回 task/status/checkpoint JSON；任务由远端 `service run --web` 的 worker pool 消费，而不是由本地 CLI 执行 `repo index-worker`。远端只读命令包括 `repo list`、`repo query`、`repo context`、`repo feature-flags`、`repo impact`、`repo report`、`repo software` 和 `repo view`；其中 `repo list` 只返回至少有一个已完成 indexed scope 的仓库，不包含尚未完成首次索引的注册项。`repo index --reset` 和 `repo index-worker` 这类维护命令在远端配置下会被拒绝，必须在服务端机器执行；`storage::sqlite::code::tasks::reset` 独占本地原子 reset。远端分发只会预先校验远端 URL 与 outbound network 设置；无关本机 runtime 和 retrieval 设置只在命令回落到本机状态时校验。

不同 fingerprint 的 task 会独立排队和持有 lease；完全相同的 full-index fingerprint 会复用现有 task。

`repo status` 会报告 `active_task`、checkpoint 计数和 scope retention；`storage::sqlite::code::tasks::status` 独占任务查询与有界 queue 投影，`checkpoint` 独占 scope 与 latest-progress 读取。后台任务成功后保留 active scope、最近两个完成 scope 以及未完成任务 scope，并淘汰更旧的仓库 scope；`storage::sqlite::code::tasks::retention` 独占该计划与事务清理。

Code-index task lease 绑定 attempt，`storage::sqlite::code::tasks::lease` 独占 claim、renew、listing 与有界 recovery。过期 running lease 会在 claim/status 路径前恢复为 retry 或 dead-letter；`storage::sqlite::code::tasks::completion` 独占 lease-checked success、retry 与 dead-letter 转换。旧 worker 不能完成或失败已经被新 worker 接管的 task，活跃 worker 会在昂贵 batch 解析前、每次提交 checkpoint batch 后、finalize 前后以及完成 task 前续租。

未实现可选 lease recovery/renewal hook 的 store 会把这些 hook 当作 no-op，以保持 status 和 indexing 读路径兼容。JSON status 中的 checkpoint 会暴露 `updated_at_ms`，便于区分慢速推进和真正卡住。

### 源码范围与 Overlay

代码仓库 source scope 不再局限于顶层 `src/` 布局。创建索引前会先检查 tracked path 的目录结构，`external_deps/`、`packages/`、`modules/`、`plugins/`、`extensions/`、`Sources/`、`lib/` 和嵌套 JVM source root 下的真实源码会默认纳入索引。

clean Git snapshot 会把 registered/requested path scope 内的 tracked tree 作为目录权威，因此 Git 跟踪的 `.cloudbuild/`、`.cid/`、`.build_config/`、`build/`、`dist/`、`vendor/` 和 `third_party/` 路径会作为候选，而不会只因目录名被拒绝。

默认 `--path src` 注册会在索引期扩展到已发现源码根。精确 selector path filter 仍只用于收窄查询，并避免扩大到无关依赖树。

`--path` 是 CLI 中 path filter 的参数名。`repo register --path` 用来保存索引范围；`repo query --path` 和 `repo feature-flags --path` 只在已索引范围内收窄读取。`repo index` 不接受 `--path`，它按注册范围和选定的 `--ref` 建索引。

非 Git 源码目录的常规流程也使用 `HEAD`：先用所需 `--path` 注册，再运行 `repo index <alias> --ref HEAD`，查询时继续用 `--ref HEAD`；状态中记录的 indexed commit 会是对应的 `filesystem:<hash>` 快照。

Git 分支、标签和工作树选择器会解析为带作用域的提交/树快照。已索引作用域可按显式引用查询；rebase 或强制移动的 HEAD 需要重新索引；相同树的分支会复用同一作用域，同时保留请求引用的审计元数据。

worktree overlay 使用 Git status：窄 `worktree_overlay::mod` facade 把 identity/snapshot 装配交给 `snapshot`、普通路径分类与记录交给 `change_recording`、有界 submodule 状态转换交给物理 `gitlinks/` 子域；被 `.gitignore` 忽略的 untracked 文件会跳过，未跟踪的宽泛依赖、缓存或构建目录需要显式 path opt-in 后才会递归展开，每个行为 owner 都直接挂载同层 UT。

`repo remove <alias>` 只删除该仓库在 relay-knowledge 中的注册记录、alias、索引 scope、task、repository-set 成员和 overlay，以及软件投影，不删除磁盘源码；删除后同一路径或 alias 可以重新注册。

### 代码检索

代码图 v1 响应区分稳定的 `canonical_symbol_id` 和快照绑定的 `symbol_snapshot_id`。引用、调用、导入和 SBOM 依赖命中会暴露 `target_hint`、`resolution_state`、置信度基点和置信度等级，避免将未解析、有歧义、声明型或锁定型边误报为确定调用。

调用图检索也支持同仓静态跨语言边：C/C++ 互调、Go cgo `C.*` 和 Rust FFI/bindings 路径可解析为代码图证据，但这不等同于完整 build-system 或 linker 分析。SQLite code-store 持久化使用唯一的物理 `storage::sqlite::code` 树：`batch`、`feature_flags`、`generated`、`impact`、`lifecycle`、`query`、`routes`、`schema`、`search`、`set`、`snapshot`、`symbols`、`tasks`、`tests`、`views` 与 `workspace` 都是具名目录；有行为的 owner 直接挂载直属测试，跨 owner fixture 与回归统一位于 `tests/`，facade 只保留组合与 port 实现。impact facade 只协调受界检索，`seed`、`evidence`、`path_selection` 分别独占图种子、SQLite 证据/命中映射与 changed-path/language 准入并挂载配对测试。`query::{hits,prepare}`、`tasks::worktree`、`set::refresh_tasks` 与 `lifecycle::{cleanup,removal,report,status}` 归真实父域所有，不使用生产 path redirect，也不重复加载文件。

代码仓库词法检索使用 SQLite FTS 候选表覆盖 symbol、reference、call、import、SBOM dependency 和 chunk。有效 path filter 会在 FTS 候选窗口内先过滤再进入有界评分；graph edge 候选在截断前按 BM25 排序；相关性层的 `fts_plan` 只组装 focused/hybrid/lifecycle/structured match，`fts_recall` 独占高信号项与 member-access leaf 恢复并排除 source path，`fts_terms` 独占大小写去重、quote、type-surface companion 与 identifier 结构，`fts_compound` 独占 24 项预算内的 compact/snake alternative，四个 owner 直接挂载 UT。fuzzy symbol 召回仍可命中任一查询词，而 typed graph edge 查询保持更窄语义；Rust 评分会识别 snake_case/CamelCase identifier 片段、多段符号名、调用方向上下文和声明形态 API chunk。Call retrieval 使用真实 `code::query::calls` 模块树：`mod.rs` 只声明 owner 并重导出查询入口，`search` 协调受界 identity/FTS 路径，`row_store` 独占 SQL 与解码，`identity_query` 独占方向性 exact gate，`hit_projection` 独占评分和命中转换，`execution_order` 独占调用点顺序，`display` 独占 caller label。其余具名子模块继续拥有 ambiguous target、caller count、direction filter、indirect binding recovery、site/context scoring 与 target ranking，不在根层制造 path alias；聚焦 owner 直接挂载同级 UT。跨域 ranking 同样使用真实 `code::query::scoring` 树，直接声明 API-sequence、chunk-path、initializer、flow、inline-usage、interface、lifecycle、local-callable、path-ranking 与 proximity owner，并挂载各自同级 regression，不保留根级 alias。Hybrid planning 使用真实 `code::query::hybrid` 树：chunk/direct gate、exact-path decision 与受界 planning 都是直接 owner，共享 hybrid 回归通过物理 `tests::hybrid` 模块装配。Import retrieval 使用真实 `code::query::imports` 树：facade 只协调 layered fallback，`row_store` 独占受界 direct/identifier/FTS SQL 与解码，`hit_projection` 独占 enrichment/ranking/hit conversion，`scoring` 独占 ranking signal，`path_context` 独占 path/target classification，`binding_terms` 独占 named-binding 与 usage-term extraction，`targets` 独占 target-symbol 与 usage context；聚焦 owner 直接挂载 UT 且依赖单向进入 primitive。Reference retrieval 是 code-query 根直接声明的物理 `code::query::references` 子域，而不是根级 alias：`identifier_text` 独占 identifier scan，`identity_gate` 独占受界 exact 准入，`call_shape` 独占 call 识别，`same_name_path` 独占同名 source-file demotion，`type_context` 独占 parameter/type affinity；每个 owner 都直接挂载 regression。同级 `chunks` owner 独占 exact definition/reference fallback 准入、声明评分与 canonical-leaf 匹配，`chunks::search` 独占 layered FTS planning、SQL/value binding、row mapping 与 chunk scoring。Code-query facade 也直接声明 `symbols`，不再通过 `code_query_symbols` 挂载；symbol retrieval 的编排、exact/API identity 准入、FTS 规划、行解码、ranking、有界 direct recovery 与 typed function-value 解释继续由具名 owner 分担，并各自挂载测试。Code-query 根只能包含 `accuracy`、`api_identities`、`calls`、`chunks`、`conversion_terms`、`excerpts`、`hits`、`hybrid`、`identifiers`、`imports`、`line_ranges`、`prepare`、`references`、`relevance`、`routes`、`rows`、`sbom`、`scoring`、`symbols` 与 `tests` 目录及 facade；跨域 primitive 都是目录 owner，有行为的测试与 owner 同级，消费者使用真实模块身份，不得恢复平铺 sibling、`code_query_*` alias 或根级 `#[path]` redirect。
共享 code-query 测试根同样只能包含 `calls`、`field_filters`、`generated`、`hybrid`、`identity`、`line_context`、`ranking`、`score` 与 `unit` 领域及声明 facade；测试 facade 直接声明每个领域，code-store facade 不得再通过跨层 path attribute 重挂单个 query regression。
Graph retrieval 根只包含物理 `advanced/`、`aliases/`、`bm25/`、`bm25_fallback/`、`context/`、`derived/`、`label_trigrams/`、`local_model/`、`ranking/`、`read_model/` 与 facade；每个行为 owner 直接挂载同目录 UT，不得恢复平铺实现或测试 sibling。确定性 token signature、本地 hashed vector、semantic overlap、cosine similarity 与 identifier-aware lexical overlap 归 `local_model/`；共享 `retrieval::terms` owner 只暴露 rerank 生产调用的 normalized-term 操作。物理 `read_model/` 子域把 DDL/retry 交给 `schema`、重建交给 `migration`、文档写入交给 `documents`、共享候选/BM25 映射交给 `candidate`/`bm25_hit`、检索编排交给 `search`。同级 `advanced/` 子域进一步把 relation/claim/event path 组装交给 `path`、时间解析与过滤交给 `temporal`、scope 汇总交给 `community`、共享事件读取交给 `event`、证据归组交给 `support`。

Feature-flag extraction 将环境/配置/预处理来源键规则、SDK receiver/call tracking 和跳过字面量的共享 lexical primitive 分配给独立 owner；每个 owner 都挂载同级直接 UT，extractor facade 只保留稳定内部重导出。Repository-set 编排把 membership API 交给 `membership`、moving-ref 与 fact-version freshness 交给 `status`、set 专用存储错误交给 `errors`、同步与租约化 overlay rebuild 交给 `refresh`；物理 `query/` 域把纯 overlay ranking 交给 `mod.rs`、异步 member/fallback 协调交给 `workflow`、dependency API planning 交给 `plan`、ranking signal 交给 `domain_affinity` 与 `identity_coverage`，并为每个 owner 直接挂载配对 UT。SQLite repository-set 持久化同样把 member 生命周期/状态行映射交给 `code::set::membership`，把 overlay 状态、刷新、import/export 匹配与 cross-edge 读取交给 `code::set::overlay`；`code::set` facade 只重导出两者的窄入口。带配对 UT 的 `manifest/` 域进一步分离数据库编排、Go workspace、pnpm/package exports、module-key 展开与有界 path/glob 规则。
物理 `code::set` 根只能包含 `manifest`、`membership`、`overlay`、`refresh_tasks` 与 `tests` 目录及 facade。三个行为 owner 直接挂载 `mod_tests.rs`；跨 owner workspace 场景与 fixture 隔离在 `tests/`，不得在 `manifest/` 旁恢复平铺实现或测试 sibling。
Code-index schema 初始化只把执行顺序、旧列兼容与 migration 编排保留在 `code::schema` facade；repository facts、durable index task、repository-set/workspace 状态和 FTS/retrieval index 分属四个 schema owner，并各自挂载同级 contract UT。`search_backfill` owner 独占 symbol、reference、import、dependency、feature flag、call、route 与 chunk 的一次性 FTS document 物化、search metadata 同步，以及 signature 升级后的事务性 call-document 重建；其同级定向测试保护 legacy call language 继承和 metadata 幂等同步合同。
Checkpointed code indexing 将 batch fact、checkpoint transition、dependency document 与 session scope 发布分别交给物理 `code::batch/{persistence,checkpoint,dependencies,session}/` owner，各自直接挂载 `mod_tests.rs`，batch 根只保留 `mod.rs` 与具名目录；`finalize/` 同样把 call target/edge、file metadata、imported/ordinary reference、phase、search document 与 symbol catalog 目录化并配对直属测试，跨 owner TypeScript 场景归 `finalize/tests`。其 `imports/{module_paths,specifier,symbol_targets}/` 把有界路径归一化、specifier 提取和符号匹配分别与直属 `mod_tests.rs` 配对，语言规则继续位于 `languages/`；共享 code-identity import resolution 只消费这些受界合同。

`repo query --kind sbom` 会返回索引期从 Cargo、npm、Go、Python、Maven effective `pom.xml`/BOM、Gradle 和 Conan manifest/lockfile 提取的依赖清单；它不会执行包管理器、访问 registry，也不提供漏洞或许可证分析。

Maven effective POM 根以物理 `pom_path/`、`property_interpolation/`、`xml/` 分别独占不越过仓库根的相对路径、受界递归属性展开和带稳定行号的 XML tree，并直接挂载 UT；raw/effective model contracts 与 parent/profile property layering 归 `model/contracts` 和 `model/properties`，coordinate alias、dependency management/profile variant 与 plugin/execution inheritance 分别归 `model/{coordinates,dependencies,plugins}` 及其配对 UT，model facade 继续协调仓库内 parent、profiles、modules 和 imported BOM 的纯索引证据解析，跨 owner review regression 只放在 `tests/`。

Call excerpt 通过 `source_scope + symbol_snapshot_id` chunk lookup 与调用行包含条件定位，避免高 fan-out caller/callee 查询把一条 call edge 放大成多个无关 chunk 候选。

代码仓库查询还会使用可选 ripgrep 兜底恢复精确源码文本。AST 和已索引词法层先执行；当 definition/reference/hybrid 存在具体召回缺口，或 import 指向未作为代码图 target 索引的 unresolved external dependency 时，再用有界 `rg` 搜索已索引 commit 内容。

`code::search::candidate_scope` 独占源码兜底的安全路径校验、去重、generated-path 排除、path/language filter 和 256 文件预算，物化与内容扫描只消费它输出的有界候选范围。

`materialization` 独占 filesystem commit 前后校验、Git blob size/batch/per-path 读取策略、read/materialized byte 双预算、worktree-overlay 内容验证和临时源码树生命周期。

同级 `scanner` 独占内部文件读取、binary 排除、handwritten/generated 分流、有界行 excerpt 和声明上下文拼接；门面只协调允许的 post-index 兜底。

Definition 兜底会选择最后一个 identifier-like 查询目标，因此自然语言提示里的命令词不会被当成 symbol 搜索。

如果 FTS read model 不可用，候选文件路径会先使用已索引 path 和 chunk 词项保持源码兜底 query-aware；如果无法产生 query-aware 候选，则暴露 read-model 降级，而不是扫描按字典序截断的文件前缀。

只有可规划源码兜底的 definition、reference 和单 identifier hybrid 查询可以把索引结果视为空；import、symbol、caller、callee 以及不可规划的 hybrid 查询会暴露 read-model 错误，不能静默返回假阴性空结果。

当前面的词法层已经产生可用命中时，后续 FTS 层 outage 会保留这些部分命中并标记 degraded，而不是清空结果或隐藏 outage。

外部依赖源码缺失会作为 unresolved edge coverage metadata 暴露，不写入 `degraded_reason`。外部依赖兜底使用 unresolved target hint 而非任意用户查询文本，排序低于结构化 import-graph 证据，并标记 `text_fallback` 与诊断，提醒 agent 这是当前仓库源码证据，不是依赖库图谱证据。

`rg` 缺失或超时只降级兜底层；结构化代码图结果仍可用，并会返回诊断信息。人工 agent 或维护者检查源码时优先使用 `rg`；如果本机未安装，可用排除 VCS 和 build 目录的有界 `grep -RIn` 继续搜索，不能因为缺少 ripgrep 就停止源码分析。

### GraphRAG、Worker 与恢复

混合检索使用基于 SQLite 的 BM25、本地语义令牌签名、本地哈希向量近似最近邻、可配置的外部语义/向量后端元数据、图证据回退、schema 指导路径遍历、时间事件检索、社区摘要和代码图文档。

候选结果先通过互惠排名融合，再在最终截断前执行确定性本地 rerank，最终返回包含检索器来源、排序和 rerank 解释、实体、来源范围、结构化图事实、直接图路径证据、代码工件、后端可用性、新鲜度、截断和预算元数据的上下文包。

BM25 读模型会为实体标签和代码符号索引生成词汇别名，但不会将这些别名作为规范标签返回。

证据可携带多模态提取元数据，包括文本范围、图像资源、OCR 文本、标题、图像嵌入、表格和布局区域。

派生的 OCR/标题/图像证据会引用父证据项；检索时按父项聚合这些命中，避免重复上下文项；后台或维护 worker 必须通过 `commit_multimodal_extraction` 提交 OCR/标题/表格/布局输出，不能阻塞查询热路径。

运维产品化能力会持久化 worker 任务、人工提案、审计事件和静默更新操作员状态。

`storage::sqlite::operations::schema` 负责初始化并兼容升级 worker-task、proposal、audit 与 operator 表；`operations::worker_tasks`、`proposals`、`audit_events` 分别独占 worker 队列、提案/冲突生命周期与审计写入/查询，`service_operator` 独占静默更新操作员状态及 JSON 行映射。多模态摄取会排队 embedding/OCR/视觉/提取器工作；`worker run-once` 在配置 HTTP 端点时调用远端 worker，否则创建确定性回退提案；`proposal accept` 通过同一图变更路径提交；服务管理器命令仅生成平台服务定义，不执行特权安装。

`evaluation` 模块提供纯 GraphRAG 测试框架和 CI 夹具门控，覆盖精确事实、多跳、时间、负面拒绝、过期索引、歧义实体和代码影响观察。

图提交还会持久化第二阶段索引恢复元数据。变更日志条目记录受影响作用域、实体 ID、证据 ID 和来源哈希，包括作用域移动和结构化事实证据引用。

作用域索引游标跟踪种类/作用域/模态新鲜度、来源哈希、后端游标，以及语义/向量 worker 可选的模型名称/维度元数据。

`ingest`、`query --freshness wait-until-fresh`、`index refresh`、`health` 和 `service doctor` 共享有界刷新队列；物理 `task_queue/` owner 分离 planning、enqueue/upsert、lease recovery、completion、failure/dead-letter 与持久 record identity/decoding。

### CLI 契约

当前 CLI 使用编译后的 `relay-knowledge` 二进制和 git 风格子命令：

adapter 将 global option/token 解析与命令族分发收敛到直接配对 UT 的
`interfaces::cli::command::parse` owner，共享 flag value 与 freshness 校验归
`command::values`。CLI error、结构化 grammar diagnostic、退出码分类以及
text/JSON stderr 编码归 `command::diagnostics`；CLI 根只重导出这些稳定合同并保留
process facade；`command`、`files`、`grammar`、`knowledge`、`map`、`operations`、`remote`、`render`、`repo`、`repo_set`、`runtime`、`service`、`setup`、`spec` 与 `version` 全部是物理 owner 目录，有行为的 owner 直接挂载 `mod_tests.rs`，CLI 根只保留这些具名域、`tests/` 与 `mod.rs`，不使用 `*_cli` alias 或生产路径重定向。仓储命令族由 `repo::mod` 只维护命令数据合同与模块装配，
`repo::parser` 独占语法/校验并直接挂载 `parser_tests`，`repo::runner` 独占异步
service workflow 与渲染并直接挂载 `runner_tests`，`repo::view` 保留嵌套 view 合同与 workflow 而不是提升为 CLI 根 sibling；machine-readable command metadata 使用物理 `spec::{data,files,repo,repo_set}` 模块树，`spec::repo::{lifecycle,indexing,retrieval}` 分别独占仓储生命周期、索引和读取 builder 及直属 UT，`data::{core,map,operations,service}` 独占聚合命令族。无需 runtime 的快路径与共享
service action dispatch 归 `runtime::dispatch`，显式/环境 remote URL 优先级及
远端能力判定归 `runtime::selection`。

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
relay-knowledge repo query relay-knowledge --query serde --kind sbom --ref HEAD --format json
relay-knowledge repo feature-flags relay-knowledge --query checkout --ref HEAD --format json
relay-knowledge repo software relay-knowledge --kind relationships --ref HEAD --format json
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace relay-knowledge --ref HEAD --priority 10 --format json
relay-knowledge repo-set remove workspace relay-knowledge --format json
relay-knowledge repo-set query workspace --query retry_policy --kind definition --format json
relay-knowledge repo impact relay-knowledge --base main --head HEAD --format json
relay-knowledge repo list --format json
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

`repo-set refresh` 会基于已索引的 member snapshot 重建跨仓 import overlay
edges。overlay 能识别 Go workspace/module manifest（`go.work`、`go.mod`）
和 pnpm workspace（`pnpm-workspace.yaml` 加 package `package.json` 的名称、
入口和 exports）。嵌套 `go.work` 只会把 `go.mod` 过滤作用在自身目录树内，
pnpm package glob 只匹配 workspace root 下的路径，package key 来自索引时保留的完整
workspace/package manifest 内容。pnpm root package 总是 included；没有 `packages`
字段的 workspace 只包含 root package；`exports` 优先于 `main`/`module` 入口 alias。
声明了 package `exports` 时，package subpath key 也会受 exports 约束：conditional
export object 只选择一个优先 runtime target，wildcard subpath export 会映射匹配的文件
pattern，未声明导出的私有文件不会获得合成 package subpath alias。
仍无法匹配到成员 package 的 import 会保留为带 target hint evidence 的 `unresolved`
跨仓 edge。

#### Kind 参考

`--kind` 的取值是命令本地的。同一个 flag 名称不代表不同命令共享同一组取值：

- `repo query --kind` 和 `repo-set query --kind` 用来选择代码检索意图：
  `hybrid`、`symbol`、`definition`、`references`、`callers`、`callees`、
  `imports` 或 `sbom`。影响分析使用 `repo impact`，feature flag 使用
  `repo feature-flags`，不要为它们发明新的 query kind。
- `repo software --kind` 用来选择仓库级软件图谱切片：`dependencies`、
  `sdks`、`files`、`topics`、`relationships`、`build`、`iac`、`design` 或
  `all`。
- `index refresh --kind` 用来选择派生检索索引族：`bm25`、`semantic` 或
  `vector`；省略 `--kind` 表示请求刷新全部受支持的索引族。
- `worker status|run-once --kind` 用来选择后台 worker 家族：
  `embedding`、`ocr`、`vision` 或 `extractor`。
- `map source add|update --kind` 用来标记 knowledge-map source 类别：
  `repo`、`file`、`doc`、`config`、`db`、`ci`、`runtime`、`wiki` 或
  `monitoring`。

读取或写入 `.knowledge/knowledge-map.yaml` 的 knowledge-map 命令会从进程启动目录
发现仓库根：先向上查找 `.git` 或 `.knowledge` 标记，找不到时兼容 fallback 到最近的
`AGENTS.md`。如果没有发现仓库标记，命令会返回稳定错误，不会把 runtime state 写进当前
目录。`map agent-snippet` 不需要仓库根发现。

CLI 参数含义是公开契约的一部分。Skills 和其它 LLM 工具在发出命令前应先读取
`relay-knowledge help --format json`；该输出会描述每条 command path、operation、读写影响、必填参数、默认值、允许值、可重复性、示例和注意事项。

本地文件索引 root 必须是绝对路径，并且必须出现在
`RELAY_KNOWLEDGE_FILE_INDEX_ROOTS` 中；`RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS`
用于设置每个 root 的扫描 timeout 预算；`application::runtime::file_index` 独占 root normalization、稳定 root ID、authorization 与 scan/query budget。`files query --format json` 会返回顶层
`freshness` 对象，包含 root cursor、index lag、stale/degraded reason、有界重扫状态和
direct-source-read instructions。使用 `--freshness wait-until-fresh` 可以在 file index
仍为 pending、degraded 或 overflow 时抑制答案，直到有界扫描完成。
应用层 `application::knowledge/{ingest,multimodal,file_freshness,index_refresh,map}/` 分别收敛工作流与直属 UT，`file_index/` 子树把 async service API 与有界 blocking scanner 分开；
其物理 `content/` 域把 read-byte 记账、抽取/chunking、capability-root 授权读取分别交给 `budget`、`extract`、`read`，并让每个 owner 直接挂载配对 UT。

SQLite `file_index` 把 metadata schema、事务性 root update、retirement、path FTS search、diagnostics、content 与跨 owner tests 分别收敛到物理目录，facade 只重导稳定 store 边界。
其 `content` 子域继续独占 schema、稳定 identity、replacement/cursor persistence、有界 FTS search
与 fact-candidate extraction，并为每个 owner 共置 UT；store adapter 继续使用稳定的
`file_index::content::search` 边界，根目录不再让生产/测试文件与子目录混排。

### 文件监听 (fs.watch)

文件监听检测源代码变更并自动推送增量索引任务。默认在支持的平台上启用。

```bash
RELAY_KNOWLEDGE_WATCHER_ENABLED=true
RELAY_KNOWLEDGE_WATCHER_DEBOUNCE_MS=3000
RELAY_KNOWLEDGE_WATCHER_MAX_WATCH_DIRS=1024
RELAY_KNOWLEDGE_WATCHER_HASH_CACHE_CAPACITY=4096
```

Watcher 根把 `config/`、`event_filter/`、`hash_cache/`、`task_seed/` 保留为直接
测试的 owner；`engine/` 分离 handle、`notify` event loop、repository registration、
任务投影与 diagnostics。事件仍经 debounce、内容哈希与路径过滤后生成有租约的
`WorktreeOverlay` 任务；状态、事件计数与降级原因继续由 `service status` 暴露。

### Semantic 与 Vector Backend

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

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` 和 `RELAY_KNOWLEDGE_VECTOR_BACKEND` 也接受
`local` 与 `disabled`。`application::runtime::retrieval` 独占 typed backend、rerank 与 remote-embedding 校验；禁用的 read-model backend 不参与 semantic/vector 检索执行和刷新调度，空 embedding model name 会在运行时配置阶段失败。

### 设置与图事实

Web Settings 页面按 agent 互操作性、检索默认值和模型 provider 分类展示。Agent/检索设置会读取同一套脱敏 runtime 与 service diagnostics，用于生成 MCP 暴露、origin allow-list、作用域策略、审计和外部模型相关环境变量。

模型 provider 设置通过 `/api/configs/model/*` 管理命名 chat/completion profile、fallback policy、`models.dev` catalog 刷新、endpoint probe 和模型发现。`model_provider::profiles` 独占 profile CRUD、secret 保留与 runtime-profile resolution；`model_provider::profile` 独占公开 profile contract、持久形态与脱敏投影；`profile_config` 独占 normalization 和 validation；`model_provider::fallback` 独占 fallback 类型、默认值、校验与持久化；`model_provider::catalog` 独占 catalog contract、cache fallback、刷新与 payload parsing；`model_provider::connectivity` 独占 probe/discovery contract、QoS HTTP 工作流、脱敏与诊断。Profile 与 fallback 文件位于解析后的配置目录，文件名为 `model-profiles.json` 和 `model-fallback.json`；公共 catalog cache 位于解析后的缓存目录，文件名为 `model-catalog-cache.json`。

Secret 只在保存时接收，回传给浏览器时只显示 configured boolean 或脱敏 header。更新 profile 时会保留已脱敏的 header secret，API 调用方可设置 `clear_api_key=true` 显式清除已保存的 API key，便于迁移到 header-only 认证。

CLI `ingest` 命令会写入 evidence 和 entity label。共享 API 还接受面向 adapter 的更丰富 Phase 1 graph fact：evidence `source_path`、source `span`、confidence、lifecycle status、类型化 relation、claim，以及引用 evidence id 的 event。

结构化事实必须引用 supporting evidence；反序列化后会重新校验 supplied confidence、span 和 version-range 字段；检索只使用 `accepted` 或 `proposed` evidence 作为上下文。Context pack item 现在会暴露从这些结构化事实派生的直接 `graph_paths`，方便 agent caller 在 raw fact provenance 旁边引用一跳 relation、claim 或 event path。

### Web、MCP 与 ACP

`service run --web --mcp streamable-http` 会在同一端口启动 Web 诊断、`/api/*` 和常驻 MCP Streamable HTTP adapter。默认绑定为 `http://127.0.0.1:8791/` 和 `http://127.0.0.1:8791/mcp`。物理 Web adapter 把 `assets/`、`files/`、`model_config/`、`operation_request/` 与 `code/` 保留为具名子域并直接挂载配对测试；`web/mod.rs` 只组合各子域 route 和共享 response/error 边界，装配后 router 的 file integration coverage 继续由 facade 持有。`code/` 域把版本化 repository route 交给 `mod.rs`，CLI-shaped index payload mapping 交给 `index_request`，code-view payload mapping 交给 `view_request`，不保留平铺 feature sibling 或生产 `#[path]` redirect。物理 MCP 根把 audit、HTTP、JSON-RPC、metrics、notification、prompt、resource、scope authorization、session state、tool contract 与 registry 分别收敛到直接测试的 owner 目录，装配型 protocol/tool 场景和 fixture 统一位于 `mcp/tests/`；`runtime/` 子树继续拥有共享 server state、HTTP transport 生命周期、JSON-RPC dispatch、可取消 tool runtime、内置只读工具和 method-error 映射，根 `mcp/mod.rs` 只保留模块声明、协议常量和稳定公开重导出。

除非通过命令或 `RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED=true` 显式启用，MCP 默认关闭；graph tool 需要 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES`，除非显式配置 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true`。`application::runtime::agent` 独占 endpoint、origin、scope、request budget 与 audit queue 设置校验。

Adapter 会校验 `initialize` 参数，然后签发不可预测的 `Mcp-Session-Id`。客户端必须发送 `notifications/initialized`，之后调用需要携带该 session header 和 `MCP-Protocol-Version`，确保 `ping`、工具请求和 `notifications/cancelled` 绑定到已签发的 session。

缺少 session header 会返回 HTTP 400；未知或已驱逐的 session ID 会返回 HTTP 404。

MCP 工具界面包含图检索、图检查、健康状况、服务状态、索引状态、授权代码图查询、授权软件全域模型查询、repository-set 代码图查询和授权代码影响分析；code-tool schema、请求校验、检索 workflow 及 software/feature-flag/impact 洞察由 `interfaces::agent::mcp::code_tools::{tool_definitions,request_contracts,retrieval_handlers,insight_handlers}` 分别拥有，contract primitive 保留直属 UT，装配后的 tool workflow 由 MCP integration test 覆盖。

Agent-facing kind 选择复用现有产品 kind：`relay_code_query` 处理代码图谱 kind，`relay_software_query` 处理软件全域模型 kind，`relay_code_feature_flags` 处理配置驱动 feature flag。

常见 agent 别名如 `dependency`、`configuration` 和 `models` 会归一到已有 `dependencies`、`relationships` 和 `design` kind，而不是新增重复 kind。

MCP 不暴露 index refresh 或 repository indexing；仓库索引需要用户主动运行 `relay-knowledge repo index` 或 `relay-knowledge repo update`。

MCP 服务器也会发布资源和提示：资源暴露服务状态、健康状况、索引状态和 Prometheus 文本指标；只有在 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true` 时才发布全图摘要资源。

提示提供检索和代码影响规划模板。`/mcp/metrics` 暴露 Prometheus 文本指标；MCP 客户端只使用原生 Streamable HTTP `/mcp` 入口。

Agent 请求会写入有界进程内审计事件，包含运行时身份、作用域、新鲜度、QoS 决策、预算、截断、结果数和状态；物理 `interfaces/agent/audit/` owner 收敛日志、JSONL sink 与直属测试，`policy/` owner 收敛共享校验、授权策略与直属测试。

设置 `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true` 后，这些事件会镜像到由 `paths` 管理的 JSONL 文件 `logs/agent-audit.jsonl`；sink 使用由 `RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH` 控制的有界异步队列，最多允许 65536 条。

本地 ACP session adapter 通过 agent-client session 暴露同一检索契约，包括进度更新、取消和上下文工件。前台服务启动时会先执行恢复流程，刷新过期的索引游标，然后再接受常驻 adapter 工作。

### 浏览器检查

Web diagnostics、operation workspace 和浏览器集成检查：

```bash
./build.sh
./run.sh start --port 8791 --daemon
curl http://127.0.0.1:8791/api/health
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

静态工作区通过同一个有界 Rust HTTP 服务暴露 health、GraphRAG、Graph canvas、
索引、worker 和 operation composer 诊断。用户工作流见
[Web 工作区能力](docs/zh/02-capabilities/12-web-workspace-capabilities.md)，
`operation_request`、`assets` 与同级测试所有权见
[工程硬约束](docs/zh/03-architecture-specs/02-engineering-hard-constraints.md)。

### 可选 Hooks

可选本地 hooks：

```bash
pre-commit install
pre-commit run --all-files
```
