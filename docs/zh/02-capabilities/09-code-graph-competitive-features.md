# 代码图竞争力特性

[中文](./09-code-graph-competitive-features.md) | [English](../../en/02-capabilities/09-code-graph-competitive-features.md)

> 文档版本: 2.0
> 编制日期: 2026-06-08
> 适用范围: 第二卷能力说明

## 能力定位

代码图能力把代码搜索从文本匹配提升到结构化理解。用户能看到 symbol、reference、call、import、chunk、canonical identity 和 edge diagnostic，而不是只得到路径和行号。

## 用户可见行为

- Symbol 命中同时包含 `symbol_snapshot_id` 和 `canonical_symbol_id`。
- Reference、caller/callee、import 和 impact 命中暴露 `edge_kind`、`edge_resolution_state`、`edge_target_hint`、`edge_confidence_basis_points` 和 `edge_confidence_tier`。
- Code query 返回 revision-scoped hit，包含 path、line range、kind、score、freshness、symbol identity、edge diagnostics 和 excerpt。
- 精确 source fallback 兜底命中的 `retrieval_layers` 包含 `lexical` 和 `text_fallback`；definition 兜底还可以包含 `definition`。这些命中的 edge diagnostic 字段保持为空，因为它们是源码文本证据，不是 resolved graph edge。

## 竞争力特性

普通代码搜索无法区分“名字相同但快照不同”的符号，也无法解释调用边是否 resolved。代码图用 snapshot symbol 和 canonical symbol 同时建模，把不确定性作为元数据返回。

相对纯 grep、纯 trigram 或纯 embedding 搜索，代码图把 Sourcegraph/Zoekt 类词法候选、Tree-sitter 结构捕获、BM25 chunk、语义/向量解释召回和版本 scope 组合起来。精确 symbol 和 resolved edge 优先，语义相似只作为补充信号，避免自然语言相关性压过结构事实。当 AST 或已索引词法读模型存在具体召回缺口时，内部 source fallback 作为有界精确文本恢复层参与协作。

## 命令/API 入口

```bash
relay-knowledge repo query core --query retry_policy --kind callers --ref HEAD --format json
relay-knowledge repo query core --query crate::retry_policy --kind imports --ref HEAD --format json
```

## Web 路由感知

代码图谱在索引期间检测 Web 框架路由处理器绑定。支持的框架包括 Express（JavaScript/TypeScript）、Flask/FastAPI（Python）和 Spring（Java）。检测器覆盖真实 Express app/router 调用、多行 route registration、以 anonymous handler 记录的 async inline callback、带 middleware 链的最终 endpoint handler、堆叠 Flask decorator、Flask tuple 或 list 形式的 `methods` 声明、FastAPI `APIRouter(prefix = ...)` 前缀、多个 Spring 类级 `@RequestMapping` 前缀、未注解 Java class 的前缀重置、顺序无关的 `value`/`path` 属性、path 数组、`RequestMethod` 数组，以及以 `http_method = "any"` 记录的无 method Spring mapping。每条检测到的路由生成一条 `CodeRouteRecord`，包含 HTTP 方法、URL 路径、处理器名称、框架标识符、源码位置，以及解析器能够匹配到结构化符号时的处理器符号链接。

路由记录会随 checkpointed batch 一起提交，持久化到 `code_repository_routes`，并作为 route search document 建入索引，因此普通的 durable 仓库索引流程可以通过 hybrid code search 回答"哪个处理函数服务于给定的 HTTP 端点？"等查询。被标注为路由处理器的符号会把 `SymbolRole::RouteHandler` 形式的 `symbol_role` 元数据持久化到 SQLite，使 checkpoint replay 或 full snapshot import 之后的下游检索仍可按 HTTP 端点语义优先排序或过滤。

## 降级与诊断

Parser 或 query failure 只隔离到受影响文件，不会中止整个仓库 batch。未解析或歧义边不会伪装成确定调用。
宽泛 regex、未解析或歧义边、parser degraded、stale code index、source fallback 候选路径和预算降级都必须在响应中可见；unresolved external dependency edge 是 coverage metadata，本身不是降级。`text_fallback` 命中只能补齐召回窗口，不能压过已有 exact symbol 或 resolved edge。benchmark 提升不能依赖已知 path、query 或 symbol 特例。

## 关联架构章节

- [代码知识图谱模型](../03-architecture-specs/11-code-knowledge-graph-model.md)
- [代码检索排序与影响分析](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

导航: 上一章: [8. 代码仓库基础能力](08-code-repository-basics.md) | 下一章: [10. 代码影响分析与报告](10-code-impact-and-reporting.md)
