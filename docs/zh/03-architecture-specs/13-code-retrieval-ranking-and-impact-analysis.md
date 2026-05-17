# 代码检索排序与影响分析

[中文](../../zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md) | [English](../../en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

代码检索的先进性来自结构信号与词法/语义信号的融合。单纯 grep 会漏掉调用关系，单纯向量会弱化精确符号，单纯 AST 会缺少自然语言意图；排序必须同时看 symbol、chunk、edge、path、language、query intent 和 freshness。

## 2. 查询类型

| 类型 | 重点信号 |
| --- | --- |
| definition | exact symbol、identifier segmentation、path/language filter |
| reference | reference edge、target hint、confidence、callsite excerpt |
| caller/callee | call edge、line containment、fan-out budget |
| import/dependency | import edge、module path、resolution state |
| explanation | doc comment、body chunk、semantic/vector similarity |
| impact | changeset diff、reverse dependency、test edge、risk score |

## 3. Ranking Signals

排序信号包括：BM25、identifier part match、CamelCase/snake_case segmentation、symbol kind prior、path proximity、language filter、graph edge confidence、call direction、caller/callee 查询的非测试源码路径优先级、import surface/re-export file、chunk quality、freshness、semantic/vector rank 和 rerank explanation。当查询本身明确包含 test 或 benchmark 意图时，caller/callee 的源码路径优先级不生效。

## 4. 候选窗口

FTS candidate window 必须先应用 scope/path/language filter，再进入有界评分。高 fan-out caller/callee 查询需要按 edge score 和 line containment 截断，避免一条调用边被多个无关 chunk 放大。

## 5. 影响分析

Impact analysis 从 changeset scope 出发：

```text
changed files
  -> changed symbols
  -> direct references/calls/imports
  -> reverse dependency expansion
  -> tests/docs/config affected candidates
  -> risk groups with evidence
```

影响分析输出不是绝对结论，而是带 evidence、path、edge confidence 和 budget truncation 的风险分组。

## 6. 验收标准

- 查询 `foo_bar` 能命中 `fooBar`、`FooBar` 和多段符号名，但 typed edge 查询不被过度放宽。
- caller/callee 结果定位到包含调用行的 chunk。
- impact 输出说明哪些结果来自 diff、调用、引用、导入或测试信号。

---

导航: 上一章: [12. Tree-sitter 抽取与增量索引](12-tree-sitter-extraction-and-incremental-indexing.md) | 下一章: [14. 开放 Agent Runtime Adapter 架构](14-open-agent-runtime-adapter-architecture.md)
