# 混合检索竞争力

[中文](./05-hybrid-retrieval-advantage.md) | [English](../../en/02-capabilities/05-hybrid-retrieval-advantage.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

混合检索是第二卷最核心的竞争力能力。它同时使用 BM25、local semantic token read model、local hashed-vector ANN、可配置 external semantic/vector backend、graph evidence fallback、code graph documents、schema path、temporal event、community summary 和 RRF。

## 用户可见行为

- 查询结果带 retriever sources 和 ranking explanation。
- BM25 会索引 entity 和 code symbol 的生成式 lexical alias，但不把 alias 当 canonical label 返回。
- Graph paths 保留节点标签、edge fact id、predicate、supporting evidence ids、confidence、status 和 version range。
- Temporal、community 和 code graph 信号可以与普通 evidence 一起进入 context pack。

## 竞争力特性

普通全文搜索容易漏概念相似，普通向量搜索容易漏精确符号，普通图查询缺少自然语言召回。混合检索把这些信号融合后再做预算分配，能同时服务事实问答、代码定位、多跳关系和 agent 上下文构造。

## 命令/API 入口

```bash
relay-knowledge query "retry policy graph path"   --freshness wait-until-fresh   --limit 10   --format json
```

## 降级与诊断

Semantic/vector backend disabled 或 cursor stale 时，BM25 和 graph evidence 仍可工作。响应的 `context_pack.backend_statuses` 会说明 configured backend、model、dimension、scope post-filter 和 indexed graph version。

## 关联架构章节

- [混合检索与 Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Semantic/Vector Provider 架构](../03-architecture-specs/10-semantic-vector-provider-architecture.md)

---

导航: 上一章: [4. 查询与 Context Pack 基础](04-query-and-context-pack-basics.md) | 下一章: [6. 新鲜度与索引恢复](06-freshness-and-index-recovery.md)
