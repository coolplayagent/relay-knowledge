# 代码影响分析与报告

[中文](./10-code-impact-and-reporting.md) | [English](../../en/02-capabilities/10-code-impact-and-reporting.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

影响分析把 changeset 变成可解释的风险线索。它从 changed path、deleted symbol name、callee identity 和 import/module seed 出发，避免对整个 scope table 做无界扫描。

## 用户可见行为

```bash
relay-knowledge repo impact core --base main --head HEAD --format json
```

Impact 输出包含变更路径、受影响符号、引用/调用/导入信号、freshness、score、excerpt 和 edge metadata。普通 `repo query` 不接受 `impact` 类型，避免把变更集结果和查询结果混在一起。

## 竞争力特性

相对普通 diff 工具，影响分析能结合代码图和检索上下文，给出“为什么这个文件或符号可能受影响”。相对测试覆盖报告，它还能提示未直接覆盖但由调用、引用或导入传播出的风险。

## 已验证结论

relay-teams E2E 已验证 Python 生产源码范围内的 definition、reference、import、caller 和 hybrid 查询，结果携带 resolved commit、tree hash、path、line range、retrieval layer、index version、freshness、score 和 excerpt metadata。

详细记录保留在 [relay-teams E2E 验证](../06-verification/04-relay-teams-e2e-2026-05-14.md) 和 [relay-teams 代码图检索准确性测试](../06-verification/05-code-graph-retrieval-accuracy-relay-teams-2026-05-15.md)。

## 降级与诊断

路径报告应区分 scope 内外变更。大范围索引或报告应优先通过 scope preview、progress、degradation summary 和 freshness state 提前解释成本。

## 关联架构章节

- [代码检索排序与影响分析](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

导航: 上一章: [9. 代码图竞争力特性](09-code-graph-competitive-features.md) | 下一章: [11. Semantic/Vector Provider 后端](11-semantic-vector-provider-backend.md)
