# 第 14 章 服务化部署指南

[中文](14-service-deployment-guide.md) | 中文深入专题；英文核心流程见 [English Chapter 9](../../en/01-user-guide/09-resident-service.md)

> 本章为 relay-knowledge 服务化部署的完整操作手册，覆盖从安装准备到升级、回滚、卸载的全生命周期。
> 架构契约参见 [第 19 章安装、发布与升级](../03-architecture-specs/19-installation-release-and-upgrade.md) 和 [第 22 章服务化部署、控制面与数据面分离](../03-architecture-specs/22-service-deployment-control-data-plane.md)。
> 常驻服务快速入门可先阅读 [第 9 章服务化部署与常驻服务](09-resident-service.md)，本章在此基础上提供更详尽的平台特定步骤和运维操作细节。

---

## 14.1 概述

relay-knowledge 的常驻服务是一个异步优先、有界资源的后台进程，托管 Web HTTP API、MCP Streamable HTTP 协议、startup index reconciler、code-index worker pool 和 repository-set refresh worker。

### 14.1.1 四种部署拓扑

| 拓扑 | 控制面 | 数据面 | 适用场景 |
|------|--------|--------|----------|
| `embedded_cli` | CLI 进程内 | `single_sqlite` | 临时命令、测试、一次性操作 |
| `resident_single_process` | `service run` HTTP/Web/MCP + worker pool | `single_sqlite` | **默认常驻服务**，最小运维成本 |
| `resident_partitioned_sqlite` | 主 SQLite 控制库 | 每仓库 SQLite shard | 大仓库或多仓库本地扩展 |
| `split_worker_preview` | 常驻控制服务 | 独立 worker 进程 | 未来进程级扩展（预览） |

默认使用 `resident_single_process`。本章重点覆盖 `resident_single_process` 和 `resident_partitioned_sqlite` 的完整部署流程。

### 14.1.2 关键原则

- **长期后台运行必须由 platform service manager 托管**（systemd / launchd / Windows Service）。`run.sh --daemon` 仅用于开发验证，不得用于生产部署。
- **二进制安装路径与运行时状态严格分离**。配置、数据库、索引、日志、缓存、临时文件使用 `paths` 模块管理的平台目录。
- **远端 CLI 不能执行维护操作**。`repo index --reset`、`repo index-worker`、split worker attempt、shard repair、backup、migration、rollback 和 uninstall 必须在服务宿主机上执行。
- **非 loopback HTTP bind 必须显式启用远端客户端策略、scope/origin 限制、QoS budget 和审计**。

---

## 14.2 安装准备

### 14.2.1 系统要求

- **操作系统**：Linux（glibc ≥ 2.28）、macOS（Apple Silicon / Intel）、Windows（x86_64 / ARM64）
- **无需另行管理应用运行时**：relay-knowledge 为单二进制，SQLite 已内置（bundled + FTS5）；仍依赖目标平台的系统库和 service manager
- **磁盘空间**：二进制约 50 MB，运行时数据取决于知识图谱和代码仓库规模，建议预留 ≥ 2 GB
- **内存**：建议 ≥ 512 MB（含 worker pool、HTTP 服务、SQLite 缓存）

### 14.2.2 获取二进制

**方式一：从 GitHub Releases 下载**

下面是 Linux x64 的完整示例。版本 tag、archive 文件名和解压后的顶层目录必须使用同一个
`vX.Y.Z`；ARM64 主机把 `TARGET` 改为 `aarch64-unknown-linux-gnu`。

```bash
set -euo pipefail

RELEASE_TAG=v1.1.13
TARGET=x86_64-unknown-linux-gnu
PACKAGE_DIR="relay-knowledge-${RELEASE_TAG}-${TARGET}"
ARCHIVE="${PACKAGE_DIR}.tar.gz"
RELEASE_URL="https://github.com/coolplayagent/relay-knowledge/releases/download/${RELEASE_TAG}"
DOWNLOAD_DIR="$PWD/relay-knowledge-download-${RELEASE_TAG}"

mkdir "$DOWNLOAD_DIR"
cd "$DOWNLOAD_DIR"

curl --fail --location --remote-name "${RELEASE_URL}/${ARCHIVE}"
curl --fail --location --remote-name "${RELEASE_URL}/checksums.txt"

# checksums.txt 还包含其他平台产物；只提取当前 archive 的唯一记录。
awk -v artifact="$ARCHIVE" \
  '$2 == artifact { print; matches += 1 } END { if (matches != 1) exit 1 }' \
  checksums.txt > "$ARCHIVE.sha256"
sha256sum --check "$ARCHIVE.sha256"

tar --extract --gzip --file "$ARCHIVE"
sudo install --owner=root --group=root --mode=0755 \
  "$PACKAGE_DIR/relay-knowledge" /usr/local/bin/relay-knowledge
```

`sha256sum` 在没有匹配记录、摘要不匹配或 archive 缺失时都必须失败；不得跳过校验后直接解压。
macOS 和 Windows archive 使用同格式的顶层目录，target 分别为
`x86_64-apple-darwin` / `aarch64-apple-darwin` 和
`x86_64-pc-windows-msvc` / `aarch64-pc-windows-msvc`；Windows archive 扩展名为 `.zip`。

**方式二：通过 crates.io 安装**

```bash
cargo install relay-knowledge
```

**验证安装**：

```bash
relay-knowledge --version
relay-knowledge status --format json
relay-knowledge setup doctor --format json
```

### 14.2.3 安装后预检

```bash
relay-knowledge setup doctor --format json
```

`setup doctor` 检查内容：
- 运行时目录可访问性和写权限
- 配置文件、数据目录、日志目录的路径解析结果
- HTTP 绑定地址、QoS 策略、代理配置的默认值
- 存储拓扑的默认设置

`setup doctor` 不访问数据库，纯静态诊断。如需检查 live 存储和索引状态，使用 `relay-knowledge service doctor --format json`。

---

## 14.3 首次部署（Linux systemd）

### 14.3.1 部署预检与配置

```bash
# 1. 静态预检
relay-knowledge setup doctor --format json

# 2. 查看 service profile 推荐配置
relay-knowledge setup profile service --format json

# 3. 预览安装计划
relay-knowledge service plan install \
  --install-dir "$HOME/.local/lib/relay-knowledge" --format json
relay-knowledge service lifecycle install --dry-run \
  --install-dir "$HOME/.local/lib/relay-knowledge" --format json
```

`service plan install --format json` 与 `service lifecycle install --dry-run --format json` 输出关键字段：

| 字段 | 说明 |
|------|------|
| `platform` | `linux` / `macos` / `windows` |
| `definition_path` | 平台 service definition 文件的写入路径 |
| `binary_path` | lifecycle 将使用或复制到安装目录的二进制路径 |
| `install_command` | 平台服务安装命令预览 |
| `start_command` | 平台服务启动命令预览 |
| `stop_command` | 平台服务停止命令预览 |
| `uninstall_command` | 平台服务卸载命令预览 |
| `lifecycle_steps` | 安装、升级、回滚或卸载的阶段化步骤 |
| `rollback_steps` | lifecycle 执行失败后要尝试的回滚步骤 |
| `permission_requirements` | systemd、launchd 或 Windows Service 的权限要求 |
| `package_manifest_checks` | 包管理器 manifest 与同一 release tag/checksum 的校验要求 |
| `runtime_state_paths` | 数据库、配置、状态、日志、缓存、shard 目录等路径 |
| `checkpoint_path` | lifecycle checkpoint 文件路径 |
| `warnings` | 备份/迁移/shard/卸载等操作提醒 |
| `checksum` | service definition 的稳定校验值 |

### 14.3.2 确认 service definition 输入

写入前请确认以下配置已固定：

```bash
# 用户级 systemd 服务使用当前用户可写的稳定路径。
export RELAY_KNOWLEDGE_DATA_DIR="$HOME/.local/share/relay-knowledge"
export RELAY_KNOWLEDGE_STATE_DIR="$HOME/.local/state/relay-knowledge"
export RELAY_KNOWLEDGE_LOG_DIR="$HOME/.local/state/relay-knowledge/logs"
export RELAY_KNOWLEDGE_INSTALL_DIR="$HOME/.local/lib/relay-knowledge"
```

先用 `service lifecycle install --dry-run` 审核完整 plan，不要在 install lifecycle 之前单独执行
`service definition write`；否则 fresh-install 的“不覆盖已有 definition”预检会拒绝继续。
`--execute` 会按已审核的 plan 复制二进制、写入 `relay-knowledge.service`、重载用户级
systemd、注册并启动服务。
生成的 Linux unit 会引用包含空格的路径，并把字面 `$` 写成 `$$`，避免 systemd 在 `ExecStart=` 或 `Environment=` 中误展开安装目录、二进制路径或数据目录。

### 14.3.3 前台验证

在安装为系统服务前，建议先前台验证：

```bash
# MCP Streamable HTTP 前台运行
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --mcp streamable-http

# 同时启动 Web + MCP + 文件监听
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
  RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  RELAY_KNOWLEDGE_WATCHER_ENABLED=true \
  relay-knowledge service run --web --mcp streamable-http
```

在另一个终端验证：

```bash
curl http://127.0.0.1:8791/api/health
relay-knowledge service status --format json
```

启动时输出格式为：

```text
relay-knowledge service running; code_index_workers=N
```

`Ctrl+C` 或 `SIGTERM` 可停止前台进程。

### 14.3.4 安装并启动 systemd 服务

Linux lifecycle 当前管理的是 **systemd user service**。用部署用户执行：

```bash
relay-knowledge service lifecycle install --dry-run \
  --install-dir "$RELAY_KNOWLEDGE_INSTALL_DIR" --format json
relay-knowledge service lifecycle install --execute \
  --install-dir "$RELAY_KNOWLEDGE_INSTALL_DIR" --format json
systemctl --user status relay-knowledge.service
journalctl --user -u relay-knowledge.service -n 100 --no-pager
```

如果服务需要在用户未登录时运行，由管理员启用 linger：

```bash
sudo loginctl enable-linger "$USER"
```

当前 plan 的 `permission_requirements` 和平台命令固定为 `systemctl --user`。不要只把
`RELAY_KNOWLEDGE_SERVICE_DIR` 改为 `/etc/systemd/system` 就宣称完成 system service 安装；该做法不会把
lifecycle 命令切换为系统级 systemd manager。

### 14.3.5 验证服务状态

```bash
# CLI 诊断
relay-knowledge service status --format json
relay-knowledge service doctor --format json
relay-knowledge health --format json

# HTTP 诊断
curl http://127.0.0.1:8791/api/health
curl http://127.0.0.1:8791/api/v1/control/service/status
curl http://127.0.0.1:8791/api/v1/control/storage/topology
```

`service status` 返回 code-index worker、operator、storage topology、queue/dead-letter、runtime path 和 degraded reason。这些诊断接口有短预算，不会同步执行大型索引。

### 14.3.6 停止与重启

```bash
# 停止服务
systemctl --user stop relay-knowledge.service

# 重启服务
systemctl --user restart relay-knowledge.service
```

---

## 14.4 首次部署（macOS launchd）

### 14.4.1 部署预检

```bash
relay-knowledge setup doctor --format json
relay-knowledge service plan install \
  --install-dir "$HOME/Library/Application Support/relay-knowledge/bin" --format json
relay-knowledge service lifecycle install --dry-run \
  --install-dir "$HOME/Library/Application Support/relay-knowledge/bin" --format json
```

生成的 launchd plist 文件名为 `com.coolplayagent.relay-knowledge.plist`，写入在 `service_dir` 指定的路径下（macOS 默认 `~/Library/LaunchAgents/`，以便 login 时由 launchd 重新加载）。

### 14.4.2 前台验证

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
  RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --web --mcp streamable-http
```

### 14.4.3 安装并启动 launchd 服务

```bash
relay-knowledge service lifecycle install --execute \
  --install-dir "$HOME/Library/Application Support/relay-knowledge/bin" --format json
launchctl print "gui/${UID}/com.coolplayagent.relay-knowledge"
```

### 14.4.4 验证服务状态

```bash
curl http://127.0.0.1:8791/api/health
relay-knowledge service doctor --format json
```

### 14.4.5 停止与卸载

```bash
relay-knowledge service lifecycle uninstall --dry-run --format json
relay-knowledge service lifecycle uninstall --execute --format json
```

---

## 14.5 首次部署（Windows Service）

### 14.5.1 部署预检

```powershell
relay-knowledge setup doctor --format json
$InstallDir = Join-Path $env:LOCALAPPDATA 'relay-knowledge\bin'
relay-knowledge service plan install --install-dir $InstallDir --format json
relay-knowledge service lifecycle install --dry-run --install-dir $InstallDir --format json
```

生成的 Windows Service definition 文件名为 `relay-knowledge-service.xml`。

### 14.5.2 前台验证

```powershell
$env:RELAY_KNOWLEDGE_HTTP_BIND = '127.0.0.1:8791'
$env:RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES = 'docs'
relay-knowledge service run --web --mcp streamable-http
```

### 14.5.3 安装并启动 Windows Service

以管理员权限打开 PowerShell：

```powershell
$ErrorActionPreference = 'Stop'
$InstallDir = Join-Path $env:LOCALAPPDATA 'relay-knowledge\bin'
relay-knowledge service lifecycle install --dry-run --install-dir $InstallDir --format json
relay-knowledge service lifecycle install --execute --install-dir $InstallDir --format json
Get-Service relay-knowledge
```

### 14.5.4 验证服务状态

```powershell
Invoke-RestMethod http://127.0.0.1:8791/api/health
relay-knowledge service doctor --format json
```

### 14.5.5 停止与卸载

```powershell
relay-knowledge service lifecycle uninstall --dry-run --format json
relay-knowledge service lifecycle uninstall --execute --format json
```

---

## 14.6 存储拓扑选择

### 14.6.1 选型指南

| 场景 | 推荐拓扑 | 配置 |
|------|----------|------|
| 个人使用、少量仓库、开发测试 | `single_sqlite` | 不设置或 `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=single_sqlite` |
| 大仓库（代码量 >1 GB）、多仓库管理 | `partitioned_sqlite` | `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite` |

`single_sqlite` 使用单个 SQLite 数据库存储所有图事实、索引和代码仓库数据。

`partitioned_sqlite` 使用一个主 SQLite 控制库管理仓库注册和任务状态，每仓库独立 SQLite shard 存储代码事实和索引。shard 目录位于运行时数据目录的 `stores/repositories/` 下。

### 14.6.2 配置方法

```bash
export RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite
```

该环境变量必须在预检、生成 definition、启动服务和所有运维命令中保持一致。切换拓扑前必须先完成显式迁移或回滚。

### 14.6.3 重要约束

- 主数据库一旦包含 active shard catalog，**不能直接用 `single_sqlite` 打开同一运行时状态**。
- 备份、迁移、doctor、卸载确认和回滚计划**必须同时覆盖主数据库和 shard 目录**。只移动或校验主数据库不能宣称操作成功。
- 任一仓库最多一个 active writer task；跨进程或跨后端部署由 durable lease 保护。

---

## 14.7 HTTP 与 MCP 配置

### 14.7.1 HTTP 绑定配置

| 环境变量 | 默认值 | 说明 |
|----------|--------|------|
| `RELAY_KNOWLEDGE_HTTP_BIND` | `127.0.0.1:8791` | HTTP 服务监听地址和端口 |
| `RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS` | `30000` | 单请求超时（毫秒） |
| `RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS` | `10000` | 优雅关闭超时（毫秒） |
| `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` | `1048576` | 请求体最大字节数（默认 1 MiB） |

默认只绑定 `127.0.0.1`（loopback），仅本机可访问。

### 14.7.2 QoS 预算配置

| 环境变量 | 默认值 | 说明 |
|----------|--------|------|
| `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` | `1024` | 最大并发连接数 |
| `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` | `256` | 最大并发请求数 |
| `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` | `512` | 最大排队请求数 |

### 14.7.3 MCP 配置

| 环境变量 | 说明 |
|----------|------|
| `RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED` | 启用 MCP Streamable HTTP 协议 |
| `RELAY_KNOWLEDGE_MCP_ENDPOINT` | MCP 端点路径 |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS` | 允许的来源域名 |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` | 允许的访问 scope（如 `docs`） |
| `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE` | 是否允许未指定 scope（`true`/`false`） |
| `RELAY_KNOWLEDGE_MCP_MAX_LIMIT` | MCP 查询最大返回条数 |
| `RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES` | MCP 上下文最大字节数 |
| `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS` | 是否允许非 loopback 远端客户端 |

### 14.7.4 远端访问配置

当前 Web、HTTP API（包括 `/api/v1/control/**`）和 MCP 不内建入站调用方身份认证。
`ALLOW_REMOTE_CLIENTS`、Origin、scope、session、QoS 和审计都不能证明调用方身份。没有外部身份网关时，
必须保持 loopback：

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
  RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --web --mcp streamable-http
```

远端访问应让 Relay 继续绑定 loopback，由同机外部身份网关执行 OIDC/token 校验与
deny-by-default ACL，或校验 mTLS 客户端证书、映射身份并执行同等 ACL。网关必须保护 Web、
`/api/**` 和 `/mcp` 的所有路径；仅有 TLS 服务端证书只加密传输，不认证调用方。跨机网关的
专用私网绑定、防火墙和非 loopback 暴露前提见[第 16 章](16-security-configuration-guide.md#163-远端访问安全)。

### 14.7.5 远端 CLI 使用

远端 CLI 通过统一 HTTP 服务访问 code repository API：

```bash
# 使用已认证、已授权的 HTTPS 网关，不直连 Relay 后端端口。
relay-knowledge --remote https://knowledge.example.com repo status my-repo --format json
relay-knowledge --remote https://knowledge.example.com repo query my-repo "service startup" --format json

# 或设置环境变量
export RELAY_KNOWLEDGE_REMOTE_BASE_URL=https://knowledge.example.com
relay-knowledge repo status my-repo --format json
```

远端模式支持的操作：
- repository index、scope preview、status、query
- feature-flags、impact、report、software projection

**远端 CLI 限制**：以下操作只能在服务宿主机执行，**不能通过远端 CLI 调用**：
- `repo index --reset`
- `repo index-worker`
- split worker attempt
- shard repair
- backup
- migration
- rollback
- uninstall

---

## 14.8 升级流程

### 14.8.1 升级顺序

```text
preflight doctor
  → 停止 ad hoc CLI writer
  → stop service through platform manager
  → 确认所有 writer 已停止
  → 按第 15 章创建停服一致的 runtime backup
  → 从已校验、已解压的新版本二进制执行 lifecycle upgrade
  → start service through platform manager
     → 首次同步打开执行 schema/index migration 与必要的 shadow rebuild
     → 打开完成后 service 才可用
  → post-upgrade doctor
```

本章选择 **lifecycle upgrade** 作为唯一二进制与 service definition 升级路径。不要再手工覆盖
binary 或单独重写 definition；否则会绕过 lifecycle 的 attempt-scoped 文件 checkpoint 和失败回滚。

### 14.8.2 停服备份边界

在执行任何 `service lifecycle upgrade --execute` 之前，完整执行
[第 15 章 15.6 节的停服一致备份流程](15-sre-operations-runbook.md#156-备份与恢复)。该流程是 runtime backup/restore
的唯一权威步骤，必须覆盖 plan 的全部 `runtime_state_paths`。`partitioned_sqlite` 必须把控制库、
仍存在的 WAL/SHM 伴生文件和 `stores/repositories/` shards 作为同一个停服快照处理。

Lifecycle checkpoint 只保护安装的 binary 和 service definition；它不是 SQLite 快照，不覆盖 runtime
state，也不能证明非托管 writer 已停止。

### 14.8.3 Linux systemd 升级

```bash
TARGET_VERSION=1.1.13
NEW_BINARY="$PWD/relay-knowledge-v1.1.13-x86_64-unknown-linux-gnu/relay-knowledge"
INSTALL_DIR="$HOME/.local/lib/relay-knowledge"

test -x "$NEW_BINARY"
"$NEW_BINARY" setup doctor --format json
"$NEW_BINARY" service plan upgrade --target-version "$TARGET_VERSION" \
  --install-dir "$INSTALL_DIR" --format json
"$NEW_BINARY" service lifecycle upgrade --dry-run --target-version "$TARGET_VERSION" \
  --install-dir "$INSTALL_DIR" --format json
"$NEW_BINARY" service lifecycle upgrade --execute --target-version "$TARGET_VERSION" \
  --install-dir "$INSTALL_DIR" --format json
"$INSTALL_DIR/relay-knowledge" service doctor --format json
curl http://127.0.0.1:8791/api/health
```

上述 `NEW_BINARY` 必须来自 14.2.2 已通过 checksum 的 archive，`INSTALL_DIR` 必须与首次安装时一致。

### 14.8.4 macOS launchd 升级

```bash
TARGET_VERSION=1.1.13
NEW_BINARY="$PWD/relay-knowledge-v1.1.13-aarch64-apple-darwin/relay-knowledge"
INSTALL_DIR="$HOME/Library/Application Support/relay-knowledge/bin"

test -x "$NEW_BINARY"
"$NEW_BINARY" setup doctor --format json
"$NEW_BINARY" service lifecycle upgrade --dry-run --target-version "$TARGET_VERSION" \
  --install-dir "$INSTALL_DIR" --format json
"$NEW_BINARY" service lifecycle upgrade --execute --target-version "$TARGET_VERSION" \
  --install-dir "$INSTALL_DIR" --format json
"$INSTALL_DIR/relay-knowledge" service doctor --format json
launchctl print "gui/${UID}/com.coolplayagent.relay-knowledge"
```

Intel Mac 把 archive target 改为 `x86_64-apple-darwin`。

### 14.8.5 Windows Service 升级

以管理员权限执行：

```powershell
$ErrorActionPreference = 'Stop'
$TargetVersion = '1.1.13'
$NewBinary = Join-Path (Get-Location) 'relay-knowledge-v1.1.13-x86_64-pc-windows-msvc\relay-knowledge.exe'
$InstallDir = Join-Path $env:LOCALAPPDATA 'relay-knowledge\bin'

if (-not (Test-Path -LiteralPath $NewBinary -PathType Leaf)) { throw "Missing release binary: $NewBinary" }
& $NewBinary setup doctor --format json
& $NewBinary service lifecycle upgrade --dry-run --target-version $TargetVersion --install-dir $InstallDir --format json
& $NewBinary service lifecycle upgrade --execute --target-version $TargetVersion --install-dir $InstallDir --format json
& (Join-Path $InstallDir 'relay-knowledge.exe') service doctor --format json
Get-Service relay-knowledge
```

完成前应保存 dry-run 和 execute JSON 报告，其中的 `rollback_steps`、`checkpoint_path`、
`package_manifest_checks` 和失败 step id 是后续审计与回滚的输入。

---

## 14.9 回滚操作

### 14.9.1 回滚原则

- Lifecycle rollback 只恢复 checkpointed **binary** 和 **service definition**，然后刷新平台注册并启动服务。
- 需要恢复 schema/data 时，必须另外使用第 15 章创建的停服一致 runtime backup；`checkpoint_path` 绝不是数据库回滚点。
- upgrade checkpoint backup 使用 attempt-scoped 文件并原子发布 checkpoint；没有旧二进制或 service definition 备份时，失败回滚和显式 rollback 只删除本次确实复制或写入的目标文件，definition-only upgrade 不会删除当前运行的 binary。
- uninstall 失败回滚和基于 uninstall checkpoint 的显式 rollback 会恢复被本次卸载删除的原 service definition，再重新注册 service。
- Windows install 将 service 创建和 registry 环境写入拆成独立步骤；Windows/macOS upgrade 会在启动前刷新 SCM 或 launchd registration，使平台 service manager 使用更新后的 service definition。
- restore、definition rewrite、unregister 或 service-registration rollback step 失败后，不会继续执行依赖的删除、reload 或 start 步骤。
- 外部 service manager 和 doctor 子进程退出或超时后，stdout/stderr 收集也有边界，继承管道的 helper 不会让执行报告无限等待。
- forward-only migration 必须在变更说明中写清楚，不能只替换旧二进制后宣称回滚完成。

### 14.9.2 回滚步骤

先查阅发布说明，确认迁移是否 forward-only。如果需要恢复 runtime state，先按
[第 15 章 15.6 节](15-sre-operations-runbook.md#156-备份与恢复)的恢复流程停服、验证备份，并使用其强制
`--defer-start` 模式恢复全部路径。该模式不运行当前 binary 或 health check；在 user service 仍保持
stopped 时再执行 lifecycle rollback，由 lifecycle 恢复旧 binary/definition、刷新注册、启动并进入验证。
Linux 用户级安装示例：

```bash
INSTALL_DIR="$HOME/.local/lib/relay-knowledge"
relay-knowledge service lifecycle rollback --dry-run \
  --install-dir "$INSTALL_DIR" --format json
relay-knowledge service lifecycle rollback --execute \
  --install-dir "$INSTALL_DIR" --format json
"$INSTALL_DIR/relay-knowledge" service doctor --format json
```

没有有效 lifecycle checkpoint 或 runtime backup 时，必须报告缺口，不能以手工替换旧 binary 的方式宣称完整回滚。

---

## 14.10 卸载

### 14.10.1 卸载原则

- 当前 lifecycle uninstall 移除 platform service registration 和 service definition，保留安装的 binary 与全部 runtime state。
- 删除配置、数据库、索引、日志、缓存、worker queue、dead-letter 或 shard 目录**必须经过用户确认**。
- `partitioned_sqlite` 下卸载确认同时覆盖主库和 shard 目录。

### 14.10.2 查看卸载计划

```bash
relay-knowledge service plan uninstall --format json
```

输出包含：
- `runtime_state_paths`：将被保留的运行时数据路径
- `uninstall_command`：平台卸载命令预览
- `warnings`：关于数据保留的提醒

### 14.10.3 Linux systemd 卸载

```bash
relay-knowledge service lifecycle uninstall --dry-run --format json
relay-knowledge service lifecycle uninstall --execute --format json
```

如果 14.2.2 的系统级 CLI 也确定不再需要，可在确认 lifecycle 已完成且目标是预期文件后，单独删除
`/usr/local/bin/relay-knowledge`。本章不提供递归删除 runtime 目录的命令。

### 14.10.4 macOS launchd 卸载

```bash
relay-knowledge service lifecycle uninstall --dry-run --format json
relay-knowledge service lifecycle uninstall --execute --format json
```

### 14.10.5 Windows Service 卸载

以管理员权限执行：

```powershell
relay-knowledge service lifecycle uninstall --dry-run --format json
relay-knowledge service lifecycle uninstall --execute --format json
```

如需永久删除 runtime data，先完成第 15 章备份，再逐项核对 uninstall plan 的
`runtime_state_paths`。只删除已核对的具体路径；不要对环境变量、通配符或平台运行时根目录执行递归删除。

---

## 14.11 code-index worker 配置

服务启动时会启动 code-index worker pool。worker 数量由环境变量控制：

```bash
export RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT=8
```

worker pool 特性：
- 每个 worker 通过 durable task lease 保护写入
- 启动时自动恢复 orphaned code-index task lease
- 支持 attempt-scoped lease、retry backoff、dead-letter 队列
- 任一仓库最多一个 active writer task

查看 worker 状态：

```bash
relay-knowledge service status --format json
# 包含 configured workers、active slots、queue depth、running leases、retry/dead-letter state
```

---

## 14.12 Operator 管理

Silent updates operator 用于控制后台自动刷新行为：

```bash
# 查看 operator 状态
relay-knowledge service operator status --format json

# 暂停静默更新
relay-knowledge service operator pause --format json

# 恢复静默更新
relay-knowledge service operator resume --format json
```

Silent updates 约束：
- 用户可配置、可观测、可逆
- 只能在授权 scope 内刷新图数据和派生索引
- 必须暴露 freshness、stale、paused、degraded 和 failure 状态

---

## 14.13 诊断与排障

### 14.13.1 常规诊断顺序

```bash
# 1. 运行时状态
relay-knowledge status --format json

# 2. 静态预检
relay-knowledge setup doctor --format json

# 3. 实时健康
relay-knowledge health --format json

# 4. 服务诊断
relay-knowledge service doctor --format json

# 5. 审计日志
relay-knowledge audit query --limit 50 --format json
```

### 14.13.2 HTTP 诊断端点

| 端点 | 说明 |
|------|------|
| `GET /api/health` | 服务健康检查 |
| `GET /api/service/status` | 服务状态快照 |
| `GET /api/project/status` | 项目身份信息 |
| `GET /api/v1/control/status` | 控制面状态 |
| `GET /api/v1/control/health` | 控制面健康检查 |
| `GET /api/v1/control/service/status` | 控制面服务状态 |
| `GET /api/v1/control/storage/topology` | 存储拓扑诊断 |

### 14.13.3 常见问题

| 问题 | 排查步骤 |
|------|----------|
| 服务启动后 Web 无法访问 | 检查 `RELAY_KNOWLEDGE_HTTP_BIND`、systemd/launchd/Windows Service 状态和日志 |
| 非 loopback bind 被拒绝 | 设置 `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`，补齐 origin/scope 限制 |
| `single_sqlite` 打不开 runtime | 检查是否残留 active `partitioned_sqlite` shard catalog，按回滚计划处理 |
| `repo status` 长时间显示 running | 查看 active task lease、checkpoint 和 dead-letter；不要杀进程或绕过 lease |
| `health` 返回 `storage_busy` | 短预算诊断降级，不代表服务不可用；继续查看 service status、index lag 和 queue depth |
| code-index worker 持续 retry | 查看 dead-letter 队列，检查 lease 过期和 attempt 匹配状态 |

更多排障步骤见 [第 13 章运维与排障](13-operations-and-troubleshooting.md)。

---

## 14.14 环境变量速查表

### 路径类

| 变量 | 说明 |
|------|------|
| `RELAY_KNOWLEDGE_HOME` | 统一运行时根目录（覆盖所有平台默认路径） |
| `RELAY_KNOWLEDGE_CONFIG_DIR` | 配置目录 |
| `RELAY_KNOWLEDGE_DATA_DIR` | 数据目录（SQLite 数据库） |
| `RELAY_KNOWLEDGE_STATE_DIR` | 状态目录 |
| `RELAY_KNOWLEDGE_CACHE_DIR` | 缓存目录 |
| `RELAY_KNOWLEDGE_LOG_DIR` | 日志目录 |
| `RELAY_KNOWLEDGE_TEMP_DIR` | 临时文件目录 |
| `RELAY_KNOWLEDGE_RUNTIME_DIR` | 运行时目录 |
| `RELAY_KNOWLEDGE_SERVICE_DIR` | service definition 写入目录 |

### 网络类

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RELAY_KNOWLEDGE_HTTP_BIND` | `127.0.0.1:8791` | HTTP 监听地址 |
| `RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS` | `30000` | 请求超时（ms） |
| `RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS` | `10000` | 关闭超时（ms） |
| `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` | `1048576` | 请求体上限 |
| `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` | `1024` | 最大连接数 |
| `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` | `256` | 最大并发请求 |
| `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` | `512` | 最大排队深度 |
| `HTTPS_PROXY` / `https_proxy` | — | HTTPS 代理 |
| `HTTP_PROXY` / `http_proxy` | — | HTTP 代理 |
| `NO_PROXY` / `no_proxy` | — | 代理排除列表 |

### MCP / Agent 类

| 变量 | 说明 |
|------|------|
| `RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED` | 启用 MCP Streamable HTTP |
| `RELAY_KNOWLEDGE_MCP_ENDPOINT` | MCP 端点路径 |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS` | CORS 允许的来源 |
| `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` | 允许的 scope |
| `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE` | 是否允许无 scope |
| `RELAY_KNOWLEDGE_MCP_MAX_LIMIT` | 查询最大返回数 |
| `RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES` | 上下文最大字节 |
| `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS` | 允许远端客户端 |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED` | 启用审计日志 |
| `RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH` | 审计队列深度 |

### 存储与 Worker 类

| 变量 | 说明 |
|------|------|
| `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY` | `single_sqlite` 或 `partitioned_sqlite` |
| `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` | code-index worker 并发数（上限 8） |
| `RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED` | 启用静默更新 |
| `RELAY_KNOWLEDGE_REMOTE_BASE_URL` | 远端 CLI 目标地址 |

### 平台默认路径

**Linux**：

| 路径 | 默认值 |
|------|--------|
| config | `~/.config/relay-knowledge` |
| data | `~/.local/share/relay-knowledge` |
| state | `~/.local/state/relay-knowledge` |
| cache | `~/.cache/relay-knowledge` |
| logs | `~/.local/state/relay-knowledge/logs` |
| service | `~/.config/relay-knowledge/service` |

**macOS**：

| 路径 | 默认值 |
|------|--------|
| config | `~/Library/Application Support/relay-knowledge/config` |
| data | `~/Library/Application Support/relay-knowledge/data` |
| cache | `~/Library/Caches/relay-knowledge` |
| logs | `~/Library/Logs/relay-knowledge` |
| service | `~/Library/LaunchAgents` |

**Windows**：

| 路径 | 默认值 |
|------|--------|
| config | `%APPDATA%/relay-knowledge` |
| data | `%LOCALAPPDATA%/relay-knowledge/data` |
| state | `%LOCALAPPDATA%/relay-knowledge/state` |
| cache | `%LOCALAPPDATA%/relay-knowledge/cache` |
| logs | `%LOCALAPPDATA%/relay-knowledge/logs` |
| service | `%APPDATA%/relay-knowledge/service` |

---

## 14.15 命令速查

| 命令 | 用途 |
|------|------|
| `relay-knowledge setup doctor --format json` | 部署前预检 |
| `relay-knowledge setup profile service --format json` | 查看 service 推荐配置 |
| `relay-knowledge service plan install --format json` | 生成安装计划 |
| `relay-knowledge service plan upgrade --target-version 1.1.13 --install-dir "$HOME/.local/lib/relay-knowledge" --format json` | 生成升级计划（Linux 用户级示例） |
| `relay-knowledge service lifecycle install --dry-run --format json` | dry-run 安装 lifecycle |
| `relay-knowledge service lifecycle upgrade --execute --target-version 1.1.13 --install-dir "$HOME/.local/lib/relay-knowledge" --format json` | 显式执行升级 lifecycle（Linux 用户级示例） |
| `relay-knowledge service lifecycle rollback --dry-run --install-dir "$HOME/.local/lib/relay-knowledge" --format json` | 预览 rollback lifecycle（Linux 用户级示例） |
| `relay-knowledge service plan uninstall --format json` | 生成卸载计划 |
| `relay-knowledge service lifecycle uninstall --dry-run --format json` | dry-run 卸载 lifecycle |
| `relay-knowledge service definition write --format json` | 写入平台 service definition |
| `relay-knowledge service run --web --mcp streamable-http` | 前台运行完整服务 |
| `relay-knowledge service run --mcp streamable-http` | 前台运行 MCP 服务 |
| `relay-knowledge service status --format json` | 服务状态 |
| `relay-knowledge service doctor --format json` | 服务诊断（同 service status） |
| `relay-knowledge service operator status --format json` | 查看 operator 状态 |
| `relay-knowledge service operator pause --format json` | 暂停静默更新 |
| `relay-knowledge service operator resume --format json` | 恢复静默更新 |
| `relay-knowledge service worker run --format json` | worker 运行单一 task |
| `relay-knowledge health --format json` | 健康检查 |
| `relay-knowledge status --format json` | 运行时状态 |
| `relay-knowledge audit query --limit 50 --format json` | 查看审计日志 |
| `relay-knowledge --remote http://127.0.0.1:8791 repo status my-repo` | 远端 repo 状态示例 |

---

**导航**：上一章：[第 13 章 运维与排障](13-operations-and-troubleshooting.md) | 下一章：[第 15 章 SRE 运维手册](15-sre-operations-runbook.md) | 返回：[用户指南](README.md)
