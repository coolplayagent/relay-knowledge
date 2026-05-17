# relay-knowledge 中文文档

[中文](../zh/README.md) | [English](../en/README.md)

本目录按“书”的方式组织：先读用户手册，再看功能说明，随后进入架构规格、研究资料、基准记录和验证记录。正文只维护在 `docs/zh/` 和 `docs/en/` 两个语言目录中。

目录职责固定如下：`01-user-guide` 只放可执行用户流程；`02-capabilities` 只描述当前已实现能力；`03-architecture-specs` 保留硬约束、接口边界和前瞻产品要求；`04-research` 保留带日期的研究和差距分析；`05-benchmarks` 放基准与优化记录；`06-verification` 放验证、审计和文档新鲜度记录。

## 第一卷：用户手册

- [使用指南总览](01-user-guide/README.md)：安装、CLI、GraphRAG、代码仓库、Web、Agent 服务、排障和高级配置的主入口。
- [第 1 章 安装与运行时目录](01-user-guide/01-install-and-runtime.md)
- [第 2 章 CLI 基础](01-user-guide/02-cli-basics.md)
- [第 3 章 知识图谱工作流](01-user-guide/03-knowledge-graph-workflow.md)
- [第 4 章 代码仓库工作流](01-user-guide/04-code-repository-workflow.md)
- [第 5 章 Web 工作区](01-user-guide/05-web-workspace.md)
- [第 6 章 Agent 与常驻服务](01-user-guide/06-agent-and-service.md)
- [第 7 章 运维与排障](01-user-guide/07-operations-and-troubleshooting.md)
- [第 8 章 高级配置参考](01-user-guide/08-advanced-configuration.md)

## 第二卷：能力说明

- [GraphRAG 功能文档](02-capabilities/graphrag-capability-guide.md)：context pack、新鲜度、后端、多模态、代码图、恢复、Web、MCP 和 ACP 行为。
- [混合检索 Context Pack](02-capabilities/hybrid-retrieval-context-pack.md)：检索器来源、RRF 融合、结构化图事实、图路径和后端状态。
- [Semantic/Vector Provider 后端](02-capabilities/semantic-vector-provider-backend.md)：外部 embedding provider 配置、脱敏诊断、Web provider 面板和降级行为。
- [代码仓库 Tree-sitter 检索](02-capabilities/code-repository-tree-sitter-retrieval.md)：仓库索引、检索、报告和影响分析。

## 第三卷：架构规格

- [工程硬约束](03-architecture-specs/engineering-hard-constraints.md)
- [基础运行时层规格](03-architecture-specs/foundational-runtime.md)
- [存储层架构设计](03-architecture-specs/storage-layer-design.md)
- [统一 API 层与交互层架构](03-architecture-specs/unified-api-and-interface-architecture.md)
- [GraphRAG 产品与实现路线规格](03-architecture-specs/graphrag-product-and-implementation-roadmap.md)
- [Source Scope 与多模态摄取规格](03-architecture-specs/source-scope-and-multimodal-ingestion.md)
- [代码仓库 Tree-sitter 检索规格](03-architecture-specs/code-repository-tree-sitter-retrieval.md)
- [代码仓库检索 v2 优化](03-architecture-specs/code-repository-retrieval-v2-optimization.md)
- [代码知识图谱能力参考](03-architecture-specs/knowledge-graph-capability-reference.md)
- [Semantic/Vector Provider Backend 规格](03-architecture-specs/semantic-vector-provider-backend.md)
- [开放 Agent Runtime 与混合检索架构](03-architecture-specs/open-agent-runtime-and-hybrid-retrieval-architecture.md)
- [常驻进程 Agent 图检索访问规格](03-architecture-specs/resident-agent-graph-retrieval-access.md)
- [后台服务、静默更新与自愈设计](03-architecture-specs/background-service-and-self-healing.md)
- [先进架构与可观测性设计](03-architecture-specs/advanced-architecture-observability.md)
- [安装部署与发布规格](03-architecture-specs/installation-and-release.md)

## 第四卷：研究资料

- [知识图谱技术研究总结](04-research/knowledge-graph-research.md)
- [arXiv 知识图谱论文深度洞察](04-research/arxiv-knowledge-graph-paper-insights.md)
- [代码仓库 Tree-sitter 检索研究材料](04-research/code-repository-tree-sitter-retrieval-research.md)
- [Agent 协议图检索接入研究](04-research/agent-protocol-graph-retrieval-research.md)
- [relay-knowledge 实现借鉴落地路线](04-research/relay-knowledge-implementation-reference.md)
- [ai-knowledge-graph 参考项目分析](04-research/ai-knowledge-graph-reference-analysis.md)
- [2026 行业能力快照与差距分析](04-research/industry-capability-snapshot-2026.md)

## 附录 A：基准记录

- [relay-teams 基线 2026-05-14](05-benchmarks/relay-teams-baseline-2026-05-14.md)
- [relay-teams 优化研究 2026-05-14](05-benchmarks/relay-teams-optimization-study-2026-05-14.md)
- [relay-teams 优化问题清单 2026-05-14](05-benchmarks/relay-teams-optimization-issues-2026-05-14.md)
- [自迭代已采纳优化记录](05-benchmarks/self-iteration-accepted-optimizations.md)

## 附录 B：验证记录

- [文档书架刷新审计 2026-05-17](06-verification/documentation-book-refresh-2026-05-17.md)：目录职责、能力关闭状态和书籍式索引刷新记录。
- [文档刷新审计 2026-05-17](06-verification/documentation-refresh-audit-2026-05-17.md)：代码仓库检索自迭代提交的文档同步记录。
- [文档刷新审计 2026-05-14](06-verification/documentation-refresh-audit-2026-05-14.md)：当前文档状态、已刷新内容和开放产品化工作。
- [relay-teams E2E 验证 2026-05-14](06-verification/relay-teams-e2e-2026-05-14.md)
- [relay-teams 代码图检索准确性测试 2026-05-15](06-verification/code-graph-retrieval-accuracy-relay-teams-2026-05-15.md)
- [Linux 代码图检索准确性测试 2026-05-15](06-verification/code-graph-retrieval-accuracy-linux-2026-05-15.md)
