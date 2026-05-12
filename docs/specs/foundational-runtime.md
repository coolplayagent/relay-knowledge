# 基础运行时层规格

> 文档版本: 1.0
> 编制日期: 2026-05-11
> 适用范围: `env`、`paths`、`net`、`net::http`、`net::qos` 和项目状态输出

## 1. 设计结论

`relay-knowledge` 的基础运行时层已经包含三个明确边界:

- `env`: 唯一读取环境变量的生产模块，负责把平台目录变量和 `RELAY_KNOWLEDGE_*` 覆盖项解析成 typed config。
- `paths`: 唯一解析运行态目录的模块，负责 config、data、state、cache、log、temp、runtime 和 service 目录。
- `net`: 唯一承载网络配置和准入策略的模块，当前包含 `net::http` 的 HTTP 运行时配置和 `net::qos` 的资源预算策略。

应用服务通过 async public API 读取 typed environment snapshot，并把解析后的路径和网络预算暴露在 `project.status` JSON 响应里。路径配置保持启动时解析结果；网络配置通过 `net::NetworkRuntime` 支持刷新后即时生效。文本 CLI 输出仍保持只输出项目名，CLI 二进制入口由 Tokio runtime 驱动。

Rust 源码集中在 `src/relay_knowledge/` 下，`Cargo.toml` 通过显式 `lib` 和 `bin`
路径指向 `src/relay_knowledge/lib.rs` 与 `src/relay_knowledge/main.rs`，避免把模块文件直接散放在 `src/` 根目录。`api`、`application`、`domain`、`env`、`paths` 和 `net` 都是独立目录模块；`project` 模块只承载 `PROJECT_NAME` 这类项目身份常量。

## 2. 环境变量

`env` 读取两类变量。

平台目录输入:

| 变量 | 用途 |
| --- | --- |
| `HOME` | Unix、macOS 和 fallback Windows 用户目录 |
| `XDG_CONFIG_HOME` | Unix config base |
| `XDG_DATA_HOME` | Unix data base |
| `XDG_STATE_HOME` | Unix state/log base |
| `XDG_CACHE_HOME` | Unix cache base |
| `XDG_RUNTIME_DIR` | Unix runtime base |
| `APPDATA` | Windows config base |
| `LOCALAPPDATA` | Windows data/cache/log/runtime base |
| `TMPDIR` / `TEMP` / `TMP` | Unix/macOS 按 `TMPDIR`、`TEMP`、`TMP` 选择；Windows 按 `TEMP`、`TMP`、`TMPDIR` 选择 |
| `HTTPS_PROXY` / `https_proxy` | 通用 HTTPS proxy 输入 |
| `HTTP_PROXY` / `http_proxy` | 通用 HTTP proxy 输入 |
| `ALL_PROXY` / `all_proxy` | 通用 fallback proxy 输入 |
| `NO_PROXY` / `no_proxy` | 通用 no-proxy 规则输入 |
| `SSL_VERIFY` / `ssl_verify` | 通用 TLS 证书校验开关 |

Relay 覆盖项:

| 变量 | 用途 |
| --- | --- |
| `RELAY_KNOWLEDGE_HOME` | 把所有运行态目录放到该根目录下的 `config`、`data`、`state`、`cache`、`logs`、`tmp`、`run`、`service` 子目录 |
| `RELAY_KNOWLEDGE_CONFIG_DIR` | 覆盖 config 目录 |
| `RELAY_KNOWLEDGE_DATA_DIR` | 覆盖 data 目录 |
| `RELAY_KNOWLEDGE_STATE_DIR` | 覆盖 state 目录 |
| `RELAY_KNOWLEDGE_CACHE_DIR` | 覆盖 cache 目录 |
| `RELAY_KNOWLEDGE_LOG_DIR` | 覆盖 log 目录 |
| `RELAY_KNOWLEDGE_TEMP_DIR` | 覆盖 temp 目录 |
| `RELAY_KNOWLEDGE_RUNTIME_DIR` | 覆盖 runtime 目录 |
| `RELAY_KNOWLEDGE_SERVICE_DIR` | 覆盖 service metadata/template 目录 |
| `RELAY_KNOWLEDGE_HTTP_BIND` | HTTP bind address，默认 `127.0.0.1:8791` |
| `RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS` | HTTP request timeout，默认 `30000` |
| `RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS` | HTTP graceful shutdown timeout，默认 `10000` |
| `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` | HTTP request body budget，默认 `1048576` |
| `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` | QoS connection budget，默认 `1024` |
| `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` | QoS in-flight request budget，默认 `256` |
| `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` | QoS queue budget，默认 `512` |

空值会失败。数字变量必须是大于零的正整数。HTTP bind 必须能解析为 `host:port`，可以是 IP literal 或 hostname，并且端口不能是 `0`。Proxy URL 必须使用 `http://` 或 `https://` 且包含 host。No-proxy 规则会按逗号切分并 trim，空规则会失败。Proxy 值可能包含凭据，状态输出只暴露是否配置，不输出原始 proxy 字符串。`SSL_VERIFY` 默认 `true`，可用 `true/false`、`1/0`、`yes/no`、`on/off`。

## 3. 路径规则

所有运行态路径必须是绝对路径，且不能包含 `..`。默认路径不依赖当前工作目录、仓库目录或 release 解压目录。

Unix 默认目录:

| 类型 | 默认 |
| --- | --- |
| config | `${XDG_CONFIG_HOME:-$HOME/.config}/relay-knowledge` |
| data | `${XDG_DATA_HOME:-$HOME/.local/share}/relay-knowledge` |
| state | `${XDG_STATE_HOME:-$HOME/.local/state}/relay-knowledge` |
| cache | `${XDG_CACHE_HOME:-$HOME/.cache}/relay-knowledge` |
| log | `${state}/logs` |
| temp | `${TMPDIR:-/tmp}/relay-knowledge` |
| runtime | `${XDG_RUNTIME_DIR}/relay-knowledge`，未设置时使用 `${state}/run` |
| service | `${config}/service` |

非交互 Unix 服务环境缺少 `HOME` 且没有 XDG 变量时，默认使用 `/etc/relay-knowledge`、`/var/lib/relay-knowledge`、`/var/cache/relay-knowledge` 和 `/tmp/relay-knowledge`，避免依赖用户 shell 环境。

macOS 默认目录:

| 类型 | 默认 |
| --- | --- |
| config | `$HOME/Library/Application Support/relay-knowledge/config` |
| data | `$HOME/Library/Application Support/relay-knowledge/data` |
| state | `$HOME/Library/Application Support/relay-knowledge/state` |
| cache | `$HOME/Library/Caches/relay-knowledge` |
| log | `$HOME/Library/Logs/relay-knowledge` |
| temp | `${TMPDIR:-/tmp}/relay-knowledge` |
| runtime | `${state}/run` |
| service | `$HOME/Library/Application Support/relay-knowledge/service` |

Windows 默认目录:

| 类型 | 默认 |
| --- | --- |
| config | `%APPDATA%\relay-knowledge` |
| data | `%LOCALAPPDATA%\relay-knowledge\data` |
| state | `%LOCALAPPDATA%\relay-knowledge\state` |
| cache | `%LOCALAPPDATA%\relay-knowledge\cache` |
| log | `%LOCALAPPDATA%\relay-knowledge\logs` |
| temp | `%LOCALAPPDATA%\relay-knowledge\tmp`，除非 `TEMP`、`TMP` 或 `TMPDIR` 显式设置；显式 temp base 仍会追加 `relay-knowledge` 子目录 |
| runtime | `%LOCALAPPDATA%\relay-knowledge\run` |
| service | `%APPDATA%\relay-knowledge\service` |

## 4. 网络和 QoS

`net::http` 目前只定义配置与验证，不启动 socket、线程或后台循环。后续 HTTP server/client 必须在该边界内接入 Tokio 或同等级成熟 async runtime，并在分配连接、请求或队列资源前调用 `net::qos`。

`net::NetworkRuntime` 是可刷新网络配置句柄。长运行进程启动时用 typed environment snapshot 构建初始配置；当环境变量或配置来源发生变化时，调用 `refresh_from_process_environment` 或 `refresh_from_environment` 会重新验证 proxy、no-proxy、TLS verification、HTTP budget 和 QoS budget，并替换 net 模块中的当前配置。HTTP client/server adapter 必须读取该句柄的 `current` 配置，不能缓存旧的环境变量字符串。

QoS admission 使用当前 snapshot 判断:

- 当前连接数达到 `max_connections` 时拒绝。
- 当前 in-flight 请求数达到 `max_in_flight_requests` 时拒绝。
- 当前排队请求数达到 `max_queue_depth` 时拒绝。
- 三个预算都未耗尽时准入。

拒绝原因使用结构化枚举区分 connection、request 和 queue budget，便于后续 metrics、日志和 HTTP 错误映射。

## 5. API 和 CLI 行为

`relay-knowledge --format json` 的 `project.status` 响应包含 `runtime` 字段:

```json
{
  "project_name": "relay-knowledge",
  "metadata": {
    "trace_id": "trace-...",
    "request_id": "req-...",
    "graph_version": 0,
    "stale": false
  },
  "runtime": {
    "config_dir": "/home/alice/.config/relay-knowledge",
    "data_dir": "/home/alice/.local/share/relay-knowledge",
    "state_dir": "/home/alice/.local/state/relay-knowledge",
    "cache_dir": "/home/alice/.cache/relay-knowledge",
    "log_dir": "/home/alice/.local/state/relay-knowledge/logs",
    "temp_dir": "/tmp/relay-knowledge",
    "runtime_dir": "/home/alice/.local/state/relay-knowledge/run",
    "service_dir": "/home/alice/.config/relay-knowledge/service",
    "http_bind": "127.0.0.1:8791",
    "http_request_timeout_ms": 30000,
    "http_graceful_shutdown_timeout_ms": 10000,
    "http_max_request_body_bytes": 1048576,
    "http_proxy_configured": false,
    "http_no_proxy_rules": 0,
    "http_ssl_verify": true,
    "qos_max_connections": 1024,
    "qos_max_in_flight_requests": 256,
    "qos_max_queue_depth": 512
  }
}
```

`--format streaming-json` 的 `item` event 同样包含 `runtime` 字段。启动配置无效时，CLI 退出码为 `1`，并输出 `failed to load runtime configuration: ...`。
`relay-knowledge version` 和标准 `relay-knowledge --version` alias 只输出包版本，不加载 runtime 配置。
`version --format json` 输出机器可读版本对象；`streaming-json` 对 version 不适用并返回参数错误。

当前已落地的 application 和 CLI 边界是 async:

- `RelayKnowledgeService::from_process_environment`
- `RelayKnowledgeService::from_environment`
- `RelayKnowledgeService::with_store`
- `RelayKnowledgeService::refresh_network_from_process_environment`
- `RelayKnowledgeService::refresh_network_from_environment`
- `RelayKnowledgeService::project_status`
- `RelayKnowledgeService::ingest`
- `RelayKnowledgeService::retrieve_context`
- `RelayKnowledgeService::inspect_graph`
- `RelayKnowledgeService::refresh_indexes`
- `RelayKnowledgeService::health`
- `RelayKnowledgeService::service_status`
- `interfaces::cli::run`

存储初始化仍由 application service 编排，但数据库路径只来自 `paths.data_dir`。
SQLite 是当前默认本地后端，所有 SQLite 调用通过 `storage` 边界进入
`tokio::task::spawn_blocking`，不能直接在 async executor 上执行阻塞数据库工作。
`indexing` 和 `retrieval` 模块分别承载 index refresh plan 与 retrieval plan 校验，
application service 只编排这些 contract，不把策略留在 CLI/Web adapter 中。

## 6. 测试策略

基础层使用不触碰开发者本地状态的确定性单元测试:

- `env` 测试环境变量解析、通用 proxy/no-proxy 优先级、Windows 大小写环境变量、Windows temp 优先级、boolean 解析、空值、非法数字和零值。
- `paths` 测试 Unix/XDG 默认值、Unix 无 `HOME` 服务 fallback、Windows temp 隔离、`RELAY_KNOWLEDGE_HOME`、相对路径拒绝和 `..` 拒绝。
- `net::http` 测试 bind address hostname/IP literal、timeout、body budget、proxy URL、no-proxy 规则和 port `0` 拒绝。
- `net::qos` 测试准入、连接预算、请求预算、队列预算和零预算拒绝。
- `net::NetworkRuntime` 测试 env snapshot refresh 后当前网络配置即时变化。
- application service 测试使用 Tokio test runtime 覆盖 async 配置加载、状态输出和网络刷新。
- 集成测试集中在 `tests/relay_knowledge/`，并按 `application`、`domain`、`interfaces` 目录组织。
- CLI 集成测试清除所有 `RELAY_KNOWLEDGE_*`、`HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY`、`NO_PROXY` 和 `SSL_VERIFY` 覆盖项，避免开发者 shell 污染测试结果。
- CLI ingest/query/health/service 集成测试使用临时 `RELAY_KNOWLEDGE_HOME`，避免写入开发者默认数据目录。
