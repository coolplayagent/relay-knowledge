# 常驻 Agent 图访问协议

[中文](../../zh/03-architecture-specs/15-resident-agent-graph-access-protocol.md) | [English](../../en/03-architecture-specs/15-resident-agent-graph-access-protocol.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

常驻进程把知识底座作为可审计服务暴露给本地 agent 和工具。协议层先进性在于：它提供 tool/resource/prompt、session、cancellation、QoS、scope policy 和 audit，而不是把 CLI 命令裸露给 agent。

## 2. MCP 能力

MCP Streamable HTTP 暴露：

- graph retrieval tool。
- graph inspection tool。
- authorized code query 和 code impact tool。
- health、service status、index status resources。
- retrieval planning 和 code impact prompts。

MCP 不暴露任意 index refresh、repo indexing 或文件系统遍历；这些操作需要用户显式 CLI/Web 操作。

## 3. Session 与传输

Server 校验 initialize 后签发不可预测 session id。客户端必须发送 initialized notification；后续请求携带 session header 和 protocol version。取消请求绑定到 session 和 in-flight operation。

## 4. ACP / Local Adapter

ACP 或本地 session adapter 使用同一 unified API，暴露 progress、artifact、cancellation 和 context pack。它不拥有独立业务逻辑，也不绕过 MCP scope policy 的同等授权检查。

## 5. Result Shape

Agent-facing 结果包含：items、graph paths、structured facts、code artifacts、freshness、degraded state、budget、truncation、audit id 和 stable error。所有可引用内容都必须有 source provenance。

## 6. 验收标准

- 未授权 scope 的 agent 请求在执行前被拒绝。
- cancellation 能释放预算并写入审计事件。
- MCP/ACP 返回同一应用服务语义，不出现接口漂移。

---

导航: 上一章: [14. 开放 Agent Runtime Adapter 架构](14-open-agent-runtime-adapter-architecture.md) | 下一章: [16. 统一 API 与交互层架构](16-unified-api-and-interface-architecture.md)
