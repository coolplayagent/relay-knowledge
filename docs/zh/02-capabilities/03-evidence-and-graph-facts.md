# 证据与图事实

[中文](./03-evidence-and-graph-facts.md) | [English](../../en/02-capabilities/03-evidence-and-graph-facts.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

证据和图事实是 GraphRAG 的基础功能。系统不把文本片段直接当答案，而是把 evidence、entity、relation、claim、event、source span 和 confidence 组织为可追溯图状态。

## 用户可见行为

- `ingest` 可以写入 source-scoped evidence 和 entity label。
- 结构化 API 可写入 source path、span、confidence、status、typed relation、claim 和 event。
- 结构化 fact 必须引用 supporting evidence ids。
- `rejected` 和 `superseded` evidence 不会作为默认检索上下文返回。

## 竞争力特性

普通 RAG 多数只保存 chunk。`relay-knowledge` 保存 evidence 和图事实之间的可审计关系，使 context pack 可以展示一跳 graph path、claim 状态、event 版本和 supporting evidence，而不是只有自然语言片段。

## 命令/API 入口

```bash
relay-knowledge ingest   --source docs   --content "Rust async services isolate blocking SQLite work"   --entity Rust   --entity SQLite   --format json

relay-knowledge graph inspect --format json
```

## 降级与诊断

写入时会重新校验 confidence、span 和 version range。缺少 supporting evidence 的结构化事实不能直接成为 accepted fact。图检查输出用于确认 evidence、entity、relation、claim、event 和 graph version 的当前状态。

## 关联架构章节

- [多模态证据摄取](../03-architecture-specs/05-multimodal-evidence-ingestion.md)
- [图事实模型与版本化](../03-architecture-specs/06-graph-fact-model-and-versioning.md)

---

导航: 上一章: [2. 本地优先运行时与 CLI](02-local-first-runtime-and-cli.md) | 下一章: [4. 查询与 Context Pack 基础](04-query-and-context-pack-basics.md)
