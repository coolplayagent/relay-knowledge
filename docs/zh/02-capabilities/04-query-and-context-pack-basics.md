# 查询与 Context Pack 基础

[中文](./04-query-and-context-pack-basics.md) | [English](../../en/02-capabilities/04-query-and-context-pack-basics.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

查询能力把图事实、证据、索引状态和预算组织成稳定的 context pack。用户得到的不只是结果列表，而是可以被 CLI、Web、MCP 和 ACP 共同理解的证据包。

## 用户可见行为

- `query` 支持 source scope、freshness、limit 和 JSON 输出。
- 响应包含 graph version、indexed graph version、retrieval mode、source scope 和 degraded reason。
- Context item 包含 retriever sources、ranking signals、entities、source span、structured facts 和 code artifact。
- `truncated` 和 `budget_used` 明确说明上下文是否被预算截断。

## 竞争力特性

Context pack 是 agent 可引用的结构化边界。它把文本、图路径、代码工件、新鲜度、后端状态和排序解释合在一个响应中，避免调用方重新猜测结果为何出现。

## 命令/API 入口

```bash
relay-knowledge query "SQLite graph state"   --source docs   --freshness wait-until-fresh   --limit 8   --format json
```

## 降级与诊断

`degraded_reason` 可能来自 stale index、graph-only、backend unavailable、parser degraded 或 budget exceeded。调用方应读取 metadata，而不是只看 item 数量。

## 关联架构章节

- [混合检索与 Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)

---

导航: 上一章: [3. 证据与图事实](03-evidence-and-graph-facts.md) | 下一章: [5. 混合检索竞争力](05-hybrid-retrieval-advantage.md)
