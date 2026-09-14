# 第 16 章 安全配置指南

[中文](16-security-configuration-guide.md) | 中文深入专题；英文核心流程见 [English Chapter 9](../../en/01-user-guide/09-resident-service.md)

本章是 `relay-knowledge` 安全配置的完整参考，涵盖 QoS 准入控制、远端访问安全、MCP scope/origin 限制、审计日志和网络安全实践。所有配置项均基于代码实际实现，零配置时优先保证本机安全。

> **关键安全边界：** 当前 Web workspace、HTTP API（包括
> `/api/v1/control/**`）和 MCP Streamable HTTP **不内建入站调用方身份认证**。
> 服务不校验登录、API key、Bearer/JWT/OIDC token 或客户端证书，也不按调用方
> 身份执行 ACL。`allow_remote_clients`、Origin、QoS、MCP scope 和 MCP session
> id 都不能证明调用方身份。没有外部认证网关时，服务必须只绑定 loopback。
> 远程访问必须先经过外部身份层，由它执行 mTLS（校验客户端证书并映射 ACL）
> 或 OIDC/token 校验，并以 deny-by-default ACL 授权每条路径和操作。

## 16.1 安全模型总览

`relay-knowledge` 的安全模型建立在分层防御之上：

1. **QoS 准入控制** — 连接、请求、排队三层预算，防止资源耗尽。
2. **绑定地址守卫** — 默认仅监听 loopback（`127.0.0.1:8791`），远端绑定必须显式授权。
3. **MCP scope/origin 限制** — 限定 agent 可检索的 source scope，过滤请求来源；两者都不是调用方认证。
4. **审计日志** — 内存环形缓冲 + JSONL 持久化，记录所有 agent 操作。
5. **传输层隔离** — 请求体大小限制、超时、TLS 验证、代理配置。
6. **Session 管理** — 有界 session 注册表，支持会话终止与驱逐。

这些层负责资源边界、暴露开关、请求过滤和可观测性，不构成入站身份系统。
远程拓扑还必须在 `relay-knowledge` 前部署外部认证与 ACL 层。仅配置 TLS 只能
加密传输并验证服务端身份；除非同时校验客户端证书并按身份执行 ACL，否则 TLS
本身不足以认证调用方。

这些机制由三个基础模块强制执行：

| 模块 | 安全职责 |
| --- | --- |
| `env` | 唯一读取环境变量的模块，所有配置入口集中校验 |
| `net::qos` | 准入控制，所有网络工作在消耗资源前经过 QoS 策略 |
| `net::http` | HTTP 监听/代理/TLS 配置，loopback 检测 |

## 16.2 QoS 策略配置

### 16.2.1 默认预算值

QoS 策略定义了三个独立的有界资源预算：

| 预算 | 环境变量 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `max_connections` | `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` | `1024` | 最大并发 TCP 连接数 |
| `max_in_flight_requests` | `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` | `256` | 最大同时在途 HTTP 请求数 |
| `max_queue_depth` | `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` | `512` | 最大排队等待请求数 |

所有预算必须为正整数，零值会被 `QosPolicy::new()` 拒绝并返回 `QosPolicyError`，环境变量解析时零值也会被 `EnvErrorKind::ZeroValue` 拒绝。

配置示例（调低预算以限制资源占用）：

```bash
RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS=512 \
RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS=64 \
RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH=128 \
relay-knowledge service run --mcp streamable-http
```

### 16.2.2 准入决策机制

`QosRuntime` 在三个层面执行准入检查，每个检查都是原子操作（`Arc<Mutex<QosSnapshot>>`）：

1. **`reserve_queue`** — 先占排队槽位，仅检查 `queued_requests < max_queue_depth`。
2. **`admit_queued_request`** — 从排队转入在途，同时检查排队预算和在途请求预算，并推进 `qos_queued_total`/`relay_knowledge_qos_queued_total`。
3. **`admit_request`** — 直接请求准入（非排队路径），检查 `in_flight_requests < max_in_flight_requests`。
4. **`admit_connection`** — 新 TCP 连接准入，检查 `connections < max_connections`。

MCP 请求通过 `admit_queued_request` 进入，Web/HTTP 路由通过 `admit_connection` + `admit_request` 进入。已校验 session 的 MCP `notifications/cancelled` 使用协议层优先路径，避免普通请求预算满载时无法取消活跃工具调用。MCP 与本地 ACP 工作触发 runtime budget 超时时，会计入 QoS timeout 诊断。

每种准入检查返回 `QosPermit`，该 permit 在 drop 时自动释放对应的预算计数（使用 `saturating_sub` 防止下溢），确保即使 panic 也不会泄漏预算。

### 16.2.3 过载保护行为

当预算耗尽时，系统返回以下拒绝原因：

| 拒绝原因 | HTTP 状态码 | 含义 |
| --- | --- | --- |
| `ConnectionBudgetExceeded` | 连接被静默丢弃 | TCP 连接数已达上限 |
| `RequestBudgetExceeded` | MCP: JSON-RPC 错误 `-32000`，Web: 无响应 | 在途请求数已达上限 |
| `QueueBudgetExceeded` | MCP: JSON-RPC 错误 `-32000`，Web: `503 Service Unavailable` | 排队请求数已达上限 |

`QosTcpListener` 在连接预算耗尽时静默丢弃新 TCP 连接（不占用内核 backlog），并在底层 accept 错误时以 1 秒间隔重试，避免空转忙等。

MCP 服务在 QoS 拒绝时通过 `record_mcp_qos_rejection` 记录审计事件（`qos_decision: Rejected`，`status: Failed`），并调用 `metrics.record_rejection` 记录拒绝指标。

### 16.2.4 调优建议

| 场景 | 建议调整 |
| --- | --- |
| 低内存边缘设备 | 设置 `max_connections=64`，`max_in_flight_requests=16` |
| 团队内部服务 | 保持默认值（1024/256/512） |
| 高并发反向代理后端 | 适度调高，注意数据库和文件描述符限制 |
| 压测/基准测试 | 临时调高预算，配合 `http_request_timeout_ms` 确保资源及时释放 |

## 16.3 远端访问安全

### 16.3.1 Loopback vs 非 Loopback 绑定

默认绑定地址为 `127.0.0.1:8791`（`DEFAULT_HTTP_BIND`）。系统通过 `remote_clients_allowed()` 判断是否允许非本地客户端：

```rust
// src/relay_knowledge/net/http/mod.rs
pub fn remote_clients_allowed(config: &HttpConfig, allow_remote_clients: bool) -> bool {
    allow_remote_clients || is_local_bind(&config.bind_address.to_string())
}
```

`is_local_bind` 检测逻辑：

- 主机名为 `localhost`（不区分大小写）。
- IP 地址满足 `IpAddr::is_loopback()`（即 `127.0.0.0/8` 或 `::1`）。

绑定到 loopback 地址时无需开启远端暴露开关；绑定到非 loopback 地址时必须显式设置
`allow_remote_clients=true`。该布尔值只决定监听器能否暴露到非 loopback，既不认证
调用方，也不授予 API 或 control 操作权限。Loopback 限制网络暴露范围，也不等同于
本机多用户环境中的身份认证。

### 16.3.2 远端访问的外部认证前提

安全的远端访问需要同时满足以下条件：

| 条件 | 作用 | 是否认证调用方 |
| --- | --- | --- |
| 外部 OIDC/token 网关，或校验客户端证书并映射 ACL 的 mTLS 网关 | 认证身份，并按 deny-by-default ACL 授权 Web、API、control 和 MCP 路径 | 是 |
| Relay 后端只绑定 loopback；跨主机 sidecar 例外时只绑定网关专用私网地址并用防火墙仅允许网关 | 避免绕过认证层直连后端 | 否 |
| `allow_remote_clients=true` | 仅允许 Relay 监听非 loopback；只用于受隔离的跨主机网关后端 | 否 |
| MCP scope allowlist | 限定 MCP 请求可读取的资源范围 | 否 |
| Origin 限制 | 过滤声明的请求来源 | 否 |
| QoS、请求超时和请求体限制 | 限制资源消耗 | 否 |
| 审计日志 | 记录操作和诊断上下文 | 否；当前不提供已认证主体 |

无外部认证网关时只允许本机访问：

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs,src \
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true \
relay-knowledge service run --mcp streamable-http
```

需要远端访问时仍让 Relay 绑定 `127.0.0.1:8791`，由同机外部身份网关监听远端
地址并转发；完整的 deny-by-default 示例见 [16.6.1](#1661-反向代理部署)。

### 16.3.3 `ensure_web_remote_bind_allowed` 机制

服务启动时，`service_cli::ensure_web_remote_bind_allowed` 和
`http_contract::ensure_remote_bind_allowed` 分别检查 Web 路由和 MCP 路由能否暴露到
非 loopback。这是监听暴露守卫，不是调用方认证或操作授权：

```rust
// src/relay_knowledge/interfaces/cli/service/mod.rs
pub(super) fn ensure_web_remote_bind_allowed(
    config: &HttpConfig,
    allow_remote_clients: bool,
) -> Result<(), CliError> {
    if remote_clients_allowed(config, allow_remote_clients) {
        Ok(())
    } else {
        Err(CliError::ServiceRunFailed(
            "Web remote bind requires allow_remote_clients=true".to_owned(),
        ))
    }
}
```

MCP 端等价的检查返回 `McpServeError::RemoteBindDisabled`，阻止远端监听器启动。这意味着：

- **非 loopback 绑定 + `allow_remote_clients=false`** → 服务启动失败。
- **loopback 绑定** → 无需额外授权。

通过此检查只代表监听器可以启动。它不会验证谁发起请求；任何能连接该端口的调用方
都能到达已注册路由。因此不能把 `allow_remote_clients=true` 当作认证开关。

`HttpBindAddress::parse()` 还拒绝端口为 `0` 的临时端口（返回 `HttpConfigError::EphemeralPort`），确保绑定地址始终显式指定端口。

## 16.4 MCP 安全控制

### 16.4.1 Scope 资源允许列表

MCP scope 策略基于 `AgentAccessPolicy`，由以下环境变量控制：

Scope 只是一层**资源 allowlist**：它约束请求可以读取哪些 source scope，但不识别
请求者，也不把某个 scope 授予某个用户或服务身份。所有能到达 MCP 端点的调用方共享
同一进程配置，因此远端 MCP 仍必须经过外部身份认证和按主体执行的 ACL。

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` | CSV 字符串 | 无 | 允许的 source scope 白名单 |
| `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE` | bool | `false` | 是否允许不指定 scope |
| `RELAY_KNOWLEDGE_MCP_MAX_LIMIT` | 正整数 | `10` | 单次检索最大返回条数 |
| `RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES` | 正整数 | `65536` | MCP 上下文输出字节上限；repository graph 按完整 `structuredContent` 计量 |

Scope 过滤流程（`scope_authorization.rs`）：

1. **scope 解析**：`normalize_scope_for_policy` 将用户输入的 scope 解析为 `SourceScope` 格式并去除空白。
2. **静态白名单匹配**：检查 `scope` 是否在 `allowed_scopes` 列表中。
3. **运行时仓库别名缓存**：检查 `RuntimeScopeAuthorizer` 中已缓存的运行时授权仓库。
4. **已注册仓库匹配**：查询 `code_repository_is_registered`，如果仓库别名已注册，则自动纳入运行时白名单并缓存（后续请求无需再查）。
5. **拒绝**：返回 `PermissionDenied` 错误，提示 "source_scope '{scope}' is not authorized"。

未指定 scope 的处理：

- `allow_unspecified_scope=true` → 允许 scope 为空，检索全局范围。
- `allow_unspecified_scope=false`（默认）→ `source_scope` 为必填项，缺失返回 `InvalidScope` 错误。

`limit`（返回条数）授权：

- 请求未指定 limit → 使用 `max_limit` 默认值（10）。
- 请求指定 limit ≤ `max_limit` → 使用请求值。
- 请求指定 limit > `max_limit` → 返回 `LimitExceeded` 错误。

`max_context_bytes` 在 `AgentRetrievalResult::from_retrieval` 中用于截断过大的检索结果。对 `relay_repository_graph`，该值限制完整紧凑 JSON `structuredContent` 的 UTF-8 字节数，计入 metadata、scope、回显 request、nodes、edges 与 truncation 状态，但不计外层文本摘要、`isError` 或 JSON-RPC envelope。Repository-graph 的序列化与近线性裁剪在四 permit 的 blocking-worker 边界执行；外层 timeout/cancellation 只停止等待，不能强制取消已启动的 blocking worker，该 worker 可以在后台完成并在返回前继续持有 permit。

`max_runtime_ms` 由 HTTP 请求超时自动派生（`request_timeout - 1ms`），作为 MCP tool call 的响应等待上限；它不能强制终止已经进入 blocking worker 的工作。

### 16.4.2 Origin 限制

MCP 服务通过 `validate_origin()` 校验 HTTP `Origin` 请求头：

Origin 是请求来源过滤信号，不是身份凭据。非浏览器客户端可以自行设置或省略该头，
浏览器中的 Origin 也只能描述页面来源，不能证明最终用户或服务身份。即使严格配置
Origin allowlist，远端端点仍需外部认证网关和 ACL。

| 配置状态 | Loopback Origin | 非 Loopback Origin | 无 Origin 头 |
| --- | --- | --- | --- |
| `mcp_allowed_origins` 为空（默认） | ✅ 允许 | ❌ `403 Forbidden` | ✅ 允许 |
| `mcp_allowed_origins` 已配置 | 必须在列表中 | 必须在列表中 | ❌ `403 Forbidden` |

环境变量 `RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS` 接受逗号分隔的 origin 列表：

```bash
# 仅允许来自本地 Web UI 和特定域的请求
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS=http://localhost:3000,https://my-agent.example.com
```

配置 origins 后，所有无 `Origin` 头的请求（如 curl 直接调用）将被拒绝。仅用于本地调试时可保持为空。

### 16.4.3 Session 管理

MCP Streamable HTTP 的会话由 `SessionRegistry` 管理：

`Mcp-Session-Id` 只关联协议状态和取消请求，不是登录 session、Bearer token 或调用方
身份。随机 session id 不能替代外部认证，也不应被外部网关当作 ACL 主体。

| 参数 | 值 | 说明 |
| --- | --- | --- |
| 最大跟踪会话数 | `1024` (`MAX_TRACKED_SESSIONS`) | 超过后驱逐最不活跃的会话 |
| Session ID 格式 | `rk-` + 64 位 hex（共 67 字符） | 由 `getrandom` 生成 32 字节密码学随机数 |
| 会话状态 | `initialized: bool` | 初始化前拒绝非 `initialize` 请求 |
| 使用历史压缩 | 当 `usage_order` 超过 `SESSION_COUNT * 2` 时压缩 | 防止内存无限增长 |

会话生命周期：

1. `initialize` 请求创建 session（返回 `mcp-session-id` 响应头）。
2. `notifications/initialized` 标记会话已初始化。
3. 后续请求附带 `mcp-session-id` 请求头。
4. `DELETE` 请求终止会话。
5. 驱逐：当会话数超过 1024 时，LRU 驱逐写入时淘汰最老的未活跃会话。

### 16.4.4 其他 MCP 安全配置

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED` | bool | `false` | 启用 MCP Streamable HTTP |
| `RELAY_KNOWLEDGE_MCP_ENDPOINT` | 路径 | `/mcp` | MCP 端点路径（以 `/` 开头） |
| `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS` | bool | `false` | 允许非 loopback 监听；仅为暴露开关，不提供认证 |

MCP 服务通过 `McpServer::checked_router()` 在构建路由器时执行所有安全检查：

1. 检查 `mcp_streamable_http_enabled`，未启用返回 `McpServeError::Disabled`。
2. 调用 `ensure_remote_bind_allowed` 检查远端绑定授权。
3. 验证通过后构建带 `RequestBodyLimitLayer` 的路由器。

HTTP 请求级校验：

- `Content-Type` 必须为 `application/json`，否则 `415 Unsupported Media Type`。
- `Accept` 必须包含 `application/json` 和 `text/event-stream`（质量值 > 0），否则 `406 Not Acceptable`。
- `mcp-protocol-version` 头（非 `initialize` 请求后必需）必须为 `2025-11-25`。
- 不支持 JSON-RPC batch 请求。

## 16.5 审计日志

### 16.5.1 AgentAuditLog 配置

审计日志由两层组成：

1. **内存环形缓冲** (`AgentAuditLog`)：最多保留 `MAX_AUDIT_EVENTS=1024` 条事件，使用 `VecDeque` + `Mutex`，事件满时从头部驱逐。
2. **持久化 JSONL Sink** (`AgentAuditSink`)：通过 `mpsc` 通道（有界队列）异步写入 JSONL 文件，写入失败静默丢弃。

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED` | bool | `false` | 启用审计日志持久化 |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH` | 正整数 (1..65536) | `1024` | 异步写入通道容量 |

持久化启用时，审计 sink 在 `McpServer::new()` 中创建，日志写入 `<log_dir>/agent-audit.jsonl`。队列深度通过 `clamp(1, 65536)` 限制，防止无界内存增长。

### 16.5.2 审计事件结构

每条审计事件（`AgentAuditEvent`）包含以下字段（JSON 序列化）：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `sequence` | `u64` | 进程内单调递增序号 |
| `protocol` | `"mcp"` | 协议类型（`AgentProtocolKind::Mcp`） |
| `operation` | `String` | 操作名（如 `retrieve_context`、`tools/call` 等） |
| `request_id` | `String` | 请求去重标识 |
| `trace_id` | `String` | 链路追踪 ID（格式 `trace-mcp-{request_id}`） |
| `runtime_identity` | `RuntimeIdentity` | 运行时身份标识 |
| `qos_decision` | `"admitted"` / `"rejected"` | QoS 准入决策 |
| `status` | `"completed"` / `"failed"` / `"cancelled"` | 操作最终状态 |
| `source_scope` | `Option<String>` | 检索范围（可选） |
| `freshness` | `Option<String>` | 新鲜度策略 |
| `limit` | `Option<usize>` | 查询限制条数 |
| `result_count` | `Option<usize>` | 实际返回条数 |
| `truncated` | `bool` | 是否因 `max_context_bytes` 截断 |
| `elapsed_ms` | `u64` | 耗时（毫秒） |
| `error_kind` | `Option<String>` | 错误类别（可选） |

这里的 `runtime_identity` 是运行时请求关联信息，不是经过登录、OIDC 或客户端证书
认证的调用方主体。需要按用户或服务追责时，外部认证网关还必须记录其认证主体、ACL
决策和对应的 Relay request/trace 标识。

### 16.5.3 审计场景

审计日志覆盖以下场景：

1. **QoS 拒绝**：QoS 预算耗尽时记录 `qos_decision=rejected`，`status=failed`，`error_kind=qos_rejected`。
2. **Tool Call 完成**：`tools/call` 执行后记录完整审计（scope、freshness、limit、result_count、truncated、elapsed_ms）。
3. **非 Tool 操作**：`resources/read`、`prompts/get` 和 `ping`/`tools/list` 等通过 `metrics.record_request` 记录统计，其中 resource/prompt 方法额外通过 `record_mcp_method_audit` 记录完整审计事件。
4. **取消操作**：收到 `notifications/cancelled` 后取消对应请求，状态标记为 `cancelled`。

### 16.5.4 持久化格式

JSONL 追加写入，每条事件一行，使用 `serde_json::to_vec` 序列化后追加换行符，每次写入后 `flush`。文件通过 `tokio::fs::OpenOptions::create(true).append(true)` 打开，自动创建父目录。

示例审计日志行：

```json
{"sequence":1,"protocol":"mcp","operation":"retrieve_context","request_id":"session:rk-abc123|string:1","trace_id":"trace-mcp-session:rk-abc123|string:1","runtime_identity":{"protocol":"mcp","request_id":"session:rk-abc123|string:1"},"qos_decision":"admitted","status":"completed","source_scope":"docs","freshness":"allow-stale","limit":10,"result_count":5,"truncated":false,"elapsed_ms":42}
```

### 16.5.5 后台索引发布与恢复

Code facts 写完不代表仓库已经 fresh。Full 与 incremental task 必须继续受 fence 保护，直到 software projection 同样成功。单 SQLite 在一个 transaction 中同时发布 scope freshness、software status、checkpoint completion 与 publication receipt。Partitioned 模式则先让新 shard route 保持 `staged`，并用 durable task 的 `staged_task_id` 记录 owner；active-only read 在激活前继续读取旧 active scope。随后一个 control transaction 同时激活 route、镜像 repository status 并记录 receipt。

Task 的 `succeeded` 是后续独立的 fenced transaction，必须验证该 receipt、匹配的 fresh scope，以及目标存在 checkpoint 时的 completed checkpoint；无 checkpoint 的 mode 不会虚构 checkpoint。服务若在 control activation 前 crash，恢复流程从 staged shard 继续；若在 activation 后、task completion 前 crash，reclaim 后的 attempt 复用 task-scoped receipt 收敛，不会重新发布，过期 attempt 仍不能报告成功。运维人员应查看 task、checkpoint 与 repository status，并让 reconciler 回收 lease；不要手工删除 lock file、catalog row 或 shard 数据。

## 16.6 网络安全建议

### 16.6.1 反向代理部署

生产环境必须让 `relay-knowledge` 后端保持 loopback，并在前置网关对所有 Web、API、
`/api/v1/control/**` 和 MCP 请求执行身份认证及 deny-by-default ACL。普通反向代理或
TLS 终止器本身不满足这个要求。下面假设 `127.0.0.1:4180` 上已有身份网关，其
`/verify` 端点只有在调用方通过 OIDC/token 校验且 ACL 允许当前路径和方法时才返回
2xx；未认证、无权限、超时或网关故障都必须返回非 2xx 并拒绝请求。

**nginx 配置示例**：

```nginx
upstream relay_knowledge {
    server 127.0.0.1:8791;
    keepalive 32;
}

upstream relay_identity_gateway {
    server 127.0.0.1:4180;
    keepalive 16;
}

server {
    listen 443 ssl;
    server_name knowledge.example.com;

    ssl_certificate     /etc/ssl/certs/knowledge.pem;
    ssl_certificate_key /etc/ssl/private/knowledge.key;

    # TLS 只加密传输；调用方身份和 ACL 由 auth_request 后端校验。
    client_max_body_size 1m;

    location = /_relay_auth {
        internal;
        auth_request off;
        proxy_pass http://relay_identity_gateway/verify;
        proxy_pass_request_body off;
        proxy_set_header Content-Length "";
        proxy_set_header X-Original-URI $request_uri;
        proxy_set_header X-Original-Method $request_method;
    }

    location / {
        # 身份网关失败或拒绝时，nginx 不会把请求转给 Relay。
        auth_request /_relay_auth;
        proxy_http_version 1.1;
        proxy_pass http://relay_knowledge;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_buffering off;
        proxy_read_timeout 35s;
        proxy_connect_timeout 5s;
    }
}
```

**Caddy 配置示例**：

```caddyfile
knowledge.example.com {
    route {
        # /verify 必须执行 OIDC/token 校验和按路径、方法的 ACL；非 2xx 默认拒绝。
        forward_auth 127.0.0.1:4180 {
            uri /verify
            copy_headers X-Authenticated-User X-Authenticated-Groups
        }

        reverse_proxy 127.0.0.1:8791
    }
}
```

Caddy 自动 TLS 只提供传输保护；安全性仍取决于 `forward_auth` 服务实际校验身份并
执行 ACL。身份网关也可以改用 mTLS，但必须验证客户端证书、将证书身份映射到主体，
并按路径/操作执行 deny-by-default ACL，不能只要求一条 TLS 连接。

启动 `relay-knowledge` 时保持 loopback 绑定，不开启远端暴露开关：

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS=https://knowledge.example.com \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs,src \
relay-knowledge service run --web --mcp streamable-http
```

不得在网关上放行未经过认证与 ACL 的旁路路径，包括健康检查、Web 静态入口、
`/api/**`、`/api/v1/control/**` 和 `/mcp`。如监控需要免认证健康探测，应使用本机
或专用管理网络，而不是在公网虚拟主机上创建匿名例外。

### 16.6.2 TLS 终止

`relay-knowledge` 本身不提供 TLS 终止能力（`DEFAULT_SSL_VERIFY=true` 仅用于出站请求的 TLS 证书验证），TLS 应由反向代理或外部 load balancer 处理。

TLS 终止只保护传输并通常验证服务端身份，**不等于入站调用方认证**。远端访问还需
OIDC/token 身份网关加 ACL，或完整 mTLS：验证受信任的客户端证书、处理吊销/轮换、
把证书身份映射到调用方并执行 ACL。只有服务端证书的 HTTPS 仍不足以保护 Relay API。

出站代理和 TLS 配置：

| 环境变量 | 说明 |
| --- | --- |
| `HTTPS_PROXY` / `https_proxy` | HTTPS 出站代理（优先于 `HTTP_PROXY`） |
| `HTTP_PROXY` / `http_proxy` | HTTP 出站代理 |
| `ALL_PROXY` / `all_proxy` | 通用代理回退 |
| `NO_PROXY` / `no_proxy` | 不走代理的域名/IP（逗号分隔） |
| `SSL_VERIFY` / `ssl_verify` | 出站 HTTPS 证书验证，默认 `true` |

代理 URL 必须为 `http://` 或 `https://` 协议且包含有效的主机名，否则 `HttpConfigError::InvalidProxyUrl`。

### 16.6.3 防火墙规则

推荐防火墙策略：

```bash
# 仅允许反向代理访问 relay-knowledge 端口
iptables -A INPUT -p tcp --dport 8791 -s 127.0.0.1 -j ACCEPT
iptables -A INPUT -p tcp --dport 8791 -j DROP

# 或使用 ufw
ufw allow from 127.0.0.1 to any port 8791 proto tcp
ufw deny 8791
```

没有外部认证网关时，不得直接监听非 loopback 地址。若认证网关必须部署在另一台
主机，Relay 只能绑定网关专用私网地址，设置远端暴露开关，并把防火墙来源精确限制
为该网关地址；网关仍必须完成身份认证和 ACL。以下只是这种受隔离后端链路的示意，
防火墙规则本身不是调用方认证：

```bash
# 仅允许明确的认证网关地址；不要放行整个内网网段
iptables -A INPUT -p tcp --dport 8791 -s 10.20.30.40 -j ACCEPT
iptables -A INPUT -p tcp --dport 8791 -j DROP
```

## 16.7 安全相关环境变量参考

### 16.7.1 HTTP 与 QoS

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_HTTP_BIND` | `host:port` | `127.0.0.1:8791` | HTTP 监听地址 |
| `RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS` | 正整数 (ms) | `30000` | 单次 HTTP 请求超时（含 MCP tool call 的最大执行时间） |
| `RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS` | 正整数 (ms) | `10000` | 优雅关闭超时 |
| `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` | 正整数 | `1048576` | HTTP 请求体最大字节数（1 MiB） |
| `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` | 正整数 | `1024` | QoS 最大并发连接数 |
| `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` | 正整数 | `256` | QoS 最大在途请求数 |
| `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` | 正整数 | `512` | QoS 最大排队请求数 |

### 16.7.2 MCP Agent 接入

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED` | bool | `false` | 启用 MCP Streamable HTTP 服务 |
| `RELAY_KNOWLEDGE_MCP_ENDPOINT` | 路径 | `/mcp` | MCP HTTP 端点路径 |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS` | CSV | 空（允许无 Origin / loopback） | 请求 Origin 过滤列表；不是身份认证 |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` | CSV | 空 | MCP 可访问的 source scope 资源 allowlist；不是身份 ACL |
| `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE` | bool | `false` | 是否允许不指定 scope |
| `RELAY_KNOWLEDGE_MCP_MAX_LIMIT` | 正整数 | `10` | 单次检索最大返回条数上限 |
| `RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES` | 正整数 | `65536` | 单次检索上下文最大字节数 |
| `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS` | bool | `false` | 允许非 loopback 监听；不是身份认证或访问授权 |

### 16.7.3 审计日志

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED` | bool | `false` | 启用审计日志 JSONL 持久化 |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH` | 正整数 (1..65536) | `1024` | 审计日志异步写入通道容量 |

### 16.7.4 网络代理与 TLS

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `HTTPS_PROXY` / `https_proxy` | URL | 空 | HTTPS 出站代理（优先于 `HTTP_PROXY`） |
| `HTTP_PROXY` / `http_proxy` | URL | 空 | HTTP 出站代理 |
| `ALL_PROXY` / `all_proxy` | URL | 空 | 通用代理回退 |
| `NO_PROXY` / `no_proxy` | CSV | 空 | 不走代理的域名/IP |
| `SSL_VERIFY` / `ssl_verify` | bool | `true` | 出站 HTTPS 证书验证 |

### 16.7.5 布尔值格式

所有布尔类型环境变量支持以下值（不区分大小写）：

| 真值 | 假值 |
| --- | --- |
| `true`、`1`、`yes`、`on` | `false`、`0`、`no`、`off` |

非法布尔值（如 `"sometimes"`）会被 `EnvErrorKind::InvalidBoolean` 拒绝。

## 16.8 安全配置最佳实践

### 本地开发

```bash
# 最小安全配置：仅本机 loopback，无需远端授权
relay-knowledge service run --mcp streamable-http
```

### 团队内网服务

“在内网”不能证明调用方身份。团队访问仍由同机外部身份网关监听团队地址并执行
OIDC/token 或 mTLS 身份认证以及 deny-by-default ACL；Relay 后端保持 loopback：

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs,src,config \
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true \
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH=2048 \
relay-knowledge service run --web --mcp streamable-http
```

如果团队浏览器固定从 `https://internal.example.com` 访问，可额外配置
`RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS=https://internal.example.com`，但它只是纵深过滤，
不能代替网关认证，也可能拒绝没有 Origin 头的 CLI 客户端。

### 生产部署（反向代理后）

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=<逗号分隔的授权 scope> \
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS=https://your-domain.example.com \
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true \
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH=4096 \
RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS=2048 \
RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS=512 \
RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS=60000 \
relay-knowledge service run --web --mcp streamable-http
```

该配置只启动 loopback 后端；前置网关必须按 [16.6.1](#1661-反向代理部署)
认证每个调用方并授权每条路径。不要给 Relay 端口增加公网或全内网旁路。

### 安全检查清单

- [ ] 是否确认 Web、`/api/**`、`/api/v1/control/**` 和 MCP 都没有内建入站调用方认证？
- [ ] 有远程访问时，是否由外部网关先执行 mTLS（客户端证书校验 + 身份 ACL）或 OIDC/token 身份认证 + deny-by-default ACL？
- [ ] 网关是否保护所有路径且失败时默认拒绝，没有匿名或直连 Relay 的旁路？
- [ ] 没有外部认证网关时，`HTTP_BIND` 是否保持 loopback？
- [ ] 是否避免把 `allow_remote_clients`、Origin、QoS、MCP scope 或 session id 当作身份认证？
- [ ] 是否配置了 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES`？空列表意味着拒绝所有 scope 指定请求（除非设置 `ALLOW_UNSPECIFIED_SCOPE=true` 可使用全局检索）。
- [ ] 是否配置了 `RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS`？配置后无 Origin 的请求将被拒绝。
- [ ] `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED` 是否已启用？生产环境强烈建议启用。
- [ ] QoS 预算是否与预期负载匹配？默认值适用于中等负载，高并发场景需调高。
- [ ] 是否配置了反向代理的 TLS，并确认 TLS alone 不足以认证调用方？
- [ ] 防火墙是否限制了非信任来源访问监听端口？
- [ ] 出站代理和 `SSL_VERIFY` 是否正确配置？禁用 TLS 验证暴露于中间人攻击。
- [ ] `max_request_body_bytes` 是否合理？默认 1 MiB，防止请求体过大导致内存压力。
- [ ] `max_runtime_ms`（由 `request_timeout` 派生）是否满足最长 tool call 的执行时间需求？

---

导航：上一章：[第 15 章 SRE 运维手册](15-sre-operations-runbook.md) | 返回：[用户指南](README.md)
