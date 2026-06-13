# 第 17 章 安全配置完整指南

[中文](../../zh/01-user-guide/17-security-configuration.md) | [English](../../en/01-user-guide/17-security-configuration.md)

本章是 `relay-knowledge` 安全配置的完整参考，涵盖 QoS 准入控制、远端访问安全、MCP scope/origin 授权、审计日志和网络安全实践。所有配置项均基于代码实际实现，零配置时优先保证本机安全。

## 17.1 安全模型总览

`relay-knowledge` 的安全模型建立在分层防御之上：

1. **QoS 准入控制** — 连接、请求、排队三层预算，防止资源耗尽。
2. **绑定地址守卫** — 默认仅监听 loopback（`127.0.0.1:8791`），远端绑定必须显式授权。
3. **MCP scope/origin 授权** — 限定 agent 可检索的 source scope，校验请求来源。
4. **审计日志** — 内存环形缓冲 + JSONL 持久化，记录所有 agent 操作。
5. **传输层隔离** — 请求体大小限制、超时、TLS 验证、代理配置。
6. **Session 管理** — 有界 session 注册表，支持会话终止与驱逐。

这些机制由三个基础模块强制执行：

| 模块 | 安全职责 |
| --- | --- |
| `env` | 唯一读取环境变量的模块，所有配置入口集中校验 |
| `net::qos` | 准入控制，所有网络工作在消耗资源前经过 QoS 策略 |
| `net::http` | HTTP 监听/代理/TLS 配置，loopback 检测 |

## 17.2 QoS 策略配置

### 17.2.1 默认预算值

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

### 17.2.2 准入决策机制

`QosRuntime` 在三个层面执行准入检查，每个检查都是原子操作（`Arc<Mutex<QosSnapshot>>`）：

1. **`reserve_queue`** — 先占排队槽位，仅检查 `queued_requests < max_queue_depth`。
2. **`admit_queued_request`** — 从排队转入在途，同时检查排队预算和在途请求预算，并推进 `qos_queued_total`/`relay_knowledge_qos_queued_total`。
3. **`admit_request`** — 直接请求准入（非排队路径），检查 `in_flight_requests < max_in_flight_requests`。
4. **`admit_connection`** — 新 TCP 连接准入，检查 `connections < max_connections`。

MCP 请求通过 `admit_queued_request` 进入，Web/HTTP 路由通过 `admit_connection` + `admit_request` 进入。已校验 session 的 MCP `notifications/cancelled` 使用协议层优先路径，避免普通请求预算满载时无法取消活跃工具调用。MCP 与本地 ACP 工作触发 runtime budget 超时时，会计入 QoS timeout 诊断。

每种准入检查返回 `QosPermit`，该 permit 在 drop 时自动释放对应的预算计数（使用 `saturating_sub` 防止下溢），确保即使 panic 也不会泄漏预算。

### 17.2.3 过载保护行为

当预算耗尽时，系统返回以下拒绝原因：

| 拒绝原因 | HTTP 状态码 | 含义 |
| --- | --- | --- |
| `ConnectionBudgetExceeded` | 连接被静默丢弃 | TCP 连接数已达上限 |
| `RequestBudgetExceeded` | MCP: JSON-RPC 错误 `-32000`，Web: 无响应 | 在途请求数已达上限 |
| `QueueBudgetExceeded` | MCP: JSON-RPC 错误 `-32000`，Web: `503 Service Unavailable` | 排队请求数已达上限 |

`QosTcpListener` 在连接预算耗尽时静默丢弃新 TCP 连接（不占用内核 backlog），并在底层 accept 错误时以 1 秒间隔重试，避免空转忙等。

MCP 服务在 QoS 拒绝时通过 `record_mcp_qos_rejection` 记录审计事件（`qos_decision: Rejected`，`status: Failed`），并调用 `metrics.record_rejection` 记录拒绝指标。

### 17.2.4 调优建议

| 场景 | 建议调整 |
| --- | --- |
| 低内存边缘设备 | 设置 `max_connections=64`，`max_in_flight_requests=16` |
| 团队内部服务 | 保持默认值（1024/256/512） |
| 高并发反向代理后端 | 适度调高，注意数据库和文件描述符限制 |
| 压测/基准测试 | 临时调高预算，配合 `http_request_timeout_ms` 确保资源及时释放 |

## 17.3 远端访问安全

### 17.3.1 Loopback vs 非 Loopback 绑定

默认绑定地址为 `127.0.0.1:8791`（`DEFAULT_HTTP_BIND`）。系统通过 `remote_clients_allowed()` 判断是否允许非本地客户端：

```rust
// src/relay_knowledge/net/http.rs
pub fn remote_clients_allowed(config: &HttpConfig, allow_remote_clients: bool) -> bool {
    allow_remote_clients || is_local_bind(&config.bind_address.to_string())
}
```

`is_local_bind` 检测逻辑：

- 主机名为 `localhost`（不区分大小写）。
- IP 地址满足 `IpAddr::is_loopback()`（即 `127.0.0.0/8` 或 `::1`）。

绑定到 loopback 地址时**始终**允许连接（因为终端只能是本机）；绑定到非 loopback 地址时**必须**显式设置 `allow_remote_clients=true`。

### 17.3.2 远端监听的前提条件

非 loopback 绑定需要同时满足以下条件：

| 条件 | Web 模式 | MCP 模式 |
| --- | --- | --- |
| `allow_remote_clients=true` | ✅ `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true` | ✅ `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true` |
| QoS 预算充足 | ✅ 自动生效 | ✅ 自动生效 |
| 请求超时（`request_timeout`） | ✅ 默认 30s | ✅ 默认 30s |
| 审计日志 | 可选 | 建议启用 |
| scope/origin 限制 | N/A（Web 走统一 API） | ✅ 必须配置 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` |

远端监听配置示例：

```bash
RELAY_KNOWLEDGE_HTTP_BIND=0.0.0.0:8791 \
RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs,src \
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true \
relay-knowledge service run --mcp streamable-http
```

### 17.3.3 `ensure_web_remote_bind_allowed` 机制

服务启动时，`service_cli::ensure_web_remote_bind_allowed` 和 `http_contract::ensure_remote_bind_allowed` 分别检查 Web 路由和 MCP 路由的远端绑定授权：

```rust
// src/relay_knowledge/interfaces/service_cli.rs
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

`HttpBindAddress::parse()` 还拒绝端口为 `0` 的临时端口（返回 `HttpConfigError::EphemeralPort`），确保绑定地址始终显式指定端口。

## 17.4 MCP 安全控制

### 17.4.1 Scope 授权机制

MCP scope 授权基于 `AgentAccessPolicy`，由以下环境变量控制：

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` | CSV 字符串 | 无 | 允许的 source scope 白名单 |
| `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE` | bool | `false` | 是否允许不指定 scope |
| `RELAY_KNOWLEDGE_MCP_MAX_LIMIT` | 正整数 | `10` | 单次检索最大返回条数 |
| `RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES` | 正整数 | `65536` | 单次检索最大上下文字节数 |

Scope 授权流程（`scope_authorization.rs`）：

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

`max_context_bytes` 在 `AgentRetrievalResult::from_retrieval` 中用于截断过大的检索结果。

`max_runtime_ms` 由 HTTP 请求超时自动派生（`request_timeout - 1ms`），作为 MCP tool call 的硬超时。

### 17.4.2 Origin 限制

MCP 服务通过 `validate_origin()` 校验 HTTP `Origin` 请求头：

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

### 17.4.3 Session 管理

MCP Streamable HTTP 的会话由 `SessionRegistry` 管理：

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

### 17.4.4 其他 MCP 安全配置

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED` | bool | `false` | 启用 MCP Streamable HTTP |
| `RELAY_KNOWLEDGE_MCP_ENDPOINT` | 路径 | `/mcp` | MCP 端点路径（以 `/` 开头） |
| `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS` | bool | `false` | 允许远端客户端连接 |

MCP 服务通过 `McpServer::checked_router()` 在构建路由器时执行所有安全检查：

1. 检查 `mcp_streamable_http_enabled`，未启用返回 `McpServeError::Disabled`。
2. 调用 `ensure_remote_bind_allowed` 检查远端绑定授权。
3. 验证通过后构建带 `RequestBodyLimitLayer` 的路由器。

HTTP 请求级校验：

- `Content-Type` 必须为 `application/json`，否则 `415 Unsupported Media Type`。
- `Accept` 必须包含 `application/json` 和 `text/event-stream`（质量值 > 0），否则 `406 Not Acceptable`。
- `mcp-protocol-version` 头（非 `initialize` 请求后必需）必须为 `2025-11-25`。
- 不支持 JSON-RPC batch 请求。

## 17.5 审计日志

### 17.5.1 AgentAuditLog 配置

审计日志由两层组成：

1. **内存环形缓冲** (`AgentAuditLog`)：最多保留 `MAX_AUDIT_EVENTS=1024` 条事件，使用 `VecDeque` + `Mutex`，事件满时从头部驱逐。
2. **持久化 JSONL Sink** (`AgentAuditSink`)：通过 `mpsc` 通道（有界队列）异步写入 JSONL 文件，写入失败静默丢弃。

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED` | bool | `false` | 启用审计日志持久化 |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH` | 正整数 (1..65536) | `1024` | 异步写入通道容量 |

持久化启用时，审计 sink 在 `McpServer::new()` 中创建，日志写入 `<log_dir>/agent-audit.jsonl`。队列深度通过 `clamp(1, 65536)` 限制，防止无界内存增长。

### 17.5.2 审计事件结构

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

### 17.5.3 审计场景

审计日志覆盖以下场景：

1. **QoS 拒绝**：QoS 预算耗尽时记录 `qos_decision=rejected`，`status=failed`，`error_kind=qos_rejected`。
2. **Tool Call 完成**：`tools/call` 执行后记录完整审计（scope、freshness、limit、result_count、truncated、elapsed_ms）。
3. **非 Tool 操作**：`resources/read`、`prompts/get` 和 `ping`/`tools/list` 等通过 `metrics.record_request` 记录统计，其中 resource/prompt 方法额外通过 `record_mcp_method_audit` 记录完整审计事件。
4. **取消操作**：收到 `notifications/cancelled` 后取消对应请求，状态标记为 `cancelled`。

### 17.5.4 持久化格式

JSONL 追加写入，每条事件一行，使用 `serde_json::to_vec` 序列化后追加换行符，每次写入后 `flush`。文件通过 `tokio::fs::OpenOptions::create(true).append(true)` 打开，自动创建父目录。

示例审计日志行：

```json
{"sequence":1,"protocol":"mcp","operation":"retrieve_context","request_id":"session:rk-abc123|string:1","trace_id":"trace-mcp-session:rk-abc123|string:1","runtime_identity":{"protocol":"mcp","request_id":"session:rk-abc123|string:1"},"qos_decision":"admitted","status":"completed","source_scope":"docs","freshness":"allow-stale","limit":10,"result_count":5,"truncated":false,"elapsed_ms":42}
```

## 17.6 网络安全建议

### 17.6.1 反向代理部署

生产环境推荐将 `relay-knowledge` 置于反向代理后，不要在公网直接暴露：

**nginx 配置示例**：

```nginx
upstream relay_knowledge {
    server 127.0.0.1:8791;
    keepalive 32;
}

server {
    listen 443 ssl;
    server_name knowledge.example.com;

    ssl_certificate     /etc/ssl/certs/knowledge.pem;
    ssl_certificate_key /etc/ssl/private/knowledge.key;

    # 限制请求体大小（与服务端 max_request_body_bytes 对齐）
    client_max_body_size 1m;

    location /mcp {
        proxy_pass http://relay_knowledge;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 35s;
        proxy_connect_timeout 5s;
    }

    location /api {
        proxy_pass http://relay_knowledge;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }

    location / {
        proxy_pass http://relay_knowledge;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
    }
}
```

**Caddy 配置示例**：

```caddyfile
knowledge.example.com {
    reverse_proxy 127.0.0.1:8791 {
        header_up Host {host}
        header_up X-Forwarded-For {remote}
    }
}
```

启动 `relay-knowledge` 时必须保持 loopback 绑定 + 远端授权：

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true \
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS=https://knowledge.example.com \
relay-knowledge service run --web --mcp streamable-http
```

### 17.6.2 TLS 终止

`relay-knowledge` 本身不提供 TLS 终止能力（`DEFAULT_SSL_VERIFY=true` 仅用于出站请求的 TLS 证书验证），TLS 应由反向代理或外部 load balancer 处理。

出站代理和 TLS 配置：

| 环境变量 | 说明 |
| --- | --- |
| `HTTPS_PROXY` / `https_proxy` | HTTPS 出站代理（优先于 `HTTP_PROXY`） |
| `HTTP_PROXY` / `http_proxy` | HTTP 出站代理 |
| `ALL_PROXY` / `all_proxy` | 通用代理回退 |
| `NO_PROXY` / `no_proxy` | 不走代理的域名/IP（逗号分隔） |
| `SSL_VERIFY` / `ssl_verify` | 出站 HTTPS 证书验证，默认 `true` |

代理 URL 必须为 `http://` 或 `https://` 协议且包含有效的主机名，否则 `HttpConfigError::InvalidProxyUrl`。

### 17.6.3 防火墙规则

推荐防火墙策略：

```bash
# 仅允许反向代理访问 relay-knowledge 端口
iptables -A INPUT -p tcp --dport 8791 -s 127.0.0.1 -j ACCEPT
iptables -A INPUT -p tcp --dport 8791 -j DROP

# 或使用 ufw
ufw allow from 127.0.0.1 to any port 8791 proto tcp
ufw deny 8791
```

如果直接监听非 loopback 地址：

```bash
# 仅允许内网和特定可信 IP
iptables -A INPUT -p tcp --dport 8791 -s 10.0.0.0/8 -j ACCEPT
iptables -A INPUT -p tcp --dport 8791 -s 192.168.0.0/16 -j ACCEPT
iptables -A INPUT -p tcp --dport 8791 -j DROP
```

## 17.7 安全相关环境变量参考

### 17.7.1 HTTP 与 QoS

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_HTTP_BIND` | `host:port` | `127.0.0.1:8791` | HTTP 监听地址 |
| `RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS` | 正整数 (ms) | `30000` | 单次 HTTP 请求超时（含 MCP tool call 的最大执行时间） |
| `RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS` | 正整数 (ms) | `10000` | 优雅关闭超时 |
| `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` | 正整数 | `1048576` | HTTP 请求体最大字节数（1 MiB） |
| `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` | 正整数 | `1024` | QoS 最大并发连接数 |
| `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` | 正整数 | `256` | QoS 最大在途请求数 |
| `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` | 正整数 | `512` | QoS 最大排队请求数 |

### 17.7.2 MCP Agent 接入

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED` | bool | `false` | 启用 MCP Streamable HTTP 服务 |
| `RELAY_KNOWLEDGE_MCP_ENDPOINT` | 路径 | `/mcp` | MCP HTTP 端点路径 |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS` | CSV | 空（允许无 Origin / loopback） | 允许的 CORS Origin 列表 |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` | CSV | 空 | 允许的 source scope 白名单 |
| `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE` | bool | `false` | 是否允许不指定 scope |
| `RELAY_KNOWLEDGE_MCP_MAX_LIMIT` | 正整数 | `10` | 单次检索最大返回条数上限 |
| `RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES` | 正整数 | `65536` | 单次检索上下文最大字节数 |
| `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS` | bool | `false` | 允许非 loopback 客户端 |

### 17.7.3 审计日志

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED` | bool | `false` | 启用审计日志 JSONL 持久化 |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH` | 正整数 (1..65536) | `1024` | 审计日志异步写入通道容量 |

### 17.7.4 网络代理与 TLS

| 环境变量 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `HTTPS_PROXY` / `https_proxy` | URL | 空 | HTTPS 出站代理（优先于 `HTTP_PROXY`） |
| `HTTP_PROXY` / `http_proxy` | URL | 空 | HTTP 出站代理 |
| `ALL_PROXY` / `all_proxy` | URL | 空 | 通用代理回退 |
| `NO_PROXY` / `no_proxy` | CSV | 空 | 不走代理的域名/IP |
| `SSL_VERIFY` / `ssl_verify` | bool | `true` | 出站 HTTPS 证书验证 |

### 17.7.5 布尔值格式

所有布尔类型环境变量支持以下值（不区分大小写）：

| 真值 | 假值 |
| --- | --- |
| `true`、`1`、`yes`、`on` | `false`、`0`、`no`、`off` |

非法布尔值（如 `"sometimes"`）会被 `EnvErrorKind::InvalidBoolean` 拒绝。

## 17.8 安全配置最佳实践

### 本地开发

```bash
# 最小安全配置：仅本机 loopback，无需远端授权
relay-knowledge service run --mcp streamable-http
```

### 团队内网服务

```bash
RELAY_KNOWLEDGE_HTTP_BIND=0.0.0.0:8791 \
RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs,src,config \
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS=http://localhost:3000,https://internal.example.com \
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true \
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH=2048 \
relay-knowledge service run --web --mcp streamable-http
```

### 生产部署（反向代理后）

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=<逗号分隔的授权 scope> \
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS=https://your-domain.example.com \
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true \
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH=4096 \
RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS=2048 \
RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS=512 \
RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS=60000 \
relay-knowledge service run --web --mcp streamable-http
```

### 安全检查清单

- [ ] `HTTP_BIND` 是否绑定到非 loopback 地址？如是，必须设置 `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`。
- [ ] 是否配置了 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES`？空列表意味着拒绝所有 scope 指定请求（除非设置 `ALLOW_UNSPECIFIED_SCOPE=true` 可使用全局检索）。
- [ ] 是否配置了 `RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS`？配置后无 Origin 的请求将被拒绝。
- [ ] `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED` 是否已启用？生产环境强烈建议启用。
- [ ] QoS 预算是否与预期负载匹配？默认值适用于中等负载，高并发场景需调高。
- [ ] 是否配置了反向代理的 TLS？`relay-knowledge` 不内置 TLS 终止。
- [ ] 防火墙是否限制了非信任来源访问监听端口？
- [ ] 出站代理和 `SSL_VERIFY` 是否正确配置？禁用 TLS 验证暴露于中间人攻击。
- [ ] `max_request_body_bytes` 是否合理？默认 1 MiB，防止请求体过大导致内存压力。
- [ ] `max_runtime_ms`（由 `request_timeout` 派生）是否满足最长 tool call 的执行时间需求？

---

导航：上一章：[第 16 章 SRE 运维手册](16-sre-operations-runbook.md) | 返回：[用户指南](README.md)
