# 代码检索排序与影响分析

[中文](../../zh/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md) | [English](../../en/03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

> 文档版本: 2.0
> 编制日期: 2026-05-24
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

排序信号包括：BM25、identifier part match、CamelCase/snake_case segmentation、query-to-symbol name normalized overlap、symbol kind prior、path proximity、language filter、graph edge confidence、call direction、caller/callee 查询的非测试源码路径优先级、查询无 test 意图时对 symbol test/benchmark 路径的小幅降权、qualified method 命中的 class-member excerpt context、import surface/re-export file、已具备 declaration-shape evidence 的 header chunk declaration surface priority、chunk quality、freshness、semantic/vector rank 和 rerank explanation。排序在 lowercased lexical scoring 前保留原始 query 大小写，用于 identifier segmentation 和 intent check。当查询本身明确包含 test 或 benchmark 意图时，test/benchmark path 调整不生效。

业界代码搜索实践要求词法、结构和语义分层：Zoekt/Google Code Search 类 trigram candidate 适合 substring/regex 初筛，BM25 适合自然语言和文档 chunk，Tree-sitter capture 适合 symbol/edge，semantic/vector 适合概念性解释查询。排序不能用语义分数覆盖 exact symbol 或 resolved edge，也不能让宽泛 regex 结果绕过 scope、path、language 和 revision filter。

代码仓库查询采用 AST 优先、精确 grep 兜底的内部协作。Definition、reference 和 hybrid 查询先访问版本化 tree-sitter 图和 SQLite FTS 读模型；当结构化路径存在明确召回缺口时，允许在同一已索引 revision scope 内执行有界 `rg` 精确文本检索。Import/dependency 查询先从 import edge 得到依赖集合；若依赖目标有代码地图或代码图 target，则使用结构化依赖证据；当 dependency library 未作为代码图 target 建立索引时，import 查询才可以用 unresolved external dependency target hint 作为 grep 词项。Grep 兜底只属于词法层：它可以恢复精确源码行并提高召回，但不能返回 AST 图没有证明的 resolved edge 或 confidence。

面向 agent 和维护者的源码检查遵循独立工具策略：优先使用 `rg` 做本地仓库搜索；如果本机未安装 `rg`，继续使用有界 `grep -RIn --exclude-dir=.git --exclude-dir=target --exclude-dir=node_modules --exclude-dir=dist <pattern> <authorized-root>`。这条人工 grep 路径只用于调查和自迭代 prompt，不能接入产品查询热路径，也不能绕过已索引 scope、freshness 或 authorization。

## 4. 候选窗口

FTS 和 grep candidate window 必须先应用 scope/path/language filter，再进入有界评分。高 fan-out caller/callee 查询需要按 edge score 和 line containment 截断，避免一条调用边被多个无关 chunk 放大。

Ripgrep 兜底与 Git 快照读取一样运行在 blocking-worker 边界之后，并受存储侧候选路径上限、候选文件数、命中数、单行长度和 timeout 预算约束。当前实现的兜底预算是最多 256 个候选文件、8 MiB 物化 blob、单行 4096 字节和 3 秒 `rg` timeout。它搜索已索引 commit 内容，而不是脏工作树；候选 blob 应优先通过 `git cat-file --batch-check` 做 size preflight，并在 byte budget 内通过 `git cat-file --batch` 成批读取，batch 不可用或包含异常对象时保留逐路径 skip 回退。物化搜索树必须包含已索引的 dot-path 候选，JSON 解码必须同时接受 ripgrep 的 `text` 字段和 base64 `bytes` 字段。超大 blob 超出物化预算时只跳过当前候选，不阻断后续仍能装入预算的小文件；definition 兜底先按定义行过滤，再执行返回命中数上限。`rg` 不存在、超时、预算耗尽或存储后端无法提供有界候选路径时，查询仍保持有效，并通过 degraded reason 暴露诊断，不能绕过 freshness 或授权边界。产品运行时不递归 shell out 到 `grep`；`grep -RIn` 只作为 agent/维护者调查 workspace 时的有界搜索 fallback 写入文档。

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
- grep 兜底命中必须标记 lexical/text-fallback provenance，且不能携带 resolved edge confidence；hybrid grep 兜底只能补齐结果窗口，不能排在已有结构化命中之前。
- 自迭代和维护者 prompt 必须同时覆盖产品 `rg` 兜底行为与人工 `grep -RIn` 检查 fallback，避免本机缺少 ripgrep 时中断源码分析。
- impact 输出说明哪些结果来自 diff、调用、引用、导入或测试信号。
- benchmark 不通过枚举已知 query、path 或 symbol 特例提升排名；优化必须来自通用排序信号、索引结构或候选下推。

---

导航: 上一章: [12. Tree-sitter 抽取与增量索引](12-tree-sitter-extraction-and-incremental-indexing.md) | 下一章: [14. 开放 Agent Runtime Adapter 架构](14-open-agent-runtime-adapter-architecture.md)
