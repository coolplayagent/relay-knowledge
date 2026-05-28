# relay-knowledge 中文文档

[中文](../zh/README.md) | [English](../en/README.md)

本目录按“书”的方式组织：先读用户手册，再看功能说明，随后进入架构规格、研究资料、基准记录和验证记录。正文只维护在 `docs/zh/` 和 `docs/en/` 两个语言目录中。

目录职责固定如下：`01-user-guide` 只放可执行用户流程；`02-capabilities` 只描述当前已实现能力；`03-architecture-specs` 保留硬约束、接口边界和前瞻产品要求；`04-research` 保留带日期的研究和差距分析；`05-benchmarks` 放基准与优化记录；`06-verification` 放验证、审计和文档新鲜度记录。各卷正文文件使用两位章节号前缀；`README.md` 是卷首目录，在阅读路径中作为第 0 章。

## 第一卷：用户手册

- [第 0 章 使用指南总览](01-user-guide/README.md)：安装、CLI、知识图谱、代码仓库图谱、Web、Agent 接入、常驻服务、后台任务、可观测性、排障和高级配置的主入口。
- [第 1 章 安装与运行时目录](01-user-guide/01-install-and-runtime.md)
- [第 2 章 CLI 基础](01-user-guide/02-cli-basics.md)
- [第 3 章 CLI 命令参考](01-user-guide/03-cli-command-reference.md)
- [第 4 章 知识图谱](01-user-guide/04-knowledge-graph.md)
- [第 5 章 代码仓库图谱工作流](01-user-guide/05-code-repository-graph-workflow.md)
- [第 6 章 Web 工作区](01-user-guide/06-web-workspace.md)
- [第 7 章 MCP Agent 接入](01-user-guide/07-mcp-agent-access.md)
- [第 8 章 ACP 本地 Adapter](01-user-guide/08-acp-local-adapter.md)
- [第 9 章 常驻服务](01-user-guide/09-resident-service.md)
- [第 10 章 Worker、Proposal 与 Audit](01-user-guide/10-workers-proposals-audit.md)
- [第 11 章 可观测性与遥测](01-user-guide/11-observability-and-telemetry.md)
- [第 12 章 高级配置参考](01-user-guide/12-advanced-configuration.md)
- [第 13 章 运维与排障](01-user-guide/13-operations-and-troubleshooting.md)

## 第二卷：能力说明

- [第 1 章 能力版图总览](02-capabilities/01-capability-overview.md)
- [第 2 章 本地优先运行时与 CLI](02-capabilities/02-local-first-runtime-and-cli.md)
- [第 3 章 证据与图事实](02-capabilities/03-evidence-and-graph-facts.md)
- [第 4 章 查询与 Context Pack 基础](02-capabilities/04-query-and-context-pack-basics.md)
- [第 5 章 混合检索竞争力](02-capabilities/05-hybrid-retrieval-advantage.md)
- [第 6 章 新鲜度与索引恢复](02-capabilities/06-freshness-and-index-recovery.md)
- [第 7 章 多模态证据能力](02-capabilities/07-multimodal-evidence-capability.md)
- [第 8 章 代码仓库基础能力](02-capabilities/08-code-repository-basics.md)
- [第 9 章 代码图竞争力特性](02-capabilities/09-code-graph-competitive-features.md)
- [第 10 章 代码影响分析与报告](02-capabilities/10-code-impact-and-reporting.md)
- [第 11 章 Semantic/Vector Provider 后端](02-capabilities/11-semantic-vector-provider-backend.md)
- [第 12 章 Web 工作区能力](02-capabilities/12-web-workspace-capabilities.md)
- [第 13 章 Agent 接入能力](02-capabilities/13-agent-access-capabilities.md)
- [第 14 章 运维与 Worker 能力](02-capabilities/14-operations-and-worker-capabilities.md)
- [第 15 章 评估与质量门禁](02-capabilities/15-evaluation-and-quality-gates.md)

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
- [第 20 章 多仓库代码图谱薄覆盖层](03-architecture-specs/20-multi-repository-code-graph-overlay.md)
- [第 21 章 软件全域建模架构](03-architecture-specs/21-software-global-domain-modeling.md)

## 第四卷：研究资料

- [第 0 章 研究资料总览](04-research/README.md)：研究来源、目标、竞争力判断、场景化落地和前瞻路线。
- [第 1 章 2026 行业能力快照与差距分析](04-research/01-industry-capability-snapshot-2026.md)
- [第 2 章 知识图谱技术研究总结](04-research/02-knowledge-graph-research.md)
- [第 3 章 arXiv 知识图谱论文深度洞察](04-research/03-arxiv-knowledge-graph-paper-insights.md)
- [第 4 章 ai-knowledge-graph 参考项目分析](04-research/04-ai-knowledge-graph-reference-analysis.md)
- [第 5 章 代码仓库 Tree-sitter 检索研究材料](04-research/05-code-repository-tree-sitter-retrieval-research.md)
- [第 6 章 Agent 协议图检索接入研究](04-research/06-agent-protocol-graph-retrieval-research.md)
- [第 7 章 relay-knowledge 实现借鉴落地路线](04-research/07-relay-knowledge-implementation-reference.md)
- [第 8 章 竞争力、高性能与本机文件检索研究 2026](04-research/08-competitive-performance-research-2026.md)
- [第 9 章 GitNexus 功能与界面实现研究 2026](04-research/09-gitnexus-reference-analysis-2026.md)
- [第 10 章 软件全域建模研究 2026](04-research/10-software-global-domain-modeling-research-2026.md)

## 附录 A：基准记录

- [附录 A.1 relay-teams 基线 2026-05-14](05-benchmarks/01-relay-teams-baseline-2026-05-14.md)
- [附录 A.2 relay-teams 优化问题清单 2026-05-14](05-benchmarks/02-relay-teams-optimization-issues-2026-05-14.md)
- [附录 A.3 relay-teams 优化研究 2026-05-14](05-benchmarks/03-relay-teams-optimization-study-2026-05-14.md)
- [附录 A.4 自迭代已采纳优化记录](05-benchmarks/04-self-iteration-accepted-optimizations.md)
- [附录 A.5 竞争力与高性能基准目标 2026-05-17](05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md)

## 附录 B：验证记录

- [附录 B.1 文档书架刷新审计 2026-05-17](06-verification/01-documentation-book-refresh-2026-05-17.md)：目录职责、能力关闭状态和书籍式索引刷新记录。
- [附录 B.2 文档刷新审计 2026-05-17](06-verification/02-documentation-refresh-audit-2026-05-17.md)：代码仓库检索自迭代提交的文档同步记录。
- [附录 B.3 文档刷新审计 2026-05-14](06-verification/03-documentation-refresh-audit-2026-05-14.md)：当前文档状态、已刷新内容和开放产品化工作。
- [附录 B.4 relay-teams E2E 验证 2026-05-14](06-verification/04-relay-teams-e2e-2026-05-14.md)
- [附录 B.5 relay-teams 代码图检索准确性测试 2026-05-15](06-verification/05-code-graph-retrieval-accuracy-relay-teams-2026-05-15.md)
- [附录 B.6 Linux 代码图检索准确性测试 2026-05-15](06-verification/06-code-graph-retrieval-accuracy-linux-2026-05-15.md)
- [附录 B.7 Grep 兜底文档刷新审计 2026-05-22](06-verification/07-grep-fallback-documentation-refresh-2026-05-22.md)：代码检索 exact-text fallback 的章节同步记录。
- [附录 B.8 软件全域建模文档刷新审计 2026-05-28](06-verification/08-software-global-modeling-documentation-refresh-2026-05-28.md)：软件全域建模研究与架构归档验证记录。
