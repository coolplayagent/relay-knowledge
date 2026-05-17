# relay-knowledge 中文文档

[中文](../zh/README.md) | [English](../en/README.md)

本目录按“书”的方式组织：先读用户手册，再看功能说明，随后进入架构规格、研究资料、基准记录和验证记录。正文只维护在 `docs/zh/` 和 `docs/en/` 两个语言目录中。

目录职责固定如下：`01-user-guide` 只放可执行用户流程；`02-capabilities` 只描述当前已实现能力；`03-architecture-specs` 保留硬约束、接口边界和前瞻产品要求；`04-research` 保留带日期的研究和差距分析；`05-benchmarks` 放基准与优化记录；`06-verification` 放验证、审计和文档新鲜度记录。各卷正文文件使用两位章节号前缀；`README.md` 是卷首目录，在阅读路径中作为第 0 章。

## 第一卷：用户手册

- [第 0 章 使用指南总览](01-user-guide/README.md)：安装、CLI、GraphRAG、代码仓库、Web、Agent 服务、排障和高级配置的主入口。
- [第 1 章 安装与运行时目录](01-user-guide/01-install-and-runtime.md)
- [第 2 章 CLI 基础](01-user-guide/02-cli-basics.md)
- [第 3 章 知识图谱工作流](01-user-guide/03-knowledge-graph-workflow.md)
- [第 4 章 代码仓库工作流](01-user-guide/04-code-repository-workflow.md)
- [第 5 章 Web 工作区](01-user-guide/05-web-workspace.md)
- [第 6 章 Agent 与常驻服务](01-user-guide/06-agent-and-service.md)
- [第 7 章 运维与排障](01-user-guide/07-operations-and-troubleshooting.md)
- [第 8 章 高级配置参考](01-user-guide/08-advanced-configuration.md)

## 第二卷：能力说明

- [第 1 章 GraphRAG 功能文档](02-capabilities/01-graphrag-capability-guide.md)：context pack、新鲜度、后端、多模态、代码图、恢复、Web、MCP 和 ACP 行为。
- [第 2 章 混合检索 Context Pack](02-capabilities/02-hybrid-retrieval-context-pack.md)：检索器来源、RRF 融合、结构化图事实、图路径和后端状态。
- [第 3 章 代码仓库 Tree-sitter 检索](02-capabilities/03-code-repository-tree-sitter-retrieval.md)：仓库索引、检索、报告和影响分析。
- [第 4 章 Semantic/Vector Provider 后端](02-capabilities/04-semantic-vector-provider-backend.md)：外部 embedding provider 配置、脱敏诊断、Web provider 面板和降级行为。

## 第三卷：架构规格

- [第 1 章 架构愿景与算法版图](03-architecture-specs/01-architecture-vision-and-algorithm-map.md)
- [第 2 章 工程硬约束](03-architecture-specs/02-engineering-hard-constraints.md)
- [第 3 章 基础运行时层](03-architecture-specs/03-foundational-runtime.md)
- [第 4 章 Source Scope 模型](03-architecture-specs/04-source-scope-model.md)
- [第 5 章 多模态证据摄取](03-architecture-specs/05-multimodal-evidence-ingestion.md)
- [第 6 章 图事实模型与版本化](03-architecture-specs/06-graph-fact-model-and-versioning.md)
- [第 7 章 存储引擎与 Mutation Log](03-architecture-specs/07-storage-engine-and-mutation-log.md)
- [第 8 章 派生索引与新鲜度](03-architecture-specs/08-derived-indexes-and-freshness.md)
- [第 9 章 混合检索与 Context Packing](03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [第 10 章 Semantic/Vector Provider 架构](03-architecture-specs/10-semantic-vector-provider-architecture.md)
- [第 11 章 代码知识图谱模型](03-architecture-specs/11-code-knowledge-graph-model.md)
- [第 12 章 Tree-sitter 抽取与增量索引](03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)
- [第 13 章 代码检索排序与影响分析](03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)
- [第 14 章 开放 Agent Runtime Adapter 架构](03-architecture-specs/14-open-agent-runtime-adapter-architecture.md)
- [第 15 章 常驻 Agent 图访问协议](03-architecture-specs/15-resident-agent-graph-access-protocol.md)
- [第 16 章 统一 API 与交互层架构](03-architecture-specs/16-unified-api-and-interface-architecture.md)
- [第 17 章 后台服务、恢复与自愈](03-architecture-specs/17-background-service-recovery-and-self-healing.md)
- [第 18 章 可观测性、诊断与 SLO](03-architecture-specs/18-observability-diagnostics-and-slo.md)
- [第 19 章 安装、发布与升级](03-architecture-specs/19-installation-release-and-upgrade.md)

## 第四卷：研究资料

- [第 1 章 2026 行业能力快照与差距分析](04-research/01-industry-capability-snapshot-2026.md)
- [第 2 章 知识图谱技术研究总结](04-research/02-knowledge-graph-research.md)
- [第 3 章 arXiv 知识图谱论文深度洞察](04-research/03-arxiv-knowledge-graph-paper-insights.md)
- [第 4 章 ai-knowledge-graph 参考项目分析](04-research/04-ai-knowledge-graph-reference-analysis.md)
- [第 5 章 代码仓库 Tree-sitter 检索研究材料](04-research/05-code-repository-tree-sitter-retrieval-research.md)
- [第 6 章 Agent 协议图检索接入研究](04-research/06-agent-protocol-graph-retrieval-research.md)
- [第 7 章 relay-knowledge 实现借鉴落地路线](04-research/07-relay-knowledge-implementation-reference.md)

## 附录 A：基准记录

- [附录 A.1 relay-teams 基线 2026-05-14](05-benchmarks/01-relay-teams-baseline-2026-05-14.md)
- [附录 A.2 relay-teams 优化问题清单 2026-05-14](05-benchmarks/02-relay-teams-optimization-issues-2026-05-14.md)
- [附录 A.3 relay-teams 优化研究 2026-05-14](05-benchmarks/03-relay-teams-optimization-study-2026-05-14.md)
- [附录 A.4 自迭代已采纳优化记录](05-benchmarks/04-self-iteration-accepted-optimizations.md)

## 附录 B：验证记录

- [附录 B.1 文档书架刷新审计 2026-05-17](06-verification/01-documentation-book-refresh-2026-05-17.md)：目录职责、能力关闭状态和书籍式索引刷新记录。
- [附录 B.2 文档刷新审计 2026-05-17](06-verification/02-documentation-refresh-audit-2026-05-17.md)：代码仓库检索自迭代提交的文档同步记录。
- [附录 B.3 文档刷新审计 2026-05-14](06-verification/03-documentation-refresh-audit-2026-05-14.md)：当前文档状态、已刷新内容和开放产品化工作。
- [附录 B.4 relay-teams E2E 验证 2026-05-14](06-verification/04-relay-teams-e2e-2026-05-14.md)
- [附录 B.5 relay-teams 代码图检索准确性测试 2026-05-15](06-verification/05-code-graph-retrieval-accuracy-relay-teams-2026-05-15.md)
- [附录 B.6 Linux 代码图检索准确性测试 2026-05-15](06-verification/06-code-graph-retrieval-accuracy-linux-2026-05-15.md)
