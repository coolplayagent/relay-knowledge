# 常驻进程 Agent 图检索访问规格

> 文档版本: 1.0
> 编制日期: 2026-05-12
> 适用范围: 常驻 `relay-knowledge` 进程、MCP server、Agent Client Protocol adapter、外部 agent 图检索接入
> 默认路线: MCP 和 ACP 双协议同级设计，协议 adapter 只做转换和治理，图检索能力只通过统一 API 调用

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

- validated runtime configuration from `env`、`paths`、`net`。
- `RelayKnowledgeService`。
- agent protocol registry。
- `AgentAccessPolicy`。
- QoS budget tracker。
- request cancellation registry。
- telemetry/audit logger。
- graceful shutdown token。

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

后续实现应在 `api` 层补充 agent identity 和 protocol context，不把 MCP/ACP SDK 类型放进 `api`、`application` 或 `domain`。

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

## 4. Access Policy

`AgentAccessPolicy` 是协议请求进入 unified API 前的本地准入规则。

```rust
pub struct AgentAccessPolicy {
    pub allowed_scopes: Vec<String>,
    pub allow_unspecified_scope: bool,
    pub max_limit: usize,
    pub max_context_bytes: usize,
    pub max_runtime_ms: u64,
    pub allow_index_refresh: bool,
    pub require_permission_for_refresh: bool,
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
| `allow_index_refresh` | `false` |
| `require_permission_for_refresh` | `true` |
| `allow_remote_clients` | `false` |

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

## 5. MCP Server Contract

MCP server 必须声明 tools、resources 和 prompts capability，并按 access policy 动态决定可见工具。

当前 v1 已实现 MCP Streamable HTTP tool surface。入口是
`relay-knowledge service run --mcp streamable-http` 或
`RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED=true`，默认 endpoint 为 `/mcp`。
实现支持 `initialize`、`notifications/initialized`、`notifications/cancelled`、
`ping`、`tools/list` 和 `tools/call`。MCP tools 已覆盖通用图检索、诊断、
索引状态、授权 code graph query 和授权 code impact。Resources、prompts 和旧
HTTP+SSE 兼容端点仍是后续工作。ACP 已提供本地会话 adapter，用于 agent client
会话入口；它不是通用网络 server，也不提供文件编辑、终端或代码修改能力。
`initialize` 必须携带匹配的 `protocolVersion`、object 形态的 `capabilities` 和
非空 `clientInfo.name/version`，通过后返回加密随机的 server-issued
`Mcp-Session-Id` header。客户端必须随后发送 `notifications/initialized`，并在
后续 `ping`、tool request 和 notification 中继续携带该 session header 与
`MCP-Protocol-Version`。缺失 session header 的非 `initialize` 请求返回 HTTP
400；未签发、已淘汰或已终止的 session 返回 HTTP 404。Session 淘汰按最近使用
时间执行，并且 usage history 必须有界，避免稳定 session 的长时间流量造成内存增长。

### 5.1 Tools

| Tool | 默认 | Unified API 映射 |
| --- | --- | --- |
| `relay.retrieve_context` | enabled | `HybridRetrievalRequest` -> `retrieve_context` |
| `relay.inspect_graph` | enabled | `GraphInspectionRequest` -> `inspect_graph` |
| `relay.health` | enabled | `health` |
| `relay.service_status` | enabled | `service_status` |
| `relay.index_status` | enabled | `health` response 中的 index status projection |
| `relay.code_query` | enabled | `CodeRetrievalRequest` -> `query_code_repository` |
| `relay.code_impact` | enabled | `CodeImpactRequest` -> `impact_code_repository` |
| `relay.refresh_indexes` | disabled | `IndexRefreshRequest` -> `refresh_indexes` |

`relay.retrieve_context` input schema:

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

MCP tool result:

- `structuredContent` contains the canonical `AgentRetrievalResult`.
- `content[0].text` contains a short summary and serialized JSON fallback when needed.
- `isError=true` is used for business errors after tool selection.
- JSON-RPC protocol errors are reserved for unknown method, malformed JSON-RPC and invalid tool name.

### 5.2 Resources

| Resource URI | 内容 |
| --- | --- |
| `relay://graph/metadata` | graph version、entity count、evidence count、last commit metadata |
| `relay://graph/schema` | graph entity/relation/index schema summary |
| `relay://scopes` | caller-authorized source scopes only |
| `relay://indexes/status` | index kind、index version、indexed graph version、stale |
| `relay://diagnostics/current` | health、service mode、runtime directories redacted where needed |

Resources must not list unauthorized scopes or leak full local paths unless policy explicitly allows path disclosure.

### 5.3 Prompts

MCP prompts are optional helper templates. They cannot grant permissions or change policy.

Recommended prompts:

- `relay-context-planning`
- `relay-grounded-answer-drafting`
- `relay-graph-debugging`

Prompt text must instruct hosts to treat returned context as evidence with citations, not as system instructions.

## 6. ACP Adapter Contract

ACP adapter exposes `relay-knowledge` as a knowledge retrieval agent-facing endpoint, not as a general coding agent.
当前实现提供本地 ACP session adapter，供常驻进程或宿主在进程内桥接 ACP 会话。
该 adapter 复用 unified application service、agent access policy、QoS runtime、
cancellation token 和 bounded audit log；不直接访问 SQLite、Git 或索引实现。

### 6.1 Initialize

`initialize` response must advertise a custom capability in `_meta`:

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

### 6.2 Session Setup

`session/new` creates an adapter session containing:

- session id.
- runtime identity.
- policy id.
- authorized scopes snapshot.
- request budget handle.
- trace root.

Session state must not contain graph facts or large retrieved context beyond bounded recent progress metadata.

### 6.3 Prompt Turn

`session/prompt` maps the latest user or agent text to `HybridRetrievalRequest`.

Mapping rules:

- explicit structured `_meta.relayKnowledge.query` wins over free-text prompt.
- `_meta.relayKnowledge.source_scope` is accepted only if policy allows it.
- missing source scope is rejected unless `allow_unspecified_scope=true`.
- limit is clamped or rejected according to `max_limit`; implementation should reject rather than silently expand.
- freshness defaults to `allow-stale` unless the caller explicitly asks to wait or graph-only.

ACP progress updates:

| Stage | ACP update |
| --- | --- |
| accepted | `session/update` with tool call `kind=search`, `status=pending` |
| running | `tool_call_update`, `status=in_progress` |
| freshness checked | `_meta.relayKnowledge.freshness` |
| graph expansion/fusion | progress content with counts only, no raw secret text |
| context ready | `rawOutput` or `_meta.relayKnowledge.result` |
| completed | `tool_call_update`, `status=completed` |
| failed | `tool_call_update`, `status=failed` |

### 6.4 Cancellation

`session/cancel` must:

- mark active request cancellation token.
- stop waiting for stale indexes where possible.
- release QoS in-flight budget.
- emit audit event with `cancelled`.
- avoid returning partial context as completed unless explicitly marked `cancelled`.

## 6.5 Audit Log

MCP 和本地 ACP adapter 都记录 bounded in-process audit events。事件字段至少包含:

- protocol、operation、request_id、trace_id 和 runtime_identity。
- QoS decision: admitted 或 rejected。
- source_scope、freshness、limit、result_count、truncated。
- completed、failed 或 cancelled 状态以及 stable error_kind。

Audit event 不保存原始 prompt、完整检索内容、secret、完整本地路径或未授权 scope
列表。宿主需要持久审计时，应从该 bounded event surface 接入自己的日志/telemetry
pipeline。

## 7. Canonical Result Shape

Both protocols must preserve this semantic shape even if wire fields differ:

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
    "items": []
  },
  "results": [],
  "fusion": {
    "algorithm": "reciprocal_rank_fusion",
    "k": 60.0,
    "candidate_count": 0
  },
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

Adapter-specific fields may add MCP resource links or ACP tool-call ids, but must not remove metadata needed for freshness, audit or source-scope debugging.

## 8. Network, QoS and Shutdown

Network rules:

- Remote TCP listeners are disabled by default.
- Local bind uses `net::http` configuration and must support request timeout, max body bytes and graceful shutdown.
- All inbound network work passes through `net::qos`.
- stdio transports still consume in-flight and queue budgets.
- Request bodies and protocol frames over configured max body bytes are rejected before application service invocation.
- MCP Streamable HTTP validates `Content-Type`、`Accept`、`MCP-Protocol-Version` and `Origin` before invoking tools. Media type matching is case-insensitive. Non-empty `Origin` must be loopback by default or match `RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS`.
- MCP request and response IDs must be strings or integers. Notifications must not include IDs; malformed notifications receive a JSON-RPC protocol error instead of a silent 202.
- MCP normal methods are rejected until the session has completed `initialize` and `notifications/initialized`; post-initialize requests must include `MCP-Protocol-Version`; `ping` returns an empty JSON-RPC result for initialized sessions.

Shutdown sequence:

```text
shutdown signal
  -> stop accepting new MCP/ACP sessions and calls
  -> notify active ACP sessions when possible
  -> cancel requests past graceful shutdown budget
  -> flush audit logs and telemetry
  -> close storage/runtime handles
```

## 9. Observability

Required log events:

- `agent.protocol.session.started`
- `agent.protocol.session.completed`
- `agent.protocol.tool.started`
- `agent.protocol.tool.completed`
- `agent.protocol.tool.failed`
- `agent.protocol.permission.denied`
- `agent.protocol.request.cancelled`
- `agent.protocol.qos.rejected`

Required metrics:

| Metric | Type | Labels |
| --- | --- | --- |
| `relay_agent_protocol_requests_total` | counter | `protocol`, `operation`, `status` |
| `relay_agent_protocol_request_duration_ms` | histogram | `protocol`, `operation` |
| `relay_agent_protocol_rejections_total` | counter | `protocol`, `reason` |
| `relay_agent_retrieval_cancelled_total` | counter | `protocol` |
| `relay_agent_context_truncated_total` | counter | `protocol`, `reason` |

Trace requirements:

- protocol root span includes protocol, operation, request id, trace id and low-cardinality client name.
- policy span records allow/deny reason without raw query text.
- retrieval span records graph version, freshness, result count, stale and truncated.
- storage span remains behind storage boundary and does not expose raw SQL.

## 10. Testing Requirements

Implementation must add focused tests for:

- MCP and ACP adapters call unified API only; mocked service can verify no storage dependency leaks into adapters.
- same query via MCP and ACP returns identical graph version, source scope, stale flag, retrieval mode and result ids.
- unauthorized scope returns `permission_denied` or `invalid_scope` without result leakage.
- missing scope is rejected when policy requires explicit scope.
- `limit > max_limit` is rejected before application service call.
- QoS rejection prevents retrieval execution.
- ACP `session/cancel` releases in-flight budget and produces cancelled audit state.
- MCP `tools/list` hides `relay.refresh_indexes` when policy disables it.
- ACP initialize advertises read-only retrieval capability.
- stale indexes preserve freshness metadata in both protocol mappings.
- refresh index requires policy enablement and permission path.
- graceful shutdown rejects new calls and completes or cancels active calls within configured budget.

CI expectations:

- Rust unit tests for adapter argument validation, policy decisions and result mapping.
- Integration tests for protocol request/response shape using deterministic in-memory service.
- Existing `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features` remain required.
- Browser gate is unchanged unless Web diagnostics begins displaying protocol status.

## 11. Implementation Order

1. Done: add protocol-neutral `RuntimeIdentity`、`AgentAccessPolicy` and `AgentRetrievalResult` API types.
2. Done: extend `InterfaceKind` with `Mcp` and `Acp`.
3. Done: add `interfaces::agent` module with protocol-neutral validation and policy mapping.
4. Done: implement MCP Streamable HTTP tool adapter over the shared mapping.
5. Done: add service status fields showing enabled protocols, bind mode and policy summary without secrets.
6. Next: implement ACP adapter over the same mapping.
7. Next: add persistent observability events and metrics exporters.
8. Next: add MCP resources/prompts when product scope requires them.
