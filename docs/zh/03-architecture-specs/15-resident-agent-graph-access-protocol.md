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

MCP 不暴露任意 index refresh、repo indexing 或文件系统遍历，MCP 自身不能启动这些写操作；显式 CLI/Web 操作仍可用，启用的受管理 watcher 也可独立通过 durable queue 对账并发布 checked-out commit。

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

## 文件诊断与内容完整性（#393）

代码索引的版本新鲜度与内容完整性分别表达。`freshness.state=fresh` 表示请求版本已追上，不保证每个文件都完整解析。仓库状态、报告与查询 freshness 的 `content_integrity` 包含 `state`（`complete`、`partial`、`unknown`）、`degraded_file_count`（按路径去重）和 `source_scope`。旧响应缺少该字段时按 `unknown` 处理。`degraded_reason` 保留为兼容诊断，不能单独用于判断是否需要重新索引。

```powershell
relay-knowledge repo diagnostics demo --ref HEAD --limit 50 --format json
relay-knowledge repo diagnostics demo --ref HEAD --path src --limit 50 --cursor $nextCursor --format json
```

分页默认 50 条，最多 200 条；按路径、消息排序。重复使用同一 ref 与路径过滤条件，传入返回的 `next_cursor` 继续读取；HEAD 移动或发布更精确的 scope 都不会改变已开始分页的快照。续页在同一存储事务中读取 scope 元数据及诊断条目，并验证仓库及解析后的 commit。游标使用固定长度指纹绑定规范化路径过滤条件，避免合法的长过滤条件生成不可用的续页游标。快照被清理后明确报错。`repo report` 继续展示最多 20 条摘要，并通过 `degradation_summary_truncated` 和 `diagnostics_command` 提供完整诊断入口。内容不完整时，即使命中的文件正常，也不能推断查询覆盖完整；缺失事实可能影响未命中文件或跨文件关系。

HTTP 入口为 `GET /api/v1/code/repositories/{alias}/diagnostics`，参数包括 `ref`、JSON 数组字符串 `path_filters`、`limit` 和 `cursor`；CLI 支持 `--remote`。MCP 工具为 `relay_code_diagnostics`，接受 `repository`、`ref_selector`、`path_filters`、`limit`、`cursor`，并遵守授权及上下文预算。

此变更复用现有诊断表，无需迁移或重建索引。升级时应将 agent 的完整性判断改为读取 `content_integrity`；旧版本仍可能对部分内容返回整体 `degraded`。版本过期、任务未完成及 graph-only 的保守处理保持有效。外部依赖不在授权索引范围内时仍使用 unresolved edge 元数据，不计入文件解析降级。

存储合同拆分后，诊断查询使用共享的 `CodeQueryReadStore` 读取能力。框架图查询也独立传递整个 scope 的内容完整性，不与版本新鲜度混合。

Partitioned report 路由放在现有 repository owner 中，保留活动快照校验及 control store 回退，并维持适配层 facade 的文件预算。
