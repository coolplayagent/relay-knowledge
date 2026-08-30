# 证据与图事实

[中文](./03-evidence-and-graph-facts.md) | [English](../../en/02-capabilities/03-evidence-and-graph-facts.md)

> 文档版本: 2.1
> 编制日期: 2026-08-30
> 适用范围: 第二卷能力说明

## 能力定位

证据和图事实是 GraphRAG 的基础功能。系统不把文本片段直接当答案，而是把 evidence、entity、relation、claim、event、source span 和 confidence 组织为可追溯图状态。

## 用户可见行为

- `ingest` 可以写入 source-scoped evidence 和 entity label。
- 结构化 API 可写入 source path、span、confidence、status、typed relation、claim 和 event。
- 结构化 fact 必须引用 supporting evidence ids。
- `rejected` 和 `superseded` evidence 不会作为默认检索上下文返回。
- Repository software ontology 以一等 `SoftwareStatement` 保留 source kind、同 scope evidence refs、extractor id/version、assertion mode、resolution state、有效期、观察时间、confidence 和 fact state；`repo software --kind statements|conflicts` 可直接检查这些字段。
- `entity_key` 表达不含 commit/source scope 的稳定软件身份，`occurrence_id` 表达该身份在一个 snapshot/evidence 集合中的出现；快照和运行事件类型故意保持 scope-bound。

## 竞争力特性

普通 RAG 多数只保存 chunk。`relay-knowledge` 保存 evidence 和图事实之间的可审计关系，使 context pack 可以展示一跳 graph path、claim 状态、event 版本和 supporting evidence，而不是只有自然语言片段。

## 命令/API 入口

```bash
relay-knowledge ingest   --source docs   --content "Rust async services isolate blocking SQLite work"   --entity Rust   --entity SQLite   --format json

relay-knowledge graph inspect --format json

relay-knowledge repo software repository --kind statements --format json
relay-knowledge repo software repository --kind conflicts --format json
```

## 降级与诊断

写入时会重新校验 confidence、span 和 version range。缺少 supporting evidence 的结构化事实不能直接成为 accepted fact。Software ontology 还会校验 predicate domain/range、object 基数、有效期、observed timestamp、跨 scope 引用和 extractor 完整性；失败 statement 以 `rejected` 保留，诊断随 software response 返回。图检查输出用于确认 evidence、entity、relation、claim、event 和 graph version 的当前状态。

## 关联架构章节

- [多模态证据摄取](../03-architecture-specs/05-multimodal-evidence-ingestion.md)
- [图事实模型与版本化](../03-architecture-specs/06-graph-fact-model-and-versioning.md)
- [软件全域建模架构](../03-architecture-specs/21-software-global-domain-modeling.md)

---

导航: 上一章: [2. 本地优先运行时与 CLI](02-local-first-runtime-and-cli.md) | 下一章: [4. 查询与 Context Pack 基础](04-query-and-context-pack-basics.md)
