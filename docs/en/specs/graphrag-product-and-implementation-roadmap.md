# GraphRAG Product and Implementation Roadmap

[English](../../en/specs/graphrag-product-and-implementation-roadmap.md) | [中文](../../zh/specs/graphrag-product-and-implementation-roadmap.md)

This is the English documentation page for `specs/graphrag-product-and-implementation-roadmap.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> 文档版本: 1.1
> 编制日期: 2026-05-12
> 进展刷新: 2026-05-13
> 范围: relay-knowledge 的 GraphRAG 产品边界、当前实现基线、优化措施和分阶段实现规格。

行业能力刷新见 [2026 行业能力快照与差距分析](../research/industry-capability-snapshot-2026.md)。本规格以该快照为外部基线，但仍以本地优先、统一 API 和可解释 context pack 为产品边界。

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
- MCP completeness: 当前 tool surface、session header 和 protocol version 已落地；resources、prompts、GET/SSE resumability、DELETE session termination 和旧 HTTP+SSE 兼容仍未产品化。
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
- 后续新增 `setup doctor` 和 `setup profile` 时，应只输出推荐配置、风险提示和下一步命令，不要求用户手写大量变量。
- 配置分层固定为 Basic、Advanced、Deployment 和 Diagnostic。Basic 面向 CLI 参数，Advanced 面向检索/网络/MCP，Deployment 面向 service manager，Diagnostic 面向 CI 和故障复现。

## 4. 分阶段路线

### Phase 1: 真实检索闭环

- 保持 typed relation、claim/event、source span 和 confidence 的 domain/storage/API 规格可回归测试。
- 对 ingest 边界重新验证反序列化后的 span、confidence 和 version range；结构化 facts 必须引用 supporting evidence ids。
- 让 context pack 覆盖 evidence、entity、code symbol、code chunk、source span、structured graph facts 和 direct graph path evidence；当前实现已从 structured relation/claim/event 派生 `graph_paths`。
- 检索候选只使用 `accepted`/`proposed` evidence，`rejected`/`superseded` evidence 保留为可检查图状态但不作为 grounding context。
- 增强 BM25/lexical 文档构建字段，覆盖 source path、entity/code symbol lexical aliases、code symbol、code chunk 和 doc comment，并补充 ranking explanation 测试；当前 SQLite FTS5 read model 已为 entity labels 和 code symbols 写入独立 alias 字段。
- 为 semantic/vector 保留 backend status metadata、scope post-filter metadata 和 `local`/`external`/`disabled` runtime mode。
- Web readiness 继续从 health/status 显示 BM25、semantic cursor、vector cursor、code graph、runtime budgets 和 index lag。

### Phase 2: 可恢复索引刷新

- 保持已落地的 scoped index cursor、mutation log affected metadata、bounded index refresh queue、active lease/attempt guard、retry backoff、lease-expiry dead-letter 和 startup reconciler 可回归测试。
- 为 semantic/vector backend 保持 model、dimension、source hash、backend-specific cursor 和 last error 元数据的持久化/API contract；refresh worker 完成任务时从已索引文档推导 model/dimension 并由 cursor 诊断返回。
- health/service doctor 继续返回 queue depth、oldest task age、dead-letter count、index lag 和结构化 stale reasons；每条 reason 必须能指向索引族或 scoped cursor，并携带 lag versions 和 last error。

### Phase 3: Agent 与常驻服务

- 保持 MCP read-only 工具矩阵: retrieve context、inspect graph、index status、service doctor、code graph query 和 code impact。
- 保持本地 ACP 会话入口，支持 progress、cancellation、context artifact 和 runtime identity。
- 保持 MCP resources/prompts: service status、health、index status、policy-gated graph summary、Prometheus metrics resource、retrieval prompt 和 code-impact prompt。
- 保持旧 HTTP+SSE 兼容入口 `/mcp/sse` + `/mcp/message`，但新集成优先使用 Streamable HTTP `/mcp`。
- 保持 bounded in-process audit log，记录 identity、scope、freshness、QoS decision、budget、truncation 和 result count；CLI/Web/service operation 写入持久 audit sink 并通过 `audit query` 暴露；MCP/ACP 可选 JSONL 持久 audit sink 通过有界 async queue 写入 `logs/agent-audit.jsonl`。
- 保持 `/mcp/metrics` Prometheus text exporter，覆盖 graph version、index refresh backlog、dead letter、QoS request counters 和 per-index stale 状态。
- service manager v1 生成 systemd/launchd/Windows Service 定义和安装/卸载/启动/停止命令预览，不在 CLI 内执行提权安装；silent-update operator state 可 status/pause/resume。
- 安装/升级/卸载文档必须覆盖 service manager 模板、运行时目录、rollback 和 diagnostics。

### Phase 4: 高级 GraphRAG

- 已接入 local semantic retrieval 和 hashed-vector ANN read model，支持 model、dimension、source hash、scope 和 graph version metadata。
- 已接入 semantic/vector backend runtime contract，支持 `local`、`external` 和 `disabled` 状态、disabled execution gate 以及 refresh cursor model metadata。
- 已增加 path retrieval、schema-guided traversal、community summary 和 temporal query。
- 已增加 multimodal evidence schema、extractor diagnostics、image/OCR/caption/table/layout modality、parent evidence grouping 和 maintenance worker 输出提交边界。
- 已建立 evaluation harness 和 CI fixture gate，覆盖 exact fact、multi-hop、temporal、negative rejection、stale index、ambiguous entity 和 code impact。

剩余 Phase 4 产品化工作:

- 扩展外部 worker 响应 contract，覆盖生产 embedding vector payload、OCR/caption/table/layout 结果、metrics exporter 和 release diagnostics。
- 将 evaluation harness 接到 CI fixture 和后续 release diagnostics。
- 规划 query router、lite-global 和 DRIFT-like expansion，不改变 `HybridRetrievalResponse` 的 canonical context pack 地位。

### Phase 5: 易用性与互操作产品化

- 实现 `setup doctor`，检查运行时目录、SQLite、索引 freshness、Web 诊断、MCP policy、服务安装状态和常见配置错误。
- 实现 `setup profile local|agent-readonly|service|external-embedding`，输出推荐配置和安全说明；默认 profile 是 `local`，无需环境变量。
- 产品化 MCP resources/prompts 和 Streamable HTTP resumability/session termination，保持 tool policy 动态可见。
- 设计 A2A gateway 的 agent card、task lifecycle、artifact mapping 和 signed identity 边界，仍通过统一 API 调用 core。
- 将 service manager 安装、silent update、rollback 和 release diagnostics 纳入端到端验收。

## 5. 验收要求

- 所有新增 public API 有生产调用方或规格支撑，并配套单元测试或集成测试。
- CLI、Web、MCP 和 ACP 共享 application service，不复制业务逻辑。
- 新增 I/O、数据库、embedding、OCR、parser、index rebuild 和 compaction 不得阻塞 async runtime hot path。
- 所有队列、检索和遍历有 limit、timeout、cancellation、budget、truncated/degraded 状态。
- 文档与实现同步更新，尤其是配置、环境变量、路径、网络、QoS、索引、后台服务、安装部署和用户可见功能。
- 新手文档不得把高级配置当成必经步骤；高级变量必须有默认行为、适用场景和回退说明。
