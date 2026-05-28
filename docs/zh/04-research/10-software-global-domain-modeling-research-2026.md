# 软件全域建模研究 2026

[中文](../../zh/04-research/10-software-global-domain-modeling-research-2026.md) | [English](../../en/04-research/10-software-global-domain-modeling-research-2026.md)

> 文档版本: 1.0
> 编制日期: 2026-05-28
> 范围: 软件全域知识图谱、依赖与 SDK 版本、生成方向、动态演化和后续产品路线。

## 1. 研究结论

`relay-knowledge` 的长期目标应从“代码仓库图谱”升级为“软件全域模型”。软件不是源码文本集合，而是由源码、构建、依赖、SDK、配置、运行时、测试、文档、发布、部署、漏洞、许可证、生成工件和操作事件共同构成的版本化系统。知识图谱必须表达这些要素的相互约束，并在任何要素变化时自动传播影响。

核心判断：

- 图事实是系统真源；BM25、语义、向量、代码索引、SBOM 视图、社区摘要和生成上下文都是派生 read model。
- SDK 版本、依赖版本、生成器版本和目标平台必须成为一等事实，因为它们决定 API surface、代码生成方向、编译约束、兼容性、漏洞暴露面和可回滚边界。
- 自动更新不是“查询时重新扫一遍仓库”。可靠路线是 durable mutation log、affected scope、版本游标、持久任务、租约、重试、dead-letter、fresh/stale/degraded 状态和可观测指标。
- 全域建模不能把缺失外部源码、缺失 SDK header 或未授权包伪装成已解析事实；这些情况必须保留为 unresolved metadata，带 `target_hint`、`resolution_state`、证据和置信度。

## 2. 论文与规范脉络

软件工程知识图谱综述把软件工程 KG 归纳为需求、设计、代码、测试、维护、缺陷和项目管理等多场景知识集成问题。对本项目的启示是：代码图只是入口，最终图谱需要跨越工程生命周期，而不是只优化单一检索器。

Code Property Graph 证明了把 AST、控制流、数据流叠加为统一图模型可以支撑漏洞发现和程序理解。Graph4Code 进一步把函数、类、调用、数据流和文档语义组织成面向机器学习的代码知识图。它们共同说明，软件图谱的竞争力来自结构层融合，而不是把所有文件切块后向量化。

近年的 Programming Knowledge Graph 与 repository-level code generation 研究强调：代码生成质量依赖项目级上下文、API 约束、调用关系、依赖关系和可用库版本。对 `relay-knowledge` 而言，生成方向不应只由相似代码片段决定，还要由 SDK/API surface、依赖锁定状态、配置开关、目标平台、历史变更和测试证据共同约束。

SBOM 规范提供了供应链事实的成熟语义。CycloneDX 强调组件、服务、依赖图、许可证、来源和 pedigree；SPDX 3.0.1 从软件包扩展到 build、AI model、dataset、provenance、漏洞、质量数据和生命周期关系。这说明全域软件模型应兼容 SBOM 语义，但不能止步于导入导出 SBOM；它要把 SBOM 依赖与源码 import、构建 target、发布 artifact 和运行服务连接起来。

动态图和动态知识图谱研究说明，真实图谱的节点、边和特征会随时间变化。EvolveGCN 等工作对动态图学习有启发，但产品底座首先要解决确定性的版本、刷新、冲突和恢复问题；学习模型只能在可追溯图事实之上作为排序、预测或风险评估层。

## 3. 顶层指导思想

### 3.1 以软件要素而不是文件组织图谱

全域模型的基本单元不是文件，而是软件要素：

| 要素 | 图谱责任 |
| --- | --- |
| 源码与符号 | 说明 API、调用、引用、导入、变更和代码生成候选 |
| 构建系统 | 说明 target、feature、profile、平台、工具链和产物关系 |
| 依赖与 SDK | 说明版本、约束、transitive dependency、API surface、漏洞和许可证 |
| 配置与特性开关 | 说明运行时分支、环境依赖、部署差异和生成条件 |
| 测试与质量 | 说明验证覆盖、失败信号、性能基线和回归保护 |
| 发布与部署 | 说明 artifact、服务、升级、回滚、运行状态和诊断路径 |
| 文档与设计 | 说明需求、架构意图、接口契约和行为解释来源 |

文件、chunk 和 embedding 是证据载体，不是领域边界。查询和生成应优先读取结构化图事实，再使用文本和向量 read model 补足表达差异。

### 3.2 用版本化变化传播表达相互作用

软件要素的变化会相互作用：

- SDK 升级会改变 API 可用性、生成模板选择、编译条件、漏洞集合和测试优先级。
- lockfile 变化会改变 SBOM、许可证风险、运行时行为和补丁建议。
- 构建 target 变化会改变源码可达性、条件编译路径和发布 artifact。
- 配置开关变化会改变受影响代码、测试路径和服务诊断解释。
- 代码生成器变化会改变生成文件、调用形态、文档同步和回滚边界。

这些传播必须通过图变更事件和派生索引刷新表达。查询热路径只能读取已经提交的图事实和带新鲜度的 read model；如果索引落后，应报告 stale/degraded，而不是临时绕过边界扫描全仓。

### 3.3 让生成方向受图谱约束

未来的代码生成或改写入口应把 `AnswerContext` 升级为 `GenerationContext`。它不只包含相似代码，还要包含：

- 当前 source scope、repository snapshot、目标语言和构建 target。
- SDK、依赖、lockfile、feature flag 和目标平台约束。
- 可用 API surface、unresolved dependency metadata 和禁止使用的过期接口。
- 相关符号、调用边、测试证据、文档契约和历史变更。
- 索引新鲜度、证据来源、冲突事实和置信度。

生成器只能在这些约束内选择方向。缺失依赖源码时，系统应提示“目标未解析但存在 target hint”，不能把猜测结果写成 accepted edge。

## 4. 后续演进方向

1. **全域 schema v1**: 在代码图模型之上补齐 `SoftwareSystem`、`BuildTarget`、`PackageComponent`、`Sdk`、`Generator`、`RuntimeService`、`ReleaseArtifact`、`Vulnerability` 和 `License` 等实体。
2. **依赖与 SDK 索引**: 从 manifest、lockfile、BOM、构建脚本和 import/include 事实生成统一 dependency read model，并区分 resolved、unresolved、ambiguous 和 external。
3. **影响传播任务**: 让 SDK、依赖、配置、构建 target、生成器和源码变更都产生 affected scope 与 durable refresh task，复用现有租约、重试和 dead-letter 机制。
4. **生成上下文**: 将 retrieval context pack 扩展为生成前约束包，支持 API 可用性、兼容性、测试覆盖和风险解释。
5. **全域质量评估**: 增加 dependency freshness、SDK drift、generation constraint hit rate、impact path recall、SBOM/source alignment 和 unresolved edge accuracy 等指标。

## 5. 参考文献

- Software Engineering Knowledge Graph systematic review. <https://www.sciencedirect.com/science/article/pii/S0950584923001829>
- Yamaguchi et al. "Modeling and Discovering Vulnerabilities with Code Property Graphs." 2014. <https://www.ieee-security.org/TC/SP2014/papers/ModelingandDiscoveringVulnerabilitieswithCodePropertyGraphs.pdf>
- Graph4Code. Semantic Web Journal. <https://www.semantic-web-journal.net/system/files/swj2575.pdf>
- "Context-Augmented Code Generation Using Programming Knowledge Graphs." 2024. <https://arxiv.org/abs/2410.18251>
- "Repository-Level Code Generation with Knowledge Graph." 2025. <https://arxiv.org/abs/2505.14394>
- CycloneDX Specification Overview. <https://cyclonedx.org/specification/overview>
- SPDX Specification 3.0.1 Scope. <https://spdx.github.io/spdx-spec/v3.0.1/scope/>
- "Source-code-based software bill of materials generation." Scientific Reports, 2025. <https://www.nature.com/articles/s41598-025-29762-0>
- "A Survey on Dynamic Knowledge Graphs: Representation Learning and Applications." <https://arxiv.org/abs/2310.04835>
- Pareja et al. "EvolveGCN: Evolving Graph Convolutional Networks for Dynamic Graphs." AAAI 2020. <https://ojs.aaai.org/index.php/AAAI/article/view/5984>

---

导航: 上一章: [9. GitNexus 功能与界面实现研究 2026](09-gitnexus-reference-analysis-2026.md)
