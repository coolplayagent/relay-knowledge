# 图事实模型与版本化

[中文](../../zh/03-architecture-specs/06-graph-fact-model-and-versioning.md) | [English](../../en/03-architecture-specs/06-graph-fact-model-and-versioning.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

图事实模型把非结构化 evidence、LLM 候选输出、代码解析结果和人工确认统一到可追溯状态机中。先进性在于：答案不是事实，候选不是提交，索引不是状态；只有通过 mutation contract 写入的 accepted fact 才能成为图状态。

## 2. 核心事实类型

| 类型 | 作用 |
| --- | --- |
| `Entity` | 人、组织、系统、文件、符号、概念等规范对象 |
| `Relation` | typed edge，描述实体之间的结构关系 |
| `Claim` | 带证据、置信度和生命周期的可争议陈述 |
| `Event` | 带业务时间、参与者和 evidence 的时间事实 |
| `Evidence` | 所有事实的来源锚点 |
| `CodeFact` | 文件、符号、引用、调用、导入、chunk 等代码结构事实 |

结构化事实必须引用 supporting evidence；没有 evidence 的事实只能作为 proposal 或 diagnostic，不得进入 accepted graph。

## 3. 版本模型

- `graph_version` 是系统提交版本，单调递增。
- `valid_from` / `valid_to` 是领域有效时间，不等于提交时间。
- mutation log 记录每次写入的 affected scope、entity、evidence、source hash 和 index family。
- derived index cursor 只声明自己覆盖到某个 graph version，不能改变 graph version。

## 4. 生命周期

```text
proposed -> validated -> accepted
        -> rejected
accepted -> superseded
accepted -> deprecated
```

LLM、OCR、parser 和 agent 输出默认进入 proposed 或 derived evidence。人工确认、规则验证或可信 parser contract 才能把结果推入 accepted。

## 5. 冲突与置信度

冲突不是删除旧事实的理由。系统应保留 competing claims、supporting evidence、confidence、validation note 和 conflict group。检索时可以基于 lifecycle、confidence、freshness 和 source authority 选择默认展示，但必须保留 provenance。

## 6. 验收标准

- 任一 accepted fact 都能追溯到 evidence、mutation 和 graph version。
- 派生索引失败不会改变事实图版本。
- 冲突事实能共存，并在 context pack 中暴露证据和状态。

---

导航: 上一章: [5. 多模态证据摄取](05-multimodal-evidence-ingestion.md) | 下一章: [7. 存储引擎与 Mutation Log](07-storage-engine-and-mutation-log.md)
