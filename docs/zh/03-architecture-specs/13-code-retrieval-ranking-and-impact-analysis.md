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

排序信号包括：BM25、identifier part match、CamelCase/snake_case segmentation、query-to-symbol name normalized overlap、symbol kind prior、path proximity、language filter、graph edge confidence、call direction、caller/callee 查询的非测试源码路径优先级、查询无 test 意图时对 symbol test/benchmark 路径的小幅降权、qualified method 命中的 class-member excerpt context、import surface/re-export file、已具备 declaration-shape evidence 的 header chunk declaration surface priority、chunk quality、freshness、semantic/vector rank 和 rerank explanation。当查询本身明确包含 test 或 benchmark 意图时，test/benchmark path 调整不生效。

业界代码搜索实践要求词法、结构和语义分层：Zoekt/Google Code Search 类 trigram candidate 适合 substring/regex 初筛，BM25 适合自然语言和文档 chunk，Tree-sitter capture 适合 symbol/edge，semantic/vector 适合概念性解释查询。排序不能用语义分数覆盖 exact symbol 或 resolved edge，也不能让宽泛 regex 结果绕过 scope、path、language 和 revision filter。

## 4. 候选窗口

FTS candidate window 必须先应用 scope/path/language filter，再进入有界评分。高 fan-out caller/callee 查询需要按 edge score 和 line containment 截断，避免一条调用边被多个无关 chunk 放大。

候选窗口应输出可观测字段：每个 layer 的 pre-filter count、post-filter count、score count、truncation reason 和耗时。影响分析、caller/callee 和 import 查询必须随 changed path、seed symbol、module hint 和 edge confidence 扩展，而不是随完整 scope table size 扩展。

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
- benchmark 不通过枚举已知 query、path 或 symbol 特例提升排名；优化必须来自通用排序信号、索引结构或候选下推。

---

导航: 上一章: [12. Tree-sitter 抽取与增量索引](12-tree-sitter-extraction-and-incremental-indexing.md) | 下一章: [14. 开放 Agent Runtime Adapter 架构](14-open-agent-runtime-adapter-architecture.md)
