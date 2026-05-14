# arXiv Knowledge Graph Paper Insights

[English](../../en/research/arxiv-knowledge-graph-paper-insights.md) | [中文](../../zh/research/arxiv-knowledge-graph-paper-insights.md)

This is the English documentation page for `research/arxiv-knowledge-graph-paper-insights.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> 项目: `relay-knowledge`
> 归档日期: 2026-05-11
> 范围: 以 arXiv 论文为主，覆盖知识图谱、LLM + KG、GraphRAG、动态图谱、KGC、评估与行业落地
> 目标: 从论文趋势中提炼对本项目架构和路线的可执行判断

## 1. 核心判断

近三年 arXiv 上的知识图谱研究已经从“如何构建 KG”扩展为“如何把 KG 作为 LLM、RAG、Agent 和企业检索系统的可更新记忆层”。对 `relay-knowledge` 来说，最重要的不是追随单个 GraphRAG 框架，而是建立一个可演进的图谱核心: 可追溯事实、版本化图状态、可刷新索引、多模式检索和可验证回答。

关键洞察:

1. **GraphRAG 正在工程化**: 2024 年 GraphRAG 关注全局摘要和结构化检索；2025-2026 年论文转向成本、延迟、增量更新、鲁棒性、可解释性和行业部署。
2. **LLM 构图不能直接写入真图**: 论文普遍承认 LLM 能显著降低抽取成本，但仍有结构不一致、幻觉、schema 漂移、关系方向错误和评估困难。项目应把 LLM 输出作为候选事实。
3. **时间维度已经成为一等问题**: TG-RAG、T-GRAG、TKGC 等论文都指向同一结论: 静态向量和普通边无法区分同一事实在不同时间的状态。项目必须从第一版设计 `valid_from`、`valid_to`、`graph_version` 和索引版本。
4. **检索不再是单一路径**: 成熟方案往往组合 BM25/向量/实体链接/图遍历/摘要树/路径搜索，并用 RRF、预算控制或 adaptive planner 选择合适模式。
5. **证据组织比召回本身更关键**: KG2RAG、StepChain、STEM、G-Retriever 都强调把候选事实组织成路径、段落、子图或 schema，而不是把 top-k chunk 原样塞给 LLM。
6. **评估正在从答案正确转向系统可信**: KG-LLM-Bench、Robust GraphRAG、XGRAG 等工作关注图编码、噪声、反事实、负例拒答和图组件贡献解释。项目需要内置评估 harness。
7. **LPG 是务实底座**: OptimusKG 等 2026 论文继续采用 labeled property graph 承载细粒度属性、来源和跨域数据。RDF/OWL 更适合作为互操作和约束层，而不是唯一内部表示。

## 2. 论文地图

| 方向 | 代表论文 | 对 relay-knowledge 的意义 |
| --- | --- | --- |
| KG 全景与构建 | [A1], [A3], [A4] | 构图要覆盖 acquisition、refinement、evolution，而不是一次性导入 |
| LLM + KG 路线图 | [A5], [A6] | KG-enhanced LLM、LLM-augmented KG、synergized KG+LLM 三类能力应拆开设计 |
| LLM 构图与本体工程 | [A7], [A8], [A9], [A10] | schema-first 与 schema-free 并存，LLM 输出要经过校验和版本化 |
| GraphRAG 综述 | [A11], [A12], [A13] | GraphRAG 可抽象为 query processor、retriever、organizer、generator、data source |
| 高效 GraphRAG | [A14], [A15], [A16], [A17] | 成本和延迟是落地瓶颈，索引结构与检索预算要成为核心设计 |
| 多跳与证据路径 | [A18], [A19], [A20], [A21] | 检索结果应返回证据链和子图，不只返回 chunk |
| 时间与动态图谱 | [A22], [A23], [A24], [A25], [A26] | 需要时间边、版本边、增量更新评估和索引新鲜度控制 |
| KGE/KGC | [A2], [A27], [A28], [A29] | 嵌入适合补全、rerank 和候选生成，不适合作为事实真源 |
| 评估与解释 | [A30], [A31], [A32] | 评估要覆盖噪声鲁棒、负例拒答、反事实和图组件贡献 |
| 行业与领域 KG | [A16], [A33], [A34], [A35], [A36], [A37] | 真实落地高度依赖 schema、来源、权限、审计和领域验证 |

## 3. 深度洞察

### 3.1 KG 与 LLM 的关系: 不要把 KG 简化成 RAG 插件

`Unifying Large Language Models and Knowledge Graphs` 把 KG/LLM 关系拆成三类: KG 增强 LLM、LLM 增强 KG、KG 与 LLM 协同演化 [A5]。这比“做一个 GraphRAG”更适合指导项目架构，因为 `relay-knowledge` 需要同时支持:

- KG 作为 LLM 的外部记忆和检索上下文。
- LLM 作为 KG 构建、补全、消歧、摘要和解释的工具。
- 图谱和模型互相反馈，例如用户修正、检索失败、答案质量评估反向触发图谱精炼。

`Large Language Models and Knowledge Graphs: Opportunities and Challenges` 进一步强调参数化知识和显式知识的混合表示 [A6]。对项目的结论是: 图谱层必须保存可审计事实，不能让 LLM 的隐式知识覆盖显式图状态。

建议:

- 将 LLM 接入点放在 `extraction`、`resolution`、`summarization`、`query_planning`、`answer_generation`，不要放在 `GraphStore` 内部。
- 图谱事实必须有结构化来源和置信度。LLM 只能提交 `ProposedFact`，由规则、schema、人工或多模型验证后变为 accepted fact。
- 设计双向反馈事件: `AnswerFailed`、`EvidenceMissing`、`UserCorrectionSubmitted`、`EntityMergeSuggested`。

### 3.2 KG 构建: 从“抽三元组”转向“多层结构 + 持续精炼”

`A Comprehensive Survey on Automatic Knowledge Graph Construction` 将构图划分为知识获取、知识精炼、知识演化三阶段 [A3]。2025 年 LLM 构图综述进一步把传统流程改写为 LLM 参与本体工程、知识抽取和知识融合 [A7]。但 `Are Large Language Models Effective Knowledge Graph Constructors?` 明确指出，很多 LLM 构图方法仍停留在实体/关系抽取，容易缺少多层语义结构和系统性评估 [A8]。

`GKG-LLM` 的方向值得重视: 它把普通 KG、事件 KG、常识 KG 放进统一 generalized KG 构建任务 [A9]。这说明项目内部不要把所有关系强行压成二元事实。复杂事实应建模为 `Claim` 或 `Event` 节点，再连接参与实体、时间、地点、证据和条件。

`Accelerating Knowledge Graph and Ontology Engineering with LLMs` 认为 LLM 会加速本体建模、扩展、修改、填充、对齐和实体消歧，但模块化本体很关键 [A10]。这和项目的工程方向一致: schema 需要可组合、可版本化，而不是一个全局巨型枚举。

建议:

- 第一版 domain model 至少有 `Entity`、`Relation`、`Evidence`、`Claim/Event`、`GraphVersion`。
- 支持候选事实状态机: `proposed`、`validated`、`accepted`、`rejected`、`superseded`。
- LLM 抽取管线输出应包含 `source_span`、`extractor_version`、`schema_version`、`confidence`、`normalization_notes`。
- 构图任务分层: document chunk -> mention -> candidate entity -> resolved entity -> fact/claim -> accepted graph mutation。

### 3.3 GraphRAG: 结构化检索的真正价值是“组织上下文”

GraphRAG 综述 [A11][A12][A13] 把系统拆成图索引、图引导检索和图增强生成。这个拆法对项目很有价值，因为它迫使我们把“找得到”和“组织得好”分开:

- Graph-based indexing: 实体图、chunk 图、摘要树、标签层次、时间图、超图。
- Graph-guided retrieval: entity linking、局部邻域、全局社区、路径搜索、schema-guided search。
- Graph-enhanced generation: 证据段落、路径解释、社区报告、因果或时间链。

`LightRAG` 提出双层检索，并把图结构与向量表示结合，同时支持增量更新 [A14]。`E^2GraphRAG` 指出 GraphRAG 和 LightRAG 的效率瓶颈，采用 summary tree、实体图、实体-chunk 双向索引和 adaptive retrieval，在论文实验中报告了显著索引/检索加速 [A15]。`Towards Practical GraphRAG` 更贴近工程: 用 dependency parsing 替代昂贵 LLM 抽取，并通过实体、chunk、relation 多粒度 embedding + RRF 做混合检索 [A16]。

这些论文共同说明: GraphRAG 的核心不是“用图数据库替代向量库”，而是把图作为检索规划和上下文组织层。

建议:

- 项目检索 API 返回 `RetrievalBundle`，包含候选实体、候选 chunk、边、路径、证据、排序来源、版本信息。
- 第一版实现三路召回: BM25、向量、实体/图遍历；融合先用 RRF。
- 增加 `organizer` 模块，把候选结果组织成局部实体包、路径包、时间包、社区摘要包。
- 预算控制必须内建: 最大 hop、最大边数、最大 chunk、最大 token、最大延迟。

### 3.4 多跳推理: 路径搜索要比 k-hop 扩展更节制

`KG2RAG` 先用语义检索拿 seed chunks，再用 KG 做 chunk expansion 和 chunk organization [A18]。`G-Retriever` 面向文本属性图，把图问答转为 Prize-Collecting Steiner Tree 优化，以限制上下文并缓解幻觉 [A19]。`StepChain GraphRAG` 只对检索到的 passage 即时构图，再用问题分解和 BFS 推理流组织显式证据链 [A20]。`STEM` 则把多跳推理改写为 schema-guided graph search，用 query schema graph 进行全局节点锚定和子图检索 [A21]。

共同结论:

- 盲目 k-hop 扩展会迅速带来噪声和上下文膨胀。
- 多跳问题应先拆成子问题、关系断言或 schema graph，再按目标路径扩展。
- 子图检索结果需要保持路径结构，否则生成阶段难以解释。

建议:

- `relay-knowledge` 应实现 `TraversalPolicy`: `local_neighbors`、`path_between`、`schema_guided`、`temporal_scope`、`budgeted_expansion`。
- 多跳检索结果必须保留 `path_id`、`edge_sequence`、`evidence_sequence`。
- 对不确定路径返回多个候选路径，而不是只返回最高分路径。

### 3.5 时间图谱: 版本、新鲜度和历史状态是核心功能

时间方向已经从传统 TKGC 扩展到时间感知 RAG。TKGC 综述区分 interpolation 和 extrapolation [A22]；`TGL-LLM` 将 temporal graph learning 引入 LLM-based TKG 模型，强调时间模式和图/语言对齐 [A23]；`T-GRAG` 针对时间冲突、冗余和时间不敏感检索提出动态 GraphRAG [A24]；`TG-RAG` 用 temporal KG + hierarchical time graph 表示动态知识，并特别关注增量更新成本和检索稳定性 [A25]；2026 年 LLM-guided TKGR distillation 说明时间推理还要面对部署成本 [A26]。

对项目的直接影响:

- 图谱中“事实是什么”必须和“事实在哪个时间范围内成立”分开。
- 向量索引不能覆盖时间语义。相同文本或实体在不同时间的事实可能有不同有效性。
- 增量更新不能只重建全量社区摘要；要支持只刷新受影响时间节点、实体节点和索引分区。

建议:

- 关系和 Claim/Event 节点支持 `valid_from`、`valid_to`、`observed_at`、`source_published_at`。
- `graph_version` 与业务时间分离: 前者表示系统状态，后者表示事实有效时间。
- 检索请求支持 `as_of`、`time_range`、`prefer_latest`。
- 索引刷新事件包含 `affected_entity_ids`、`affected_time_ranges`、`source_hashes`。

### 3.6 KGE/KGC: 作为补全与排序工具，不作为事实真源

经典 KG 综述和 KGE 综述仍然重要 [A1][A2]，但对 `relay-knowledge` 来说，KGE 的角色应谨慎定位。KGE 可以发现相似实体、候选关系、缺失边和排序特征，但它无法替代证据链。

`KICGPT` 用结构知识作为 in-context prompt 来缓解长尾实体问题，不需要额外 fine-tuning [A27]。`OL-KGC` 把本体知识转换成 LLM 可理解文本，为 KGC 提供逻辑指导 [A28]。这些工作说明:

- LLM/KGE 结合可以提升补全能力。
- 本体约束和结构知识可以降低错误传播。
- 补全结果仍需要证据或验证，不应自动成为 accepted edge。

建议:

- 增加 `SuggestedRelation` 或 `CandidateCompletion`，不要直接写入 `Relation`。
- 补全任务输出要和证据检索绑定: 没有证据支持的补全只能作为推荐。
- KGE 模型版本、训练图版本、负采样策略和评估集要记录，否则补全结果不可审计。

### 3.7 评估与解释: 可信 GraphRAG 需要内置而不是外接

`KG-LLM-Bench` 关注 KG textualization 对 LLM 推理的影响 [A30]。这提醒我们: 即使同一张图，编码成 triples、paths、JSON、自然语言摘要或 schema graph，答案质量也会不同。

`Towards Robust RAG Based on Knowledge Graph` 在 RGB 场景下评估噪声鲁棒、信息整合、负例拒答和反事实鲁棒 [A31]。`XGRAG` 进一步通过图扰动衡量图组件对答案的贡献，尝试让 GraphRAG 的解释图原生化 [A32]。

建议:

- 评估集至少覆盖: exact fact、multi-hop、temporal、negative rejection、counterfactual、stale index、ambiguous entity。
- 记录每次回答的 `retrieval_trace`: 查询改写、召回器、候选、融合分、过滤原因、最终证据。
- UI/CLI 展示答案时要能展开证据路径和来源，而不是只展示最终文本。
- 对 graph explanation 预留接口: `explain(answer_id)` 返回贡献节点、贡献边和反事实删除结果。

### 3.8 行业论文的共同模式: schema、来源和审计优先

行业/领域论文对项目更有现实启发:

- `Towards Practical GraphRAG` 在企业遗留代码迁移数据上强调成本和低延迟 [A16]。
- `Knowledge Graph RAG: Agentic Crawling...` 用法规文档测试 recursive crawling 和多跳引用，强调层级与交叉引用 [A33]。
- 临床 KG 构建论文用多 LLM、一致性验证、熵不确定性、RDF/OWL schema 和持续精炼处理高风险领域 [A34]。
- `OntoLogX` 用本体约束把安全日志转为 KG，并连接 MITRE ATT&CK 战术 [A35]。
- `OptimusKG` 采用 LPG 统一生物医学多模态知识，保留 schema、属性、cross-reference 和 provenance [A36]。

共同结论:

- 真正有价值的 KG 系统大多不是开放域大杂烩，而是受 schema、来源、权限和审计约束的领域图谱。
- `relay-knowledge` 应把“可解释的图谱工作流”作为产品核心，而不是只追求更高 top-k 命中。

## 4. 对 relay-knowledge 的架构落点

### 4.1 数据模型

建议最小核心类型:

- `Entity`: 稳定 ID、类型、label、aliases、属性、来源、置信度、状态。
- `Relation`: 类型、source、target、属性、方向、证据、有效时间、图版本。
- `Claim`: 对复杂事实的声明节点，可连接多个实体、时间、地点、条件和证据。
- `Evidence`: 原始来源、span、hash、抽取器、发布时间、采集时间。
- `GraphVersion`: 每次提交后的单调版本，关联 mutation log。
- `IndexVersion`: BM25、向量、语义摘要、社区摘要各自的版本和图版本依赖。

### 4.2 服务边界

建议按以下 trait/模块拆分:

- `GraphStore`: async 图写入、事务、查询、版本读取。
- `EventBus`: bounded event pipeline，支持 backpressure、timeout、cancellation。
- `Extractor`: 规则/LLM/混合抽取器，输出候选事实。
- `Resolver`: 实体消歧、别名合并、冲突检测。
- `Indexer`: BM25、向量、摘要、时间索引刷新。
- `Retriever`: 多路召回、RRF 融合、图遍历、路径组织。
- `Evaluator`: 固定 fixture、回归查询、鲁棒性和新鲜度评估。

### 4.3 检索策略

第一版建议实现五种检索模式:

1. `keyword`: BM25，适合实体名、术语、代码符号、source path。
2. `semantic`: embedding 召回，适合同义表达和自然语言问题。
3. `entity_local`: 实体链接后做受限邻域扩展。
4. `path`: 根据 source/target 或 query schema 做路径搜索。
5. `temporal`: 带 `as_of` 或 `time_range` 的时间过滤检索。

融合策略:

- 默认 RRF，原因是不同召回器分数不可直接比较。
- 每条结果带 `retriever_id`、`rank`、`score`、`graph_version`、`index_version`。
- 检索输出先进入 `organizer`，再进入生成或展示层。

### 4.4 版本和增量更新

从论文趋势看，增量能力不是优化项，而是核心能力。建议:

- 每个 mutation 都生成 `GraphEvent`。
- 索引器消费 graph mutation log，记录最后处理到的 `graph_version`。
- 查询时如果 index lag > threshold，返回 stale warning 或等待刷新。
- 对 summary/community/tag/time tree 类索引做局部失效，不做默认全量重建。

### 4.5 评估清单

建立 `tests/fixtures` 或 `docs/eval-fixtures` 中的小型逻辑集，覆盖:

- 实体别名与消歧。
- 二元关系和 n 元 Claim。
- 多跳路径问题。
- 同一实体随时间变化的问题。
- source conflict 和 rejected fact。
- 索引滞后与图版本不一致。
- 无答案/负例拒答。

指标:

- `recall@k`、`MRR`、`nDCG`。
- path hit rate、evidence completeness。
- stale answer rate。
- entity duplicate rate。
- graph mutation -> index refresh latency p95。

## 5. 优先阅读路线

P0: 直接决定架构边界

- [A5] Unifying LLMs and KGs: Roadmap
- [A3] Automatic KG Construction Survey
- [A13] Retrieval-Augmented Generation with Graphs
- [A16] Practical GraphRAG
- [A25] TG-RAG

P1: 决定检索和上下文组织

- [A14] LightRAG
- [A15] E^2GraphRAG
- [A18] KG2RAG
- [A19] G-Retriever
- [A21] STEM

P2: 决定质量、可信和解释

- [A8] Are LLMs Effective KG Constructors?
- [A30] KG-LLM-Bench
- [A31] Robust KG-RAG
- [A32] XGRAG

P3: 后续增强

- [A2] KGE Survey
- [A27] KICGPT
- [A28] OL-KGC
- [A36] OptimusKG

## 6. 参考文献

- [A1] Shaoxiong Ji, Shirui Pan, Erik Cambria, Pekka Marttinen, Philip S. Yu. “A Survey on Knowledge Graphs: Representation, Acquisition and Applications.” arXiv:2002.00388. <https://arxiv.org/abs/2002.00388>
- [A2] Jiahang Cao, Jinyuan Fang, Zaiqiao Meng, Shangsong Liang. “Knowledge Graph Embedding: A Survey from the Perspective of Representation Spaces.” arXiv:2211.03536. <https://arxiv.org/abs/2211.03536>
- [A3] Lingfeng Zhong, Jia Wu, Qian Li, Hao Peng, Xindong Wu. “A Comprehensive Survey on Automatic Knowledge Graph Construction.” arXiv:2302.05019. <https://arxiv.org/abs/2302.05019>
- [A4] Jiapu Wang et al. “A Survey on Temporal Knowledge Graph Completion: Taxonomy, Progress, and Prospects.” arXiv:2308.02457. <https://arxiv.org/abs/2308.02457>
- [A5] Shirui Pan, Linhao Luo, Yufei Wang, Chen Chen, Jiapu Wang, Xindong Wu. “Unifying Large Language Models and Knowledge Graphs: A Roadmap.” arXiv:2306.08302. <https://arxiv.org/abs/2306.08302>
- [A6] Jeff Z. Pan et al. “Large Language Models and Knowledge Graphs: Opportunities and Challenges.” arXiv:2308.06374. <https://arxiv.org/abs/2308.06374>
- [A7] Haonan Bian. “LLM-empowered knowledge graph construction: A survey.” arXiv:2510.20345. <https://arxiv.org/abs/2510.20345>
- [A8] Ruirui Chen et al. “Are Large Language Models Effective Knowledge Graph Constructors?” arXiv:2510.11297. <https://arxiv.org/abs/2510.11297>
- [A9] Jian Zhang et al. “GKG-LLM: A Unified Framework for Generalized Knowledge Graph Construction.” arXiv:2503.11227. <https://arxiv.org/abs/2503.11227>
- [A10] Cogan Shimizu, Pascal Hitzler. “Accelerating Knowledge Graph and Ontology Engineering with Large Language Models.” arXiv:2411.09601. <https://arxiv.org/abs/2411.09601>
- [A11] Boci Peng et al. “Graph Retrieval-Augmented Generation: A Survey.” arXiv:2408.08921. <https://arxiv.org/abs/2408.08921>
- [A12] Qinggang Zhang et al. “A Survey of Graph Retrieval-Augmented Generation for Customized Large Language Models.” arXiv:2501.13958. <https://arxiv.org/abs/2501.13958>
- [A13] Haoyu Han et al. “Retrieval-Augmented Generation with Graphs (GraphRAG).” arXiv:2501.00309. <https://arxiv.org/abs/2501.00309>
- [A14] Zirui Guo, Lianghao Xia, Yanhua Yu, Tu Ao, Chao Huang. “LightRAG: Simple and Fast Retrieval-Augmented Generation.” arXiv:2410.05779. <https://arxiv.org/abs/2410.05779>
- [A15] Yibo Zhao, Jiapeng Zhu, Ye Guo, Kangkang He, Xiang Li. “E^2GraphRAG: Streamlining Graph-based RAG for High Efficiency and Effectiveness.” arXiv:2505.24226. <https://arxiv.org/abs/2505.24226>
- [A16] Congmin Min et al. “Towards Practical GraphRAG: Efficient Knowledge Graph Construction and Hybrid Retrieval at Scale.” arXiv:2507.03226. <https://arxiv.org/abs/2507.03226>
- [A17] Wenbiao Tao, Xinyuan Li, Yunshi Lan, Weining Qian. “TagRAG: Tag-guided Hierarchical Knowledge Graph Retrieval-Augmented Generation.” arXiv:2601.05254. <https://arxiv.org/abs/2601.05254>
- [A18] Xiangrong Zhu, Yuexiang Xie, Yi Liu, Yaliang Li, Wei Hu. “Knowledge Graph-Guided Retrieval Augmented Generation.” arXiv:2502.06864. <https://arxiv.org/abs/2502.06864>
- [A19] Xiaoxin He et al. “G-Retriever: Retrieval-Augmented Generation for Textual Graph Understanding and Question Answering.” arXiv:2402.07630. <https://arxiv.org/abs/2402.07630>
- [A20] Tengjun Ni et al. “StepChain GraphRAG: Reasoning Over Knowledge Graphs for Multi-Hop Question Answering.” arXiv:2510.02827. <https://arxiv.org/abs/2510.02827>
- [A21] Peng Yu, En Xu, Bin Chen, Haibiao Chen, Yinfei Xu. “STEM: Structure-Tracing Evidence Mining for Knowledge Graphs-Driven Retrieval-Augmented Generation.” arXiv:2604.22282. <https://arxiv.org/abs/2604.22282>
- [A22] Borui Cai et al. “Temporal Knowledge Graph Completion: A Survey.” arXiv:2201.08236. <https://arxiv.org/abs/2201.08236>
- [A23] He Chang et al. “Integrate Temporal Graph Learning into LLM-based Temporal Knowledge Graph Model.” arXiv:2501.11911. <https://arxiv.org/abs/2501.11911>
- [A24] Dong Li et al. “T-GRAG: A Dynamic GraphRAG Framework for Resolving Temporal Conflicts and Redundancy in Knowledge Retrieval.” arXiv:2508.01680. <https://arxiv.org/abs/2508.01680>
- [A25] Jiale Han et al. “RAG Meets Temporal Graphs: Time-Sensitive Modeling and Retrieval for Evolving Knowledge.” arXiv:2510.13590. <https://arxiv.org/abs/2510.13590>
- [A26] Wang Xing et al. “LLM-Guided Knowledge Distillation for Temporal Knowledge Graph Reasoning.” arXiv:2602.14428. <https://arxiv.org/abs/2602.14428>
- [A27] Yanbin Wei, Qiushi Huang, James T. Kwok, Yu Zhang. “KICGPT: Large Language Model with Knowledge in Context for Knowledge Graph Completion.” arXiv:2402.02389. <https://arxiv.org/abs/2402.02389>
- [A28] Wenbin Guo, Xin Wang, Jiaoyan Chen, Zhao Li, Zirui Chen. “Ontology-Enhanced Knowledge Graph Completion using Large Language Models.” arXiv:2507.20643. <https://arxiv.org/abs/2507.20643>
- [A29] Ziwei Zhang et al. “Graph Meets LLMs: Towards Large Graph Models.” arXiv:2308.14522. <https://arxiv.org/abs/2308.14522>
- [A30] Elan Markowitz, Krupa Galiya, Greg Ver Steeg, Aram Galstyan. “KG-LLM-Bench: A Scalable Benchmark for Evaluating LLM Reasoning on Textualized Knowledge Graphs.” arXiv:2504.07087. <https://arxiv.org/abs/2504.07087>
- [A31] Hazem Amamou, Stéphane Gagnon, Alan Davoust, Anderson R. Avila. “Towards Robust Retrieval-Augmented Generation Based on Knowledge Graph: A Comparative Analysis.” arXiv:2603.05698. <https://arxiv.org/abs/2603.05698>
- [A32] Zhuoling Li, Ha Linh Hong Tran Nguyen, Valeria Bladinieres, Maxim Romanovsky. “XGRAG: A Graph-Native Framework for Explaining KG-based Retrieval-Augmented Generation.” arXiv:2604.24623. <https://arxiv.org/abs/2604.24623>
- [A33] Koushik Chakraborty, Koyel Guha. “Knowledge Graph RAG: Agentic Crawling and Graph Construction in Enterprise Documents.” arXiv:2604.14220. <https://arxiv.org/abs/2604.14220>
- [A34] Udiptaman Das et al. “Clinical Knowledge Graph Construction and Evaluation with Multi-LLMs via Retrieval-Augmented Generation.” arXiv:2601.01844. <https://arxiv.org/abs/2601.01844>
- [A35] Luca Cotti et al. “OntoLogX: Ontology-Guided Knowledge Graph Extraction from Cybersecurity Logs with Large Language Models.” arXiv:2510.01409. <https://arxiv.org/abs/2510.01409>
- [A36] Lucas Vittor et al. “OptimusKG: Unifying biomedical knowledge in a modern multimodal graph.” arXiv:2604.27269. <https://arxiv.org/abs/2604.27269>
- [A37] Yang Zhao et al. “CLAUSE: Agentic Neuro-Symbolic Knowledge Graph Reasoning via Dynamic Learnable Context Engineering.” arXiv:2509.21035. <https://arxiv.org/abs/2509.21035>
