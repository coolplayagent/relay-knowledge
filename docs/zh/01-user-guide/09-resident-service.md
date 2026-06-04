# 第 9 章 常驻服务

[中文](../../zh/01-user-guide/09-resident-service.md) | [English](../../en/01-user-guide/09-resident-service.md)

常驻服务用于托管 Web、API、MCP、startup reconciler 和后台运维入口。开发机可以前台运行；长期后台运行必须交给平台 service manager。

当前服务化拓扑分为 `embedded_cli`、`resident_single_process` 和 `resident_partitioned_sqlite`。前两者使用单个运行时数据库；启用 `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite` 后，控制状态仍在主数据库，代码仓库数据进入每仓库 shard。未来 split worker 只允许通过控制面 claim 持久 task 后执行，不提供未受管后台循环。

## 9.1 前台服务

启动 MCP Streamable HTTP:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --mcp streamable-http
```

启动同端口 Web/API/MCP 服务:

```bash
./build.sh
./run.sh start --port 8791 --daemon
```

对应底层命令:

```bash
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
target/release/relay-knowledge service run --web --mcp streamable-http
```

`service run` 启动时会先执行 startup index reconciler，尽量在接受 resident adapter 请求前恢复落后的索引任务，然后作为 resident master 管理持久化 code-index 和 repository-set overlay refresh worker。Master 拥有配置、启动 lease 恢复、有界 worker pool 启动、队列监督和优雅关闭；code-index worker 只 claim 带 lease 的任务并执行有界 batch。Code-index pool 默认并发为 2，通过 `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` 配置，并按上限 clamp 到 8。没有启用 MCP 或 Web 时，命令仍会作为前台服务等待 shutdown signal。

使用 `relay-knowledge service status --format json` 查看 `storage` 和 `code_index_workers`。`storage` 返回当前 topology、主库路径、`partitioned_sqlite` shard 目录、active/staged/missing shard 计数、runtime state paths 和缺失 shard degraded reason；`code_index_workers` 返回 configured worker count、active worker slots、queue depth、queued/running/retrying/dead-letter task counts、running leases 和 last error。这些诊断可以解释 master 是空闲、饱和、正在重试、等待另一个 repository writer lease，还是 partitioned 数据面缺少 shard 文件。

HTTP `/api/health` 和 CLI `health` 是 liveness-safe 入口：它只做短预算只读快照，不会排队 index refresh，也不会等待大型 repository indexing 完成。存储读通道繁忙时，health 会返回 cached 或最小 degraded 响应，并用 `storage_busy`、stale metadata 或 degraded reason 暴露压力。普通代码查询不会因此被排除；`allow-stale` 查询在目标 ref 和 filters 正在索引时读取最新兼容的已完成 committed scope，`wait-until-fresh` 查询才要求目标 scope 已 finalize。

稳定外部控制面仍保持 preview 范围，当前只提供只读 HTTP route：`/api/v1/control/status`、`/api/v1/control/health`、`/api/v1/control/service/status` 和 `/api/v1/control/storage/topology`。这些 route 复用 CLI/Web/MCP 的共享 API 类型，不同步执行索引、迁移或 shard 修复。

## 9.2 Web 中的 Service Run

Web 页面中的 service run 操作只通过 `/api/web/operations/execute` 返回当前 service runtime snapshot，用于检查即将运行的配置和 MCP 状态。实际常驻服务必须由 CLI、`run.sh` 或平台 service manager 启动。

## 9.3 Service Manager

service manager v1 生成平台定义和命令预览，不自动执行需要权限的安装命令:

```bash
relay-knowledge setup profile service --format json
relay-knowledge service plan install --format json
relay-knowledge service definition write --format json
```

Linux 输出 systemd user service 计划，macOS 输出 launchd plist 计划，Windows 输出 service XML/PowerShell 计划。runtime state、graph database、indexes、audit 和 worker 队列仍使用 `paths` 解析后的 platform data/state/log/cache 目录，不写入 release extraction directory。

启用 `partitioned_sqlite` 时，service doctor、备份、迁移和卸载确认必须同时覆盖主数据库和 `stores/repositories/` shard 目录。只移动主数据库会让代码事实不可见，不能被视为成功迁移或回滚。

`service plan install|uninstall --format json` 的 `runtime_state_paths` 会列出主库、配置、状态、日志、缓存路径；启用 `partitioned_sqlite` 时还会列出 shard 目录，并在 `warnings` 中提示备份、迁移、回滚和卸载确认必须覆盖主库与 shard 目录。

## 9.4 Silent Update Operator

查看、暂停或恢复后台更新 operator:

```bash
relay-knowledge service operator status --format json
relay-knowledge service operator pause
relay-knowledge service operator resume
```

Silent updates 必须用户可配置、可观测、可逆。它们只能在授权 scope 内刷新图数据和派生索引，并暴露 freshness、stale、paused、degraded 和 failure 状态。

## 9.5 Split Worker Preview

`relay-knowledge service worker run [--task-id <id>] --format json` 是进程级 split worker 的 preview 入口。它一次最多 claim 一个 durable code-index task，持有 attempt-scoped lease 后执行，并通过同一个 storage contract complete 或 fail；未 claim、lease 过期或 attempt 不匹配时不会写入成功结果。该命令不替代平台 service manager，也不提供未受管后台循环。

## 9.6 运行建议

开发机临时验证优先使用前台命令或 `run.sh`:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
```

长期后台运行应使用 `service plan` 和 `service definition write` 生成平台 service manager 配置，再由用户或安装器执行需要权限的安装动作。不要用未受管 CLI 循环替代 systemd、Windows Service 或 launchd。运行时数据、日志、缓存、worker 队列和 dead-letter 数据必须留在 `paths` 管理的目录中，而不是 release 解压目录或仓库目录。
