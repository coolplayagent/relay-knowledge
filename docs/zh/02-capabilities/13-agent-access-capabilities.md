# Agent 接入能力

[中文](./13-agent-access-capabilities.md) | [English](../../en/02-capabilities/13-agent-access-capabilities.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

Agent 接入让外部 runtime 安全使用知识底座。系统提供 MCP Streamable HTTP 和本地 ACP session adapter，但不接管 planning、handoff 或最终答案生成。

## 用户可见行为

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs relay-knowledge service run --web --mcp streamable-http
```

默认 MCP 地址是 `http://127.0.0.1:8791/mcp`。客户端先 initialize，保存 `Mcp-Session-Id`，发送 initialized notification 后再调用工具。

## 竞争力特性

MCP tools 暴露 retrieve context、inspect graph、health、service status、index status、授权 code graph query 和授权 code impact。MCP resources 暴露 service、health、index 和 metrics 只读上下文，prompts 提供 retrieval 与 code-impact 模板。

## 命令/API 入口

MCP 不暴露任意 repository indexing；仓库索引需要用户主动运行 `repo index` 或 `repo update`。本地 ACP session adapter 复用相同检索 contract，支持 progress、cancellation、context artifact、QoS admission 和 audit。

## 降级与诊断

未配置 allowed scopes 时，graph tools 拒绝 unspecified scope，除非显式允许。远程 bind 默认拒绝，非本机监听需要显式启用远程客户端策略。

## 关联架构章节

- [开放 Agent Runtime Adapter 架构](../03-architecture-specs/14-open-agent-runtime-adapter-architecture.md)
- [常驻 Agent 图访问协议](../03-architecture-specs/15-resident-agent-graph-access-protocol.md)

---

导航: 上一章: [12. Web 工作区能力](12-web-workspace-capabilities.md) | 下一章: [14. 运维与 Worker 能力](14-operations-and-worker-capabilities.md)
