# 软件全域建模、CodeGraph 与 Search Everything 对比研究 2026

[中文](../../zh/04-research/11-software-global-codegraph-search-everything-comparison-2026.md) | [English](../../en/04-research/11-software-global-codegraph-search-everything-comparison-2026.md)

> 文档版本: 1.0
> 编制日期: 2026-05-31
> 范围: 2026 年开源软件、论文和系统工程资料在软件全域建模、代码图谱和全域搜索上的对比。

## 1. 研究结论

2026 年的软件理解工具正在从“读文件和 grep”转向三类互补底座：

- **软件全域建模**: 把源码、构建、依赖、SDK、配置、测试、文档、发布、运行时、漏洞、许可证和操作事件建成版本化事实模型。
- **CodeGraph**: 用 Tree-sitter、LSP、CPG、调用/引用/import 边和社区发现为 Agent 提供结构化代码导航。
- **Search Everything**: 把文件路径、metadata、全文、BM25、向量、符号、图路径和权限过滤统一成低延迟检索体验。

优秀系统的共同点不是“有向量库”或“有 MCP 工具”，而是把结构事实、派生索引、刷新状态和查询预算分开管理。`relay-knowledge` 应继续坚持图事实为真源，BM25、语义、向量、本机文件路径、代码符号和社区摘要都是带版本的新鲜度 read model。

## 2. 对比矩阵

| 方向 | 代表来源 | 强项 | 不足 | relay-knowledge 应吸收 |
| --- | --- | --- | --- | --- |
| 软件全域建模 | RepoDoc、RPG、SemanticForge、SBOM/SPDX/CycloneDX | 把 repository、模块、依赖、生成、文档和供应链纳入统一生命周期 | 多数论文验证集中在生成或文档任务，离本地服务运维还有距离 | 将 `SoftwareSystem`、`BuildTarget`、`PackageComponent`、`Sdk`、`ReleaseArtifact` 和运行状态作为一等事实 |
| CodeGraph | Codebase-Memory、jcodemunch-mcp、code-graph-mcp、GitNexus、Tree-sitter、Code Property Graph | 能回答调用链、引用、入口、影响范围、死代码和跨文件导航问题 | 早期开源项目常缺少 durable task、权限 scope、索引版本和恢复语义 | 用持久图谱服务 Agent，但所有抽取、刷新和查询都必须有 source scope、版本和诊断 |
| Search Everything | Everything、plocate、Zoekt、Sourcegraph Code Search、ripgrep、Vespa/OpenSearch RRF | 路径/名称/全文/regex/BM25/混合排序成熟，高性能机制明确 | 单独使用时不理解软件生命周期和图事实 | 拆分路径、metadata、内容、代码符号、向量和图路径 read model，再用 query router 与 RRF/phased ranking 融合 |
| 确定性 Code RAG | RAGdeterm、Repository-Level Code Generation with KG、KCoEvo | 通过结构库、依赖边和可重复检索降低 embedding 随机性 | 可能覆盖语言和动态运行事实不足 | 对代码生成、影响分析和审计查询优先使用结构事实；向量只做补召回和重排信号 |
| 漏洞与质量分析 | KG-HiAttention、Code Property Graph、Graph4Code | AST、CFG、DFG、专家特征和图注意力适合解释风险路径 | 模型输出不能替代可审计证据 | 漏洞、测试失败、复杂度、churn 和运行事件应成为图事实或可追溯派生信号 |

## 3. 开源软件观察

### 3.1 MCP 原生 CodeGraph

Codebase-Memory 是 2026 年最直接的信号之一：它把 Tree-sitter 解析、多语言抽取、持久知识图、调用图遍历、影响分析和 MCP 暴露组合在一起，目标是减少 Agent 反复读文件和 grep 的成本。它的论文把评估放在真实仓库和 Agent 查询上，说明代码图已经从 IDE 辅助能力转为 Agent runtime 的核心上下文服务。

jcodemunch-mcp、code-graph-mcp、CodeGraphContext、Octocode 和同类项目进一步显示：开源社区正在快速收敛到“本地 SQLite/嵌入式存储 + Tree-sitter + FTS/BM25 + 可选向量 + MCP 工具”的组合。这个方向适合本地优先产品，但也暴露出两类风险：

- 工具容易把“能查到关系”当成“关系一定新鲜且授权可见”，缺少 source scope、权限摘要、索引游标和 stale/degraded 状态。
- MCP 工具数量越多，越需要 query router、任务意图分类、token budget、trace 和负证据，否则 Agent 仍会在工具调用上浪费成本。

### 3.2 代码搜索与符号检索

Zoekt、Sourcegraph Code Search、GitHub Code Search 和 ripgrep 代表了成熟代码搜索的工程下限：trigram、regex literal 提取、分片索引、并发遍历、路径过滤和符号优先必须足够快。对 `relay-knowledge` 来说，结构图谱不能替代这些低层召回器；正确路线是先用过滤和候选窗口缩小搜索，再把符号边、BM25、exact text、向量和图路径作为可解释排序信号。

### 3.3 Search Everything 与本机文件索引

Everything 和 plocate 说明“搜一切”的体验首先来自文件名、路径和 metadata 的极低延迟索引，而不是把所有内容都 embedding。平台 watcher、USN/FSEvents/inotify/fanotify 和 bounded rescan 思路可以作为文件 read model 的增量刷新来源。`relay-knowledge` 不应依赖外部桌面搜索守护进程，但应吸收它们的机制：路径索引和内容索引分离、变更游标可诊断、cursor overflow 后进入受控重扫、权限过滤在候选窗口前生效。

## 4. 论文路线观察

RepoDoc 把 repository knowledge graph 用于文档生成和增量更新，重点不只是生成质量，而是把模块聚类、跨引用、Mermaid 图和变更影响传播串成完整文档生命周期。这与 `relay-knowledge` 的启示是：文档也是软件图谱的派生视图，不能脱离代码、依赖和版本变化。

RPG 和 Repository-Level Code Generation with KG 把代码生成问题提升到仓库级规划，强调文件结构、能力、数据流、函数和依赖约束。SemanticForge 进一步把静态/动态知识图、约束求解和增量维护结合起来。它们共同指向一个结论：生成上下文必须有结构约束，不能只由相似片段或长上下文拼接决定。

RAGdeterm 提醒代码检索存在强确定性要求。对审计、生成、修复和影响分析而言，同一仓库版本、同一查询策略和同一权限 scope 应返回可复现的证据集合。向量检索可以补充概念召回，但不能成为代码事实和依赖事实的唯一入口。

KCoEvo 和 KG-HiAttention 代表图谱参与代码演化和漏洞分析的方向。它们说明 KG 不只用于问答，也可以承载演化轨迹、候选变更、风险路径和解释性信号。产品落地时应先保证图事实、证据、测试和运行事件可追溯，再把学习模型放在排序或风险评估层。

## 5. `relay-knowledge` 落地原则

1. **图事实是真源**: 源码符号、依赖、SDK、构建 target、配置、测试、文档、发布和运行事件都进入版本化图事实；派生索引只记录可重建视图。
2. **多 read model 并行**: `code_symbol`、`code_text`、`local_file_path`、`local_file_metadata`、`local_file_content`、`semantic`、`vector` 和 `graph_path` 分开刷新、观测和降级。
3. **查询先路由后融合**: exact symbol、file path、conceptual、impact、dependency、documentation、vulnerability 和 temporal 查询进入不同召回器组合，再用 RRF 或 phased ranking 合并。
4. **刷新必须可恢复**: 抽取、embedding、FTS、边终结、社区发现和文档派生都走 durable task、attempt lease、bounded queue、retry backoff、checkpoint、dead-letter 和 index lag 指标。
5. **缺失外部范围不可伪装**: 未授权仓库、缺失 SDK/header、外部包和生成 SDK 只能写成 unresolved metadata，不能变成 `degraded_reason` 或猜测 accepted edge。
6. **Agent 接口要有诊断**: MCP/ACP 返回的不只是答案，还要包含 source scope、graph version、index cursor、candidate window、ranking contribution、truncation reason 和 stale/degraded state。

## 6. 后续产品路线

| 优先级 | 建议 | 验收信号 |
| --- | --- | --- |
| P0 | 为全域模型定义代码、文件、依赖、SDK、构建、测试、文档和发布事实的最小 schema。 | 架构文档能说明每类事实的 owner、版本和派生索引。 |
| P0 | 将代码图检索 trace 标准化，覆盖 exact symbol、BM25、source fallback、graph path、semantic/vector 和 RRF contribution。 | Context Pack 能解释每条证据来自哪个召回器和图版本。 |
| P1 | 增加本机文件 Search Everything read model，先做路径/metadata，再做内容/semantic/vector。 | 文件名查询不被内容抽取拖慢，cursor overflow 有 stale/degraded 诊断。 |
| P1 | 将 RepoKG 文档生命周期纳入派生视图设计。 | 文档更新能按 affected scope 定位，而不是全仓重新生成。 |
| P1 | 为结构化 CodeGraph 增加 Agent golden set。 | 评估覆盖调用链、引用、入口、死代码、影响范围、依赖漂移和跨语言搜索。 |

## 7. 参考来源

- Codebase-Memory: Tree-Sitter-Based Knowledge Graphs for LLM Code Exploration via MCP. <https://arxiv.org/abs/2603.27277>
- RepoDoc: A Knowledge Graph-Based Framework to Automatic Documentation Generation and Incremental Updates. <https://arxiv.org/abs/2604.26523>
- RAGdeterm: Deterministic retrieval-augmented generation for code generation. <https://www.sciencedirect.com/science/article/pii/S2352711026001299>
- KCoEvo: A Knowledge Graph Augmented Framework for Evolutionary Code Generation. <https://arxiv.org/abs/2603.07581>
- KG-HiAttention: synergizing AI-based knowledge graphs and deep learning for explainable software vulnerability analysis. <https://www.frontiersin.org/journals/artificial-intelligence/articles/10.3389/frai.2026.1794125/full>
- RPG: A Repository Planning Graph for Unified and Scalable Codebase Generation. <https://arxiv.org/abs/2509.16198>
- Knowledge Graph Based Repository-Level Code Generation. <https://arxiv.org/abs/2505.14394>
- SemanticForge: Repository-Level Code Generation through Semantic Knowledge Graphs and Constraint Satisfaction. <https://arxiv.org/abs/2511.07584>
- Code Property Graphs. <https://www.ieee-security.org/TC/SP2014/papers/ModelingandDiscoveringVulnerabilitieswithCodePropertyGraphs.pdf>
- Graph4Code. <https://www.semantic-web-journal.net/system/files/swj2575.pdf>
- Tree-sitter. <https://github.com/tree-sitter/tree-sitter>
- Zoekt. <https://github.com/sourcegraph/zoekt>
- Sourcegraph Code Search. <https://sourcegraph.com/docs/code-search/features>
- Everything indexes. <https://www.voidtools.com/support/everything/indexes>
- plocate. <https://plocate.sesse.net/>
- ripgrep performance notes. <https://burntsushi.net/ripgrep/>

---

导航: 上一章: [10. 软件全域建模研究 2026](10-software-global-domain-modeling-research-2026.md)
