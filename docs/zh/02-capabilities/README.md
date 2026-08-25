# 能力说明

[中文](README.md) | [English](../../en/02-capabilities/README.md)

本卷只描述当前 `relay-knowledge` 实现中可执行、可观察、可验证的行为：能力做什么、从哪个界面进入、如何降级，以及用什么证据验收。前瞻要求属于[架构规格](../03-architecture-specs/README.md)，带日期的结果属于[基准记录](../05-benchmarks/README.md)或[验证记录](../06-verification/README.md)。

## 阅读路径

- 第 1–7 章介绍本地运行时、图事实、Context Pack、混合检索、新鲜度和多模态证据。
- 第 8–10 章介绍代码仓库索引、代码图检索、影响分析与报告。
- 第 11–14 章介绍 Provider、Web、Agent 接入和 Worker 运维面。
- 第 15 章定义评估与质量门禁。

## 章节目录

1. [能力版图总览](01-capability-overview.md)
2. [本地优先运行时与 CLI](02-local-first-runtime-and-cli.md)
3. [证据与图事实](03-evidence-and-graph-facts.md)
4. [查询与 Context Pack 基础](04-query-and-context-pack-basics.md)
5. [混合检索竞争力](05-hybrid-retrieval-advantage.md)
6. [新鲜度与索引恢复](06-freshness-and-index-recovery.md)
7. [多模态证据能力](07-multimodal-evidence-capability.md)
8. [代码仓库基础能力](08-code-repository-basics.md)
9. [代码图竞争力特性](09-code-graph-competitive-features.md)
10. [代码影响分析与报告](10-code-impact-and-reporting.md)
11. [Semantic/Vector Provider 后端](11-semantic-vector-provider-backend.md)
12. [Web 工作区能力](12-web-workspace-capabilities.md)
13. [Agent 接入能力](13-agent-access-capabilities.md)
14. [运维与 Worker 能力](14-operations-and-worker-capabilities.md)
15. [评估与质量门禁](15-evaluation-and-quality-gates.md)

## 状态用语

“已实现”表示文中入口存在生产调用方；“降级”表示响应仍在明确边界内可用，并显式报告受影响层。研究目标或架构要求只有在本卷给出可执行证据时，才构成当前能力声明。

---

导航：[中文文档总目录](../README.md) | 下一章：[1. 能力版图总览](01-capability-overview.md)
