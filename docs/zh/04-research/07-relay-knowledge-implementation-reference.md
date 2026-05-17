# relay-knowledge 实现借鉴落地路线

[中文](../../zh/04-research/07-relay-knowledge-implementation-reference.md) | [英文](../../en/04-research/07-relay-knowledge-implementation-reference.md)

> 编制日期: 2026-05-13
> 进展刷新: 2026-05-17
> 范围: 结合 docs 中的知识图谱、GraphRAG、Agentic KG、Tree-sitter 代码图、协议接入和后台服务材料，对照当前 Rust 实现，记录已关闭能力和开放产品化路线。
> 定位: 工程借鉴文档，不替代 `docs/zh/03-architecture-specs/` 中的硬约束和接口规格。

## 研究定位

| 维度 | 结论 |
| --- | --- |
| 研究来源 | 汇总前六章研究、当前代码和能力文档，并对照 GraphRAG、Agentic KG、Tree-sitter、时间图谱和多模态证据方向。 |
| 研究目标 | 把研究结论压缩成可执行落地路线，明确哪些能力已经闭环、哪些仍是差距、哪些应保留为未来接口。 |
| 关键竞争力 | 研究不以功能数量取胜，而以共享核心服务、版本化事实、可解释 context pack、代码图和服务化运维形成系统优势。 |
| 场景与未来 | 面向 v1 产品化、后续 GraphRAG 扩展、Agent 接入、静默更新、评估闭环和安装部署治理。 |

## 1. 执行结论

`relay-knowledge` 当前已经具备一个可继续演进的知识图谱底座: 统一 API、异步 application service、SQLite 图状态、图版本、结构化事实、索引新鲜度元数据、带 source hash/backend cursor/model metadata 的 scoped index cursor、bounded refresh queue、task lease/reconciler 诊断、结构化 stale reasons、FTS5 BM25 read model、local semantic/vector read model、可配置外部 semantic/vector embedding 后端契约、schema path/temporal/community retrieval、RRF context pack、本地确定性 rerank、Tree-sitter 代码仓库索引、多模态 evidence schema、后台/maintenance 多模态抽取提交入口、worker proposal lifecycle、MCP Streamable HTTP、本地 ACP session adapter、MCP resources/prompts、Prometheus metrics exporter、可选 JSONL audit sink、CLI/Web 入口、GraphRAG evaluation fixture gate、`env`/`paths`/`net` 基础边界和 QoS 配置。它还不是完整服务安装产品，当前开放产品化集中在具体外部 embedding/OCR/vision/table/layout provider、特权 service install/rollback/package manifests/watchdog、conflict/valid-time 产品语义、远端 ACP/A2A gateway、query router 和 release-facing 评测报告。

基于现有材料，后续路线不应追求复制某个 GraphRAG 框架，而应把项目定位为 **knowledge substrate**:

- 核心负责事实、证据、版本、scope、索引、检索、诊断和审计。
- 外部 agent runtime 负责 planning、tool calling、审批、长任务会话和最终 LLM 生成。
- LLM 或 agent 输出只能进入 proposal、diagnostic、summary 或 derived index，不能绕过 graph mutation contract 直接覆盖 accepted facts。
- GraphRAG 的价值落在检索规划和上下文组织上: BM25、semantic、vector 和 graph expansion 需要协同返回可解释 context pack，而不是只返回自然语言答案。

## 2. 当前实现基线

### 2.1 已经可复用的核心基础

当前实现已经完成了几条重要边界:

- `api` 层定义了 CLI、Web、HTTP 和 agent adapter 可共享的 request/response 类型，包括 ingest、hybrid retrieval、context pack、graph inspection、index refresh、health、service status、agent identity 和 code repository API；Web adapter 已通过同源 `/api/web/operations/execute` 将操作 composer 接到 application service。
- `application` 层通过 `RelayKnowledgeService` 收口业务入口，CLI 和未来 adapter 不需要直接访问 SQLite 或 tree-sitter。
- `storage` 层通过 trait 隔离图事实、mutation log、index metadata 和 code graph 查询，SQLite 实现把阻塞数据库操作放到 `spawn_blocking` worker 中。
- `domain` 层已有 `GraphVersion`、`SourceScope`、`FreshnessPolicy`、`IndexStatus`、`GraphMutationBatch`、`EvidenceRecord` 和代码图类型，适合继续扩展成更完整的事实模型。
- `code` 和 `application::code_service` 已经实现 Git 仓库注册、clean snapshot 索引、增量 diff、worktree overlay、Tree-sitter 多语言解析、代码图检索和 diff impact。
- `net::http` 和 `net::qos` 已经拥有配置校验、事件驱动 HTTP server、超时、请求体预算和 admission policy 基础；MCP Streamable HTTP 已经在这些边界内运行。
- `interfaces::agent::mcp` 已经实现 MCP Streamable HTTP session、protocol header 校验、tool calls、resources、prompts、Prometheus metrics endpoint、access policy、QoS admission、cancellation registry、index refresh 权限控制、code graph query、code impact、bounded audit log 和可选 JSONL audit sink。
- `interfaces::agent::acp` 已经实现本地 ACP session adapter，支持 initialize metadata、session/new、session/prompt progress、cancellation、context artifact、runtime identity、QoS admission、bounded audit log 和可选 JSONL audit sink。

这些基础与研究材料的主线一致: async-first、统一 API、图存储解耦、索引新鲜度、代码图和 scope 隔离都已经有雏形。

### 2.2 当前能力边界

需要明确的是，当前实现已经关闭本地 GraphRAG 主路径，但还不是完整安装发布产品:

- `retrieve_context` 已经使用 SQLite FTS5 BM25、graph evidence fallback、code graph documents、local semantic token read model、local hashed-vector ANN read model、schema path、temporal event、community summary、RRF context pack 和本地确定性 rerank；context item 会携带 structured facts、由 facts 派生的一跳 `graph_paths`、source span、code artifact、rerank signal 和 backend availability metadata。semantic/vector backend status 现在由 read model cursor 与 runtime backend 配置生成，支持 `local`、`external` 和 `disabled` 模式。
- `index_status` 记录了 BM25、semantic、vector 等索引家族的聚合新鲜度；scoped cursor 按 kind/scope/modality 记录 graph version、source hash、backend cursor，并允许 semantic/vector worker 在完成任务时写入 model name/dimension。`refresh_indexes` 会调度持久化 task、获取 lease、replay mutation log 并更新 cursor；semantic/vector refresh completion 会从已索引文档推导模型元数据，避免 runtime label 与实际 read model provenance 分离。BM25 文档随 evidence/code graph 写入更新，并为 entity labels 与 code symbols 记录生成式 lexical alias 字段；semantic/vector read model 随 evidence 写入记录 model、dimension、source hash、scope 和 graph version metadata。
- 通用知识图谱已经从 evidence/entity 扩展到 typed relation、claim/event、confidence、source span、status、version-range validation 和 worker proposal lifecycle；valid time、conflict resolution 以及更完整的事实审批产品体验仍是开放产品化工作。
- 后台服务状态已暴露为 API，foreground `service run` 启动时会执行最小 startup index reconciler；foreground refresh 主路径已具备任务表、leases、retry、dead-letter 计数、reconciler 补发、stale diagnostics 和按索引族/scope 归因的 stale reasons。service manager 定义生成和 silent-update operator state 已落地，特权安装、rollback、watchdog 和维护任务编排仍需产品化。
- MCP Streamable HTTP 和本地 ACP session adapter 已经可用，并已有 access policy、QoS、bounded audit log、可选 JSONL audit sink、code graph query/impact tools、MCP resources/prompts、metrics exporter。

## 3. 可借鉴方向

### 3.1 GraphRAG 与 LightRAG: 先做可解释 context pack

GraphRAG、LightRAG 和相关材料共同指向一个结论: 图不是向量库替代品，而是检索规划、关系扩展和上下文组织层。对本项目而言，优先级应是继续把 `HybridRetrievalResponse` 作为可审计 context pack 扩展:

- 返回命中的实体、关系、chunk、source scope、graph version、index versions、retriever source 和 score explanation。
- 本地问题优先走 entity linking + limited neighborhood + evidence chunk。
- 全局问题后续走 community/summary read model。
- 多跳问题保留 path 结构，避免盲目 k-hop 扩展造成噪声膨胀。
- 所有结果都带 stale、degraded、truncated 和 freshness policy 信息。

近期不需要直接实现 LLM answer generator。core 只负责组织 grounded context，由外部 runtime 或 UI 决定是否生成最终答案。

### 3.2 Agentic KG: core 不做 runtime

Agentic KG 和协议接入材料说明，`relay-knowledge` 应作为常驻知识服务提供图检索和知识维护能力，但不应变成通用 agent runtime。

推荐借鉴点:

- adapter 只做协议转换、权限前置、identity 注入、QoS admission 和错误映射。
- MCP 默认暴露 tool/resource/prompt，适合作为其它 agent 的知识工具入口。
- ACP 适合会话式检索入口，但不默认提供文件编辑、终端执行或代码修改能力。
- 高风险操作，例如 mutation commit、entity merge、index rebuild，应通过 proposal/approval 或显式 permission。
- 每次 agent 请求记录 trace、runtime identity、source scope、freshness、QoS decision 和 result truncation。

当前统一 API 已经作为 MCP adapter 下游，后续不需要为 MCP/ACP 复制检索逻辑。

### 3.3 Tree-sitter 代码图: 优先产品化

代码仓库检索是当前最接近可交付的能力。已有实现覆盖 Git
snapshot、增量 diff、worktree overlay、Rust/Python/JavaScript/JSX/
TypeScript/TSX/Go/Java/Kotlin/Scala/C/C++/C#/Ruby/PHP/Swift/Bash
tree-sitter parsing、symbol/reference/import/call/chunk 和影响分析。

后续应优先增强:

- scope metadata: 查询响应明确返回 repository、resolved commit、tree hash、path filters 和 indexed ref。
- 增量可靠性: changed paths -> content hash skip -> tombstones -> reverse dependents -> scoped refresh。
- 代码图 context pack: 将 symbol、reference、call、import、chunk 和 impact hit 组织为 agent 可读包。
- 查询预算: limit、path filter、language filter、timeout、truncated reason 和 degraded reason 必须进入 API。
- 语义边界: tree-sitter 输出标记为 syntax-level facts，跨文件解析不确定时保留 ambiguous/unresolved 状态。

这条路线能最快体现知识图谱对开发者和 coding agent 的价值。

### 3.4 时间图谱与版本: 区分系统版本和事实时间

现有 `GraphVersion` 是系统状态版本，适合作为 mutation log 和 index freshness 的基础。研究材料进一步要求区分事实有效时间:

- `graph_version`: 图数据库提交状态，用于 replay、index cursor 和 stale 判断。
- `valid_from` / `valid_to`: 事实在业务世界中成立的时间范围。
- `observed_at` / `source_published_at`: evidence 被观察或发布的时间。
- `as_of` / `time_range`: 检索请求对时间状态的约束。

不要用向量相似度解决时间冲突。相同实体或相似文本在不同时间可能对应不同事实，必须由图版本和事实时间共同约束。

### 3.5 多模态 evidence: 统一来源和派生关系

PDF、图片、OCR、图注、表格和 layout region 不应各自形成孤立索引。可借鉴的最小模型是统一 `Evidence`:

- 原始 evidence 保存 source URI/hash、media hash、modality、extractor、extractor version、scope 和 parent evidence。
- OCR、caption、vision description 是派生 evidence，不覆盖原始图片或页面。
- 抽取失败记录 diagnostic 和 degraded reason，不能阻塞其它 modality 的摄取。
- 检索组织时合并同一父 evidence 的 OCR、caption、image hit 和 text hit，避免重复展示。

当前 evidence/fact API 已支持 source path、span、confidence、status、relation、claim、event 和 version range validation；后续多模态扩展应在兼容现有 evidence provenance 的前提下增加 modality、extractor、parent evidence 和 diagnostic 字段。

## 4. 差距分析

| 方向 | 当前状态 | 主要差距 | 建议优先级 |
| --- | --- | --- | --- |
| 统一 API | 已有 ingest/query/context pack/status/health/code repo/agent identity/API operations/audit API | 更细的 context artifact 和 release diagnostics 仍可增强 | P1 |
| 图事实模型 | evidence/entity、typed relation、claim/event、confidence、source span、status、多模态 extraction metadata、proposal lifecycle + graph version | valid-time 产品语义、conflict resolution 和审批 UI 仍需产品化 | P1 |
| 混合检索 | 有 BM25、graph evidence、code graph documents、local semantic/vector、可配置 external backend metadata、path/temporal/community、RRF、本地 rerank 和 context pack | query router、lite-global/DRIFT-like expansion 和外部模型 rerank provider 仍待产品化 | P1 |
| 代码图 | 已有 tree-sitter 多语言索引、scope metadata、path/language filter、报告、query/impact 和 MCP tools | 多仓库联邦调用解析和更大的真实性能报告仍待扩展 | P1 |
| 后台服务 | 有 status API、foreground service run、startup reconciler、refresh queue、lease/reconciler 诊断、dead-letter、metrics、service definition preview 和 silent-update state | 特权 service install、watchdog、rollback、package manifest 和维护任务编排仍待产品化 | P1 |
| Agent 接入 | 已有 MCP Streamable HTTP、本地 ACP adapter、resources/prompts、access policy、QoS、audit log、JSONL audit sink、code graph query/impact tools 和 metrics | 远程 ACP host integration、A2A gateway 和更完整 host integration 仍需后续产品化 | P2 |
| 多模态 | 有 evidence modality/extraction schema、extractor diagnostics、parent grouping、modality read model metadata、worker contract 和 maintenance 提交边界 | 具体 OCR/caption/table/layout provider、image embedding backend 和模型共存策略仍待接入 | P2 |
| 时间图谱 | 有 graph version、event `occurred_at` 和 `as_of`/年份 temporal retrieval | valid-time range index invalidation 和 hierarchical time graph 仍待实现 | P2 |

## 5. 阶段关闭和开放工作

当前本地 GraphRAG 主路径已经从 Phase 1 推进到 Phase 4 并关闭；后续实现应把这些阶段当成回归基线，而不是继续作为待办清单。

| 阶段 | 关闭状态 | 仍开放的产品化方向 |
| --- | --- | --- |
| Phase 1 真实检索闭环 | Typed facts、source span、confidence、BM25 aliases、context pack、graph paths 和 code artifact 已关闭。 | 只保留回归保护。 |
| Phase 2 可恢复索引刷新 | Mutation log、scoped cursor、bounded queue、lease/retry/dead-letter、startup reconciler 和 stale reasons 已关闭。 | 只保留容量、故障和恢复测试扩展。 |
| Phase 3 Agent/服务基础 | MCP Streamable HTTP、本地 ACP、resources/prompts、metrics、audit sink、QoS 和 Web operations 已关闭。 | 远端 ACP、A2A gateway、host integration 和特权服务生命周期仍开放。 |
| Phase 4 高级 GraphRAG/多模态基础 | Local semantic/vector、schema/temporal/community retrieval、多模态 schema、worker proposal contract 和 evaluation fixture gate 已关闭。 | 具体外部 provider、query router、lite-global/DRIFT、release 评测报告仍开放。 |

Web 工作区当前通过 `/api/web/operations/execute` 执行 retrieve、ingest、graph inspect、index refresh、code repository workflow 和 service status/run snapshot，并在成功后刷新诊断状态。`service run` 会挂载 Web endpoints；启用 MCP Streamable HTTP 时，MCP 与 Web routes 合并到同一 `net::http` listener 和 QoS budget。

## 6. 工程约束

后续实现必须继续遵守项目硬约束:

- `env` 只负责环境变量，`paths` 只负责平台路径，`net` 只负责网络和 HTTP 能力。
- I/O、数据库、tree-sitter、embedding、OCR、索引 rebuild 和 compaction 不得阻塞 async runtime hot path。
- 所有队列有界，所有检索和图遍历有 limit、timeout、cancellation 和 truncated/degraded 状态。
- CLI、Web、HTTP、MCP 和 ACP 共享 application service，不复制业务逻辑。
- 新 public API 必须有生产调用方或规格支撑，并配套测试。
- 文档与实现同步更新，尤其是配置、环境变量、路径、网络、QoS、索引、后台服务和安装部署行为。

## 7. 推荐下一步

短期最有价值的实现顺序:

1. 为外部 embedding/OCR/vision/table/layout provider 增加具体 worker adapter、模型共存刷新策略、provider 级限流和生产诊断。
2. 建立 service manager install/upgrade/uninstall、rollback、package manifest、watchdog 和维护任务的端到端产品路径。
3. 为 valid-time、conflict resolution 和事实审批 UI 补齐产品语义；当前 proposal 持久化 provenance、manual-review policy、accept/reject/supersede 和 proposed structured facts 已关闭。
4. 规划 query router、lite-global/DRIFT-like expansion、外部 rerank provider 和 A2A gateway，但保持 `HybridRetrievalResponse` 作为 canonical context pack。
5. 扩充 GraphRAG evaluation fixture 数据集规模、长期指标报告和 release-facing 质量阈值，继续覆盖 stale index、ambiguous entity、多跳、时间和 code impact。

这一路线能最大限度复用当前实现，同时把研究材料中最关键的 GraphRAG、Agentic KG、Tree-sitter 代码图和后台新鲜度能力落到可测试的工程闭环。
