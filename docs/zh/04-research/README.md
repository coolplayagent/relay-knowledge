# 研究资料总览

[中文](../../zh/04-research/README.md) | [English](../../en/04-research/README.md)

> 文档版本: 1.0
> 编制日期: 2026-05-17
> 范围: 研究来源、研究目标、竞争力判断、场景化落地和前瞻路线。

`04-research` 不是资料归档目录，而是 `relay-knowledge` 的前瞻判断层。研究结论必须回答四个问题: 来源是否可信，目标是否服务产品场景，外部经验如何取长补短，以及哪些能力会形成未来竞争力。

## 研究原则

- **来源优先级清晰**: 官方规范和产品文档用于确认协议、接口和生态方向；论文用于识别算法边界和长期趋势；开源项目用于验证工程可行性；内部基准用于校准 relay-knowledge 的真实差距。
- **目标面向未来**: 研究不只解释当前实现，还要提前识别 GraphRAG、Agent 协议、代码图、多模态证据、后台服务和评估体系的演进压力。
- **取长补短**: 借鉴成熟系统的产品抽象、算法组合和运维经验，但不复制会破坏本地优先、可审计、版本一致性和授权边界的设计。
- **场景驱动**: 每条研究结论都应能落到用户上手、代码仓库理解、Agent 检索、服务化运行、索引恢复或质量评估等明确场景。

## 来源分层

| 来源层级 | 典型来源 | 用途 | 采用规则 |
| --- | --- | --- | --- |
| 官方规范与产品文档 | Microsoft GraphRAG、MCP、A2A、OpenAI File Search、Neo4j GraphRAG | 判断生态接口、协议约束和用户体验默认值 | 作为事实来源，避免用二手解读替代 |
| 论文与综述 | KG construction、KG refinement、RAG、GraphRAG、KGE、HybridRAG | 判断算法边界、质量风险和长期路线 | 只转化为可测试的架构原则 |
| 开源实现与工程案例 | ai-knowledge-graph、Tree-sitter、Codebase-Memory、GitHub code navigation | 验证 pipeline、解析、索引和 Agent 接入可行性 | 采用能力语义，不照搬脚本边界 |
| 项目内部材料 | 架构规格、能力说明、relay-teams 基准、自迭代记录 | 对照当前实现、关闭差距和安排优先级 | 作为落地约束和验收基线 |

核心一手来源入口:

- Microsoft GraphRAG 查询引擎: <https://microsoft.github.io/graphrag/query/overview/>
- Microsoft Research DRIFT Search: <https://www.microsoft.com/en-us/research/blog/introducing-drift-search-combining-global-and-local-search-methods-to-improve-quality-and-efficiency/>
- MCP Streamable HTTP 传输规范: <https://modelcontextprotocol.io/specification/2025-11-25/basic/transports>
- A2A 协议规范: <https://a2a-protocol.org/dev/specification/>
- OpenAI File Search: <https://platform.openai.com/docs/guides/tools-file-search/>
- Neo4j GraphRAG: <https://neo4j.com/labs/genai-ecosystem/graphrag/>

## 关键竞争力

- **本地优先且可治理**: 默认在用户授权范围内运行，配置、索引、日志和服务状态可诊断、可暂停、可恢复。
- **图版本与索引新鲜度**: 图事实、BM25、语义索引、向量索引和社区摘要必须能说明对应版本，避免过期上下文进入答案。
- **三层检索与可解释 Context Pack**: 关键词、语义和向量检索各自发挥优势，再通过图路径、证据和排序解释组织上下文。
- **代码知识图谱**: 用 Git snapshot、Tree-sitter、符号/引用图和影响分析，把代码仓库从文本搜索升级为结构化知识空间。
- **Agent 互操作**: MCP/ACP 是接入层，核心服务仍保持清晰 API 和授权边界，未来可扩展到 A2A 网关。

## 章节导读

- [第 1 章 2026 行业能力快照与差距分析](01-industry-capability-snapshot-2026.md): 从行业信号提炼产品差距和前瞻方向。
- [第 2 章 知识图谱技术研究总结](02-knowledge-graph-research.md): 从论文和工程研究提炼图模型、索引和质量原则。
- [第 3 章 arXiv 知识图谱论文深度洞察](03-arxiv-knowledge-graph-paper-insights.md): 将研究前沿转化为 relay-knowledge 的算法雷达。
- [第 4 章 ai-knowledge-graph 参考项目分析](04-ai-knowledge-graph-reference-analysis.md): 从开源 pipeline 中选择性吸收可产品化经验。
- [第 5 章 代码仓库 Tree-sitter 检索研究材料](05-code-repository-tree-sitter-retrieval-research.md): 明确代码图、增量索引和混合检索的工程路线。
- [第 6 章 Agent 协议图检索接入研究](06-agent-protocol-graph-retrieval-research.md): 规划 MCP/ACP/A2A 下的图检索互操作。
- [第 7 章 relay-knowledge 实现借鉴落地路线](07-relay-knowledge-implementation-reference.md): 将研究结论收敛为实现优先级和差距关闭路线。
