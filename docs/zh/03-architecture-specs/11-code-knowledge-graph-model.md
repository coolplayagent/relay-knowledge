# 代码知识图谱模型

[中文](../../zh/03-architecture-specs/11-code-knowledge-graph-model.md) | [English](../../en/03-architecture-specs/11-code-knowledge-graph-model.md)

> 文档版本: 2.0
> 编制日期: 2026-05-24
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

代码仓库不是普通文本目录。先进代码检索必须理解 Git 快照、路径、语言、符号、引用、调用、导入、文档注释、chunk 和 changeset evidence。代码知识图谱把这些结构化为可版本化事实，而不是把代码行简单塞进向量库。

## 2. 核心实体

| 实体 | 说明 |
| --- | --- |
| `CodeRepository` | 本地稳定仓库身份和授权范围 |
| `CodeFile` | 某 tree hash 下的文件实例 |
| `CodeSymbol` | 类、函数、方法、接口、变量、常量、模块等符号快照 |
| `CanonicalSymbol` | 跨 snapshot 的稳定符号身份候选 |
| `CodeChunk` | 可检索上下文单元，绑定行列范围和父符号 |
| `CodeChangeSet` | base/head 范围内的 diff 与影响证据 |

`symbol_snapshot_id` 表示某快照下的定义；`canonical_symbol_id` 表示跨快照稳定身份。二者不能混用。

## 3. 边类型

代码边包括：defines、references、calls、imports、implements、overrides、contains、documents、changed_by、tested_by 和 affects。每条边必须有 resolution state：resolved、unresolved、ambiguous 或 inferred。

`imports` 边是依赖集合的首要结构来源。依赖解析必须先从 import/include/module import 事实和已索引代码地图中获取目标；当依赖目标没有代码地图或代码图索引时，系统保留 unresolved target hint，允许查询时使用有界 `rg` 精确文本兜底补充当前仓库源码证据。兜底命中不能创建依赖图事实，不能把边标记为 resolved，也不能伪装成依赖库自身的代码地图证据。

## 4. 置信度

引用、调用和导入解析可能不确定。结果必须暴露 target hint、confidence basis points、confidence tier 和 resolution reason，不能把推断边伪装成确定调用。

## 5. Scope 绑定

代码事实绑定 repository snapshot 或 changeset scope。相同文件路径在不同 tree hash 下是不同事实实例；worktree overlay 必须显式标注。

## 6. 验收标准

- 检索结果区分 canonical symbol 与 snapshot symbol。
- 未解析或歧义边在 API、CLI、Web 和 context pack 中可见。
- 缺失依赖代码地图时，import 查询仍能暴露 unresolved target hint，并把文本兜底命中标记为当前仓库 lexical evidence。
- 同路径不同 commit 的代码事实不会共享事实主键。

---

导航: 上一章: [10. Semantic/Vector Provider 架构](10-semantic-vector-provider-architecture.md) | 下一章: [12. Tree-sitter 抽取与增量索引](12-tree-sitter-extraction-and-incremental-indexing.md)
