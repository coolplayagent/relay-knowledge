# 第 1 章 安装与运行时目录

[中文](../../zh/01-user-guide/01-install-and-runtime.md) | [英文](../../en/01-user-guide/01-install-and-runtime.md)

## 1.1 前置条件

本仓库使用 Rust 2024 edition，`rust-toolchain.toml` 固定到兼容工具链。推荐通过 `rustup` 安装 Rust，然后在仓库根目录运行:

```bash
./setup.sh
```

`setup.sh` 只准备开发环境、Rust 组件和 hooks，不构建产物、不启动服务、不跑完整质量门。

常用脚本按职责拆分:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
./check.sh
```

`build.sh` 构建 `target/release/relay-knowledge` 和 `web/dist`。`run.sh` 只管理本地服务进程，发现缺少产物会提示先运行 `./build.sh`。`check.sh` 执行 fmt、clippy、测试、覆盖率、Web build 和可用时的浏览器集成门。

开发环境还包含一个静态 Web 工作区和浏览器集成测试。需要验证 Web 时安装 Node.js、npm、Python 和 uv，然后运行第 7 章中的验证命令。

## 1.2 本地运行

未安装到系统路径时，可以直接运行调试二进制:

```bash
cargo build
target/debug/relay-knowledge status
target/debug/relay-knowledge --version
target/debug/relay-knowledge setup doctor --format json
```

也可以通过 Cargo 运行:

```bash
cargo run -- status --format json
cargo run -- query -- --help
```

`relay-knowledge` 启动 Tokio runtime，CLI、Web、MCP 和本地 agent adapter 都通过共享 application service 进入核心能力。

同端口 Web 服务:

```bash
./build.sh
./run.sh start --port 8791 --daemon
curl http://127.0.0.1:8791/api/health
./run.sh stop --force
```

底层入口是:

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
  target/release/relay-knowledge service run --web --mcp streamable-http
```

## 1.3 零配置默认值

普通本地使用不需要先设置环境变量。默认行为是:

- 运行时目录由平台规则解析，不写入仓库目录。
- SQLite 本地存储和本地 deterministic semantic/vector read models 自动启用。
- 网络和 QoS 使用保守默认值。
- MCP 写入、index refresh tool 和远程监听默认关闭。

`status --format json` 会显示当前配置和状态。需要隔离一次性实验时，设置一个临时 `RELAY_KNOWLEDGE_HOME` 即可:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-demo \
  target/debug/relay-knowledge status --format json
```

设置 `RELAY_KNOWLEDGE_HOME` 后，配置、数据、状态、缓存、日志、临时、runtime 和 service 目录都会落在该根目录下的子目录中。完整目录覆盖项见 [第 8 章 高级配置参考](08-advanced-configuration.md)。
不确定当前机器的基础配置是否 ready 时，先运行 `relay-knowledge setup doctor --format json`；
它会把 runtime path、network/QoS budget、retrieval backend metadata、MCP
policy、service directory 和 worker budget 检查聚合到一个不触碰 SQLite 的只读响应里。
随后用 `relay-knowledge health --format json` 或 `relay-knowledge service doctor --format json`
确认 graph storage、index freshness 和 worker/service live health。

## 1.4 网络与 QoS

所有覆盖路径必须是绝对路径，且不能包含 `..`。路径解析只在 `env` 和 `paths` 边界内完成。

常驻服务和 MCP Streamable HTTP 使用 `net::http` 和 `net::qos` 统一处理网络能力。日常本地使用不需要调整网络预算；需要远程监听、调大请求体或复现代理问题时，再进入 [第 8 章](08-advanced-configuration.md)。常用覆盖项:

```text
RELAY_KNOWLEDGE_HTTP_BIND
RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS
RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS
RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES
RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS
RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS
RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH
```

代理和证书验证继承 `HTTPS_PROXY`、`HTTP_PROXY`、`ALL_PROXY`、`NO_PROXY` 和 `SSL_VERIFY`。这些变量只在环境边界读取，业务模块不直接访问进程环境。

Agent audit 持久化默认关闭。开启 `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true` 后，常驻 MCP 和本地 ACP adapter 会把 bounded audit events 异步镜像到当前 log 目录下的 `agent-audit.jsonl`；日志目录仍由 `paths` 模块解析，默认不写入仓库或当前工作目录。

## 1.5 安装与发布路径

当前开发路径以 `cargo build`、本地二进制和仓库脚本为主。正式发布要求见 [安装部署与发布规格](../03-architecture-specs/15-installation-and-release.md): 稳定版本需要 GitHub Releases、crates.io、校验和、包管理器清单和平台 service manager 安装路径。使用指南中的命令均适用于本地构建出的 `relay-knowledge` 二进制。
