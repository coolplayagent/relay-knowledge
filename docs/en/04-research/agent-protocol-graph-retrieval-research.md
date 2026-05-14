# Agent Protocol Graph Retrieval Research

[English](../../en/04-research/agent-protocol-graph-retrieval-research.md) | [中文](../../zh/04-research/agent-protocol-graph-retrieval-research.md)

This is the English documentation page for `research/agent-protocol-graph-retrieval-research.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> 文档版本: 1.0
> 编制日期: 2026-05-12
> 研究范围: 常驻 `relay-knowledge` 进程通过 MCP server 和 Agent Client Protocol adapter 向其它 agent 暴露图检索能力
> 结论摘要: MCP 和 ACP 都可以接入图检索，但它们解决的问题不同；v1 应双协议同级支持，同时保持核心检索能力只实现一次
> 协议刷新: MCP 链接已按 2025-11-25 规范刷新；早期 HTTP+SSE 只作为兼容方向，不作为新实现默认路径。

## 1. 背景

`relay-knowledge` 已经把 CLI、Web、MCP、本地 ACP 和未来 HTTP API 收口到统一 API 层，并把混合检索、图检查、索引刷新、健康状态和后台服务状态定义为 application service 能力。当前实现已提供 MCP Streamable HTTP tool/resource/prompt adapter、本地 ACP session adapter、可选 JSONL 持久审计、Prometheus metrics exporter 和旧 HTTP+SSE 兼容端点；后续重点是平台 service manager 集成、silent update operator、跨进程 worker orchestration 和更完整的远程 ACP adapter。

本研究把用户提到的 ACP 明确为 **Agent Client Protocol**。这里不把 ACP 解释为 Agent Communication Protocol 或 Agent Control Protocol。

核心研究问题:

- MCP server 是否适合作为其它 agent 的图检索工具入口。
- ACP agent-facing adapter 是否适合作为 IDE、agent client 或 agent host 的图检索会话入口。
- 两种协议如何共享同一套权限、QoS、检索新鲜度、审计和错误语义。
- 常驻进程如何避免把 protocol adapter 变成第二套业务层。

## 2. 协议事实

### 2.1 MCP

MCP 采用 host / client / server 架构。host 管理 client 生命周期、权限、授权、LLM 集成和上下文聚合；server 暴露专业能力，并通过 capability negotiation 声明 resources、tools 和 prompts。参考:

- [MCP Architecture](https://modelcontextprotocol.io/specification/2025-11-25/architecture)
- [MCP Tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
- [MCP Resources](https://modelcontextprotocol.io/specification/2025-11-25/server/resources)
- [MCP Prompts](https://modelcontextprotocol.io/specification/2025-11-25/server/prompts)

对 `relay-knowledge` 的含义:

- `relay-knowledge` 适合做 MCP server，提供图检索、图状态、索引状态和诊断资源。
- MCP host 继续负责模型调用、tool selection、用户确认、跨 server 编排和完整对话状态。
- `relay-knowledge` server 不应该读取完整 conversation，也不应该接管 host 的 agent runtime 职责。
- MCP tool 输出应优先使用 `structuredContent`，同时提供简短文本，便于旧客户端或调试场景读取。

### 2.2 Agent Client Protocol

Agent Client Protocol 是 agent 与 client application 之间的 JSON-RPC 协议。它定义初始化、认证、session 创建或恢复、prompt turn、session/update、session/cancel、permission request、tool-call progress 和 `_meta` 扩展点。参考:

- [ACP Overview](https://agentclientprotocol.com/protocol/overview)
- [ACP Architecture](https://agentclientprotocol.com/get-started/architecture)
- [ACP Tool Calls](https://agentclientprotocol.com/protocol/tool-calls)
- [ACP Extensibility](https://agentclientprotocol.com/protocol/extensibility)

对 `relay-knowledge` 的含义:

- ACP 更适合把 `relay-knowledge` 暴露成一个可被 client 驱动的知识检索 agent 会话。
- ACP `session/prompt` 可以承载自然语言检索请求，`session/update` 可以承载检索进度、索引新鲜度、降级原因和结果准备状态。
- ACP tool-call `kind=search` 适合表达图检索，`kind=read` 适合表达 graph metadata/resource 读取，`kind=other` 适合表达健康和服务状态。
- ACP 的 permission request 适合保护高风险操作，例如手动 index refresh；普通只读检索仍要执行本地 access policy。

## 3. 协议定位对比

| 维度 | MCP server | ACP adapter |
| --- | --- | --- |
| 主要关系 | host/client 调用 server 能力 | client 驱动 agent session |
| 最适合场景 | 其它 agent 把图检索当工具调用 | IDE 或 agent client 把知识图谱当会话式检索 agent |
| 主要入口 | `tools/list`、`tools/call`、`resources/read`、`prompts/get` | `initialize`、`session/new`、`session/prompt`、`session/update`、`session/cancel` |
| 进度表达 | tool result 或 streaming transport 上的状态 | `session/update` 和 tool-call update |
| 权限表达 | host UI 和 server-side policy 配合 | client permission request 和 adapter policy 配合 |
| 输出形态 | structured tool result、resource、prompt | session updates、tool-call content、final prompt response |
| 风险 | server 被误当 runtime，承担过多规划职责 | 知识服务被误当通用代码修改 agent |

研究结论:

- v1 需要同时定义 MCP 和 ACP，因为它们覆盖不同集成面。
- 两种协议不能分别实现检索逻辑，必须映射到同一组 unified API request / response / error / metadata。
- MCP 是默认推荐的 agent-to-tool 入口；ACP 是 agent-client 会话入口。
- ACP adapter 不应默认提供文件编辑、终端、代码修改或 agent planning 能力。

## 4. 共享能力模型

协议层只负责转换，不拥有图检索规则。共享模型如下:

```text
----------------------+      +----------------------+
| MCP Server Adapter   |      | ACP Session Adapter  |
| tools/resources      |      | session/tool updates |
+----------+-----------+      +----------+-----------+
           |                             |
           +-------------+---------------+
                         |
                         v
               Agent Access Policy
                         |
                         v
                 Unified API Contract
                         |
                         v
              RelayKnowledgeService
                         |
       +-----------------+-----------------+
       v                 v                 v
   Retrieval          Storage          Indexing
```

共享能力必须包括:

- source scope 解析和授权。
- freshness policy。
- graph version 和 indexed graph version。
- index stale / degraded 状态。
- result limit、context byte budget 和 timeout。
- QoS admission、rate limit 和 cancellation。
- trace、metrics、audit log 和 stable error mapping。

## 5. 安全研究

Agent 协议入口会把外部 agent、host 和用户输入直接接到图检索能力上，因此安全边界必须在协议 adapter 内外同时存在。

必要规则:

- 所有请求必须先通过 `AgentAccessPolicy`，再构造 unified API request。
- 默认只允许读向能力，不允许 mutation、commit、entity merge、delete 或跨 scope 写入。
- `refresh_indexes` 虽然不是领域写入，但会消耗 CPU、I/O 和索引资源，默认应关闭或要求 permission。
- prompt injection 内容只能作为 evidence/context，不得改变 access policy、freshness policy 或授权状态。
- MCP tool annotation、ACP `_meta` 和客户端传入身份都只能作为 untrusted input 处理，必须被验证后再进入审计上下文。
- 错误消息不得泄露未授权 scope、完整本地路径、secret、原始代理配置或内部 SQL。

## 6. 运行形态研究

常驻进程应由 OS service manager 托管。协议 adapter 是该进程的外部入口，而不是新的后台调度系统。

推荐运行形态:

- `service` 模式默认只绑定本机地址或 stdio transport。
- MCP 支持 stdio 和本机 streamable HTTP 两种集成方向，远程监听必须显式配置。
- ACP 优先支持 stdio，因为 ACP 常见 client 会按需启动 agent subprocess；如果接入已运行常驻进程，应由本地 launcher/proxy 连接服务。
- 所有协议请求都进入同一套 `net::qos` 预算。即使是 stdio transport，也要纳入 in-flight request 和 queue depth 预算。
- shutdown 时先停止接受新的 MCP/ACP 请求，再取消或完成已接收请求，最后 flush telemetry。

## 7. 检索体验研究

外部 agent 需要的不只是命中文本，还需要可解释上下文。

推荐 context pack 字段:

- `metadata`: trace、request、graph version、indexed graph version、stale。
- `source_scope`: 实际检索范围。
- `freshness`: 实际执行的新鲜度策略。
- `retrieval_mode`: hybrid、graph_only 或 degraded hybrid。
- `results`: graph/evidence/result hits。
- `citations`: evidence id、entity id、scope id 和可展示位置。
- `indexes`: 每类索引状态。
- `degraded_reason`: 索引不可用、stale、timeout、budget truncate 等原因。
- `truncated`: 是否因预算截断。

MCP 中该结构进入 tool `structuredContent`。ACP 中该结构进入 `session/update` 的 `_meta.relayKnowledge`，最终 prompt response 只需要返回 stop reason 和 artifact id 或简短摘要。

## 8. 工程结论

v1 设计与当前实现采用以下路线:

1. 双协议同级: MCP 和 ACP 都在常驻进程中作为一等 adapter 设计。
2. 读向优先: v1 对外开放检索、图检查、健康、服务状态和索引状态；索引刷新默认受限；写入后续再做。
3. 单核心: 两种 adapter 均调用 unified API，不复制检索、索引或存储逻辑。
4. 本地安全默认: 默认本机可访问、scope 最小化、refresh 关闭、远程监听关闭。
5. 可观察: 每次协议请求都有 trace、runtime identity、policy decision、QoS decision、freshness 和 result truncation 记录。
6. 可取消: ACP cancellation 和 MCP transport disconnect 都必须释放预算并停止不必要工作。
