# GraphRAG 产品与实现路线规格

> 文档版本: 1.1
> 编制日期: 2026-05-12
> 进展刷新: 2026-05-13
> 范围: relay-knowledge 的 GraphRAG 产品边界、当前实现基线、优化措施和分阶段实现规格。

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
- 混合检索: SQLite FTS5 BM25 read model、graph evidence fallback、code graph documents、local semantic/vector read model、schema path、temporal/community retrieval、RRF 融合、source span/structured fact context pack 和 backend availability metadata。
- 代码仓库能力: Git 仓库注册、full/incremental index、worktree overlay、tree-sitter 多语言解析、symbol/reference/chunk 查询和 diff impact。
- Agent 接入基础: MCP Streamable HTTP server、本地 ACP session adapter、session/protocol header 校验、access policy、QoS admission、tool-level graph retrieval、graph inspect、health、service status、index status、授权 code graph query、授权 code impact 和受权限控制的 index refresh。
- Web 诊断面: `/api/project/status` 和 `/api/health` 驱动的 health、index、runtime、operation composer 和 GraphRAG readiness 视图。

这些能力已经从 v1 底座推进到 Phase 4 的本地 GraphRAG read model: typed fact schema、scoped index cursor、后台 task lease/reconciler/dead-letter、local semantic/vector read model、多模态 evidence schema、schema path、temporal query、community summary 和 evaluation harness 已有 Rust 实现。外部 embedding/OCR/vision 后端、proposal lifecycle、service manager 安装面和 silent update operator 仍属于后续实现。

## 3. 优化措施

### 3.1 检索与 Context Pack

- 保持 `HybridRetrievalResponse` 作为 canonical context pack，不新增只返回自然语言答案的 core API。
- BM25 字段质量优先覆盖 evidence content、entity label、source path、code symbol、code chunk 和 doc comment。
- RRF 融合必须保留每个 retriever 的 rank、score 和 explanation。
- graph expansion 必须限制深度、节点数、时间和输出字节，超限时返回 `truncated=true` 和原因。
- semantic/vector 当前使用确定性的本地 token/hash read model，已在 ranking explanation 和 cursor diagnostics 中记录 model、dimension、source hash、scope、backend cursor 和 graph version；外部 embedding backend 接入时必须保持同一 metadata contract 和 scope post-filter。

### 3.2 事实模型

- 通用图已从 evidence/entity 扩展到 typed relation、claim、event、confidence、source span 和 status；proposal lifecycle 仍需后续补齐。
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

当前 Rust 实现已支持 evidence modality/extraction metadata、OCR/caption/image embedding parent grouping、`as_of:<date>` 与年份事件检索，以及 community summary context item。真实 OCR、vision caption 和外部 embedding worker 仍需要在后台 worker 边界后接入。

## 4. 分阶段路线

### Phase 1: 真实检索闭环

- 保持 typed relation、claim/event、source span 和 confidence 的 domain/storage/API 规格可回归测试。
- 对 ingest 边界重新验证反序列化后的 span、confidence 和 version range；结构化 facts 必须引用 supporting evidence ids。
- 让 context pack 覆盖 evidence、entity、code symbol、code chunk、source span、structured graph facts 和 direct graph path evidence；当前实现已从 structured relation/claim/event 派生 `graph_paths`。
- 检索候选只使用 `accepted`/`proposed` evidence，`rejected`/`superseded` evidence 保留为可检查图状态但不作为 grounding context。
- 增强 BM25/lexical 文档构建字段，覆盖 source path、entity/code symbol lexical aliases、code symbol、code chunk 和 doc comment，并补充 ranking explanation 测试；当前 SQLite FTS5 read model 已为 entity labels 和 code symbols 写入独立 alias 字段。
- 为 semantic/vector 保留 adapter trait、backend status metadata 和 scope post-filter metadata，但默认允许 unavailable/degraded。
- Web readiness 继续从 health/status 显示 BM25、semantic cursor、vector cursor、code graph、runtime budgets 和 index lag。

### Phase 2: 可恢复索引刷新

- 保持已落地的 scoped index cursor、mutation log affected metadata、bounded index refresh queue、active lease/attempt guard、retry backoff、lease-expiry dead-letter 和 startup reconciler 可回归测试。
- 为 semantic/vector backend 保持 model、dimension、source hash、backend-specific cursor 和 last error 元数据的持久化/API contract；未配置真实 backend 时 model/dimension 为空，但 refresh worker 可在完成任务时写入并由 cursor 诊断返回。
- health/service doctor 继续返回 queue depth、oldest task age、dead-letter count、index lag 和结构化 stale reasons；每条 reason 必须能指向索引族或 scoped cursor，并携带 lag versions 和 last error。

### Phase 3: Agent 与常驻服务

- 保持 MCP read-only 工具矩阵: retrieve context、inspect graph、index status、service doctor、code graph query 和 code impact。
- 保持本地 ACP 会话入口，支持 progress、cancellation、context artifact 和 runtime identity。
- 保持 bounded in-process audit log，记录 identity、scope、freshness、QoS decision、budget、truncation 和 result count；后续补持久审计 sink 和 metrics exporters。
- 安装/升级/卸载文档必须覆盖 service manager 模板、运行时目录、rollback 和 diagnostics。

### Phase 4: 高级 GraphRAG

- 已接入 local semantic retrieval 和 hashed-vector ANN read model，支持 model、dimension、source hash、scope 和 graph version metadata。
- 已增加 path retrieval、schema-guided traversal、community summary 和 temporal query。
- 已增加 multimodal evidence schema、extractor diagnostics、image/OCR/caption/table/layout modality 和 parent evidence grouping。
- 已建立 evaluation harness，覆盖 exact fact、multi-hop、temporal、negative rejection、stale index、ambiguous entity 和 code impact。

剩余 Phase 4 产品化工作:

- 接入可替换的外部 text/image embedding backend，并保留本地 read model 作为 deterministic fallback。
- 把真实 OCR、caption、table/layout extractor 放入后台 worker/maintenance 边界，写入当前 multimodal schema。
- 将 evaluation harness 接到 CI fixture 和后续 release diagnostics。

## 5. 验收要求

- 所有新增 public API 有生产调用方或规格支撑，并配套单元测试或集成测试。
- CLI、Web、MCP 和 ACP 共享 application service，不复制业务逻辑。
- 新增 I/O、数据库、embedding、OCR、parser、index rebuild 和 compaction 不得阻塞 async runtime hot path。
- 所有队列、检索和遍历有 limit、timeout、cancellation、budget、truncated/degraded 状态。
- 文档与实现同步更新，尤其是配置、环境变量、路径、网络、QoS、索引、后台服务、安装部署和用户可见功能。
