# 第 9 章 服务化部署与常驻服务

[中文](../../zh/01-user-guide/09-resident-service.md) | [English](../../en/01-user-guide/09-resident-service.md)

本章是服务化部署的用户指南，覆盖从本机前台验证到平台 service manager 托管、远端访问、运维诊断、升级回滚和卸载的完整路径。架构契约见 [第 19 章安装、发布与升级](../03-architecture-specs/19-installation-release-and-upgrade.md) 和 [第 22 章服务化部署、控制面与数据面分离](../03-architecture-specs/22-service-deployment-control-data-plane.md)。

常驻服务托管 Web、HTTP API、MCP Streamable HTTP、startup reconciler、code-index worker pool、repository-set refresh worker 和运维入口。开发机可以用前台命令或 `run.sh` 验证；长期后台运行必须交给 systemd、launchd 或 Windows Service。当前 CLI 提供 service plan、service definition 和 staged lifecycle 命令；dry-run 是默认行为，只有显式 `service lifecycle <action> --execute` 才会写入本地文件并调用平台 service manager。JSON API 调用方可以只发送 `execute: true`，不需要额外发送 `dry_run: false`；显式 `dry_run: true` 仍表示 dry-run 请求。执行失败会返回操作错误并带出失败 step id，而不是把含 `failed_step_id` 的执行报告包装成成功响应。

> ⚠️ **开发环境限定**：`run.sh` 和 `run.sh --daemon` 仅用于开发环境验证，不可用于生产部署。
> 长期后台运行必须由 systemd（Linux）、launchd（macOS）或 Windows Service 托管。
> 生产部署请参阅第 15 章「服务化部署用户指南」。

## 9.1 选择部署拓扑

| 拓扑 | 适用场景 | 服务管理 |
| --- | --- | --- |
| `embedded_cli` | 一次性 CLI、测试、临时查询 | 不安装常驻服务 |
| `resident_single_process` | 默认本地常驻 Web/API/MCP 和 worker | 一个平台 service |
| `resident_partitioned_sqlite` | 大仓库或多仓库，本地控制库加每仓 shard | 一个平台 service，备份必须覆盖 shard |
| `split_worker_preview` | 预览独立 worker 进程 claim durable task | 不能替代 service manager |

默认使用 `resident_single_process`。需要每仓 SQLite shard 时，在预检、生成 definition、启动服务和所有运维命令中保持同一个环境变量:

```bash
export RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite
```

主数据库一旦包含 active shard catalog，就不能直接用 `single_sqlite` 打开同一运行时状态。回退前必须完成显式 rollback，并确认主库和 `stores/repositories/` shard 目录一起处理。

## 9.2 部署前预检

先构建或安装二进制，并确认配置 readiness:

```bash
relay-knowledge setup doctor --format json
relay-knowledge setup profile service --format json
relay-knowledge service plan install --format json
relay-knowledge service lifecycle install --dry-run --format json
```

检查 `service plan install` 的这些字段:

- `platform`: 当前平台对应 `linux`、`macos` 或 `windows`。
- `definition_path`: `service definition write` 将写入的平台定义文件。
- `install_command`、`start_command`、`stop_command`、`uninstall_command`: 需要用户或安装器执行的命令预览。
- `lifecycle_steps`: install、upgrade、rollback 或 uninstall 的阶段化动作，包含写入路径、删除路径和命令。
- `rollback_steps`: lifecycle 执行失败时要尝试的回滚动作。
- `permission_requirements`: 当前平台执行 service manager 命令所需的权限范围。
- `package_manifest_checks`: Homebrew、Scoop、winget、发行版包必须引用同一 release tag 和 checksum 的校验链路。
- `runtime_state_paths`: 数据库、配置、状态、日志和缓存路径；`partitioned_sqlite` 还包含 shard 目录。
- `checkpoint_path`: lifecycle 执行时记录 rollback checkpoint 的路径。
- `warnings`: 是否提示需要覆盖 shard、备份、迁移、回滚或卸载确认。
- `checksum`: 生成 service definition 的稳定校验值，用于变更审计。

写入 service definition:

```bash
relay-knowledge service definition write --format json
```

写入 definition 前应确定以下配置已经固定:

- `RELAY_KNOWLEDGE_DATA_DIR`、`RELAY_KNOWLEDGE_STATE_DIR`、`RELAY_KNOWLEDGE_LOG_DIR` 等运行时目录使用绝对路径，且不指向 release 解压目录或仓库目录。
- Linux systemd service definition 会引用包含空格的路径，并把字面 `$` 写成 `$$`，避免 systemd 把安装目录或数据目录中的 `$` 当作环境变量展开。
- `RELAY_KNOWLEDGE_HTTP_BIND` 默认保持 loopback，例如 `127.0.0.1:8791`。
- MCP 暴露给 agent 时设置 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES`，不要用未限定 scope 的远端服务。
- 非 loopback bind 必须显式设置远端客户端策略、origin/scope 限制和 QoS budget。
- code-index 并发通过 `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` 配置，当前上限为 8。

## 9.3 前台验证

MCP Streamable HTTP:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --mcp streamable-http
```

同端口 Web/API/MCP:

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --web --mcp streamable-http
```

另一个终端检查:

```bash
curl http://127.0.0.1:8791/api/health
relay-knowledge service status --format json
```

开发机也可以用脚本验证:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
```

`run.sh --daemon` 只适合开发验证。正式后台运行使用平台 service manager。

`service run` 启动时会先执行 startup index reconciler，并恢复 orphaned code-index leases。随后它作为 resident master 管理 code-index worker pool 和 repository-set refresh worker。没有启用 Web 或 MCP 时，命令仍会等待 shutdown signal，适合 service manager 托管。

## 9.4 平台 Service Manager 部署

通用流程:

```bash
relay-knowledge setup doctor --format json
relay-knowledge service plan install --format json
relay-knowledge service lifecycle install --dry-run --format json
relay-knowledge service definition write --format json
```

确认 dry-run 输出中的路径、权限和 rollback 计划后，可以由安装器或运维人员执行 JSON 中的 `install_command` 和 `start_command`，或显式运行:

```bash
relay-knowledge service lifecycle install --execute --format json
```

Linux systemd user service:

```bash
systemctl --user daemon-reload
systemctl --user enable "<definition_path>"
systemctl --user start relay-knowledge.service
systemctl --user status relay-knowledge.service
journalctl --user -u relay-knowledge.service -n 100 --no-pager
```

如果服务需要在用户未登录时运行，应由安装器或管理员配置 user linger:

```bash
loginctl enable-linger "$USER"
```

macOS launchd:

```bash
launchctl load "<definition_path>"
launchctl start com.coolplayagent.relay-knowledge
launchctl list | grep com.coolplayagent.relay-knowledge
```

Windows Service 使用 PowerShell，并按 `service plan` 的输出以管理员权限执行:

```powershell
relay-knowledge service plan install --format json
relay-knowledge service definition write --format json
$ErrorActionPreference = 'Stop'
New-Service relay-knowledge -BinaryPathName '"<relay-knowledge path>" service run --web --mcp streamable-http' -ErrorAction Stop
Start-Service relay-knowledge
Get-Service relay-knowledge
```

Windows 生成的 `relay-knowledge-service.xml` 是 service definition 产物。安装器或运维脚本可以使用该文件生成等价 Windows Service 配置，但仍必须保留同一二进制、参数、环境变量、运行时目录和卸载计划。

安装后验证:

```bash
relay-knowledge service doctor --format json
curl http://127.0.0.1:8791/api/health
curl http://127.0.0.1:8791/api/v1/control/service/status
```

`service status` 和 `/api/v1/control/service/status` 返回 code-index worker、operator、storage topology、queue/dead-letter、runtime path 和 degraded reason。它们是短预算诊断入口，不会同步执行大型索引或 shard repair。

## 9.5 远端访问

默认只绑定本机 loopback。需要从其他主机访问时，应先配置网络边界和访问策略，再启动服务:

```bash
RELAY_KNOWLEDGE_HTTP_BIND=0.0.0.0:8791 \
RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --web --mcp streamable-http
```

远端 code repository CLI 使用同一 HTTP 服务:

```bash
relay-knowledge --remote http://host:8791 repo status my-repo --format json
relay-knowledge --remote http://host:8791 repo query my-repo "service startup" --format json
```

也可以在自动化环境中设置:

```bash
export RELAY_KNOWLEDGE_REMOTE_BASE_URL=http://host:8791
```

远端模式支持 repository index、scope preview、status、query、feature-flags、impact、report 和 software projection。`repo index --reset` 与 `repo index-worker` 必须在服务宿主机执行，不能通过远端 CLI 绕过本地维护边界。

> ⚠️ **远端 CLI 限制**：以下维护操作仅能在服务宿主机上执行，远端 CLI 不可调用：
> - `repo index --reset`：代码索引重置
> - `repo index-worker`：索引 worker 管理
> - `shard repair`：分片修复
> - `backup`：数据备份
> - `migration`：数据迁移
> - `rollback`：版本回滚
> - `uninstall`：服务卸载

QoS 是 admission control，不是认证。生产网络暴露应在受信网络或外部反向代理后运行，并保留 request timeout、body limit、connection budget、scope policy 和审计。

## 9.6 Operator 与 Worker

查看、暂停或恢复 silent-update operator:

```bash
relay-knowledge service operator status --format json
relay-knowledge service operator pause --format json
relay-knowledge service operator resume --format json
```

Silent updates 必须用户可配置、可观测、可逆，只能在授权 scope 内刷新图数据和派生索引，并暴露 freshness、stale、paused、degraded 和 failure 状态。

预览独立 worker 只运行一个 durable code-index task:

```bash
relay-knowledge service worker run --format json
relay-knowledge service worker run --task-id <id> --format json
```

该命令最多 claim 一个 task，必须持有 attempt-scoped lease 后才能写入 complete/fail。lease 过期、attempt 不匹配或未 claim 时不能写成功结果。不要用循环调用它来替代平台 service manager。

## 9.7 升级、回滚与卸载

升级顺序:

```text
preflight doctor
  -> backup or migration checkpoint
  -> stop service through platform manager
  -> install new binary and service definition
  -> run schema/index migration through normal startup
  -> start service through platform manager
  -> post-upgrade doctor
```

操作命令:

```bash
relay-knowledge setup doctor --format json
relay-knowledge service plan upgrade --target-version 1.2.3 --install-dir /opt/relay-knowledge --format json
relay-knowledge service lifecycle upgrade --dry-run --target-version 1.2.3 --install-dir /opt/relay-knowledge --format json
relay-knowledge service doctor --format json
```

备份必须覆盖 `runtime_state_paths` 中列出的所有路径。`partitioned_sqlite` 下必须同时备份主数据库和 shard 目录，只备份主库会让代码事实不可见。

卸载服务但保留 runtime data:

```bash
relay-knowledge service plan uninstall --format json
relay-knowledge service lifecycle uninstall --dry-run --format json
systemctl --user disable --now relay-knowledge.service
```

macOS 使用 `launchctl unload "<definition_path>"`，Windows 使用 PowerShell `Stop-Service relay-knowledge` 和 `sc.exe delete relay-knowledge`。删除 runtime data、日志、缓存、dead-letter 或 shard 目录必须经过用户确认；卸载 service definition 不应默认删除这些状态。

回滚时同时回滚二进制、service definition 和数据迁移 checkpoint。`service lifecycle rollback --dry-run` 会显示 checkpoint 恢复步骤；`--execute` 会尝试恢复 lifecycle checkpoint 中的二进制和 service definition 备份。安装到显式 `--install-dir` 时，install 会先拒绝覆盖已有目标二进制；Windows install 会先创建 service，再以独立步骤写入 service 环境，环境写入失败时会卸载本次创建的 service。upgrade 会 checkpoint 已有二进制和 service definition，checkpoint backup 使用 attempt-scoped 文件并在 checkpoint 原子发布后才成为当前回滚依据；没有旧备份时，只有本次确实复制或写入的目标文件会在失败回滚或显式 rollback 中被删除，definition-only upgrade 不会删除当前运行的 binary。Windows upgrade 会在启动前刷新 SCM `BinaryPathName` 和环境，macOS upgrade 会重新 unload/load launchd plist。uninstall 失败回滚和基于 uninstall checkpoint 的显式 rollback 都会恢复被卸载删除的原 service definition，再重新注册 service。lifecycle 失败报告只有在选中的 rollback steps 全部成功时才把 rollback 标为完成；restore、definition rewrite、unregister 或 service-registration rollback step 失败后不会继续执行依赖的删除、reload 或 start 步骤；在任何文件或 service manager 状态变化前失败时，不会停止、恢复或重新启动既有 service。外部 service manager 和 doctor 子进程有执行超时，并在等待期间持续读取 stdout/stderr；进程退出或超时后，输出收集也有边界，避免继承 stdout/stderr 的 helper 或大输出命令卡住后无法返回执行报告。forward-only migration 必须在变更说明里写清楚，不能只替换旧二进制后宣称回滚完成。

## 9.8 诊断与排障

常规诊断顺序:

```bash
relay-knowledge status --format json
relay-knowledge setup doctor --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
relay-knowledge audit query --limit 50 --format json
```

HTTP 诊断:

```bash
curl http://127.0.0.1:8791/api/health
curl http://127.0.0.1:8791/api/service/status
curl http://127.0.0.1:8791/api/v1/control/storage/topology
```

常见问题:

- 服务启动后 Web 无法访问: 检查 `RELAY_KNOWLEDGE_HTTP_BIND`、systemd/launchd/Windows Service 状态和日志。
- 非 loopback bind 被拒绝: 设置 `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`，并补齐 origin/scope 限制。
- `single_sqlite` 打不开 runtime: 检查是否残留 active `partitioned_sqlite` shard catalog，按回滚计划处理。
- `repo status` 长时间显示 running: 查看 active task lease、checkpoint 和 dead-letter；不要杀进程或绕过 lease。
- `health` 返回 `storage_busy` 或 stale diagnostics: 这表示短预算诊断降级，不代表服务必然不可用；继续查看 service status、index lag 和 queue depth。

更多排障步骤见 [第 13 章运维与排障](13-operations-and-troubleshooting.md)。
