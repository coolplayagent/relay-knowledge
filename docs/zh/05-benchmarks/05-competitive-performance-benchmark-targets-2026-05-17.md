# 竞争力与高性能基准目标 2026-05-17

[中文](../../zh/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md) | [English](../../en/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md)

本文把竞争力和高性能研究转化为后续 benchmark 应跟踪的指标。它不是一次实测记录，而是设计回归门禁和优化实验时的目标清单。

## 1. 检索质量指标

| 场景 | 指标 |
| --- | --- |
| 混合图谱问答 | Recall@k、MRR、negative rejection、stale rejection、graph path coverage、context pack token budget。 |
| 代码检索 | exact symbol rank、caller/callee rank、import/reference resolution rate、source fallback recall/provenance、false positive count、impact precision、query p50/p95/p99。 |
| 本机文件检索 | filename/path query p50/p95/p99、content query p50/p95/p99、permission-filter cost、candidate window size、stale/degraded rate。 |

## 2. 索引性能指标

| 场景 | 指标 |
| --- | --- |
| Cold graph/code/file index | indexed item count、elapsed、peak RSS、write batch count、parse/extract throughput、index size。 |
| Incremental update | changed item count、affected item count、refresh elapsed、cursor lag、missed event count、fallback rescan count。 |
| No-op refresh | elapsed、blob/file reads、SQLite writes、queue tasks created、freshness state。 |
| Background worker | queue depth、lease recovery count、dead-letter count、retry count、worker saturation、timeout count。 |

## 3. 本机文件检索基准集

后续应准备三个 fixture 层级：

- Small: 1K-10K 文件，覆盖常见文档、源码、隐藏目录、ignore 规则和权限过滤。
- Medium: 100K-500K 文件，覆盖多 root、深目录、重复文件名、二进制和大文件跳过。
- Stress: 1M+ 文件或生成式路径列表，重点测 path/trigram/posting list、metadata filter、watcher lag 和 bounded rescan。

每个 fixture 至少包含：

- 文件名精确查询、模糊路径查询、扩展名查询、目录限定查询。
- 内容词项查询、短语查询、大小/mtime/mime 组合过滤。
- 删除、rename、move、permission change、watcher overflow 或 cursor invalidation 的恢复场景。

## 4. 高性能算法观测字段

检索 trace 和 benchmark 输出应记录：

- retriever family、candidate count、post-filter count、RRF rank contribution、rerank score、truncation reason。
- scope、authorization root、index cursor、graph/file/code version、stale lag、degraded reason。
- code source fallback trigger reason、candidate file count、materialized bytes、`text_fallback` hit count、candidate/budget degraded reason。
- query latency breakdown: normalize、filter、candidate recall、scoring、graph expansion、context packing、storage IO。
- worker latency breakdown: enqueue、lease wait、scan/parse/extract、write batch、cursor commit、reconcile。

## 5. 回归原则

- 不通过枚举 benchmark query、path、symbol 或 fixture 名称解决质量问题。
- 性能优化必须能解释通用机制，例如候选下推、索引结构、批处理、缓存、增量更新或并发边界。
- source fallback 只能作为有界 exact-text recovery；候选查询失败或预算耗尽时必须记录 degraded reason，不能绕过结构化排序和 scope 授权。
- 文件名查询和内容查询分开设预算；内容索引失败不得拖累文件定位。
- 所有指标必须能在 CLI、Web 或 benchmark harness 中复现，并记录命令、环境变量和数据版本。

## 6. 关联文档

- [竞争力、高性能与本机文件检索研究 2026](../04-research/08-competitive-performance-research-2026.md)
- [派生索引与新鲜度](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [混合检索与 Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [代码检索排序与影响分析](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)
- [C/C++ 语法型自迭代测评集 2026-05-20](06-c-cpp-syntax-self-iteration-evaluation.md)
- [多语言语法型自迭代测评集 2026-05-20](07-multilingual-syntax-self-iteration-evaluation.md)
