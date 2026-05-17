# Tree-sitter 抽取与增量索引

[中文](../../zh/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md) | [English](../../en/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

Tree-sitter 是代码结构入口，但不是全能语义分析器。架构必须把 grammar、query capture、错误降级、增量候选缩小和索引刷新串成可恢复 pipeline，使 unsupported language 或 parse error 只降级局部能力，不破坏整体检索。

## 2. 语言注册

每个语言注册项包含：language id、file extensions、tree-sitter grammar、capture queries、comment rules、identifier segmentation 和 fallback chunker。缺失 grammar 时，文件仍进入 text chunk 和 BM25 路径。

## 3. Capture Contract

Query captures 输出统一结构：definition、reference、call、import、doc comment、symbol span、body span 和 chunk span。Capture 结果在写入前必须经过 scope、path、line/column 和 content hash 校验。

## 4. 全量构建

```text
resolve snapshot
  -> enumerate authorized files
  -> batch parse and chunk
  -> write file/symbol/reference/chunk facts
  -> finalize cross-batch edges
  -> refresh code/BM25/semantic/vector indexes
  -> mark scope fresh
```

全量构建过程中旧 fresh scope 继续服务查询；新 scope 只有 finalize 成功后才成为 fresh。

## 5. 增量更新

增量算法先缩小工作集：

1. 使用 Git diff/status 和 blob hash 找 changed files。
2. 加入 deleted/renamed/moved files。
3. 用反向依赖和 import/call/reference edge 扩散 affected files。
4. 只刷新受影响的 code facts、chunks 和 index families。

## 6. 高性能边界

代码索引采用 Sourcegraph/Zoekt、GitHub Code Search、ripgrep 和 Tree-sitter 类系统的共同原则：先用路径、语言、trigram、symbol name 和 blob hash 缩小候选，再做 AST capture、edge resolution 和语义/向量刷新。AST chunk 应沿函数、类型、模块、doc comment 和 import block 边界切分；fallback text chunk 只在结构解析不可用时接管。

全量 cold index、语义 embedding、跨 batch edge finalization、large file skip/hash 和 parser-heavy work 都属于后台 worker 或 maintenance 边界，不能阻塞查询热路径。增量索引必须记录 changed file count、affected file count、parse throughput、write batch count、candidate window 和 stale lag，便于区分真正增量和隐藏全仓扫描。

## 7. 降级策略

Parse error、grammar panic、capture mismatch 或 unsupported language 生成 parse status 诊断，并回退到 text chunk。降级结果必须出现在 repo status、health 和 context pack metadata 中。

## 8. 验收标准

- 大仓库索引能报告 progress，不替换旧 fresh scope。
- 增量更新只处理 changed 和 affected files，不能全仓扫描伪装为增量。
- 解析失败文件仍能通过文本检索召回。
- 索引 trace 能说明候选缩小、parse、写入和刷新各阶段耗时。

---

导航: 上一章: [11. 代码知识图谱模型](11-code-knowledge-graph-model.md) | 下一章: [13. 代码检索排序与影响分析](13-code-retrieval-ranking-and-impact-analysis.md)
