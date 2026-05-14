# 知识图谱技术研究总结

[中文](../../zh/research/knowledge-graph-research.md) | [英文](../../en/research/knowledge-graph-research.md)

> 项目: `relay-knowledge`
> 日期: 2026-05-11
> 目标读者: 架构设计、核心实现、后续技术选型
> 结论类型: 论文与工程研究总结，面向本项目落地

## 1. 执行摘要

`relay-knowledge` 的目标不是只保存三元组，而是建设一个可持续更新、可检索、可解释、可被 CLI 与 Web 共用的知识图谱核心服务。结合近年知识图谱构建、GraphRAG、混合检索、图数据库标准化研究，建议项目从第一版就按以下方向收敛:

1. 内部模型采用属性图 (Labeled Property Graph, LPG)，节点和边都允许携带属性、来源、版本、置信度和时间信息。RDF/OWL 可以作为导入导出或互操作格式，不建议作为内部唯一模型。
2. 架构保持事件驱动和异步优先。摄取、抽取、实体消歧、图写入、索引刷新、检索评估都应通过可追踪事件串联，所有队列必须有边界、超时和取消语义。
3. 检索层从第一天起按三层建设: BM25 关键词检索、语义检索、向量 ANN 检索。三层结果用 RRF 或加权融合聚合，再通过图邻域、证据和 reranker 做上下文组织。
4. 索引新鲜度是系统成败关键。每次图变更都要产生 `graph_version`，BM25、语义索引、向量索引和社区摘要必须记录自己对应的图版本，避免回答基于过期状态。
5. 图谱质量不能只靠 LLM 抽取。论文普遍指出 KG 构建包含获取、精炼、演化三个阶段；项目应保留实体合并、关系验证、冲突检测、人工/规则校正和可回滚事件日志。

## 2. 研究脉络与关键论文

### 2.1 知识图谱构建: 从抽取到演化

Zhong et al. 的自动知识图谱构建综述将 KG 构建拆成知识获取、知识精炼、知识演化三个阶段 [R1]。这对本项目的直接启示是: `relay-knowledge` 不能只实现一次性导入。真实系统会持续接收文档、代码、网页、数据库记录或用户输入，图谱需要在变更中保持一致。

项目落地建议:

- 以 `IngestedDocument`、`ExtractedFact`、`ResolvedEntity`、`GraphMutation`、`IndexRefreshRequested` 这类事件表达管线状态。
- 将 LLM 抽取结果视为候选事实，而不是最终事实。候选事实必须带来源、置信度、抽取器版本和证据片段。
- 明确区分 construction 和 refinement。construction 负责从来源生成节点/边；refinement 负责实体合并、关系补全、错误删除、冲突标注和图谱版本演化。

### 2.2 LLM 赋能 KG 构建: 有用但不可无约束

2025 年的 LLM-empowered KG construction 综述指出，LLM 正在把传统的本体工程、知识抽取、知识融合流程改造成语言驱动流程 [R2]。但另有研究专门评估 LLM 是否适合直接构建 KG，指出当前方法仍面临句子级抽取、预定义 schema 依赖、结构/语义质量评估不足等问题 [R3]。

项目落地建议:

- 第一版不要把 LLM 作为唯一真源。LLM 可以生成实体、关系、类型建议和摘要，但图写入需要结构化校验。
- schema-first 与 schema-free 都要支持: 核心领域类型应 schema-first；探索性导入可以 schema-free，但必须带 `proposed_type` 和 `confidence`。
- 抽取器输出建议采用结构化 JSON，然后由 Rust 域层校验 ID、类型、边方向、属性类型和证据引用。

### 2.3 KG refinement: 覆盖率与正确率的长期权衡

Paulheim 的 KG refinement 综述强调，知识图谱通常不可能同时做到完全覆盖和完全正确，尤其使用启发式或自动抽取方法时，覆盖率与正确率存在长期权衡 [R4]。因此，系统要把精炼能力设计成核心流程，而不是后处理脚本。

项目落地建议:

- 节点和边需要 `confidence`、`confidence_tier`、`evidence_refs`、`created_by`、`updated_by`、`valid_from_version`、`valid_to_version`。
- 冲突不应立即覆盖。建议保留多候选事实，使用状态字段表达 `proposed`、`accepted`、`rejected`、`superseded`。
- 对自动合并实体设置可解释规则，例如 label 归一化、别名、外部 ID、嵌入相似度、邻域重合度；合并操作也作为事件记录。

### 2.4 RAG 与 GraphRAG: 从局部片段到全局结构

Lewis et al. 的 RAG 工作把参数化模型与外部非参数化记忆结合，用检索结果增强生成 [R5]。普通 RAG 适合局部事实问答，但对全局问题、跨文档主题、复杂关系链条支持有限。

Microsoft GraphRAG 的核心贡献是先用 LLM 从源文档构建实体图，再预生成社区摘要，用于回答全局 sensemaking 问题 [R6]。Microsoft GraphRAG 文档还区分了 local search: 用查询相关实体作为图入口，拉取连接实体、关系、协变量和原始文本块，再组织进上下文窗口 [R7]。

2025 年 KG2RAG 进一步强调，KG 可以用于事实级 chunk expansion 和上下文组织，而不仅是把图谱摘要塞给模型 [R8]。HybridRAG 则把知识图谱与向量检索结合，用两种检索证据互补 [R9]。HyperGraphRAG 提醒我们，普通二元边不适合所有场景，复杂事件、多人关系、因果链可能需要超边或事件节点建模 [R10]。

项目落地建议:

- `relay-knowledge` 的检索 API 不应只返回文本 chunk，应返回 `AnswerContext`: 匹配节点、边、证据片段、路径、索引版本、排序解释。
- 对全局问题保留社区/主题摘要接口；对局部问题走实体链接、邻域扩展、证据块排序。
- 对 n 元事实不要急于引入超图数据库。第一版可用 `Event` 或 `Claim` 节点连接多个参与实体，保留向超边模型演进的空间。

### 2.5 混合检索: BM25、语义、向量和融合

BM25 来自 Okapi/TREC 传统信息检索体系，仍是关键词精确匹配的强基线 [R11]。HNSW 是现代向量 ANN 检索的常用图结构，支持高召回近似近邻检索 [R12]。RRF 是简单有效的多路排序融合方法，适合把 BM25、向量和图结构得分合成一个候选集 [R13]。

本项目的“三层检索”建议定义如下:

| 层级 | 目标 | 输入 | 输出 |
| --- | --- | --- | --- |
| BM25 关键词检索 | 处理精确术语、实体名、代码符号、短语 | query text | 文档、chunk、节点属性匹配 |
| 语义检索 | 处理意图、同义表达、实体链接、图邻域扩展 | query text + graph state | 相关实体、关系路径、候选主题 |
| 向量 ANN 检索 | 处理语义相似片段和 embedding 近邻 | query embedding | top-k embedding records |

项目落地建议:

- BM25 先覆盖 entity label、aliases、relation label、document title、chunk text、source path。
- 向量记录必须包含 `embedding_model`、`dimension`、`source_hash`、`graph_version`，模型升级时允许并存多套索引。
- 融合第一版建议用 RRF，因为它不要求不同检索器分数同尺度；后续有评估集后再训练加权模型或 reranker。
- 所有检索结果必须能追溯到证据和图版本，不能只返回生成后的自然语言答案。

### 2.6 图数据库与标准化

属性图模型已成为大量图数据库的事实基础。Angles 对 property graph database model 给出了形式化描述 [R14]。GQL 在 2024 年作为 ISO/IEC 39075:2024 发布，说明属性图查询语言正在标准化 [R15]。这会影响长期互操作: 项目内部不一定要立即实现 GQL，但应该避免把查询模型设计成某个厂商 API 的薄封装。

图数据库选型建议:

- MVP 阶段优先定义接口: `GraphStore`、`GraphQuery`、`GraphMutationLog`、`IndexStore`。先保证领域层与具体数据库解耦。
- 本地零依赖路线可采用 SQLite + Tantivy + HNSW 库或向量插件，适合开发者单机和测试。
- Rust 原生/多模型路线可评估 SurrealDB。其文档显示同时支持 graph、full-text search 和 vector search，适合减少组件数量 [R16]。
- 企业/图算法路线可接入 Neo4j、NebulaGraph、Memgraph 等外部图数据库。Neo4j 已有 full-text 与 vector index 能力 [R17]，但运维成本和部署依赖更高。

## 3. 面向 relay-knowledge 的架构建议

### 3.1 核心分层

建议将项目分成五个稳定边界:

1. `domain`: 实体、关系、证据、事件、版本、错误类型。无数据库依赖，可做纯单元测试。
2. `graph_store`: 图写入、事务、遍历、版本查询、变更日志。通过 trait 暴露 async API。
3. `indexing`: BM25、embedding、向量 ANN、社区摘要、索引版本表。只消费图变更事件。
4. `retrieval`: 查询理解、实体链接、混合召回、图扩展、rerank、上下文组装。
5. `interfaces`: CLI、Web、未来 MCP/API。只调用核心服务，不复制业务逻辑。

### 3.2 最小可用图模型

当前 `domain` 目录模块只有 `KnowledgeEntity { id, label }` 和 `GraphVersion`，可作为最小起点，但不足以承载研究结论。建议演进为:

- `Entity`: `id`、`kind`、`label`、`aliases`、`properties`、`evidence_refs`、`confidence`、`version_range`。
- `Relation`: `id`、`kind`、`source_id`、`target_id`、`properties`、`evidence_refs`、`confidence`、`version_range`。
- `Evidence`: `id`、`source_uri`、`source_hash`、`span`、`extractor`、`created_at`。
- `GraphEvent`: `event_id`、`event_type`、`graph_version`、`payload`、`trace_id`、`timestamp`。

对复杂事件使用事件节点表达，例如 “A 在时间 T 因原因 R 影响 B” 建成 `Event` 节点，再由 `PARTICIPATES_IN`、`CAUSES`、`OCCURRED_AT` 等关系连接实体，避免强行压扁成二元边属性。

### 3.3 事件驱动索引刷新

推荐异步管线:

```text
Source ingest
  -> extraction requested
  -> facts extracted
  -> entities resolved
  -> graph mutation committed
  -> index refresh requested
  -> BM25/vector/semantic indexes refreshed
  -> retrieval ready at graph_version
```

工程要求:

- 每个阶段使用 bounded channel，设置最大队列长度、超时、取消 token 和重试上限。
- 图写入成功后再发布索引刷新事件；索引失败不能回滚图写入，但要记录索引滞后状态。
- 查询时返回 `graph_version` 和 `index_version`。如果索引落后，调用方可选择等待、降级或返回 stale 标记。
- CPU 密集型 embedding、批量解析、大文件读取放入显式 worker，不阻塞 async runtime。

### 3.4 检索与回答流程

推荐查询流程:

1. 解析 query，生成关键词、embedding、候选实体 mention。
2. 并发执行 BM25、向量 ANN、实体链接/图查询。
3. 用 RRF 融合候选，再按证据质量、图距离、版本新鲜度、权限过滤重排。
4. 对 top-k 节点做图邻域扩展，限制 hop 数、边类型、token budget 和总候选数。
5. 组装 `AnswerContext`，包含证据片段、实体/关系、路径和排序解释。
6. 如果接入 LLM，生成阶段只能使用 `AnswerContext`，并把引用来源返回给用户。

### 3.5 质量评估

知识图谱系统的评估要覆盖三类质量:

- 图质量: 实体重复率、关系正确率、证据覆盖率、孤立节点比例、冲突事实数量。
- 检索质量: recall@k、MRR、nDCG、路径命中率、跨文档多跳问题成功率。
- 系统质量: 索引延迟、事件积压、查询 p95/p99、图版本与索引版本滞后、失败重试率。

第一版可以用小型固定 fixture 建评估集，例如 20 个实体、50 条边、10 个文档、30 个查询。不要等到接入真实大库后才设计评估。

## 4. 阶段性路线图

### Phase 1: 核心模型与本地可测闭环

- 扩展实体/关系/证据/事件模型，全部放在可单测的 domain 层。
- 定义 async trait: 图存储、事件发布、索引刷新、检索。
- 使用内存或本地嵌入式存储实现最小闭环，保证不依赖外部图数据库也能跑测试。
- 建立小型 fixture 和检索评估样例。

### Phase 2: 三层检索与版本化索引

- 接入 BM25 索引，覆盖 label、alias、chunk、source path。
- 接入 embedding 与向量 ANN，记录模型、维度、hash、graph version。
- 实现 RRF 融合和 `AnswerContext`。
- 图变更后自动触发索引刷新，查询结果显示 stale/fresh 状态。

### Phase 3: 图数据库适配与 Web/CLI 共用服务

- 在 `GraphStore` 后增加可替换适配器，例如 SurrealDB 或 Neo4j。
- CLI 与 Web 共用同一组 core service，接口层只做参数解析、认证和展示。
- 增加社区摘要、路径解释、图邻域浏览和导入导出。

### Phase 4: 高级 KG 构建与 GraphRAG

- 引入 LLM 抽取器、实体消歧、关系验证和冲突处理。
- 增加 community summary / global search，用于回答跨文档全局问题。
- 对复杂事实引入 `Claim`/`Event` 节点模式，必要时评估超图表示。
- 建立人工校正、回滚和审计工作流。

## 5. 设计原则清单

- 图谱事实必须可追溯: 每个节点/边都能找到来源和抽取过程。
- 自动抽取必须可撤销: LLM 或规则抽取写入候选事实，不直接不可逆覆盖。
- 检索必须版本化: 答案需要知道自己基于哪个图版本和索引版本。
- 查询必须可降级: 向量索引不可用时仍能用 BM25 和图查询返回结果。
- 接口必须共享核心: CLI、Web、未来 API/MCP 不能各自实现一套业务逻辑。
- 数据库必须可替换: 领域模型和检索策略不绑定单一图数据库语法。

## 参考文献

- [R1] Lingfeng Zhong, Jia Wu, Qian Li, Hao Peng, Xindong Wu. “A Comprehensive Survey on Automatic Knowledge Graph Construction.” 2023. <https://arxiv.org/abs/2302.05019>
- [R2] Haonan Bian. “LLM-empowered knowledge graph construction: A survey.” 2025. <https://arxiv.org/abs/2510.20345>
- [R3] Ruirui Chen et al. “Are Large Language Models Effective Knowledge Graph Constructors?” 2025. <https://arxiv.org/abs/2510.11297>
- [R4] Heiko Paulheim. “Knowledge graph refinement: A survey of approaches and evaluation methods.” Semantic Web, 2017. <https://journals.sagepub.com/doi/10.3233/SW-160218>
- [R5] Patrick Lewis et al. “Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks.” 2020/2021. <https://arxiv.org/abs/2005.11401>
- [R6] Darren Edge et al. “From Local to Global: A Graph RAG Approach to Query-Focused Summarization.” 2024/2025. <https://arxiv.org/abs/2404.16130>
- [R7] Microsoft GraphRAG documentation, “Local Search.” <https://microsoft.github.io/graphrag/query/local_search/>
- [R8] Xiangrong Zhu, Yuexiang Xie, Yi Liu, Yaliang Li, Wei Hu. “Knowledge Graph-Guided Retrieval Augmented Generation.” NAACL 2025. <https://aclanthology.org/2025.naacl-long.449/>
- [R9] Bhaskarjit Sarmah et al. “HybridRAG: Integrating Knowledge Graphs and Vector Retrieval Augmented Generation for Efficient Information Extraction.” 2024. <https://arxiv.org/abs/2408.04948>
- [R10] Haoran Luo et al. “HyperGraphRAG: Retrieval-Augmented Generation via Hypergraph-Structured Knowledge Representation.” 2025. <https://arxiv.org/abs/2503.21322>
- [R11] Stephen Robertson, S. Walker, S. Jones, M. M. Hancock-Beaulieu, M. Gatford. “Okapi at TREC-3.” 1995. <https://www.microsoft.com/en-us/research/publication/okapi-at-trec-3/>
- [R12] Yu. A. Malkov, D. A. Yashunin. “Efficient and robust approximate nearest neighbor search using Hierarchical Navigable Small World graphs.” 2016/2018. <https://arxiv.org/abs/1603.09320>
- [R13] Gordon V. Cormack, Charles L. A. Clarke, Stefan Buettcher. “Reciprocal rank fusion outperforms condorcet and individual rank learning methods.” SIGIR 2009. <https://doi.org/10.1145/1571941.1572114>
- [R14] Renzo Angles. “The Property Graph Database Model.” 2018. <https://ceur-ws.org/Vol-2100/paper26.pdf>
- [R15] ISO/IEC 39075:2024, Graph Query Language (GQL) standard notice. <https://www.gqlstandards.org/>
- [R16] SurrealDB documentation, “Using SurrealDB as a Vector Database.” <https://surrealdb.com/docs/surrealdb/models/vector>
- [R17] Neo4j Cypher Manual, semantic indexes. <https://neo4j.com/docs/cypher-manual/current/indexes/semantic-indexes/>
