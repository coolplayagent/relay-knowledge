# 基准与评测记录

[中文](README.md) | [English](../../en/05-benchmarks/README.md)

本卷保存带日期的基准、测评集、回归预算和自迭代采纳记录。结果只对文中记录的 revision、fixture、配置、后端、硬件和 freshness 条件成立；未经同条件复测，不得把历史结果表述为当前性能。

## 当前入口

1. [relay-teams 基线 2026-05-14](01-relay-teams-baseline-2026-05-14.md)
2. [relay-teams 优化问题 2026-05-14](02-relay-teams-optimization-issues-2026-05-14.md)
3. [relay-teams 优化研究 2026-05-14](03-relay-teams-optimization-study-2026-05-14.md)
4. [自迭代优化状态账本](04-self-iteration-accepted-optimizations.md)
5. [竞争力与高性能基准目标 2026-05-17](05-competitive-performance-benchmark-targets-2026-05-17.md)
6. [C/C++ 语法型自迭代测评集](06-c-cpp-syntax-self-iteration-evaluation.md)
7. [多语言语法型自迭代测评集](07-multilingual-syntax-self-iteration-evaluation.md)
8. [Code Index Fact Versioning](08-code-index-fact-versioning.md)
9. [Code Query Foundational Ranking Notes](09-code-query-ranking-foundational.md)
10. [Profile Full Performance And Source Surface Notes](10-profile-all-performance-source-surface-2026-06-04.md)
11. [Coding-Agent 端到端评测门禁](11-coding-agent-e2e-evaluation.md)
12. [大仓库索引弹性长预算模型](12-elastic-index-budgets.md)

历史自迭代运行详情已从主目录移入[归档索引](archive/README.md)，避免历史快照与当前 A.4 主记录共享章节编号。

## 结果判读

- 只有 commit、fixture revision、profile、后端、freshness 和资源预算相同时才能直接比较。
- timeout、stale scope、parser degradation、跳过阶段或未完成 checkpoint 都是失败或未完成测量，不是成功。
- 性能优化不得破坏 durable lease、有界工作、单 writer publication 或完整索引阶段。
- 每项性能改动都必须有能在回归重现时失败的 case 或 metric。

---

导航：[中文文档总目录](../README.md) | 下一篇：[1. relay-teams 基线](01-relay-teams-baseline-2026-05-14.md)
