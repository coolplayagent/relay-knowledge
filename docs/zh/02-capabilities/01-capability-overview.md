# 能力版图总览

[中文](./01-capability-overview.md) | [English](../../en/02-capabilities/01-capability-overview.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

第二卷只描述当前已经落地、用户可以理解和验证的能力。它不替代第三卷架构白皮书，也不保存研究过程或基准流水账。

`relay-knowledge` 的基础能力是本地知识图谱、证据写入、查询、CLI、Web 和诊断。竞争力特性来自混合检索、版本化新鲜度、代码知识图谱、Agent 接入、后台恢复、多模态证据和可观测运维。

## 用户可见行为

- 安装后可以用本地 SQLite 和确定性 semantic/vector read model 零配置运行。
- 用户可以写入 evidence 和实体标签，查询 context pack，并用 JSON 输出集成脚本。
- 用户可以注册 Git 仓库，索引 clean snapshot，查询 symbol/reference/chunk，并分析 changeset impact。
- 常驻服务可以同时提供 Web、HTTP API、MCP Streamable HTTP 和本地 ACP session adapter。
- Health、service doctor、index status、repo status 和 Web readiness 共享同一诊断语义。

## 竞争力特性

| 能力域 | 普通实现 | relay-knowledge 差异 |
| --- | --- | --- |
| GraphRAG | 只返回文本片段 | 返回带 source span、结构化事实、graph path、freshness 和 ranking explanation 的 context pack |
| 检索 | BM25 或向量单一路径 | BM25、semantic、vector、图路径、代码结构和 RRF 融合 |
| 新鲜度 | 索引状态不透明 | 每个 index cursor 绑定 scope、graph version、backend 和 stale reason |
| 代码检索 | grep 或普通全文搜索 | Git snapshot、tree-sitter、symbol/reference/call/import/chunk、impact analysis 和有界精确文本兜底 |
| Agent 接入 | 裸 tool 调用 | MCP/ACP 共享应用服务、scope policy、QoS、cancellation 和 audit |
| 运维 | 本地命令堆叠 | worker、proposal、silent update、service definition、OTLP 和 Prometheus-ready diagnostics |

## 命令/API 入口

常用入口包括 `status`、`ingest`、`query`、`repo register`、`repo index`、`repo query`、`repo impact`、`health`、`service doctor`、`service run --web --mcp streamable-http` 和 Web operation composer。

## 降级与诊断

能力默认可降级：semantic/vector 可 disabled，代码解析失败可 text-only，`ripgrep` 缺失只影响精确文本兜底，index stale 可解释，外部 worker 不可用时可以保留 BM25 和图路径。降级状态必须进入响应 metadata，而不是静默丢失。

## 关联架构章节

- [架构愿景与算法版图](../03-architecture-specs/01-architecture-vision-and-algorithm-map.md)
- [混合检索与 Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [后台服务、恢复与自愈](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)

---

导航: 下一章: [2. 本地优先运行时与 CLI](02-local-first-runtime-and-cli.md)
