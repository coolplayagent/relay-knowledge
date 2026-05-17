# GraphRAG 产品与实现路线规格

[中文](../../zh/03-architecture-specs/02-graphrag-product-and-implementation-roadmap.md) | [英文](../../en/03-architecture-specs/02-graphrag-product-and-implementation-roadmap.md)

> 文档版本: 1.2
> 编制日期: 2026-05-12
> 进展刷新: 2026-05-17
> 范围: relay-knowledge 的 GraphRAG 产品边界、当前实现基线、已关闭阶段和开放产品化工作。

行业能力刷新见 [2026 行业能力快照与差距分析](../04-research/01-industry-capability-snapshot-2026.md)。本规格以该快照为外部基线，但仍以本地优先、统一 API 和可解释 context pack 为产品边界。

## 1. 定位

`relay-knowledge` 的核心定位是本地优先的 knowledge substrate。它负责事实、证据、图版本、source scope、派生索引、检索、诊断、QoS 和审计，不内置通用 agent runtime，也不直接生成最终 LLM 答案。

外部 agent runtime、CLI、Web、MCP 和本地 ACP adapter 只能通过统一 application API 访问能力。它们不得直接访问 SQLite、Git、tree-sitter parser、索引 writer、环境变量、运行时路径或网络 socket。

GraphRAG 能力必须保持可解释:

- 每次检索返回 `graph_version`、freshness、source scope、retriever source、ranking explanation、truncated/degraded 状态和预算消耗。
- LLM 或 agent 输出只能进入 proposal、diagnostic、summary 或 derived index，不能绕过 graph mutation contract 直接覆盖 accepted facts。
- BM25、semantic、vector 和 graph expansion 是协同召回源，不互相替代。
- semantic/vector 是派生 read model，不能成为事实真源。

## 2. 当前基线

当前 Rust 实现已经具备以下可复用基础:

- 统一 API: ingest、hybrid retrieval、graph inspection、index refresh、health、service status 和 code repository API。
- 异步 application service: CLI、Web、MCP adapter 共用 `RelayKnowledgeService`，阻塞 SQLite 和 Git/tree-sitter 工作被隔离到边界内。
- SQLite 图状态: evidence/entity、typed relation、claim、event、graph mutation log、graph version、index status 和 code graph tables。
- 混合检索: SQLite FTS5 BM25 read model、graph evidence fallback、code graph documents、local semantic/vector read model、可配置 external backend metadata、schema path、temporal/community retrieval、RRF 融合、source span/structured fact context pack 和 backend availability metadata。
- 代码仓库能力: Git 仓库注册、full/incremental index、worktree overlay、tree-sitter 多语言解析、symbol/reference/chunk 查询和 diff impact。
- Agent 接入基础: MCP Streamable HTTP server、本地 ACP session adapter、session/protocol header 校验、access policy、QoS admission、tool-level graph retrieval、graph inspect、health、service status、index status、授权 code graph query、授权 code impact 和受权限控制的 index refresh。
- Web 诊断面: `/api/project/status` 和 `/api/health` 驱动的 health、index、runtime、operation composer 和 GraphRAG readiness 视图。

这些能力已经从 v1 底座推进到 Phase 4 的本地 GraphRAG read model: typed fact schema、scoped index cursor、后台 task lease/reconciler/dead-letter、local semantic/vector read model、多模态 evidence schema、schema path、temporal query、community summary 和 evaluation harness 已有 Rust 实现。外部 embedding/OCR/vision/extractor worker 现在有持久队列、HTTP worker contract 和 deterministic fallback；proposal lifecycle、service manager 定义生成、持久 audit sink 和 silent-update operator state 已形成统一 CLI/Web/service API 面。

## 2.1 行业差距刷新

对照 2026 行业能力，当前主要差距如下:

- GraphRAG query strategy: 当前已有 local-style hybrid retrieval 和 community summary context item，但还没有显式 query router、lite-global search 或 DRIFT-like expansion 接口。
- MCP completeness: 当前 tool surface、session header、protocol version、resources、prompts、DELETE session termination 和 metrics endpoint 已落地；GET/SSE resumability 仍未产品化。
- A2A gateway: 当前只在架构上保留 A2A 方向；还没有 agent card、task lifecycle、artifact mapping、signed identity 或 gateway 测试。
- Provider productization: 外部 text/image embedding、OCR、caption、table/layout extractor 仍只有 contract 和维护边界，没有具体 provider。
- Install/service productization: service manager 安装、silent update operator、rollback 和 release diagnostics 仍是文档规格，未成为端到端用户路径。
- Ease of use: 用户入口必须保持零配置默认，不应把 embedding、QoS、HTTP、MCP 和目录变量放在 quickstart 主路径。

## 3. 优化措施

### 3.1 检索与 Context Pack

- 保持 `HybridRetrievalResponse` 作为 canonical context pack，不新增只返回自然语言答案的 core API。
- 新增检索策略时应优先作为 context pack 的 strategy metadata 和 retriever contribution 表达，而不是新增并行 answer API。
- 后续 query router 应在 `local`、`global/community`、`lite-global` 和 `drift-like` 策略之间选择，并把选择原因、预算和降级状态返回给调用方。
- BM25 字段质量优先覆盖 evidence content、entity label、source path、code symbol、code chunk 和 doc comment。
- RRF 融合必须保留每个 retriever 的 rank、score 和 explanation。
- graph expansion 必须限制深度、节点数、时间和输出字节，超限时返回 `truncated=true` 和原因。
- semantic/vector 默认使用确定性的本地 token/hash read model，已在 ranking explanation 和 cursor diagnostics 中记录 model、dimension、source hash、scope、backend cursor 和 graph version；runtime backend mode 支持 `local`、`external` 和 `disabled`，`disabled` 会跳过对应 retriever 与 refresh，外部 embedding worker 必须保持同一 metadata contract 和 scope post-filter。

### 3.2 事实模型

- 通用图已从 evidence/entity 扩展到 typed relation、claim、event、confidence、source span 和 status；worker 输出先进入 proposal lifecycle，人工 accept 后才通过 graph mutation contract 写入。
- `graph_version` 表示系统提交版本，用于 replay、index cursor 和 stale 判断。
- 事实有效时间使用 `valid_from`、`valid_to`、`observed_at` 和 `source_published_at`，不得用 `graph_version` 替代。
- 冲突事实不能静默覆盖，应通过 status、confidence、source span 和 proposal/approval 流程表达。

### 3.3 代码知识图谱

- 继续借鉴 code-review-graph 的 SQLite + FTS5 + tree-sitter + SHA-256 增量路线，但保留 relay-knowledge 的 async-first、scope、QoS 和统一 API 边界。
- 代码结果必须返回 repository、resolved commit、tree hash、path filters、language filters、line range、symbol/reference id 和 parse diagnostics。
- tree-sitter 输出是 syntax-level fact。无法唯一解析的调用、引用或 import 必须保留 `unresolved` 或 `ambiguous` 状态，不能写成确定关系。
- 增量流程应按 changed paths、content hash skip、tombstone、reverse dependents 和 scoped refresh 缩小工作集。

### 3.4 索引刷新与后台恢复

- v1 已经把 `refresh_indexes`、ingest 后置刷新和 `wait-until-fresh` 查询接到
  bounded refresh queue、persistent task lease、retry backoff、mutation-log
  replay 和 scoped cursor 更新路径。
- index cursor 必须按 kind、scope、modality、model 和 graph version 记录，不能只用全局 freshness 代表所有快照；当前 Rust 实现已覆盖 kind/scope/text modality、source hash、backend cursor，以及 semantic/vector worker 可回传的 model name/dimension 元数据。
- 后台服务必须使用 bounded queues、retry backoff、lease、dead-letter、startup reconciler 和 stale diagnostics；当前 foreground service path 已暴露 queue depth、oldest task age、dead-letter count、per-kind lag 和结构化 stale reasons。
- 启动时如果 graph version 领先 index cursor，reconciler 必须补发刷新或报告 degraded；当前 health/service status 会补发缺失 refresh task，显式 `refresh_indexes` 负责 drain。

### 3.5 Agent 与服务化

- MCP/ACP adapter 只做协议转换、identity 注入、access policy、QoS admission、cancellation、错误映射和审计。
- 默认 agent policy 只读。index refresh、mutation、merge、rebuild 或 service operation 需要显式许可。
- 所有 adapter 请求都必须经过 `net::qos`，包括 HTTP transport 和未来 stdio/session transport。
- service mode 由 systemd、Windows Service 或 launchd 管理；应用内部只负责 graceful shutdown、heartbeat、任务恢复和 diagnostics。

### 3.6 多模态与时间图谱

- PDF、图片、OCR、caption、table 和 layout region 统一建模为 evidence 或 derived evidence。
- 原始 evidence 保存 source URI/hash、media hash、modality、extractor、extractor version、scope 和 parent evidence。
- 检索时合并同一 parent evidence 的文本、OCR、caption、image 和 table hit，避免重复展示。
- temporal query 必须支持 `as_of` 或 time range，并参与 index invalidation 与 context pack metadata。

当前 Rust 实现已支持 evidence modality/extraction metadata、OCR/caption/image embedding parent grouping、`as_of:<date>` 与年份事件检索，以及 community summary context item。生产级 OCR、vision caption、embedding payload 适配和 worker metrics 仍需在现有 worker contract 上扩展。

### 3.7 易用性与配置分层

- 默认本地 profile 必须零配置可用: 本地 SQLite、平台目录、本地 deterministic semantic/vector read models、保守网络/QoS 和只读 agent policy。
- README 和用户指南主路径只展示 `status`、`ingest`、`query`、`health` 等最小闭环；环境变量清单集中到高级配置文档。
- `setup doctor` 和 `setup profile` 已作为只读诊断/推荐输出落地，只输出推荐配置、风险提示和下一步命令，不要求用户手写大量变量。
- 配置分层固定为 Basic、Advanced、Deployment 和 Diagnostic。Basic 面向 CLI 参数，Advanced 面向检索/网络/MCP，Deployment 面向 service manager，Diagnostic 面向 CI 和故障复现。

## 4. 阶段关闭状态

本节是当前规格的关闭台账。已关闭阶段不再作为待办项描述，而应通过回归测试、用户文档和验证记录持续保护；仍未产品化的事项集中到 Phase 5。

### Phase 1: 真实检索闭环（已关闭）

- Typed relation、claim/event、source span、confidence、status、version-range validation 和 supporting evidence 校验已进入 domain/storage/API 主路径。
- Context pack 已覆盖 evidence、entity、code symbol、code chunk、source span、structured facts、direct graph path evidence、ranking explanation、freshness、truncation 和 backend availability metadata。
- BM25/lexical 文档已覆盖 source path、entity/code symbol aliases、code symbol、code chunk 和 doc comment；semantic/vector backend status、scope post-filter metadata 和 Web readiness 已落地。

### Phase 2: 可恢复索引刷新（已关闭）

- Scoped index cursor、mutation log affected metadata、bounded index refresh queue、active lease/attempt guard、retry backoff、lease-expiry dead-letter 和 startup reconciler 已落地。
- Semantic/vector cursor 已持久化 model、dimension、source hash、backend cursor 和 last error；refresh completion 可从已索引文档推导 model/dimension。
- `health`、`service doctor`、显式 refresh 和 Web readiness 已返回 queue depth、oldest task age、dead-letter count、index lag 和结构化 stale reasons。

### Phase 3: Agent 与常驻服务基础（已关闭）

- MCP Streamable HTTP、本地 ACP session adapter、QoS admission、cancellation、scope policy、code graph query/impact、index refresh 权限控制和错误映射已共享 application service。
- MCP resources/prompts、`/mcp/metrics` Prometheus exporter、bounded audit log、持久 audit sink 和 `audit query` 已落地。
- Service manager v1 已生成 systemd/launchd/Windows Service 定义和命令预览；silent-update operator state 已可 status/pause/resume。提权安装、卸载和 rollback 仍属于 Phase 5 产品化。

### Phase 4: 高级 GraphRAG 与多模态基础（已关闭）

- Local semantic retrieval、hashed-vector ANN read model、RRF fusion、本地确定性 rerank、schema-guided path、temporal query 和 community summary 已落地。
- Multimodal evidence schema、extractor diagnostics、image/OCR/caption/table/layout modality、parent evidence grouping 和 maintenance worker 提交边界已落地。
- 外部 worker 已有 HTTP contract、持久队列、deterministic fallback proposal 和人工 review policy；具体生产 provider adapter 仍属于 Phase 5 产品化。
- Evaluation harness 和 CI fixture gate 已覆盖 exact fact、multi-hop、temporal、negative rejection、stale index、ambiguous entity 和 code impact。

### Phase 5: 易用性与互操作产品化（开放）

- 已关闭: `setup doctor`、`setup profile local|agent-readonly|service|external-embedding`、MCP resources/prompts、Streamable HTTP session termination、service definition preview、silent-update status/pause/resume、持久 audit sink 和 metrics exporter。
- 开放: 特权 service install/upgrade/uninstall、rollback、release diagnostics、包管理器 manifest 和安装后 watchdog/maintenance 工作流。
- 开放: 具体外部 embedding/OCR/vision/table/layout provider adapter、模型共存策略、生产 worker metrics 和 provider 级运维文档。
- 开放: query router、lite-global/DRIFT-like expansion、A2A gateway、远端 ACP host integration、更大真实数据集和 release-facing 质量阈值。

## 5. 验收要求

- 所有新增 public API 有生产调用方或规格支撑，并配套单元测试或集成测试。
- CLI、Web、MCP 和 ACP 共享 application service，不复制业务逻辑。
- 新增 I/O、数据库、embedding、OCR、parser、index rebuild 和 compaction 不得阻塞 async runtime hot path。
- 所有队列、检索和遍历有 limit、timeout、cancellation、budget、truncated/degraded 状态。
- 文档与实现同步更新，尤其是配置、环境变量、路径、网络、QoS、索引、后台服务、安装部署和用户可见功能。
- 新手文档不得把高级配置当成必经步骤；高级变量必须有默认行为、适用场景和回退说明。
