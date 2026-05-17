# ai-knowledge-graph 参考项目分析

[中文](../../zh/04-research/04-ai-knowledge-graph-reference-analysis.md) | [英文](../../en/04-research/04-ai-knowledge-graph-reference-analysis.md)

> 编制日期: 2026-05-15
> 参考项目: <https://github.com/robert-mcdermott/ai-knowledge-graph>
> 参考版本: `40b7019`，2025-12-27，`Merge pull request #19 from Deepak-png981/dj/Introduce-prompt-factory`
> 范围: 只做架构、算法、性能、可靠性借鉴分析，不引入代码实现，不复制参考项目源码。

## 1. 执行结论

`ai-knowledge-graph` 是一个 Python 单机流水线: 读取文本文件，按词数切块，用 OpenAI-compatible Chat Completions API 抽取 Subject-Predicate-Object 三元组，执行实体标准化和关系推断，最后用 NetworkX、Louvain 社区发现和 PyVis 生成交互式 HTML 图谱。它适合作为“LLM 抽取型知识图谱最小闭环”的参考，但不适合按实现方式直接移植到 `relay-knowledge`。

可借鉴的核心点有四类:

- 架构上，阶段化流水线边界清楚: chunking、SPO extraction、entity standardization、relationship inference、visualization/export 各自有独立职责。
- 算法上，实体标准化和关系推断采用“确定性候选生成 + 可选 LLM 裁决”的思路，适合改造成 proposal 和 derived fact 流程。
- 性能上，切块、候选数量上限、代表实体抽样、上下文截断和原始/推断边区分，提示了 GraphRAG pipeline 必须先有预算再调用模型或遍历图。
- 可靠性上，参考项目暴露了脚本式方案的风险: 同步网络请求、无超时重试、无持久任务状态、JSON 修复式解析、推断边缺少证据和置信度。`relay-knowledge` 应把这些风险转化为架构约束，而不是复制脚本行为。

## 2. 参考项目流程拆解

参考项目的主流程位于 `src/knowledge_graph/main.py`:

1. `load_config` 从 `config.toml` 读取 LLM、chunking、standardization、inference 和 visualization 配置。
2. `chunk_text` 按词数切块，并保留固定 overlap。
3. `process_with_llm` 通过 prompt factory 取得抽取提示词，调用 LLM，尝试从响应中提取 JSON 数组。
4. 每个有效对象必须包含 `subject`、`predicate`、`object`，predicate 会被裁剪到最多 3 个词。
5. `standardize_entities` 先做小写、停用词移除、词根/包含关系等确定性归并，再可选调用 LLM 对实体别名分组。
6. `infer_relationships` 识别连通分量，按社区间和社区内候选调用 LLM 推断关系，同时增加传递推断和词面相似推断。
7. `_deduplicate_triples` 按三元组去重，并优先保留非推断关系。
8. `visualize_knowledge_graph` 计算度、betweenness、eigenvector、社区编号、节点大小和推断边样式，输出 HTML 和 JSON。

这个流程证明了低成本闭环的价值: 即使没有完整数据库和后台服务，也能快速把非结构化文本变成可检查图谱。但它的工程假设是本地批处理脚本，与本项目 async-first、service-first、可恢复索引和多入口共享 application service 的目标不同。

## 3. 架构借鉴

### 3.1 阶段化 pipeline 适合保留为产品语义

参考项目将抽取、标准化、推断和可视化分为显式阶段。`relay-knowledge` 可以借鉴这个产品语义，但实现边界应保持在现有架构内:

- `application` 负责编排 ingestion、proposal、index refresh 和 diagnostics。
- `domain` 保存 evidence、entity、relation、claim、event、inferred/derived 状态、source span、confidence 和 graph version。
- `storage` 保存原始 evidence、accepted facts、proposal、derived index 和 mutation log。
- `net` 继续承载 HTTP 和外部 provider 通信；任何 LLM、embedding、OCR 或网络调用都不得绕过 `net` 与 QoS。

不应新增一个脚本式“generate graph”旁路。CLI、Web、MCP 和 ACP 仍应共享同一个 application service。

### 3.2 可选 LLM 阶段应表达为策略

参考项目允许关闭 `standardization` 和 `inference`，这是重要的产品能力。`relay-knowledge` 可借鉴为三层策略:

- `disabled`: 不调用模型，只保留原始 facts 和确定性索引。
- `candidate-only`: 运行确定性候选生成，但只输出 proposal、diagnostic 或 review queue。
- `assisted`: 调用外部模型生成 entity merge 或 inferred relation proposal，必须带 provider、model、prompt version、source hash、scope、confidence 和审核状态。

这些策略应进入配置、API 响应和诊断，不应隐含在临时命令参数里。

### 3.3 Prompt factory 的启示是版本化契约

参考项目把 main extraction、entity resolution、relationship inference 的 prompt 分离成 prompt factory。`relay-knowledge` 不需要复制提示词文本，但应借鉴“prompt 是可审计契约”的思想:

- prompt id、prompt version、model、temperature、输入证据 hash 和输出 schema version 要进入派生结果 provenance。
- prompt 升级应能触发 scoped index/proposal refresh，而不是静默覆盖旧结果。
- LLM 原始响应只可作为诊断或审计材料保存，不能直接作为 accepted facts。

### 3.4 原始边和推断边必须分层

参考项目用 `inferred` 标记推断关系，并在可视化中用虚线展示。`relay-knowledge` 已有 evidence/status/version 基础，后续应继续强化:

- extracted fact、normalized entity alias、inferred relation、community summary 和 generated answer 是不同层级。
- 推断边默认不是 accepted fact，应进入 derived fact 或 proposal。
- 查询返回必须说明关系来源: 原文证据、规则推断、LLM 推断、社区摘要或索引派生。

## 4. 算法借鉴

### 4.1 SPO 抽取: 作为候选事实，不作为直接提交

参考项目通过 LLM 输出 `{subject, predicate, object}` JSON 数组，并做字段存在性校验。对 `relay-knowledge` 来说，SPO 抽取可以借鉴为输入标准，但需要增强为事实候选:

- 每个候选必须引用 source scope、source URI/hash、chunk id、source span、extractor、model 和 prompt version。
- predicate 不应只靠“最多 3 个词”裁剪，应映射到 typed relation、alias 或未归一化 label。
- 候选事实需要 status: proposed、accepted、rejected、superseded 或 derived。
- 同一 chunk 多次抽取要幂等，使用 source hash、chunk range、model 和 prompt version 做去重键。

### 4.2 实体标准化: 分两段处理

参考项目先用小写、停用词移除、词集合包含、短词根等方法分组，再可选让 LLM 对实体列表进行归并。可借鉴的算法形态是:

- 确定性阶段只生成候选别名组和相似度理由。
- LLM 阶段只裁决高价值或歧义候选，不扫描全量实体。
- 高频实体优先、每批实体数量上限、上下文长度上限都应变成预算参数。
- entity merge 必须可逆: 合并前后的 canonical id、alias、scope 和旧查询影响要可审计。

不能借鉴的是直接小写覆盖实体名。人名、项目名、代码符号、路径、缩写和跨语言实体都需要保留原始 display label。

### 4.3 关系推断: 只做可解释候选

参考项目使用四种推断来源: 社区间 LLM 推断、社区内 LLM 推断、二跳传递推断、词面相似推断。`relay-knowledge` 可借鉴为候选生成器:

- 社区间推断适合发现图谱断裂点，但需要限制为 top-k 社区、代表实体和证据上下文。
- 社区内推断适合补齐语义近邻，但要避免把共享词误判成事实关系。
- 传递推断只能对有明确传递语义的 predicate 白名单启用，例如 `part_of`、`located_in`、`depends_on` 的受限形式。
- 词面相似只应产生 `possibly_related` 或 entity alias proposal，不应直接产生事实边。

所有推断结果都必须带 `inferred_by`、input facts、rule id 或 prompt id、confidence、review state 和 stale invalidation 条件。

### 4.4 图指标: 用于排序、解释和 UI，不用于事实

参考项目计算 degree、betweenness、eigenvector 和 Louvain community，并用节点大小、颜色和虚线边表达图结构。`relay-knowledge` 可借鉴到两处:

- 检索排序: centrality、community membership 和 bridge node 可作为 rerank signal。
- UI 与 diagnostics: 图检查、Web canvas、agent context pack 可显示 community、original/inferred edge、node importance 和 truncation reason。

这些指标是 read model 或 diagnostics，不是 domain fact。它们应随 graph version 和 scope 失效刷新。

## 5. 性能借鉴

参考项目已经有一些隐含性能策略: 固定 chunk size/overlap、只让 LLM 看最多 100 个实体、只比较前 5 个大社区、每个社区取最多 5 个代表实体、LLM 上下文三元组限制为 20 条、社区内候选最多 10 对。这些策略值得借鉴为明确的资源预算:

- ingestion chunking 应同时考虑 token、句段、source span 和 overlap，不只按空格分词。
- LLM extraction、entity resolution 和 inference 应使用有界并发、请求超时、取消、retry backoff、provider rate limit 和 dead-letter。
- graph traversal、community detection、centrality 和 full index rebuild 不得运行在 query hot path。
- community summary、centrality 和 relation inference 应作为 scoped read model 或 maintenance worker 输出。
- context pack 应在 API 层暴露 `limit`、`timeout`、`truncated`、`degraded`、`budget_exhausted` 和 retriever source。
- 对相同 source hash、chunk range、model、prompt version 的 LLM 输出应缓存或复用，避免重复成本。

参考项目的同步 `requests.post`、串行 chunk 处理和无 timeout 行为不适合 `relay-knowledge`。任何外部模型调用都要进入 worker 边界，不能阻塞 async runtime executor。

## 6. 可靠性借鉴

参考项目有实用的容错思路: 按 chunk 失败隔离、过滤无效三元组、尝试从 code block 或不完整响应中恢复 JSON、输出原始 JSON 文件、把 inferred edge 与 original edge 分开。这些可以借鉴为更严格的可靠性设计:

- LLM 响应解析必须使用 schema validation；修复后的 JSON 只能进入 degraded proposal，不应自动 accepted。
- 每个 chunk 的成功、失败、重试、模型、prompt、token budget 和错误类型应进入结构化 diagnostics。
- 阶段输出必须可恢复: extraction、standardization、inference、index refresh 和 visualization/read model 各自有 cursor 或 lease。
- 失败隔离要按 scope、source、chunk、provider 和 stage 记录，不能因为某个 chunk 或模型失败阻塞其它来源。
- secrets 不应写入示例 `config.toml`；API key 必须经 `env` 读取和脱敏诊断。
- 推断结果必须可撤销，graph mutation、proposal acceptance 和 index invalidation 要保持一致。

参考项目的关系推断示例输出里出现过负数新增计数再输出正数总增量的日志现象。这类不一致说明 diagnostics 不能只靠 `print`，`relay-knowledge` 的计数必须来自持久任务状态和可测试的 stage result。

## 7. 对 relay-knowledge 的落地参考

后续如果要吸收这些经验，应按下面顺序进入规格和任务，不在本分析中实现:

1. 将 LLM SPO extraction 定义为 `proposal` 生产者，而不是直接图提交路径。
2. 为 entity resolution 建立 deterministic candidate generator、LLM adjudication worker、review state 和 reversible merge audit。
3. 为 inferred relation 建立 rule/prompt provenance、confidence、input fact ids、失效条件和默认非 accepted 状态。
4. 将 community、centrality、bridge node 和 original/inferred edge 样式纳入 read model 与 Web/agent diagnostics。
5. 为 extraction/standardization/inference 三类 worker 统一 provider timeout、QoS、rate limit、retry、dead-letter、cursor 和 prompt version metadata。
6. 在 GraphRAG evaluation fixture 中增加 entity merge、false merge、transitive-rule whitelist、inferred-edge rejection 和 degraded JSON response 场景。

### 2026-05-15 选择性吸收进展

本轮只吸收与现有 worker/proposal 主路径兼容的部分，不改变普通 `ingest`
直接提交 accepted facts 的能力，也不引入新的脚本式抽取入口:

- 已为 proposal 增加持久化 provenance，记录 producer、provider、model、
  prompt id/version、schema version、input source hash、input fact ids、stale
  条件和预算说明。
- `worker run-once` 调用外部 endpoint 时使用 `contract_version=2` 请求，
  明确 manual-review policy、HTTP timeout、lease、max attempts 和
  max-in-flight 预算。
- 外部 extractor 返回的结构化 relation/claim/event 会保留在 proposal
  payload 中，但默认降为 `proposed`，防止 LLM SPO 抽取或关系推断直接写入
  accepted facts。
- deterministic fallback proposal、OCR/vision/embedding 已有能力保持可用；
  本轮没有移除多模态派生 evidence、worker 队列、人工 accept/reject/supersede、
  audit sink、索引刷新和现有 GraphRAG 检索能力。

尚未吸收的内容继续保留为后续工作: entity merge 的可逆审计、transitive rule
白名单、community/centrality read model、完整 provider rate-limit/cursor 以及更大规模
GraphRAG evaluation fixture。

## 8. 不应直接照搬的内容

- 不直接复制 Python 代码、prompt 文本、HTML 模板或 PyVis 输出。
- 不新增绕过 `env`、`paths`、`net`、`application`、`storage` 边界的脚本式主流程。
- 不把 LLM 推断关系直接写入 accepted facts。
- 不用小写化实体名覆盖 canonical display label。
- 不在 query hot path 运行外部 LLM、全图 community detection、centrality 或全索引重建。
- 不用无 timeout 的同步 HTTP 请求访问模型服务。

## 9. 后续文档影响

本分析应作为研究材料被引用。真正进入实现前，需要同步更新对应规格:

- GraphRAG 产品路线: 明确 SPO proposal、entity resolution、inferred relation 的阶段状态。
- Source Scope 与多模态摄取: 明确 chunk、source span、extractor provenance 和 provider diagnostics。
- Background service/self-healing: 增加 extraction/standardization/inference worker 的 lease、retry 和 dead-letter 要求。
- Advanced observability: 增加 prompt version、model、provider latency、schema validation failure 和 degraded proposal 指标。

截至 2026-05-15，本分析已开始被选择性吸收到 worker/proposal 主路径；未完成项仍需按对应架构规格拆分实现。
