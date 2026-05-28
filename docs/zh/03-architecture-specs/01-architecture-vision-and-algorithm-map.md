# 架构愿景与算法版图

[中文](../../zh/03-architecture-specs/01-architecture-vision-and-algorithm-map.md) | [English](../../en/03-architecture-specs/01-architecture-vision-and-algorithm-map.md)

> 文档版本: 2.1
> 编制日期: 2026-05-28
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

`relay-knowledge` 的核心定位是 **local-first knowledge substrate**：它不是 agent runtime，也不是只包装向量库的 RAG 工具，而是把证据、图事实、派生索引、检索算法、后台恢复和 agent 访问协议合成一个可验证的知识底座。

先进性来自五个组合，而不是某个单点功能：

1. **证据锚定的图事实**：所有实体、关系、claim、事件和代码结构都必须回到 evidence 与 source scope。
2. **版本化事实图与派生索引分离**：GraphStore 是真源，BM25、semantic、vector、community、code index 都只是带 freshness 的读模型。
3. **多路召回与结构扩展协同**：精确词法、语义签名、向量近邻、图路径和代码符号信号进入统一融合与 context packing。
4. **可恢复后台架构**：刷新、OCR、embedding、解析和维护任务都运行在有界 worker、lease、retry 和 dead-letter 边界内。
5. **开放 agent 接入**：MCP、ACP、A2A gateway 或 SDK bridge 只通过统一 API 使用知识能力，不能穿透到存储或索引实现。

## 2. 系统分层

```text
CLI / Web / MCP / ACP / future A2A
        |
        v
Unified API and Interface Contracts
        |
        v
Application Services: policy, orchestration, freshness, budgets
        |
        +--> Retrieval: BM25, semantic, vector, graph expansion, rerank
        +--> Indexing: mutation log consumers and scoped read models
        +--> Storage: graph facts, evidence, versions, mutation log
        +--> Background Workers: parsing, OCR, embedding, recovery
        +--> Observability: logs, metrics, traces, diagnostics
        |
        v
Domain Model: source scope, evidence, facts, code graph, errors
```

依赖方向必须单向向内。任何 UI、协议 adapter 或 worker 都不能直接访问 SQLite、tree-sitter parser、embedding client 或 index writer；它们只能请求应用服务执行受预算、权限和 freshness policy 约束的工作。

`src/relay_knowledge/domain` 源码树按高内聚领域职责组织，同时保持
`crate::domain::{...}` 的 crate 级 API 稳定：`core/` 负责 source scope、graph
version、index、error 和基础 entity；`graph/` 负责 mutation fact、evidence
extraction metadata 和 retrieval context contract；`code/` 负责 code graph fact、
repository indexing/query request、repository set、dependency 和 call-target 规则；
`operations/` 负责 worker、proposal、service operation、audit 和软件全域建模类型；
`knowledge/` 负责 knowledge map topic、source、route 和 history。

## 3. 算法版图

| 算法域 | 目标 | 主路径 |
| --- | --- | --- |
| Source scope | 让知识只在授权、版本化、可审计的范围内生效 | 规范化 source identity，解析 snapshot/change set，绑定索引分区 |
| Graph fact modeling | 把 LLM 和解析器输出变成可追溯事实 | evidence anchoring，claim lifecycle，graph version，mutation log |
| Hybrid retrieval | 覆盖精确术语、概念相似、多跳关系和代码影响 | BM25 + semantic + vector + graph expansion + RRF + rerank |
| Context packing | 把召回结果变成 agent 可引用证据包 | 去重、分组、预算分配、source span、graph path、freshness metadata |
| Code graph | 让代码检索理解符号、引用、调用和变更影响 | tree-sitter captures，stable symbol id，incremental indexing，impact propagation |
| Recovery | 让派生状态能在崩溃、重启和部分失败后回到一致 | persistent cursor，bounded queue，lease，reconciler，dead-letter |

## 4. 阅读顺序

第三卷按“全局到局部、基础到高级、架构到运维”的顺序组织：

1. 第 1-3 章定义架构愿景、硬约束和基础运行时。
2. 第 4-8 章定义 source、evidence、事实图、存储和索引新鲜度。
3. 第 9-13 章定义检索、semantic/vector 后端、代码知识图谱、tree-sitter 抽取和代码影响分析。
4. 第 14-16 章定义开放 agent runtime、常驻图检索协议和统一 API/interface。
5. 第 17-19 章定义后台自愈、可观测性、安装发布和升级。

## 5. 非目标

- 不把外部 agent framework 引入 domain、storage、retrieval 或 indexing 类型。
- 不把向量库作为事实真源。
- 不用 benchmark fixture 的枚举特殊规则替代可泛化算法。
- 不在查询热路径执行大文件扫描、embedding、OCR、全索引重建或数据库压缩。

## 6. 验收标准

- 任一用户可从本章进入第三卷，并理解系统为何不是普通 RAG、普通全文搜索或普通 agent 插件。
- 每个后续章节都能映射到本章的一项算法域或运行时边界。
- 架构先进性必须通过机制、状态、边界和验收指标表达，不靠营销语句。

---

导航: 下一章: [2. 工程硬约束](02-engineering-hard-constraints.md)
