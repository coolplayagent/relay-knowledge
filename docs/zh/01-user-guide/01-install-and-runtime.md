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

> ⚠️ **开发环境限定**：`run.sh` 和 `run.sh --daemon` 仅用于开发环境验证，不可用于生产部署。
> 长期后台运行必须由 systemd（Linux）、launchd（macOS）或 Windows Service 托管。
> 生产部署请参阅第 14 章「服务化部署指南」。

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
./check.sh
./check.sh --deep
```

`build.sh` 构建 `target/release/relay-knowledge` 和 `web/dist`。`run.sh` 只管理本地服务进程，发现缺少产物会提示先运行 `./build.sh`。`check.sh` 执行文档、fmt、`cargo check`、Clippy、测试、覆盖率、Web build 和可用时的浏览器集成门。`check.sh --deep` 还会执行确定性 benchmark、无 FFI 的 Miri 子集和 AddressSanitizer。deep profile 要求 nightly 组件；缺失时会给出安装命令并失败，不会静默跳过：

```bash
rustup toolchain install nightly --profile minimal --component miri,rust-src
./check.sh --deep
```

AddressSanitizer 只支持脚本显式选择的 Linux 与 macOS target；Linux x86_64 CI 是跨开发环境的权威 sanitizer 门禁。

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
- 文件监听 (fs.watch) 默认启用，自动检测源码变更并推送增量索引任务。
- 交互式文本 CLI 会按 24 小时缓存周期检查稳定新版本并只输出提示；不会自动安装或替换二进制。

`status --format json` 会显示当前配置和状态。需要隔离一次性实验时，只设置一个临时 `RELAY_KNOWLEDGE_HOME`:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-demo \
  target/debug/relay-knowledge status --format json
```

设置 `RELAY_KNOWLEDGE_HOME` 后，配置、数据、状态、缓存、日志、临时、runtime 和 service 目录都会落在该根目录下的子目录中。完整目录覆盖项见 [第 12 章 高级配置参考](12-advanced-configuration.md)。

Windows 新安装的主库为
`D:\relay-knowledge\users\<user-sid>\data\relay-knowledge.sqlite`，
仓库分片位于同一数据目录下的 `stores/repositories/`。
SID 来自 Windows 进程令牌，迁移用户配置目录或修改 LocalAppData 不会改变默认库。
实际打开 SQLite 或执行服务安装/升级/回滚预检时创建账户目录并设置受保护 ACL，仅允许该账户、SYSTEM 和 Administrators；
服务计划和卸载不创建数据目录。服务固定的 SID 路径仍保留权限策略，
每次启动的新服务进程都会重新校验 ACL 和重解析点。
已有目录 ACL 不安全、存在重解析点，或父目录允许其他账户删除或修改权限时会报错。
已有数据库、恢复文件和分片也会检查 ACL 与链接，不能仅依靠私有父目录保护搬入的文件。
已有受管理目录和文件必须保留账户、SYSTEM、Administrators 完整权限，拒绝 deny 规则。
旧目录发现使用原生 Windows API，不依赖 PowerShell。自动默认路径需要 Windows PowerShell 5.1 和支持 ACL 的本地卷；D: 不满足条件时，
请显式指定已配置私有权限的数据目录。
`status --format json` 会显示实际目录。配置、日志等其他目录仍使用 AppData/TEMP 默认值，
Linux、macOS 的数据目录规则不变。

数据目录优先级为 `RELAY_KNOWLEDGE_DATA_DIR` > `RELAY_KNOWLEDGE_HOME/data` >
已有的 Windows LocalAppData 数据目录 > 新的平台默认值。
没有显式覆盖时，只要 `%LOCALAPPDATA%\relay-knowledge\data` 目录存在，启动就继续使用它，
保留主库、恢复文件和全部分片。旧目录与新的用户目录同时存在时，必须通过
`RELAY_KNOWLEDGE_DATA_DIR` 明确选择。探测目录失败或超时会报错，不会在其他位置另开空库。

在 PowerShell 中指定其他 SQLite 存储目录：

```powershell
$env:RELAY_KNOWLEDGE_DATA_DIR = 'E:\KnowledgeData'
relay-knowledge status --format json
# 可选：持久化到当前用户环境，供后续新终端使用。
[Environment]::SetEnvironmentVariable('RELAY_KNOWLEDGE_DATA_DIR', 'E:\KnowledgeData', 'User')
```

变量值是目录，不是数据库文件名。空值、相对路径和包含 `..` 的路径会被拒绝。
新安装时，D 盘不存在或目标目录不可写会导致创建或打开数据库失败，需要指定可访问的绝对路径。
已有 LocalAppData 数据库在没有 D 盘时仍可继续使用。

升级会原地保留已有数据库，不会自动搬迁。也可以显式固定原 Windows 目录，包括服务配置：

```powershell
$env:RELAY_KNOWLEDGE_DATA_DIR = Join-Path $env:LOCALAPPDATA 'relay-knowledge\data'
```

需要迁移时，先停止托管服务及其他 writer，对完整数据目录做一致性备份，再把主库、
存在的 WAL/SHM 恢复文件和 `stores/repositories` 一起复制到目标目录，保留原副本以便回滚。
所有 CLI 和服务必须使用同一数据目录。已安装服务会在服务定义中保存显式数据路径，
只修改终端变量不会改变已有服务；应重新生成并应用生命周期计划，再运行 `status`、
`health` 和 `service doctor` 检查。恢复旧二进制还需遵守
[升级与回滚合同](../03-architecture-specs/19-installation-release-and-upgrade.md)。
卸载默认保留运行时数据。

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

版本提示使用同一网络边界和代理/TLS 策略。可用
`RELAY_KNOWLEDGE_UPDATE_CHECK_ENABLED=false` 关闭提示，用
`RELAY_KNOWLEDGE_UPDATE_SOURCES=github,crates.io` 配置检测源，用
`RELAY_KNOWLEDGE_UPDATE_CHECK_INTERVAL_MS` 调整缓存周期，用
`RELAY_KNOWLEDGE_UPDATE_GITHUB_REPO=owner/name` 指向自托管 fork 的 release 源。
