# 第 15 章 完整服务化部署指南

[中文](../../zh/01-user-guide/15-service-deployment-full-guide.md)

> 本章为 relay-knowledge 服务化部署的完整操作手册，覆盖从安装准备到升级、回滚、卸载的全生命周期。
> 架构契约参见 [第 19 章安装、发布与升级](../03-architecture-specs/19-installation-release-and-upgrade.md) 和 [第 22 章服务化部署、控制面与数据面分离](../03-architecture-specs/22-service-deployment-control-data-plane.md)。
> 常驻服务快速入门可先阅读 [第 9 章服务化部署与常驻服务](09-resident-service.md)，本章在此基础上提供更详尽的平台特定步骤和运维操作细节。

---

## 15.1 概述

relay-knowledge 的常驻服务是一个异步优先、有界资源的后台进程，托管 Web HTTP API、MCP Streamable HTTP 协议、startup index reconciler、code-index worker pool 和 repository-set refresh worker。

### 15.1.1 四种部署拓扑

| 拓扑 | 控制面 | 数据面 | 适用场景 |
|------|--------|--------|----------|
| `embedded_cli` | CLI 进程内 | `single_sqlite` | 临时命令、测试、一次性操作 |
| `resident_single_process` | `service run` HTTP/Web/MCP + worker pool | `single_sqlite` | **默认常驻服务**，最小运维成本 |
| `resident_partitioned_sqlite` | 主 SQLite 控制库 | 每仓库 SQLite shard | 大仓库或多仓库本地扩展 |
| `split_worker_preview` | 常驻控制服务 | 独立 worker 进程 | 未来进程级扩展（预览） |

默认使用 `resident_single_process`。本章重点覆盖 `resident_single_process` 和 `resident_partitioned_sqlite` 的完整部署流程。

### 15.1.2 关键原则

- **长期后台运行必须由 platform service manager 托管**（systemd / launchd / Windows Service）。`run.sh --daemon` 仅用于开发验证，不得用于生产部署。
- **二进制安装路径与运行时状态严格分离**。配置、数据库、索引、日志、缓存、临时文件使用 `paths` 模块管理的平台目录。
- **远端 CLI 不能执行维护操作**。`repo index --reset`、`repo index-worker`、split worker attempt、shard repair、backup、migration、rollback 和 uninstall 必须在服务宿主机上执行。
- **非 loopback HTTP bind 必须显式启用远端客户端策略、scope/origin 限制、QoS budget 和审计**。

---

## 15.2 安装准备

### 15.2.1 系统要求

- **操作系统**：Linux（glibc ≥ 2.31）、macOS（Apple Silicon / Intel）、Windows（x86_64）
- **无外部运行时依赖**：relay-knowledge 为单二进制，SQLite 已内置（bundled + FTS5），无需额外安装数据库或运行时
- **磁盘空间**：二进制约 50 MB，运行时数据取决于知识图谱和代码仓库规模，建议预留 ≥ 2 GB
- **内存**：建议 ≥ 512 MB（含 worker pool、HTTP 服务、SQLite 缓存）

### 15.2.2 获取二进制

**方式一：从 GitHub Releases 下载**

```bash
# 从 GitHub Releases 下载对应平台的压缩包
# 发布地址：https://github.com/coolplayagent/relay-knowledge/releases
# 解压后将 relay-knowledge 复制到 PATH 目录
tar -xzf relay-knowledge-linux-x86_64.tar.gz
sudo cp relay-knowledge /usr/local/bin/
chmod +x /usr/local/bin/relay-knowledge
```

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

### 15.2.3 安装后预检

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

## 15.3 首次部署（Linux systemd）

### 15.3.1 部署预检与配置

```bash
# 1. 静态预检
relay-knowledge setup doctor --format json

# 2. 查看 service profile 推荐配置
relay-knowledge setup profile service --format json

# 3. 预览安装计划
relay-knowledge service plan install --format json
```

`service plan install --format json` 输出关键字段：

| 字段 | 说明 |
|------|------|
| `platform` | `linux` / `macos` / `windows` |
| `definition_path` | 平台 service definition 文件的写入路径 |
| `install_command` | 平台服务安装命令预览 |
| `start_command` | 平台服务启动命令预览 |
| `stop_command` | 平台服务停止命令预览 |
| `uninstall_command` | 平台服务卸载命令预览 |
| `runtime_state_paths` | 数据库、配置、状态、日志、缓存、shard 目录等路径 |
| `warnings` | 备份/迁移/shard/卸载等操作提醒 |
| `checksum` | service definition 的稳定校验值 |

### 15.3.2 写入 service definition

写入前请确认以下配置已固定：

```bash
# 建议显式设置运行时目录（使用绝对路径，不指向 release 解压目录）
export RELAY_KNOWLEDGE_DATA_DIR=/var/lib/relay-knowledge/data
export RELAY_KNOWLEDGE_STATE_DIR=/var/lib/relay-knowledge/state
export RELAY_KNOWLEDGE_LOG_DIR=/var/log/relay-knowledge
```

写入 service definition：

```bash
relay-knowledge service definition write --format json
```

这将生成 systemd service 文件 `relay-knowledge.service`，写入到平台约定的 service 目录。

### 15.3.3 前台验证

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

```
relay-knowledge service running; code_index_workers=N
```

`Ctrl+C` 或 `SIGTERM` 可停止前台进程。

### 15.3.4 安装并启动 systemd 服务

**用户级服务（user service）**：

```bash
# 1. 重新加载 systemd 配置
systemctl --user daemon-reload

# 2. 启用并启动服务
systemctl --user enable --now relay-knowledge.service

# 3. 查看服务状态
systemctl --user status relay-knowledge.service

# 4. 查看日志
journalctl --user -u relay-knowledge.service -n 100 --no-pager
```

如果服务需要在用户未登录时运行，由管理员启用 linger：

```bash
sudo loginctl enable-linger "$USER"
```

**系统级服务（system service）**：

```bash
# 写入系统级 service definition 前设置 service_dir
export RELAY_KNOWLEDGE_SERVICE_DIR=/etc/systemd/system
relay-knowledge service definition write --format json

sudo systemctl daemon-reload
sudo systemctl enable --now relay-knowledge.service
sudo systemctl status relay-knowledge.service
sudo journalctl -u relay-knowledge.service -n 100 --no-pager
```

### 15.3.5 验证服务状态

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

### 15.3.6 停止与重启

```bash
# 停止服务
systemctl --user stop relay-knowledge.service

# 重启服务
systemctl --user restart relay-knowledge.service
```

---

## 15.4 首次部署（macOS launchd）

### 15.4.1 部署预检

```bash
relay-knowledge setup doctor --format json
relay-knowledge service plan install --format json
relay-knowledge service definition write --format json
```

生成的 launchd plist 文件名为 `com.coolplayagent.relay-knowledge.plist`，写入在 `service_dir` 指定的路径下（默认为 `~/Library/Application Support/relay-knowledge/service/`）。

### 15.4.2 前台验证

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
  RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --web --mcp streamable-http
```

### 15.4.3 安装并启动 launchd 服务

```bash
# 从 service plan 输出的 definition_path 加载服务
launchctl load "<definition_path>"

# 启动服务
launchctl start com.coolplayagent.relay-knowledge

# 查看运行状态
launchctl list | grep com.coolplayagent.relay-knowledge
```

### 15.4.4 验证服务状态

```bash
curl http://127.0.0.1:8791/api/health
relay-knowledge service doctor --format json
```

### 15.4.5 停止与卸载

```bash
launchctl stop com.coolplayagent.relay-knowledge
launchctl unload "<definition_path>"
```

---

## 15.5 首次部署（Windows Service）

### 15.5.1 部署预检

```powershell
relay-knowledge setup doctor --format json
relay-knowledge service plan install --format json
relay-knowledge service definition write --format json
```

生成的 Windows Service definition 文件名为 `relay-knowledge-service.xml`。

### 15.5.2 前台验证

```powershell
set RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791
set RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs
relay-knowledge service run --web --mcp streamable-http
```

### 15.5.3 安装并启动 Windows Service

以管理员权限打开 PowerShell：

```powershell
# 从 service plan JSON 中获取 BinaryPathName
relay-knowledge service plan install --format json

# 创建 Windows Service
New-Service relay-knowledge -BinaryPathName "<relay-knowledge 完整路径> service run --web --mcp streamable-http"

# 启动服务
Start-Service relay-knowledge

# 查看状态
Get-Service relay-knowledge
```

### 15.5.4 验证服务状态

```powershell
curl http://127.0.0.1:8791/api/health
relay-knowledge service doctor --format json
```

### 15.5.5 停止与卸载

```powershell
Stop-Service relay-knowledge
Remove-Service relay-knowledge
```

---

## 15.6 存储拓扑选择

### 15.6.1 选型指南

| 场景 | 推荐拓扑 | 配置 |
|------|----------|------|
| 个人使用、少量仓库、开发测试 | `single_sqlite` | 不设置或 `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=single_sqlite` |
| 大仓库（代码量 >1 GB）、多仓库管理 | `partitioned_sqlite` | `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite` |

`single_sqlite` 使用单个 SQLite 数据库存储所有图事实、索引和代码仓库数据。

`partitioned_sqlite` 使用一个主 SQLite 控制库管理仓库注册和任务状态，每仓库独立 SQLite shard 存储代码事实和索引。shard 目录位于运行时数据目录的 `stores/repositories/` 下。

### 15.6.2 配置方法

```bash
export RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite
```

该环境变量必须在预检、生成 definition、启动服务和所有运维命令中保持一致。切换拓扑前必须先完成显式迁移或回滚。

### 15.6.3 重要约束

- 主数据库一旦包含 active shard catalog，**不能直接用 `single_sqlite` 打开同一运行时状态**。
- 备份、迁移、doctor、卸载确认和回滚计划**必须同时覆盖主数据库和 shard 目录**。只移动或校验主数据库不能宣称操作成功。
- 任一仓库最多一个 active writer task；跨进程或跨后端部署由 durable lease 保护。

---

## 15.7 HTTP 与 MCP 配置

### 15.7.1 HTTP 绑定配置

| 环境变量 | 默认值 | 说明 |
|----------|--------|------|
| `RELAY_KNOWLEDGE_HTTP_BIND` | `127.0.0.1:8791` | HTTP 服务监听地址和端口 |
| `RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS` | `30000` | 单请求超时（毫秒） |
| `RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS` | `10000` | 优雅关闭超时（毫秒） |
| `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES` | `1048576` | 请求体最大字节数（默认 1 MB） |

默认只绑定 `127.0.0.1`（loopback），仅本机可访问。

### 15.7.2 QoS 预算配置

| 环境变量 | 默认值 | 说明 |
|----------|--------|------|
| `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` | `1024` | 最大并发连接数 |
| `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` | `256` | 最大并发请求数 |
| `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` | `512` | 最大排队请求数 |

### 15.7.3 MCP 配置

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

### 15.7.4 远端访问配置

若需要从其他主机访问服务，必须同时配置网络绑定和访问策略：

```bash
RELAY_KNOWLEDGE_HTTP_BIND=0.0.0.0:8791 \
  RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true \
  RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --web --mcp streamable-http
```

**安全提醒**：
- QoS 是 admission control，不是认证。生产环境应在受信网络或外部反向代理后运行。
- 非 loopback bind 必须显式启用远端客户端策略、scope/origin 限制和 QoS budget。
- 建议配置 `RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED=true` 以启用审计日志。

### 15.7.5 远端 CLI 使用

远端 CLI 通过统一 HTTP 服务访问 code repository API：

```bash
# 使用 --remote 指定远端地址
relay-knowledge --remote http://host:8791 repo status my-repo --format json
relay-knowledge --remote http://host:8791 repo query my-repo "service startup" --format json

# 或设置环境变量
export RELAY_KNOWLEDGE_REMOTE_BASE_URL=http://host:8791
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

## 15.8 升级流程

### 15.8.1 升级顺序

```
preflight doctor
  → backup or migration checkpoint
  → stop service through platform manager
  → install new binary
  → write or update service definition
  → run schema/index migration through normal startup
  → start service through platform manager
  → post-upgrade doctor
```

### 15.8.2 Linux systemd 升级

```bash
# 1. 升级前预检
relay-knowledge setup doctor --format json

# 2. 备份运行时数据
tar -czf relay-knowledge-backup-$(date +%Y%m%d).tar.gz \
  $(relay-knowledge service plan install --format json | jq -r '.runtime_state_paths[]')

# 3. 停止服务
systemctl --user stop relay-knowledge.service

# 4. 安装新二进制（覆盖旧版本）
sudo cp relay-knowledge /usr/local/bin/relay-knowledge

# 5. 更新 service definition
relay-knowledge service definition write --format json
systemctl --user daemon-reload

# 6. 启动服务（自动执行 schema/index migration）
systemctl --user start relay-knowledge.service

# 7. 升级后验证
relay-knowledge service doctor --format json
curl http://127.0.0.1:8791/api/health
```

### 15.8.3 macOS launchd 升级

```bash
# 1-2. 预检和备份（同上）
relay-knowledge setup doctor --format json
# ... 备份步骤 ...

# 3. 停止服务
launchctl stop com.coolplayagent.relay-knowledge
launchctl unload "<definition_path>"

# 4. 安装新二进制
sudo cp relay-knowledge /usr/local/bin/relay-knowledge

# 5. 更新 service definition
relay-knowledge service definition write --format json
launchctl load "<definition_path>"

# 6. 启动并验证
launchctl start com.coolplayagent.relay-knowledge
relay-knowledge service doctor --format json
```

### 15.8.4 Windows Service 升级

以管理员权限执行：

```powershell
# 1. 预检
relay-knowledge setup doctor --format json

# 2. 备份（手动复制运行时目录）

# 3. 停止服务
Stop-Service relay-knowledge

# 4. 安装新二进制
# ... 覆盖 relay-knowledge.exe ...

# 5. 更新 service definition
relay-knowledge service definition write --format json

# 6. 启动服务
Start-Service relay-knowledge

# 7. 验证
relay-knowledge service doctor --format json
```

### 15.8.5 备份注意事项

- 备份必须覆盖 `service plan install --format json` 中 `runtime_state_paths` 列出的**所有**路径。
- `partitioned_sqlite` 下必须同时备份主 SQLite 数据库和 `stores/repositories/` shard 目录。只备份主库会让代码事实不可见。

---

## 15.9 回滚操作

### 15.9.1 回滚原则

- 回滚时同时回滚**二进制**、**service definition** 和**数据迁移 checkpoint**。
- forward-only migration 必须在变更说明中写清楚，不能只替换旧二进制后宣称回滚完成。

### 15.9.2 回滚步骤

```bash
# 1. 停止当前服务
systemctl --user stop relay-knowledge.service

# 2. 从备份恢复运行时数据
tar -xzf relay-knowledge-backup-YYYYMMDD.tar.gz -C /

# 3. 替换为旧版本二进制
sudo cp /path/to/old/relay-knowledge /usr/local/bin/relay-knowledge

# 4. 重新生成 service definition
relay-knowledge service definition write --format json
systemctl --user daemon-reload

# 5. 启动并验证
systemctl --user start relay-knowledge.service
relay-knowledge service doctor --format json
```

---

## 15.10 卸载

### 15.10.1 卸载原则

- 默认卸载只移除**二进制**和**service definition**。
- 删除配置、数据库、索引、日志、缓存、worker queue、dead-letter 或 shard 目录**必须经过用户确认**。
- `partitioned_sqlite` 下卸载确认同时覆盖主库和 shard 目录。

### 15.10.2 查看卸载计划

```bash
relay-knowledge service plan uninstall --format json
```

输出包含：
- `runtime_state_paths`：将被保留的运行时数据路径
- `uninstall_command`：平台卸载命令预览
- `warnings`：关于数据保留的提醒

### 15.10.3 Linux systemd 卸载

```bash
# 1. 停止并禁用服务
systemctl --user disable --now relay-knowledge.service

# 2. 移除二进制
sudo rm /usr/local/bin/relay-knowledge

# 3. 如需清理运行时数据（需用户确认）
rm -rf /var/lib/relay-knowledge/data
rm -rf /var/lib/relay-knowledge/state
rm -rf /var/log/relay-knowledge
```

### 15.10.4 macOS launchd 卸载

```bash
launchctl stop com.coolplayagent.relay-knowledge
launchctl unload "<definition_path>"
sudo rm /usr/local/bin/relay-knowledge
# 运行时数据默认在 ~/Library/Application Support/relay-knowledge/，按需删除
```

### 15.10.5 Windows Service 卸载

以管理员权限执行：

```powershell
Stop-Service relay-knowledge
Remove-Service relay-knowledge
# 删除二进制文件
# 运行时数据默认在 %LOCALAPPDATA%/relay-knowledge/，按需删除
```

---

## 15.11 code-index worker 配置

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

## 15.12 Operator 管理

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

## 15.13 诊断与排障

### 15.13.1 常规诊断顺序

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

### 15.13.2 HTTP 诊断端点

| 端点 | 说明 |
|------|------|
| `GET /api/health` | 服务健康检查 |
| `GET /api/service/status` | 服务状态快照 |
| `GET /api/project/status` | 项目身份信息 |
| `GET /api/v1/control/status` | 控制面状态 |
| `GET /api/v1/control/health` | 控制面健康检查 |
| `GET /api/v1/control/service/status` | 控制面服务状态 |
| `GET /api/v1/control/storage/topology` | 存储拓扑诊断 |

### 15.13.3 常见问题

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

## 15.14 环境变量速查表

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
| service | `~/Library/Application Support/relay-knowledge/service` |

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

## 15.15 命令速查

| 命令 | 用途 |
|------|------|
| `relay-knowledge setup doctor --format json` | 部署前预检 |
| `relay-knowledge setup profile service --format json` | 查看 service 推荐配置 |
| `relay-knowledge service plan install --format json` | 生成安装计划 |
| `relay-knowledge service plan uninstall --format json` | 生成卸载计划 |
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
| `relay-knowledge --remote http://host:8791 repo status <alias>` | 远端 repo 状态 |

---

**导航**：上一章：[第 9 章 服务化部署与常驻服务](09-resident-service.md) | 下一章：[第 13 章 运维与排障](13-operations-and-troubleshooting.md)
