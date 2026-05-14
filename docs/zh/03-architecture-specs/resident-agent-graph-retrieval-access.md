# 常驻进程 Agent 图检索访问规格

[中文](../../zh/03-architecture-specs/resident-agent-graph-retrieval-access.md) | [英文](../../en/03-architecture-specs/resident-agent-graph-retrieval-access.md)

> 文档版本: 1.0
> 编制日期: 2026-05-12
> 适用范围: 常驻 `relay-knowledge` 进程、MCP server、Agent Client Protocol adapter、外部 agent 图检索接入
> 默认路线: MCP 和 ACP 双协议同级设计，协议 adapter 只做转换和治理，图检索能力只通过统一 API 调用

协议基线以 MCP Streamable HTTP 2025-11-25 为准；A2A 作为后续 agent-to-agent gateway 方向记录在 [开放 Agent Runtime 与混合检索架构](open-agent-runtime-and-hybrid-retrieval-architecture.md)。

## 1. 设计结论

常驻 `relay-knowledge` 进程启动后，应允许其它 agent 通过 MCP server 或 Agent Client Protocol adapter 访问图检索能力。MCP 与 ACP 都是外层 interface adapter，不能拥有业务规则、检索策略、索引刷新逻辑或存储访问。

核心结论:

1. **双协议同级**: v1 同时定义 MCP server 和 ACP adapter。MCP 面向 tool/resource/prompt 集成；ACP 面向 session/prompt/update 集成。
2. **只读优先**: v1 默认开放 `retrieve_context`、`inspect_graph`、`health`、`service_status` 和 `index_status`。`refresh_indexes` 默认关闭或要求显式授权。写入和 mutation proposal 不属于本规格 v1。
3. **统一 API 是唯一业务入口**: 协议 adapter 必须调用 `RelayKnowledgeService` 的统一 API contract，不直接访问 SQLite、索引 metadata、队列、图遍历实现或 mutation log。
4. **常驻进程治理优先**: 所有协议请求必须经过 access policy、QoS admission、timeout、cancellation、scope authorization 和审计。
5. **协议输出同语义**: MCP 和 ACP 的 wire shape 可以不同，但 graph version、index freshness、scope、result、degraded reason、trace 和错误分类必须一致。

## 2. 运行边界

常驻进程启动时初始化:

- 来自 `env`、`paths`、`net` 的已验证运行时配置。
- `RelayKnowledgeService`。
- agent protocol registry（协议注册表）。
- `AgentAccessPolicy`。
- QoS budget tracker（预算跟踪器）。
- request cancellation registry（请求取消注册表）。
- telemetry/audit logger（遥测/审计日志记录器）。
- graceful shutdown token（优雅关闭令牌）。

固定依赖方向:

```text
interfaces::agent::mcp -> api -> application -> domain
interfaces::agent::acp -> api -> application -> domain
                                  |
                                  +-> retrieval traits
                                  +-> storage traits
                                  +-> indexing traits
```

禁止事项:

- MCP/ACP adapter 直接打开数据库、读取索引文件或执行 SQL。
- ACP adapter 实现通用代码修改 agent、planner、terminal 或 file edit 能力。
- MCP server 读取完整 host conversation 或跨 server 状态。
- 协议层用 prompt、tool annotation 或 `_meta` 覆盖 access policy。
- 远程监听默认开启。

## 3. 公共 API 扩展

当前实现已在 `api` 层提供 protocol-neutral agent identity 和 protocol context；后续扩展仍不能把 MCP/ACP SDK 类型放进 `api`、`application` 或 `domain`。

```rust
pub enum InterfaceKind {
    Cli,
    Web,
    Api,
    Mcp,
    Acp,
}

pub enum AgentProtocolKind {
    Mcp,
    Acp,
}

pub struct RuntimeIdentity {
    pub protocol: AgentProtocolKind,
    pub adapter_name: String,
    pub adapter_version: Option<String>,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub host_name: Option<String>,
    pub actor_id: Option<String>,
    pub session_id: Option<String>,
    pub tool_call_id: Option<String>,
}

pub struct AgentRequestContext {
    pub request: RequestContext,
    pub runtime_identity: RuntimeIdentity,
    pub policy_id: String,
}
```

规则:

- MCP request 使用 `InterfaceKind::Mcp`。
- ACP request 使用 `InterfaceKind::Acp`。
- `runtime_identity` 在 agent protocol 请求中必填。
- `actor_id` 可以为空，但必须记录 client/host/session/tool call 可用身份。
- `session_id` 和 `tool_call_id` 只进入 audit/trace/api metadata，不进入 domain facts。

## 4. 访问策略

`AgentAccessPolicy` 是协议请求进入 unified API 前的本地准入规则。

```rust
pub struct AgentAccessPolicy {
    pub allowed_scopes: Vec<String>,
    pub allow_unspecified_scope: bool,
    pub max_limit: usize,
    pub max_context_bytes: usize,
    pub max_runtime_ms: u64,
    pub allow_remote_clients: bool,
}
```

默认值:

| 字段 | 默认 |
| --- | --- |
| `allowed_scopes` | 空，表示只允许显式配置后访问实际 source scope |
| `allow_unspecified_scope` | `false` |
| `max_limit` | `10` |
| `max_context_bytes` | 保守值，由实现根据现有 response budget 设定 |
| `max_runtime_ms` | 不超过 `RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS` |
| `allow_remote_clients` | `false` |

`max_context_bytes` 适用于 agent 结果中保留的序列化上下文负载，包括检索命中、context-pack 条目、图事实、来源范围、代码工件元数据和后端状态元数据。它不限于原始证据文本字节。

准入流程:

```text
protocol request
  -> parse JSON-RPC / method arguments
  -> authenticate local client identity where configured
  -> build RuntimeIdentity
  -> QoS admission
  -> AgentAccessPolicy check
  -> construct unified API request
  -> call RelayKnowledgeService
  -> protocol response mapping
  -> audit completion
```

拒绝原因必须映射为稳定错误:

- `permission_denied`
- `invalid_scope`
- `limit_exceeded`
- `qos_rejected`
- `timeout`
- `cancelled`
- `unsupported_operation`

## 5. MCP Server 契约

MCP server 必须声明 tools、resources 和 prompts capability，并按 access policy 动态决定可见工具。

当前 v1 已实现 MCP Streamable HTTP tool/resource/prompt surface。入口是
`relay-knowledge service run --web --mcp streamable-http`、
`relay-knowledge service run --mcp streamable-http` 或
`RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED=true`，默认 endpoint 为 `/mcp`。
实现支持 `initialize`、`notifications/initialized`、`notifications/cancelled`、
`prompts/list` 和 `prompts/get`。MCP tools 已覆盖通用图检索、诊断、索引状态、
授权 code graph query 和授权 code impact；resources 暴露 service status、health、
index status、Prometheus metrics，以及仅在 `allow_unspecified_scope=true` 时暴露的
graph summary；prompts 暴露 retrieval 和 code-impact planning templates。旧 HTTP+SSE 兼容端点通过 `/mcp/sse` 和
`/mcp/message?sessionId=<id>` 保留；legacy client 保持 `/mcp/sse` 打开接收
`endpoint` 和 JSON-RPC `message` events，`/mcp/message` POST 只负责提交 payload。
新集成优先使用 Streamable HTTP。ACP 已提供本地
会话 adapter，用于 agent client 会话入口；它不是通用网络 server，也不提供文件编辑、终端或代码修改能力。
`initialize` 必须携带匹配的 `protocolVersion`、object 形态的 `capabilities` 和
非空 `clientInfo.name/version`，通过后返回加密随机的 server-issued
`Mcp-Session-Id` header。客户端必须随后发送 `notifications/initialized`，并在
后续 `ping`、tool request 和 notification 中继续携带该 session header 与
`MCP-Protocol-Version`。缺失 session header 的非 `initialize` 请求返回 HTTP
400；未签发、已淘汰或已终止的 session 返回 HTTP 404。Session 淘汰按最近使用
时间执行，并且 usage history 必须有界，避免稳定 session 的长时间流量造成内存增长。

已补齐 MCP v1 surface:

- Resources：graph metadata、graph schema、authorized scopes、index status 和 current diagnostics。
- Prompts：context planning、grounded answer drafting 和 graph debugging helper templates。
- Transport lifecycle：`DELETE /mcp` 终止 session，并同样执行 protocol version、Origin allow-list 和 QoS admission 检查；GET/SSE resumability 返回稳定未实现错误。
- Security：Origin allow-list、远程监听 profile 和默认 localhost 安全提示已写入用户文档。

### 5.1 工具

| 工具 | 默认 | Unified API 映射 |
| --- | --- | --- |
| `relay_retrieve_context` | 启用 | `HybridRetrievalRequest` -> `retrieve_context` |
| `relay_inspect_graph` | 启用 | `GraphInspectionRequest` -> `inspect_graph` |
| `relay_health` | 启用 | `health` |
| `relay_service_status` | 启用 | `service_status` |
| `relay_index_status` | 启用 | `health` response 中的 index status projection |
| `relay_code_query` | 启用 | `CodeRetrievalRequest` -> `query_code_repository` |
| `relay_code_impact` | 启用 | `CodeImpactRequest` -> `impact_code_repository` |

`relay_retrieve_context` 输入 schema：

```json
{
  "type": "object",
  "properties": {
    "query": { "type": "string", "minLength": 1 },
    "source_scope": { "type": "string" },
    "limit": { "type": "integer", "minimum": 1 },
    "freshness": {
      "type": "string",
      "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
    }
  },
  "required": ["query"]
}
```

MCP 工具结果：

- `structuredContent` 包含规范 `AgentRetrievalResult`。
- `content[0].text` 包含简短摘要，并在需要时包含序列化 JSON 回退内容。
- `isError=true` 用于工具选中后的业务错误。
- JSON-RPC protocol error 只用于 unknown method、malformed JSON-RPC 和 invalid tool name。

### 5.2 资源

| 资源 URI | 内容 |
| --- | --- |
| `relay://graph/metadata` | 图版本、实体数量、证据数量和最近提交元数据 |
| `relay://graph/schema` | 图实体、关系和索引 schema 摘要 |
| `relay://scopes` | 仅调用方已授权 source scope |
| `relay://indexes/status` | 索引种类、索引版本、已索引图版本和 stale 状态 |
| `relay://diagnostics/current` | health、service mode、按需脱敏的 runtime directories |

Resource 不得列出未授权 scope，也不得泄露完整本地 path，除非 policy 显式允许 path disclosure。

### 5.3 Prompts 模板

MCP prompt 是可选辅助模板，不能授予权限或改变 policy。

推荐 prompts：

- `relay-context-planning`
- `relay-grounded-answer-drafting`
- `relay-graph-debugging`

Prompt 文本必须要求 host 将返回 context 视为带 citation 的 evidence，而不是 system instruction。

## 6. ACP Adapter 契约

ACP adapter 将 `relay-knowledge` 暴露为面向 agent 的知识检索 endpoint，而不是通用 coding agent。
当前实现提供本地 ACP session adapter，供常驻进程或宿主在进程内桥接 ACP 会话。
该 adapter 复用 unified application service、agent access policy、QoS runtime、
cancellation token 和 bounded audit log；不直接访问 SQLite、Git 或索引实现。

### 6.1 初始化

`initialize` 响应必须在 `_meta` 中声明自定义 capability：

```json
{
  "_meta": {
    "relayKnowledge": {
      "graphRetrieval": true,
      "readOnly": true,
      "supportsCancellation": true,
      "supportsIndexRefreshPermission": true
    }
  }
}
```

### 6.2 Session 设置

`session/new` 会创建 adapter session，包含：

- session id（会话 ID）。
- runtime identity（运行时身份）。
- policy id（策略 ID）。
- authorized scopes snapshot（已授权 scope 快照）。
- request budget handle（请求预算句柄）。
- trace root（trace 根节点）。

Session state 不得保存 graph fact 或大型 retrieved context，只能保留有界 recent progress metadata。

### 6.3 Prompt 回合

`session/prompt` 会把最新用户或 agent 文本映射为 `HybridRetrievalRequest`。

映射规则：

- 显式结构化 `_meta.relayKnowledge.query` 优先于自由文本 prompt。
- 只有 policy 允许时才接受 `_meta.relayKnowledge.source_scope`。
- 缺少 source scope 时会拒绝请求，除非 `allow_unspecified_scope=true`。
- limit 根据 `max_limit` 截断或拒绝；实现应拒绝超限请求，而不是静默扩大。
- freshness 默认是 `allow-stale`，除非调用方显式要求等待新鲜索引或只读图。

ACP 进度更新：

| 阶段 | ACP 更新 |
| --- | --- |
| 已接受 | `session/update`，带 tool call `kind=search`、`status=pending` |
| 运行中 | `tool_call_update`、`status=in_progress` |
| 已检查新鲜度 | `_meta.relayKnowledge.freshness` |
| 图扩展/融合 | 只包含计数的进度内容，不包含原始 secret 文本 |
| 上下文就绪 | `rawOutput` 或 `_meta.relayKnowledge.result` |
| 已完成 | `tool_call_update`、`status=completed` |
| 已失败 | `tool_call_update`、`status=failed` |

### 6.4 取消

`session/cancel` 必须：

- 标记活动请求的 cancellation token。
- 在可行时停止等待过期索引。
- 释放 QoS in-flight 预算。
- 发出带有 `cancelled` 状态的审计事件。
- 除非明确标记为 `cancelled`，否则不得把部分上下文作为 completed 返回。

## 6.5 审计日志

MCP 和本地 ACP adapter 都记录 bounded in-process audit events。事件字段至少包含:

- protocol、operation、request_id、trace_id 和 runtime_identity。
- QoS decision: admitted 或 rejected。
- source_scope、freshness、limit、result_count、truncated。
- completed、failed 或 cancelled 状态以及 stable error_kind。

Audit event 不保存原始 prompt、完整检索内容、secret、完整本地路径或未授权 scope
列表。当前实现默认保留 bounded in-process event surface；设置
`RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true` 后，会通过有界 async queue 将同一事件
镜像到 path-owned `logs/agent-audit.jsonl`。宿主仍可从该 bounded event surface 接入自己的
日志/telemetry pipeline。

## 7. 规范结果形状

即使 wire field 不同，两个协议也必须保留相同语义 shape：

```json
{
  "metadata": {
    "trace_id": "trace-...",
    "request_id": "req-...",
    "graph_version": 42,
    "index_version": 12,
    "indexed_graph_version": 42,
    "stale": false
  },
  "runtime_identity": {
    "protocol": "mcp",
    "client_name": "example-agent",
    "session_id": "sess-..."
  },
  "source_scope": "docs",
  "freshness": "allow-stale",
  "retrieval_mode": "hybrid",
  "context_pack": {
    "graph_version": 42,
    "source_scope": "docs",
    "freshness": "allow-stale",
    "truncated": false,
    "backend_statuses": [],
    "items": [
      {
        "result_id": "ev-1",
        "source_scope": "docs",
        "source_path": "docs/phase-1.md",
        "source_span": {
          "start_byte": 0,
          "end_byte": 42,
          "start_line": 1,
          "end_line": 1
        },
        "entities": [],
        "graph_facts": [],
        "retriever_sources": ["bm25"],
        "ranking": []
      }
    ]
  },
  "results": [],
  "fusion": {
    "algorithm": "reciprocal_rank_fusion",
    "k": 60.0,
    "candidate_count": 0
  },
  "backend_statuses": [],
  "indexes": [],
  "degraded_reason": null,
  "truncated": false,
  "budget_used": {
    "limit": 10,
    "candidate_count": 0,
    "returned_count": 0,
    "context_bytes": 8192,
    "elapsed_ms": 4
  }
}
```

Adapter 专用字段可以增加 MCP resource link 或 ACP tool-call id，但不得移除 freshness、audit 或 source-scope debugging 所需 metadata。

## 8. 网络、QoS 与关闭

网络规则：

- 远程 TCP listener 默认关闭。
- 本地 bind 使用 `net::http` 配置，并必须支持请求 timeout、最大 body 字节数和 graceful shutdown。
- 所有入站网络工作都经过 `net::qos`。
- stdio transport 仍消耗 in-flight 和 queue 预算。
- 超过已配置最大 body 字节数的请求体和协议 frame，在调用 application service 前拒绝。
- MCP Streamable HTTP 在调用工具前校验 `Content-Type`、`Accept`、`MCP-Protocol-Version` 和 `Origin`。Media type 匹配不区分大小写。非空 `Origin` 默认必须是 loopback，或匹配 `RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS`。
- MCP request 和 response ID 必须是字符串或整数。Notification 不得包含 ID；格式错误的 notification 返回 JSON-RPC protocol error，而不是静默 202。
- Session 完成 `initialize` 与 `notifications/initialized` 前，普通 MCP method 会被拒绝；初始化后的请求必须包含 `MCP-Protocol-Version`；已初始化 session 的 `ping` 返回空 JSON-RPC result。

关闭顺序：

```text
shutdown signal
  -> stop accepting new MCP/ACP sessions and calls
  -> notify active ACP sessions when possible
  -> cancel requests past graceful shutdown budget
  -> flush audit logs and telemetry
  -> close storage/runtime handles
```

## 9. 可观测性

必要日志事件：

- `agent.protocol.session.started`
- `agent.protocol.session.completed`
- `agent.protocol.tool.started`
- `agent.protocol.tool.completed`
- `agent.protocol.tool.failed`
- `agent.protocol.permission.denied`
- `agent.protocol.request.cancelled`
- `agent.protocol.qos.rejected`

必要指标：

| 指标 | 类型 | 标签 |
| --- | --- | --- |
| `relay_agent_protocol_requests_total` | counter | `protocol`, `operation`, `status` |
| `relay_agent_protocol_request_duration_ms` | histogram | `protocol`, `operation` |
| `relay_agent_protocol_rejections_total` | counter | `protocol`, `reason` |
| `relay_agent_retrieval_cancelled_total` | counter | `protocol` |
| `relay_agent_context_truncated_total` | counter | `protocol`, `reason` |

当前实现提供 `/mcp/metrics` Prometheus text exporter 和 `relay://metrics/prometheus`
resource，覆盖 graph version、index refresh queue depth、dead-letter count、
QoS in-flight/queued request count 和 per-index stale 状态。协议级 counter/histogram
由 `observability` runtime 记录，并在 `RELAY_OTEL_METRICS=true` 时通过 OTLP
HTTP/protobuf 导出到 Collector。`RELAY_OTEL_TRACES=true` 时，同一 runtime 安装
tracing/OpenTelemetry bridge，把 service mode 的结构化 spans 导出到 OTLP traces
endpoint。OTLP 初始化或导出失败只降级 observability，不阻塞 MCP/ACP 请求。
单个 signal exporter 初始化失败不得阻断另一个 signal；trace exporter 初始化失败时仍保留本地
tracing fallback。service shutdown 必须按配置的 export timeout flush 已安装的
OTLP trace/metrics providers。

Trace 要求：

- protocol root span 包含 protocol、operation、request id、trace id 和低基数 client name。
- policy span 记录 allow/deny reason，但不记录原始 query text。
- retrieval span 记录 graph version、freshness、result count、stale 和 truncated。
- storage span 保持在 storage 边界后面，不暴露原始 SQL。

## 10. 测试要求

实现必须增加聚焦测试，覆盖：

- MCP 和 ACP adapter 只能调用 unified API；mock service 可验证没有 storage 依赖泄漏到 adapter。
- 同一查询通过 MCP 和 ACP 返回相同 graph version、source scope、stale 标志、retrieval mode 和 result id。
- 未授权 scope 返回 `permission_denied` 或 `invalid_scope`，且不泄露结果。
- policy 要求显式 scope 时，缺失 scope 会被拒绝。
- `limit > max_limit` 在调用 application service 前被拒绝。
- QoS 拒绝会阻止执行检索。
- ACP `session/cancel` 会释放 in-flight 预算，并生成 cancelled 审计状态。
- MCP `tools/list` 只包含 ASCII 字母、数字和 `_` 组成的 tool name，且不暴露 index refresh 或 repository indexing tool。
- ACP initialize 声明只读检索能力。
- 过期索引在两个协议映射中都保留 freshness metadata。
- 刷新索引要求启用 policy 并经过权限路径。
- graceful shutdown 拒绝新调用，并在已配置预算内完成或取消活动调用。
- MCP resources/prompts shape、authorized scope filtering、diagnostics path redaction 和 `DELETE /mcp` session termination 的 Origin/QoS policy。
- OTLP exporter 初始化、失败降级、shutdown flush 和 agent protocol metrics snapshot。

CI 预期：

- Rust 单元测试覆盖 adapter 参数校验、policy decision 和 result mapping。
- 集成测试使用确定性内存 service 覆盖协议 request/response 形状。
- 既有 `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features` 仍然是必要门禁。
- 除非 Web 诊断开始展示协议状态，否则浏览器门禁不变。

## 11. 实现顺序

1. 已完成：添加 protocol-neutral `RuntimeIdentity`、`AgentAccessPolicy` 和 `AgentRetrievalResult` API 类型。
2. 已完成：用 `Mcp` 和 `Acp` 扩展 `InterfaceKind`。
3. 已完成：添加 `interfaces::agent` 模块，包含 protocol-neutral validation 和 policy mapping。
4. 已完成：在共享 mapping 上实现 MCP Streamable HTTP tool adapter。
5. 已完成：添加 service status 字段，在不含 secret 的情况下展示已启用协议、bind mode 和 policy summary。
6. 已完成：在同一 mapping 上实现本地 ACP session adapter。
7. 已完成：添加持久可观测事件和 metrics exporter。
8. 已完成：在产品范围需要时添加 MCP resources/prompts。
