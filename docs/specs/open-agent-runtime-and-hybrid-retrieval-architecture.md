# 开放 Agent Runtime 与混合检索架构

> 文档版本: 1.0
> 编制日期: 2026-05-11
> 适用范围: 外部 Agent Runtime 集成、LLM 知识处理、混合检索、模块解耦和后续 MCP/ACP/A2A adapter 设计
> 默认路线: 支持开放 runtime，不内置 runtime；知识图谱和检索能力通过统一 API 暴露

## 1. 设计结论

`relay-knowledge` 应定位为 **knowledge substrate**，而不是 agent runtime。项目负责图谱事实、证据、版本、索引、检索、事件和审计；外部 agent runtime 负责 planning、tool calling、handoff、approval、长任务状态和最终 LLM 交互编排。

核心结论:

1. **支持开放 runtime，但不实现 runtime**: 兼容 MCP、A2A 或本地 SDK bridge 等 adapter 形态，但 core 不依赖 LangGraph、OpenAI Agents SDK、CrewAI、AutoGen 或任何单一框架。
2. **Agent 只能通过统一 API 访问知识系统**: runtime adapter 不直接访问 SQLite、索引表、事件队列或 mutation log。
3. **LLM 输出默认是候选事实**: 实体、关系、claim、摘要和冲突判断都先进入 proposal / validation / approval 流程，不能绕过 graph mutation contract 直接写入 accepted facts。
4. **混合检索是 agent 可用性的基础能力**: BM25、semantic retrieval、vector retrieval 和 graph expansion 必须协同工作，才能同时覆盖精确术语、概念查询、相似内容、多跳关系和代码影响分析。
5. **所有 agent action 可审计**: 每次检索、候选 mutation、验证、提交和索引刷新都必须携带 `trace_id`、`runtime_identity`、`source_scope`、`graph_version`、`index_versions` 和降级状态。

常驻进程对其它 agent 暴露图检索能力的协议级细节，见
[常驻进程 Agent 图检索访问规格](resident-agent-graph-retrieval-access.md)。

## 2. 外部架构洞察

本设计参考以下公开架构方向，但不绑定其实现:

| 方向 | 关键洞察 | 对 `relay-knowledge` 的影响 |
| --- | --- | --- |
| [Model Context Protocol architecture](https://modelcontextprotocol.io/specification/2025-06-18/architecture) | MCP 采用 host / client / server 架构，server 暴露 tools、resources、prompts，host 管理权限、生命周期和上下文隔离 | `relay-knowledge` 适合做 MCP server，但不应接管 host/runtime 职责 |
| [OpenAI Agents SDK](https://developers.openai.com/api/docs/guides/agents) | Agent 应用负责 orchestration、tool execution、approvals、state、handoffs 和 tracing | 这些运行时职责应留给外部应用，core 只提供知识工具和审计数据 |
| [A2A protocol](https://github.com/a2aproject/A2A) | A2A 面向不同框架和厂商 agent 的发现、能力协商、长任务协作和结果交换 | 后续可提供 A2A gateway，让知识服务成为 specialist agent 能力，而不是内置多 agent 系统 |
| [GraphRAG Query Engine](https://microsoft.github.io/graphrag/query/overview/) | Local search 结合图谱和原文 chunks，global search 使用 community reports，DRIFT 扩展局部检索起点 | 检索层应保留局部实体检索、全局摘要检索和社区信息扩展能力 |
| [LightRAG](https://arxiv.org/abs/2410.05779) | 图结构与向量表示结合可以提升实体关系检索和增量更新效果 | 三层检索不能只做向量搜索，必须把图关系、文本索引和索引新鲜度纳入主路径 |
| [Agent interoperability survey](https://arxiv.org/abs/2505.02279) | MCP、ACP、A2A、ANP 分别覆盖工具访问、结构化消息、agent 协作和开放发现 | v1 从 MCP tool/resource/prompt 暴露开始，保留更高层 agent 协议 adapter 的扩展点 |

## 3. 总体架构边界

```text
+-------------------------------------------------------------+
| External Agent Runtime / Host                              |
| planning, tool calls, handoffs, approvals, long-run state   |
+-----------------------------+-------------------------------+
                              |
                              v
+-------------------------------------------------------------+
| Agent Runtime Adapter                                       |
| MCP server / A2A gateway / local SDK bridge                 |
+-----------------------------+-------------------------------+
                              |
                              v
+-------------------------------------------------------------+
| Unified API Contract                                        |
| request, response, stream events, metadata, stable errors    |
+-----------------------------+-------------------------------+
                              |
                              v
+-------------------------------------------------------------+
| Application Services                                        |
| policies, orchestration, validation, freshness, budgets      |
+-----------+-----------------+----------------+--------------+
            |                 |                |
            v                 v                v
     Retrieval           Indexing          Graph Storage
     hybrid recall       BM25/semantic/    facts, evidence,
     graph expansion     vector refresh    mutation log
            \                 |                /
             v                v               v
                    Domain + Observability
```

### 3.1 依赖方向

固定依赖方向:

```text
agent_adapter -> api -> application -> domain
                              |
                              +-> retrieval traits
                              +-> storage traits
                              +-> indexing traits
                              +-> event runtime traits
                              +-> observability traits
```

禁止事项:

- `agent_adapter` 不能持有数据库连接、向量库 client 或索引 writer。
- `domain`、`application`、`api` 不能出现 MCP、A2A、OpenAI、LangGraph 等 provider-specific 类型。
- Agent prompt、tool schema、runtime state 不能成为图谱事实真源。
- LLM answer 不能覆盖 evidence、claim 或 graph version。

### 3.2 模块职责

| 模块 | 职责 | 禁止事项 |
| --- | --- | --- |
| `agent_adapter` | 协议转换、tool/resource/prompt 暴露、runtime identity 注入、权限前置检查 | 不实现 planning，不访问 storage/indexing 细节 |
| `api` | 稳定 request/response/error/stream event 类型 | 不包含 runtime SDK 类型和业务编排 |
| `application` | 检索、mutation proposal、validation、approval、commit、freshness policy 编排 | 不拼接 SQL，不调用 LLM SDK |
| `retrieval` | BM25、semantic、vector、graph expansion、fusion、rerank、context packing | 不写图事实，不绕过 scope filter |
| `indexing` | 消费 graph mutation，刷新 scoped BM25/semantic/vector read models | 不生成 accepted facts |
| `storage` | 图事实、证据、版本、mutation log、index metadata | 不做 agent 编排或 prompt 处理 |
| `domain` | 实体、关系、claim、evidence、scope、版本、领域错误 | 不依赖协议和后端实现 |
| `observability` | traces、metrics、diagnostics、health 聚合 | 不改变业务决策 |

## 4. Agent Runtime Port

`relay-knowledge` 后续应定义面向 adapter 的知识端口。下面是能力边界，不是 v1 代码实现承诺。

```rust
pub trait AgentKnowledgePort {
    async fn retrieve_context(
        &self,
        request: HybridRetrievalRequest,
        context: RequestContext,
    ) -> Result<HybridRetrievalResponse, ApiError>;

    async fn inspect_graph(
        &self,
        request: GraphInspectionRequest,
        context: RequestContext,
    ) -> Result<GraphInspectionResponse, ApiError>;

    async fn propose_knowledge_mutation(
        &self,
        proposal: KnowledgeMutationProposal,
        context: RequestContext,
    ) -> Result<AgentTaskReceipt, ApiError>;

    async fn validate_mutation(
        &self,
        request: MutationValidationRequest,
        context: RequestContext,
    ) -> Result<MutationValidationReport, ApiError>;

    async fn commit_approved_mutation(
        &self,
        request: CommitApprovedMutationRequest,
        context: RequestContext,
    ) -> Result<CommitReceipt, ApiError>;

    async fn refresh_indexes(
        &self,
        request: ScopedIndexRefreshRequest,
        context: RequestContext,
    ) -> Result<IndexRefreshReceipt, ApiError>;

    async fn explain_trace(
        &self,
        request: TraceExplanationRequest,
        context: RequestContext,
    ) -> Result<TraceExplanationResponse, ApiError>;
}
```

设计要求:

- `RequestContext` 必须包含 `interface=agent_adapter` 或更具体的 adapter kind、`request_id`、`trace_id`。
- 所有请求必须携带 `RuntimeIdentity`，至少包含 runtime 名称、adapter 名称、版本和可选 actor/user identity。
- 高风险写入必须返回 pending 状态，由外部 runtime 或上层应用负责人审批后再提交。
- 长任务只返回 receipt 和 stream events，不把 runtime loop 放入 core。

### 4.1 MCP 映射

MCP server adapter 推荐暴露:

| MCP primitive | `relay-knowledge` 映射 |
| --- | --- |
| tools | `relay.retrieve_context`、`relay.inspect_graph`、`relay.propose_mutation`、`relay.validate_mutation`、`relay.commit_approved_mutation`、`relay.refresh_indexes`、`relay.explain_trace` |
| resources | graph schema、source scopes、graph snapshot metadata、index health、retrieval diagnostics、pending mutation proposals |
| prompts | entity extraction、claim normalization、conflict analysis、context planning、grounded answer drafting |

MCP adapter 只暴露协议能力。host 仍负责完整对话、权限弹窗、用户授权、tool selection、模型调用和跨 server 编排。

### 4.2 A2A / Agent Gateway 映射

A2A 或类似 agent interoperability gateway 推荐作为后续 adapter，而不是 core 依赖。

可暴露的 specialist agent 能力:

- `KnowledgeRetrievalAgent`: 给定 scope 和问题，返回 grounded context pack。
- `KnowledgeExtractionAgent`: 给定 evidence，返回 mutation proposal。
- `GraphValidationAgent`: 给定 proposal，返回冲突、重复、缺证据和置信度报告。
- `IndexMaintenanceAgent`: 给定 graph/index 状态，触发 scoped refresh 或报告 stale 原因。

gateway 应把外部 task lifecycle 映射为 `ApiStreamEvent`，并把 A2A agent card、task id、artifact id 等信息放在 adapter metadata 中，不能进入 domain 类型。

## 5. LLM 知识处理工作流

### 5.1 摄取后抽取

```text
source scope resolved
  -> evidence persisted
  -> runtime calls extraction prompt/tool
  -> mutation proposal created
  -> deterministic validation
  -> optional LLM-assisted conflict review
  -> approval gate
  -> graph commit
  -> scoped index refresh requested
```

要求:

- LLM 抽取结果必须绑定 `source_scope`、`evidence_id`、`extractor`、`extractor_version`、`model_id`、`runtime_identity` 和 `trace_id`。
- 没有 evidence 的实体、关系或 claim 只能进入低置信度 proposed 状态。
- 抽取失败不应回滚原始 evidence 入库。
- 同一 evidence 重新抽取时，proposal 应可幂等去重。

### 5.2 查询时上下文组织

```text
agent question
  -> retrieve_context tool
  -> scoped hybrid retrieval
  -> graph expansion and freshness check
  -> context pack with citations and budgets
  -> runtime generates answer outside core
```

`relay-knowledge` 可以组织 context pack，但不负责最终自然语言回答。最终回答如果要入库，必须作为新的 claim proposal 或 conversation artifact 显式提交。

### 5.3 图谱维护任务

外部 runtime 可驱动这些后台或人工辅助任务:

- 实体消歧和 alias 合并建议。
- 重复 claim 检测。
- 支持证据和反证证据聚合。
- 社区摘要或主题摘要刷新。
- 跨 source scope 的逻辑符号关系建议。
- 检索失败分析和 index stale 诊断。

这些任务都应产出 proposal、diagnostic 或 derived index，不直接修改 accepted facts。

## 6. 混合检索设计

`relay-knowledge` 的检索不是单一路径。三层检索必须作为独立 read model 保持可替换、可降级和可解释。

| 层 | 主要对象 | 最适合的问题 | 典型失败 |
| --- | --- | --- | --- |
| BM25 | 原文 chunk、符号名、路径、错误码、claim 文本 | 精确术语、API 名称、代码符号、日志错误、配置键 | 同义词、抽象概念、跨语言表述 |
| Semantic retrieval | 实体、关系、claim、summary、community report | 概念问题、主题理解、实体关系归纳、全局问题 | 细粒度字面匹配不稳定 |
| Vector retrieval | 文本/图片/layout/table embedding | 相似段落、多模态内容、自然语言近义查询 | 版本/作用域过滤弱时容易召回相似但错误的内容 |
| Graph expansion | 邻居、路径、依赖、调用、证据链、社区 | 多跳关系、影响分析、因果链、代码 review | 起点错会扩散噪声 |

### 6.1 查询流程

```text
HybridRetrievalRequest
  -> source scope resolution
  -> freshness policy check
  -> BM25 sparse recall
  -> semantic entity/claim/summary recall
  -> vector recall
  -> graph neighborhood expansion
  -> candidate normalization
  -> RRF or weighted fusion
  -> optional rerank
  -> context packing
  -> HybridRetrievalResponse
```

融合规则:

- 当前实现默认使用 RRF 融合候选排名，后续可按 query intent 切换 weighted fusion。
- 每个候选保留 `retriever_sources`、`ranking.source`、`ranking.score`、`ranking.rank`、`source_scope`、`source_path`、`evidence_id`、`graph_version` 和 `indexed_graph_version`。
- rerank 只能重排候选，不能生成新事实。
- context packing 必须受 token、节点数、边数、evidence 数和耗时预算约束。

Phase 1 已落地 SQLite FTS5 BM25 read model。该 read model 覆盖 evidence content、entity labels、生成式 entity/code symbol lexical aliases、source scope/path、tree-sitter code symbols 和 code chunks，并把 `created_graph_version` 写入 BM25 文档以保证 snapshot 查询不会读到未来图版本。`HybridRetrievalResponse.context_pack.items[*]` 同时保留 structured facts 和由 facts 派生的一跳 `graph_paths`，供 agent 直接引用关系、claim 或 event path provenance。

### 6.2 必须使用混合检索的场景

| 场景 | 默认策略 |
| --- | --- |
| 代码符号、函数名、错误码、配置项 | BM25 召回优先，graph expansion 补调用/依赖邻域，vector 只做补充 |
| “这个模块为什么失败”类排障 | BM25 找日志/错误，semantic 找相关 claim，graph expansion 找依赖链 |
| “总结某个主题/项目状态” | semantic summary 和 community report 优先，BM25 补原文证据 |
| 多模态文档问答 | text BM25 + OCR/caption semantic + image/vector recall，按 parent evidence 合并 |
| 代码 review 影响分析 | changeset scope 约束，BM25 找 changed symbols，graph expansion 找 callers/tests/dependencies |
| rebase 或多分支查询 | scope filter 强制生效，禁止跨 `tree_hash` 混召回 |
| 索引落后 | 按 freshness policy 等待、降级到 graph/BM25，或返回 stale metadata |

### 6.3 Freshness 和版本

检索响应必须返回:

- `graph_version`: 查询可见的图版本。
- `scope_id`: 实际使用的 source scope。
- `index_versions`: BM25、semantic、vector、summary 等索引版本。
- `indexed_graph_version`: 每个索引处理到的图版本。
- `stale`: 是否使用落后索引。
- `degraded_reason`: OCR、embedding、vector backend、rerank 或索引不可用时的原因。

## 7. 公共接口草案

以下接口用于约束后续实现方向，字段可在落代码时按现有 `api` 模块风格细化。

```rust
pub struct RuntimeIdentity {
    pub runtime_name: String,
    pub runtime_version: Option<String>,
    pub adapter_kind: AgentAdapterKind,
    pub adapter_version: Option<String>,
    pub actor_id: Option<String>,
}

pub struct AgentActionPolicy {
    pub allowed_scopes: Vec<SourceScopeSelector>,
    pub allow_write_proposals: bool,
    pub allow_direct_commit: bool,
    pub require_human_approval: bool,
    pub max_runtime_ms: u64,
    pub max_context_tokens: usize,
}

pub struct HybridRetrievalRequest {
    pub query: String,
    pub source_scope: SourceScopeSelector,
    pub modalities: Vec<Modality>,
    pub retrieval_layers: Vec<RetrievalLayer>,
    pub freshness_policy: FreshnessPolicy,
    pub top_k: usize,
    pub budget: RetrievalBudget,
    pub runtime_identity: Option<RuntimeIdentity>,
}

pub struct HybridRetrievalResponse {
    pub context_pack: RetrievedContextPack,
    pub candidates: Vec<RetrievedCandidate>,
    pub fusion: FusionDiagnostics,
    pub metadata: RetrievalMetadata,
}

pub struct KnowledgeMutationProposal {
    pub proposal_id: String,
    pub source_scope: SourceScope,
    pub evidence_ids: Vec<String>,
    pub proposed_entities: Vec<EntityProposal>,
    pub proposed_relations: Vec<RelationProposal>,
    pub proposed_claims: Vec<ClaimProposal>,
    pub runtime_identity: RuntimeIdentity,
    pub confidence: f32,
    pub rationale: Option<String>,
}
```

默认策略:

- `allow_direct_commit` 默认为 `false`。
- `freshness_policy` 默认为不跨 scope、不跨 graph version 静默返回。
- `retrieval_layers` 默认启用 BM25、semantic、vector 和 graph expansion，但 backend 不可用时可显式降级。
- `runtime_identity` 在 agent adapter 请求中必填，在 CLI/Web 普通请求中可为空。

## 8. 安全与治理

Agent runtime 集成会把外部文本、tool 调用和模型输出带进写入路径，因此必须把安全边界放在模型上下文之外。

要求:

- 所有 tool 调用都先经过 `AgentActionPolicy` 检查。
- Prompt injection 文档内容只能作为 data/evidence，不能成为系统指令或 commit 授权来源。
- 所有写入先生成 `KnowledgeMutationProposal`，再经过 deterministic validation。
- 高风险 mutation 包括删除、合并实体、修改 accepted fact、跨 scope 合并和大批量写入，必须人工审批。
- Adapter 必须记录 runtime、actor、tool name、arguments hash、source scope 和 trace id。
- 错误消息不能泄露 secrets、完整本地路径或未授权 scope 内容。

## 9. 可观测性

新增建议日志事件:

- `agent.tool.started`
- `agent.tool.completed`
- `agent.tool.failed`
- `agent.mutation.proposed`
- `agent.mutation.validation.completed`
- `agent.mutation.approval.required`
- `retrieval.hybrid.started`
- `retrieval.hybrid.fusion.completed`
- `retrieval.context_pack.truncated`

新增建议指标:

| 指标 | 类型 | 说明 |
| --- | --- | --- |
| `relay_agent_tool_calls_total` | counter | 按 adapter、tool、status 统计 |
| `relay_agent_mutation_proposals_total` | counter | 按 proposal status 统计 |
| `relay_agent_mutation_approval_required_total` | counter | 需要人工审批的写入数量 |
| `relay_hybrid_retrieval_layer_hits_total` | counter | 各检索层召回命中数量 |
| `relay_hybrid_retrieval_fusion_duration_ms` | histogram | 融合和 rerank 耗时 |
| `relay_context_pack_truncated_total` | counter | 因预算截断的 context pack |

Trace 应串联:

```text
agent.tool
  -> application.policy_check
  -> retrieval.hybrid_search
  -> retrieval.graph_expand
  -> retrieval.fusion
  -> retrieval.context_pack
  -> agent.tool.result
```

写入路径 trace 应串联:

```text
agent.tool
  -> mutation.propose
  -> mutation.validate
  -> approval.wait
  -> graph.commit
  -> event.publish
  -> index.refresh
```

## 10. 测试场景

后续实现必须覆盖:

- MCP adapter 调用检索时，只能通过 unified API 获取结果，不能直接访问 storage。
- Agent 生成 mutation proposal 后，未审批前不会改变 `graph_version`。
- BM25、semantic、vector 各自命中不同候选时，fusion 输出保留来源、排名和分数解释。
- Vector index stale 时，系统按 policy 等待、降级到 BM25 + graph retrieval，或返回 stale metadata。
- Source scope 不匹配时，agent 查询不能跨仓库快照或文档集合泄漏结果。
- Prompt injection 文档内容不能触发 commit、refresh 或外部 tool 调用。
- Trace 能串联 agent task、retrieval、mutation validation、commit 和 index refresh。
- 多模态 evidence 同时被 OCR、caption 和 image embedding 命中时，context pack 按 parent evidence 合并。

## 11. 实施顺序

1. 在 `api` 设计中补 `RuntimeIdentity`、`AgentActionPolicy`、`HybridRetrievalRequest`、`HybridRetrievalResponse` 和 mutation proposal 类型。
2. 在 `application` 中增加 agent-facing use cases，但保持 runtime orchestration 在 adapter 外部。
3. 在 `retrieval` 中实现可降级的 BM25、semantic、vector、graph expansion 和 fusion pipeline。
4. 在 `storage/indexing` 中补 scoped index metadata，保证 stale 判断按 scope 和 index kind 计算。
5. 首个 runtime adapter 优先实现 MCP server，因为它和本项目的 tool/resource/prompt 暴露边界最匹配。
6. 后续再按需求添加 A2A 或本地 SDK bridge，不改变 core API 行为语义。
