# relay-knowledge Implementation Reference

[English](../../en/04-research/relay-knowledge-implementation-reference.md) | [中文](../../zh/04-research/relay-knowledge-implementation-reference.md)

This is the English documentation page for `research/relay-knowledge-implementation-reference.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> 编制日期: 2026-05-13
> 进展刷新: 2026-05-13
> 范围: 结合 docs 中的知识图谱、GraphRAG、Agentic KG、Tree-sitter 代码图、协议接入和后台服务材料，对照当前 Rust 实现，提炼后续实现路线。
> 定位: 工程借鉴文档，不替代 `docs/zh/03-architecture-specs/` 中的硬约束和接口规格。

## 1. 执行结论

`relay-knowledge` 当前已经具备一个可继续演进的知识图谱底座: 统一 API、异步 application service、SQLite 图状态、图版本、结构化事实、索引新鲜度元数据、带 source hash/backend cursor/model metadata 的 scoped index cursor、bounded refresh queue、task lease/reconciler 诊断、结构化 stale reasons、FTS5 BM25 read model、local semantic/vector read model、可配置外部 semantic/vector embedding 后端契约、schema path/temporal/community retrieval、RRF context pack、本地确定性 rerank、Tree-sitter 代码仓库索引、多模态 evidence schema、后台/maintenance 多模态抽取提交入口、MCP Streamable HTTP、本地 ACP session adapter、MCP resources/prompts、Prometheus metrics exporter、可选 JSONL audit sink、CLI/Web 入口、GraphRAG evaluation fixture gate、`env`/`paths`/`net` 基础边界和 QoS 配置。它还不是完整服务安装产品，当前差距集中在外部 embedding/OCR/vision worker、安装后的 service manager/silent update operator、proposal/conflict lifecycle、远程 ACP adapter 和 extractor 产品化。

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

需要明确的是，当前实现仍是 v1 底座，不是完整产品形态:

- `retrieve_context` 已经使用 SQLite FTS5 BM25、graph evidence fallback、code graph documents、local semantic token read model、local hashed-vector ANN read model、schema path、temporal event、community summary、RRF context pack 和本地确定性 rerank；context item 会携带 structured facts、由 facts 派生的一跳 `graph_paths`、source span、code artifact、rerank signal 和 backend availability metadata。semantic/vector backend status 现在由 read model cursor 与 runtime backend 配置生成，支持 `local`、`external` 和 `disabled` 模式。
- `index_status` 记录了 BM25、semantic、vector 等索引家族的聚合新鲜度；scoped cursor 按 kind/scope/modality 记录 graph version、source hash、backend cursor，并允许 semantic/vector worker 在完成任务时写入 model name/dimension。`refresh_indexes` 会调度持久化 task、获取 lease、replay mutation log 并更新 cursor；semantic/vector refresh completion 会从已索引文档推导模型元数据，避免 runtime label 与实际 read model provenance 分离。BM25 文档随 evidence/code graph 写入更新，并为 entity labels 与 code symbols 记录生成式 lexical alias 字段；semantic/vector read model 随 evidence 写入记录 model、dimension、source hash、scope 和 graph version metadata。
- 通用知识图谱已经从 evidence/entity 扩展到 typed relation、claim/event、confidence、source span、status 和 version-range validation；valid time、conflict state 和 proposal lifecycle 仍未形成完整产品闭环。
- 后台服务状态已暴露为 API，foreground `service run` 启动时会执行最小 startup index reconciler；foreground refresh 主路径已具备任务表、leases、retry、dead-letter 计数、reconciler 补发、stale diagnostics 和按索引族/scope 归因的 stale reasons。service manager 安装、silent update 配置、维护任务和 operator 工作流仍主要停留在规格。
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
| 统一 API | 已有 ingest/query/context pack/status/health/code repo/agent identity API | audit metadata、protocol diagnostics 和更细的 context artifact 仍需扩展 | P0 |
| 图事实模型 | evidence/entity、typed relation、claim/event、confidence、source span、status、多模态 extraction metadata + graph version | valid time、proposal/conflict lifecycle 和事实审批流缺失 | P0 |
| 混合检索 | 有 BM25、graph evidence、code graph documents、local semantic/vector、可配置 external backend metadata、path/temporal/community、RRF、本地 rerank 和 context pack | 预算化深图 expansion 和外部模型 rerank provider 仍待接入 | P0 |
| 代码图 | 已有 tree-sitter 索引和查询 | 需要更强 scope metadata、预算控制和 agent context pack | P0 |
| 后台服务 | 有 status API、foreground service run、最小 startup index reconciler、foreground refresh queue、task lease/reconciler 诊断、dead-letter 元数据和 metrics exporter | 缺 service install、watchdog、silent update operator 和维护任务 | P1 |
| Agent 接入 | 已有 MCP Streamable HTTP、本地 ACP adapter、access policy、QoS、audit log、可选 JSONL audit sink、code graph query/impact tools、resources/prompts | 远程 ACP adapter 和更完整 host integration 仍需后续产品化 | P1 |
| 多模态 | 有 evidence modality/extraction schema、extractor diagnostics、parent grouping、modality read model metadata 和 maintenance worker 输出提交边界 | 真实 OCR/caption/table/layout worker 和 image embedding backend 仍待接入 | P2 |
| 时间图谱 | 有 graph version、event `occurred_at` 和 `as_of`/年份 temporal retrieval | valid-time range index invalidation 和 hierarchical time graph 仍待实现 | P2 |

## 5. 分阶段路线

### Phase 1: 让现有闭环更真实

目标是把当前 v1 底座从“BM25/RRF 可用”推进到“GraphRAG context 更完整”:

- 保持 typed relation、claim/event、evidence span、confidence、status 和 version range 的 domain/storage/API 回归覆盖。
- 继续验证 ingest 边界传入的 span、confidence 和 version range，并要求 structured facts 引用 supporting evidence ids。
- 继续增强 `HybridRetrievalResponse` context pack，保留更多实体、关系、路径、source span 和 code graph artifact。
- 保持 `rejected`/`superseded` evidence 不进入 grounding context，只作为可检查图状态保留。
- 提升 BM25 字段质量，覆盖 entity aliases、source path、doc comment、code symbol 和 code chunk。当前 SQLite FTS5 read model 已为 entity labels 和 code symbols 写入生成式 lexical alias 字段，且不会把 alias 混入返回的 canonical labels。
- 接入真实 semantic/vector read model 前，保持 backend unavailable metadata、scope post-filter metadata 和可解释降级语义。
- 完善 code repository response 的 scope 和 index metadata，使 agent 能稳定引用 commit、tree hash、path 和 symbol id。

截至 2026-05-13，Phase 1 剩余实现点已补齐: context pack 从
relation/claim/event 结构化 facts 派生 direct graph path evidence；BM25 read
model 增加独立 `entity_aliases` 字段以支持 entity/code symbol lexical alias
召回；相关回归测试和用户文档已同步。

### Phase 2: 建立索引刷新和后台恢复主路径

目标是让 graph mutation 和 derived indexes 形成可恢复闭环:

- 巩固 mutation log、affected scope/source hash、scoped cursor、bounded refresh queue、active lease/attempt guard、retry/dead-letter 和 stale diagnostics 的已落地主路径。
- 保持 health、service doctor、index refresh 和 Web readiness 暴露结构化 stale reasons；reason 应说明 index family 或 scoped cursor 的 failed、lag、not-fresh 或 last-error 状态。
- 保持 semantic/vector 接入所需的 model、dimension、source hash 和 backend-specific cursor 元数据已落地路径: refresh completion 可写入 model name/dimension，cursor 诊断返回 source hash 和 backend cursor。
- 保持 startup/diagnostics reconciler 行为: graph version 领先 index cursor 时补发 refresh 或报告 degraded；显式 refresh/wait-until-fresh 在 queue cap 阻止必要入队时返回错误。
- 保持 running refresh task 的 claimed target 不被后续 enqueue 覆盖；如果完成期间同 scope 出现新 mutation，则完成路径重置普通 attempt 计数并重新排队后续 refresh。
- 诊断 reconciler 保留 dead-letter 隔离，避免 health/status 流量自动复活 attempt-exhausted task；只有显式 refresh/retry 路径可以重置。

### Phase 3: Agent 协议和常驻服务

目标是让外部 agent 安全访问图检索能力:

- 保持 agent access policy 默认只读，index refresh 和 mutation 类操作需要显式许可。
- 巩固 MCP Streamable HTTP 和本地 ACP session adapter 已共享的 retrieval mapping、code graph query/impact、QoS admission、cancellation 和 bounded audit log。
- 保持 MCP resources/prompts、可选 JSONL audit sink、metrics exporter。
- 所有 adapter 请求进入 `net::qos`，即使后续 stdio/session transport 也要计入 in-flight 和 queue budgets。
- service mode 交给 systemd、Windows Service 或 launchd，应用内部只做 graceful shutdown、heartbeat 和任务恢复。

当前 Phase 2 已落地 mutation log affected metadata、scoped cursor source hash/backend
cursor、semantic/vector model metadata contract、bounded refresh queue、lease/attempt
guard、retry/dead-letter、startup reconciler、queue-cap 错误、dead-letter 隔离和
结构化 stale reasons。真实 semantic/vector read model 仍留在 Phase 4。

当前 Phase 3 已落地 MCP code graph/impact tools、MCP resources/prompts、Prometheus metrics exporter、本地 ACP session adapter、bounded
audit log、可选 JSONL 持久 audit sink、adapter QoS admission 和 `service run`
startup index reconciler。剩余项主要是平台 service install/upgrade/uninstall、跨进程
worker orchestration、watchdog 集成、silent update operator 和更完整的远程 ACP adapter。

当前 Web 工作区进展已刷新: diagnostics 继续读取 `/api/project/status` 和
`/api/health`；Operations 面板现在可以通过 `/api/web/operations/execute`
执行 retrieve、ingest、graph inspect、index refresh、code repository workflow 和
service status/run snapshot，并在成功后刷新诊断状态。Rust Web adapter 只做 HTTP
JSON 解析、错误映射和 QoS 之外的最小路由分发，业务行为仍由 `RelayKnowledgeService`
统一提供。`service run` 会挂载 Web endpoints；启用 MCP Streamable HTTP 时，MCP 与
Web routes 合并到同一 `net::http` listener 和 QoS budget。

### Phase 4: 高级 GraphRAG 与多模态

目标是增强复杂问答、全局理解和多模态来源:

- 已引入 local semantic retrieval 和 hashed-vector ANN read model，记录 model、dimension、source hash、scope 和 graph version。
- 已把 semantic/vector backend 配置接到 read model contract: `local`、`external` 和 `disabled` 模式从 `env` 进入 runtime，retrieval backend status 会携带配置的 model/dimension；`disabled` 会阻止对应 retriever 执行和 read model refresh 调度，refresh cursor completion 会携带已索引文档的 model/dimension。
- 已增加 path retrieval、schema-guided traversal、`as_of`/年份 temporal query 和 community summary。
- 已增加 multimodal evidence schema，支持 OCR、caption、image embedding、table、layout region、extractor diagnostics 和 parent evidence grouping；后台或 maintenance worker 可通过 `commit_multimodal_extraction` 提交派生 evidence，查询热路径不运行 OCR/caption/table/layout 工作。
- 已增加 evaluation harness 和 fixture CI gate，覆盖 exact fact、multi-hop、temporal、negative rejection、stale index、ambiguous entity 和 code impact。

剩余进展刷新:

- 外部 embedding/OCR/vision 现在有 runtime 与 maintenance 接入契约；后续工作是接入具体 provider、worker 调度和模型并存策略。
- evaluation harness 已进入 CI fixture gate；后续需要扩充真实数据集规模和指标报告。

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

1. 巩固已接入的 semantic/vector backend runtime contract，补 provider 级认证、限流、模型并存和重建策略。
2. 为外部 embedding/OCR/vision provider 增加具体 worker 实现、模型并存刷新策略和 provider 级诊断。
3. 为 proposal/conflict lifecycle、valid time 和事实审批流补齐 domain/API/storage 语义。
4. 建立 service manager install/upgrade/uninstall 与 silent update operator 的平台实现。
5. Done: 增加 MCP resources/prompts、持久 audit sink、metrics exporter。
6. 扩充 GraphRAG evaluation fixture 数据集规模和指标报告，继续覆盖 stale index、ambiguous entity、多跳、时间和 code impact。

这一路线能最大限度复用当前实现，同时把研究材料中最关键的 GraphRAG、Agentic KG、Tree-sitter 代码图和后台新鲜度能力落到可测试的工程闭环。
