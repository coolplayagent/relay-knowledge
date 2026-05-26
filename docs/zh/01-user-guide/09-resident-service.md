# 第 9 章 常驻服务

[中文](../../zh/01-user-guide/09-resident-service.md) | [English](../../en/01-user-guide/09-resident-service.md)

常驻服务用于托管 Web、API、MCP、startup reconciler 和后台运维入口。开发机可以前台运行；长期后台运行必须交给平台 service manager。

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

`service run` 启动时会先执行 startup index reconciler，尽量在接受 resident adapter 请求前恢复落后的索引任务，然后用有界常驻 worker 消费持久化 code-index 队列和 repository-set overlay refresh 队列。没有启用 MCP 或 Web 时，命令仍会作为前台服务等待 shutdown signal。

HTTP `/api/health` 和 CLI `health` 是 liveness-safe 入口：它只做短预算只读快照，不会排队 index refresh，也不会等待大型 repository indexing 完成。存储读通道繁忙时，health 会返回 cached 或最小 degraded 响应，并用 `storage_busy`、stale metadata 或 degraded reason 暴露压力。普通代码查询不会因此被排除；`allow-stale` 查询在目标 ref 正在索引时读取上一个已完成 committed scope，`wait-until-fresh` 查询才要求目标 scope 已 finalize。

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

## 9.4 Silent Update Operator

查看、暂停或恢复后台更新 operator:

```bash
relay-knowledge service operator status --format json
relay-knowledge service operator pause
relay-knowledge service operator resume
```

Silent updates 必须用户可配置、可观测、可逆。它们只能在授权 scope 内刷新图数据和派生索引，并暴露 freshness、stale、paused、degraded 和 failure 状态。

## 9.5 运行建议

开发机临时验证优先使用前台命令或 `run.sh`:

```bash
./build.sh
./run.sh start --port 8791 --daemon
./run.sh status
./run.sh stop --force
```

长期后台运行应使用 `service plan` 和 `service definition write` 生成平台 service manager 配置，再由用户或安装器执行需要权限的安装动作。不要用未受管 CLI 循环替代 systemd、Windows Service 或 launchd。运行时数据、日志、缓存、worker 队列和 dead-letter 数据必须留在 `paths` 管理的目录中，而不是 release 解压目录或仓库目录。
