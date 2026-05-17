# 代码图竞争力特性

[中文](./09-code-graph-competitive-features.md) | [English](../../en/02-capabilities/09-code-graph-competitive-features.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

代码图能力把代码搜索从文本匹配提升到结构化理解。用户能看到 symbol、reference、call、import、chunk、canonical identity 和 edge diagnostic，而不是只得到路径和行号。

## 用户可见行为

- Symbol 命中同时包含 `symbol_snapshot_id` 和 `canonical_symbol_id`。
- Reference、caller/callee、import 和 impact 命中暴露 `edge_kind`、`edge_resolution_state`、`edge_target_hint`、`edge_confidence_basis_points` 和 `edge_confidence_tier`。
- Code query 返回 revision-scoped hit，包含 path、line range、kind、score、freshness、symbol identity、edge diagnostics 和 excerpt。

## 竞争力特性

普通代码搜索无法区分“名字相同但快照不同”的符号，也无法解释调用边是否 resolved。代码图用 snapshot symbol 和 canonical symbol 同时建模，把不确定性作为元数据返回。

## 命令/API 入口

```bash
relay-knowledge repo query core --query retry_policy --kind callers --ref HEAD --format json
relay-knowledge repo query core --query crate::retry_policy --kind imports --ref HEAD --format json
```

## 降级与诊断

Parser 或 query failure 只隔离到受影响文件，不会中止整个仓库 batch。未解析或歧义边不会伪装成确定调用。

## 关联架构章节

- [代码知识图谱模型](../03-architecture-specs/11-code-knowledge-graph-model.md)
- [代码检索排序与影响分析](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

导航: 上一章: [8. 代码仓库基础能力](08-code-repository-basics.md) | 下一章: [10. 代码影响分析与报告](10-code-impact-and-reporting.md)
