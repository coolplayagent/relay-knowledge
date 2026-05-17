# 混合检索与 Context Packing

[中文](../../zh/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md) | [English](../../en/03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

混合检索是系统的算法核心。普通向量检索擅长相似内容，普通 BM25 擅长精确词项；`relay-knowledge` 需要同时回答术语、概念、多跳关系、时间事实、代码符号和影响分析，因此必须把多路召回、结构扩展、融合、rerank 和 context packing 作为一个算法整体。

## 2. 查询流程

```text
normalize query
  -> resolve source scope and freshness policy
  -> plan retriever families
  -> lexical / semantic / vector / graph / code recall
  -> candidate normalization and dedup
  -> weighted reciprocal-rank fusion
  -> graph expansion and local rerank
  -> context pack budgeting
  -> response with provenance and freshness metadata
```

任何 retriever 都不能绕过 scope filter、authorization policy 或 freshness policy。

## 3. 融合模型

基础融合使用加权 RRF：

```text
score(candidate) = sum(weight_i / (k + rank_i)) + structural_bonus - penalty
```

`structural_bonus` 来自 source authority、direct graph path、accepted lifecycle、exact symbol match、fresh index 和 evidence confidence。`penalty` 来自 stale lag、degraded backend、ambiguous entity、low confidence 或 duplicate parent evidence。

## 4. 图扩展

Graph expansion 从高置信候选出发，只在预算内扩展：

- entity neighborhood。
- direct relation/claim/event path。
- schema-guided path。
- temporal predecessor/successor。
- code symbol reference/call/import edge。

扩展结果必须带 path provenance，不能只返回“相关上下文”。

## 5. Context Pack

Context pack 是 agent 和 UI 的稳定证据包。它包含：query metadata、retriever sources、rank explanations、context items、source spans、graph paths、structured facts、code artifacts、freshness、degraded state、budget 和 truncation reason。

Context packing 优先保证多样性和可引用性：同一父 evidence、同一 symbol、同一 source span 的重复命中会合并；低置信扩展不能挤掉直接 evidence。

## 6. 验收标准

- 精确术语、概念相似、多跳关系、时间事实和代码符号查询都有对应 retriever 信号。
- 返回结果能解释每个 item 的来源、rank 贡献和 freshness。
- 任一 backend degraded 时仍能以可解释方式降级，而不是静默缺失。

---

导航: 上一章: [8. 派生索引与新鲜度](08-derived-indexes-and-freshness.md) | 下一章: [10. Semantic/Vector Provider 架构](10-semantic-vector-provider-architecture.md)
