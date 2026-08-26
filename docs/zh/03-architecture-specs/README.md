# 架构规格

[中文](README.md) | [English](../../en/03-architecture-specs/README.md)

本卷是 `relay-knowledge` 的规范性架构契约。能力页说明当前能做什么；本卷规定实现必须保留的所有权边界、不变量、资源上限、恢复语义和验收证据。

## 阅读路径

- 第 1–4 章建立系统愿景、工程硬约束、基础运行时和 Source Scope。
- 第 5–13 章规定证据、图事实、存储、派生索引、检索、Provider、代码图、索引、排序和影响分析。
- 第 14–18 章规定 Agent Adapter、统一接口、后台恢复、可观测性和 SLO。
- 第 19–23 章规定安装发布、多仓覆盖、软件全域建模、服务部署和 HTTP API。
- 第 24–27 章给出 Knowledge 开发闭环、索引保留、Git commit 心智模型，以及业务知识到技术图谱映射合同。

## 章节目录

1. [架构愿景与算法版图](01-architecture-vision-and-algorithm-map.md)
2. [工程硬约束](02-engineering-hard-constraints.md)
3. [基础运行时层](03-foundational-runtime.md)
4. [Source Scope 模型](04-source-scope-model.md)
5. [多模态证据摄取](05-multimodal-evidence-ingestion.md)
6. [图事实模型与版本化](06-graph-fact-model-and-versioning.md)
7. [存储引擎与 Mutation Log](07-storage-engine-and-mutation-log.md)
8. [派生索引与新鲜度](08-derived-indexes-and-freshness.md)
9. [混合检索与 Context Packing](09-hybrid-retrieval-and-context-packing.md)
10. [Semantic/Vector Provider 架构](10-semantic-vector-provider-architecture.md)
11. [代码知识图谱模型](11-code-knowledge-graph-model.md)
12. [Tree-sitter 抽取与增量索引](12-tree-sitter-extraction-and-incremental-indexing.md)
13. [代码检索排序与影响分析](13-code-retrieval-ranking-and-impact-analysis.md)
14. [开放 Agent Runtime Adapter 架构](14-open-agent-runtime-adapter-architecture.md)
15. [常驻 Agent 图访问协议](15-resident-agent-graph-access-protocol.md)
16. [统一 API 与交互层架构](16-unified-api-and-interface-architecture.md)
17. [后台服务、恢复与自愈](17-background-service-recovery-and-self-healing.md)
18. [可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md)
19. [安装、发布与升级](19-installation-release-and-upgrade.md)
20. [多仓库代码图谱薄覆盖层](20-multi-repository-code-graph-overlay.md)
21. [软件全域建模架构](21-software-global-domain-modeling.md)
22. [服务化部署、控制面与数据面分离](22-service-deployment-control-data-plane.md)
23. [HTTP API 参考](23-api-reference.md)
24. [代码地图驱动的 Knowledge 开发闭环](24-code-map-backed-knowledge-development-loop.md)
25. [代码索引保留策略](25-code-index-retention.md)
26. [Git Commit + Knowledge：开发迭代理念与 Loop](26-git-commit-knowledge-development-loop.md)
27. [业务知识与技术图谱映射](27-business-knowledge-technical-mapping.md)

API 专题页收在[参考资料索引](reference/README.md)，不重复占用章节编号。

## 契约解释

“必须”“不得”“要求”等规范词表示验收条件。带日期的基准只能证明它明确覆盖的条件，不能豁免架构不变量。实现与文档不一致时，变更必须恢复契约，或同步更新规格及其验收证据。

---

导航：[中文文档总目录](../README.md) | 下一章：[1. 架构愿景与算法版图](01-architecture-vision-and-algorithm-map.md)
