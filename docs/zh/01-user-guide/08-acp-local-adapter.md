# 第 8 章 ACP 本地 Adapter

[中文](../../zh/01-user-guide/08-acp-local-adapter.md) | [English](../../en/01-user-guide/08-acp-local-adapter.md)

ACP 本地 adapter 面向同进程或本机 agent-client 会话。它暴露与 MCP 相同的检索语义，但更适合需要 progress、cancellation 和 context artifact 的本地交互。

## 8.1 使用边界

ACP adapter 不创建独立业务逻辑，也不绕过统一 API。检索、权限、QoS、audit 和 context pack 结构都与 MCP/CLI/Web 共用核心服务。

MCP 更适合作为其它 agent runtime 的工具服务入口；ACP 更适合本地 agent-client 会话入口。两者都不应直接访问 storage、indexing 或 graph mutation 实现。

## 8.2 会话能力

本地 ACP session adapter 支持:

- initialize capability payload。
- bounded local session。
- progress updates。
- cancellation。
- context artifact。
- bounded in-process audit events。

Prompt turn 运行时会受 max runtime 预算约束。超时或取消时，adapter 返回明确状态，不在后台继续占用查询热路径资源。

## 8.3 Scope 与身份

ACP 使用本地 adapter identity，并携带 untrusted client identity 进入 audit metadata。它遵循与 MCP 同等的授权边界；请求中的 source scope 必须满足 runtime policy，不能因为本地调用而默认提升权限。

需要让 agent 访问某个知识范围时，仍应明确配置 scope policy 或先通过代码仓库注册流程建立 alias。

## 8.4 Context Artifact

ACP prompt response 可以返回 context artifact。artifact 用于把检索到的 context pack、ranking、graph facts、source span、`provenance_trace`、budget 和 truncation 状态交给调用方，而不是把检索细节压成不可审计的自然语言摘要。`provenance_trace` 会列出 cited evidence、visited-but-uncited context、visited nodes/edges、ranking contributions、stale/degraded 状态和授权裁剪结果。

调用方应保留 artifact 中的 source、freshness、degraded reason 和 audit correlation，便于后续复现或解释 agent 输出。

## 8.5 与 MCP 的选择

优先使用 MCP:

- 需要接入外部 agent runtime。
- 需要标准 Streamable HTTP tool/resource/prompt surface。
- 需要 Prometheus metrics endpoint。

优先使用 ACP:

- agent client 与 `relay-knowledge` 在本机协作。
- 需要 progress、cancellation 和 context artifact。
- 不希望为本地会话开放 HTTP 远程访问面。
