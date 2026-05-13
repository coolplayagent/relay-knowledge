# 第 1 章 安装与运行时目录

## 1.1 前置条件

本仓库使用 Rust 2024 edition，`rust-toolchain.toml` 固定到兼容工具链。推荐通过 `rustup` 安装 Rust，然后在仓库根目录运行:

```bash
cargo build
cargo test --all-targets --all-features
```

开发环境还包含一个静态 Web 工作区和浏览器集成测试。需要验证 Web 时安装 Node.js、npm、Python 和 uv，然后运行第 7 章中的验证命令。

## 1.2 本地运行

未安装到系统路径时，可以直接运行调试二进制:

```bash
cargo build
target/debug/relay-knowledge status
target/debug/relay-knowledge --version
```

也可以通过 Cargo 运行:

```bash
cargo run -- status --format json
cargo run -- query -- --help
```

`relay-knowledge` 启动 Tokio runtime，CLI、Web、MCP 和本地 agent adapter 都通过共享 application service 进入核心能力。

## 1.3 运行时目录

运行时状态不应写入仓库目录。默认目录由平台规则解析，`status --format json` 会显示当前配置和状态。需要隔离一次性实验时，设置 `RELAY_KNOWLEDGE_HOME` 最直接:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-demo \
  target/debug/relay-knowledge status --format json
```

设置 `RELAY_KNOWLEDGE_HOME` 后，配置、数据、状态、缓存、日志、临时、runtime 和 service 目录都会落在该根目录下的子目录中。也可以分别覆盖:

```text
RELAY_KNOWLEDGE_CONFIG_DIR
RELAY_KNOWLEDGE_DATA_DIR
RELAY_KNOWLEDGE_STATE_DIR
RELAY_KNOWLEDGE_CACHE_DIR
RELAY_KNOWLEDGE_LOG_DIR
RELAY_KNOWLEDGE_TEMP_DIR
RELAY_KNOWLEDGE_RUNTIME_DIR
RELAY_KNOWLEDGE_SERVICE_DIR
```

所有覆盖路径必须是绝对路径，且不能包含 `..`。路径解析只在 `env` 和 `paths` 边界内完成。

## 1.4 网络与 QoS 配置

常驻服务和 MCP Streamable HTTP 使用 `net::http` 和 `net::qos` 统一处理网络能力。常用覆盖项:

```text
RELAY_KNOWLEDGE_HTTP_BIND
RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS
RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS
RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES
RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS
RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS
RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH
```

代理和证书验证继承 `HTTPS_PROXY`、`HTTP_PROXY`、`ALL_PROXY`、`NO_PROXY` 和 `SSL_VERIFY`。这些变量只在环境边界读取，业务模块不直接访问进程环境。

## 1.5 安装与发布路径

当前开发路径以 `cargo build` 和本地二进制为主。正式发布要求见 [安装部署与发布规格](../specs/installation-and-release.md): 稳定版本需要 GitHub Releases、crates.io、校验和、包管理器清单和平台 service manager 安装路径。使用指南中的命令均适用于本地构建出的 `relay-knowledge` 二进制。
