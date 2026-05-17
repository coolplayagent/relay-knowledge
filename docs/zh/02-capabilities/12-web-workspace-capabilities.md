# Web 工作区能力

[中文](./12-web-workspace-capabilities.md) | [English](../../en/02-capabilities/12-web-workspace-capabilities.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

Web 工作区把本地能力组织成可视化操作面。它不是单独业务实现，而是复用 application service 的诊断、查询、摄取、代码、索引和服务操作。

## 用户可见行为

Web workspace 从同源服务读取 `/api/project/status`、`/api/health` 和 `/api/web/operations/execute`。页面展示 graph version、health、index lag、GraphRAG readiness、runtime budgets、refresh recovery、stale reasons、operation composer 和执行结果。

## 竞争力特性

Web operation composer 生成 typed command/request preview，用户能在执行前看到 payload 和命令语义。执行时 Rust Web adapter 复用 application service，不复制 CLI 逻辑。

## 命令/API 入口

```bash
relay-knowledge service run --web
curl http://127.0.0.1:8791/api/health
```

加上 `--mcp streamable-http` 后，Web 和 MCP routes 共用同一 HTTP listener 与 QoS budget。

## 降级与诊断

Web execute 请求受 HTTP body budget 和 loopback policy 约束。服务未启用 MCP 时，Web 仍能提供诊断和本地操作组合器。

## 关联架构章节

- [统一 API 与交互层架构](../03-architecture-specs/16-unified-api-and-interface-architecture.md)
- [可观测性、诊断与 SLO](../03-architecture-specs/18-observability-diagnostics-and-slo.md)

---

导航: 上一章: [11. Semantic/Vector Provider 后端](11-semantic-vector-provider-backend.md) | 下一章: [13. Agent 接入能力](13-agent-access-capabilities.md)
