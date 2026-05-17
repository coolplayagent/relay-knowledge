# 第 1 章 安装与运行时目录

[中文](../../zh/01-user-guide/01-install-and-runtime.md) | [English](../../en/01-user-guide/01-install-and-runtime.md)

本章只覆盖把本地开发环境跑起来所需的最短路径。发布、安装器和服务托管的完整要求见 [第 9 章 常驻服务](09-resident-service.md) 和 [安装、发布与升级](../03-architecture-specs/19-installation-release-and-upgrade.md)。

## 1.1 前置条件

仓库使用 Rust 2024 edition，`rust-toolchain.toml` 固定兼容工具链。推荐先安装 `rustup`，再在仓库根目录运行:

```bash
./setup.sh
```

`setup.sh` 只准备 Rust 组件和 hooks，不构建发布产物、不启动服务、不跑完整质量门。

常用脚本按职责拆分:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
./check.sh
```

`build.sh` 构建 `target/release/relay-knowledge` 和 `web/dist`。`run.sh` 只管理本地服务进程，发现缺少产物会提示先运行 `./build.sh`。`check.sh` 执行 fmt、clippy、测试、覆盖率、Web build 和可用时的浏览器集成门。

## 1.2 本地运行

未安装到系统路径时，直接运行调试二进制:

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

`relay-knowledge` 启动 Tokio runtime。CLI、Web、MCP 和本地 ACP adapter 都通过同一个 application service 进入核心能力，避免接口行为分叉。

## 1.3 同端口本地服务

需要浏览器工作区或本机 MCP endpoint 时，先构建，再启动同端口 Web/API/MCP 服务:

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

长期后台运行不要使用未受管 CLI 循环；改用第 9 章的 service manager plan 和 definition。

## 1.4 零配置默认值

普通本地使用不需要先设置环境变量。默认行为是:

- 运行时目录由平台规则解析，不写入仓库目录。
- SQLite 本地存储和本地 deterministic semantic/vector read models 自动启用。
- 网络和 QoS 使用保守默认值。
- MCP 写入、远程监听和后台 silent updates 默认关闭。

`status --format json` 会显示当前配置和状态。需要隔离一次性实验时，只设置一个临时 `RELAY_KNOWLEDGE_HOME`:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-demo \
  target/debug/relay-knowledge status --format json
```

设置 `RELAY_KNOWLEDGE_HOME` 后，配置、数据、状态、缓存、日志、临时、runtime 和 service 目录都会落在该根目录下的子目录中。完整目录覆盖项见 [第 12 章 高级配置参考](12-advanced-configuration.md)。

## 1.5 配置 readiness

不确定当前机器是否 ready 时，先运行只读配置诊断:

```bash
relay-knowledge setup doctor --format json
```

`setup doctor` 不打开 SQLite，不迁移 schema，也不刷新索引。它只检查 runtime path、network/QoS budget、retrieval backend metadata、MCP policy、service directory 和 worker budget。配置通过后，再用:

```bash
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

确认 graph storage、index freshness、worker/service live health 和 telemetry 状态。

## 1.6 网络与路径边界

所有覆盖路径必须是绝对路径，且不能包含 `..`。路径解析只在 `env` 和 `paths` 边界内完成。

常驻服务和 MCP Streamable HTTP 使用 `net::http` 和 `net::qos` 统一处理网络能力。日常本地使用不需要调整网络预算；需要远程监听、调大请求体或复现代理问题时，再查 [第 12 章](12-advanced-configuration.md)。

代理和证书验证继承 `HTTPS_PROXY`、`HTTP_PROXY`、`ALL_PROXY`、`NO_PROXY` 和 `SSL_VERIFY`。这些变量只在环境边界读取，业务模块不直接访问进程环境。
