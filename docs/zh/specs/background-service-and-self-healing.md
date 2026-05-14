# 后台服务、静默更新与自愈设计

[中文](../../zh/specs/background-service-and-self-healing.md) | [英文](../../en/specs/background-service-and-self-healing.md)

> 文档版本: 1.0
> 编制日期: 2026-05-11
> 适用范围: `relay-knowledge` 安装后常驻进程、静默图谱更新、索引刷新、资源治理、假死检测和自愈恢复
> 默认路线: OS service manager 托管进程，应用内部用事件日志、任务租约、资源预算和 reconciler 保证可恢复

## 1. 设计结论

`relay-knowledge` 不能按一次性 CLI 脚本设计后台能力。安装后的图数据刷新、BM25 / semantic / vector 索引维护、WAL checkpoint、旧索引清理和健康诊断都需要一个长期存在、可观测、可恢复、可限制资源的后台运行面。

核心结论:

1. **常驻进程交给操作系统托管**: Linux 用 systemd，Windows 用 Windows Service，macOS 用 launchd。应用不要自己实现脆弱的双进程守护、pid 文件轮询或无限重启循环。
2. **静默更新是用户可控能力**: 安装后必须允许用户启用、关闭和限制 silent background update。静默只表示不打断用户，不表示不可见、不可取消或无限消耗资源。
3. **后台任务必须事件驱动和幂等**: 图变更、索引刷新和维护任务都从持久化事件、source fingerprint、mutation log 或任务表恢复，进程重启后不能丢任务或重复污染图状态。
4. **资源预算是一等配置**: CPU、并发数、队列长度、单批大小、磁盘占用、WAL 大小、维护窗口和索引滞后阈值必须可配置，并被 runtime 强制执行。
5. **自愈分两层**: OS service manager 负责进程级重启；应用内部 reconciler 负责检测 stuck task、补发漏掉的 index refresh、重试可恢复错误和隔离失败任务。
6. **所有退化必须可见**: 当索引落后、后台暂停、队列阻塞、存储 busy 或某个索引失败时，CLI / Web / diagnostics 必须显示 stale、degraded、paused 或 failed 状态。

## 2. 运行模式

后台能力至少支持四种模式:

| 模式 | 用途 | 行为 |
| --- | --- | --- |
| `foreground` | 开发、CI、调试 | 前台运行，日志输出到终端，收到信号后优雅停止 |
| `service` | 安装后常驻 | 由 OS service manager 拉起、停止、重启和监控 |
| `maintenance` | 一次性维护 | 执行 repair、checkpoint、compact、index rebuild 等显式任务 |
| `disabled` | 用户关闭后台能力 | 不自动扫描、不静默刷新，只响应显式 CLI / Web 操作 |

CLI 和 Web 不能复制后台逻辑。它们只通过 application service 发送命令、读取状态和展示诊断。

### 2.1 Linux：systemd

Linux 默认使用 systemd user service；需要系统级部署时再使用 system service。推荐单元行为:

- `Type=notify`，进程启动完成后发送 ready 信号。
- `WatchdogSec=` 配置进程假死检测，进程定期发送 watchdog heartbeat。
- `Restart=on-failure`，只在异常退出时重启。
- `RestartSec=` 设置退避起点，避免快速崩溃循环。
- `TimeoutStopSec=` 限制优雅停机时间，超时后由 systemd 终止。
- 使用 systemd 的 state / cache / log 目录约定承载本地数据库、索引和日志，不把运行态写进仓库。

应用内部必须把 service status 映射为简短可读的运行状态，例如 `ready: graph_version=42 index_lag_max=2 queue_depth=7`。这类状态应进入 diagnostics，便于用户区分“进程活着”和“工作真的在推进”。

### 2.2 Windows：Windows Service

Windows 默认使用 Windows Service 托管后台进程。安装器或 `relay-knowledge service install` 必须配置:

- 服务启动类型，可选 auto、manual、disabled。
- failure actions，至少包含异常退出后 restart service。
- 最大失败重启次数或 reset period，避免无限快速拉起。
- 事件日志或文件日志路径。
- 运行账号和数据目录，避免把索引写到不可预测的当前目录。

Windows 的服务恢复只负责进程生命周期。任务恢复必须仍由应用的持久化任务表、mutation log 和 index cursor 完成。

### 2.3 macOS：launchd

macOS 默认使用 launchd agent；系统级服务再使用 launchd daemon。推荐配置:

- `RunAtLoad` 控制登录或开机后启动。
- `KeepAlive` 控制异常退出后恢复。
- `ThrottleInterval` 限制快速重启。
- `StandardOutPath` 和 `StandardErrorPath` 指向受控日志目录。
- 数据目录使用用户级 Application Support / Cache 约定，避免写入仓库目录。

如果用户选择关闭后台能力，launchd 配置应保留但禁用自动启动，或直接卸载 agent。

## 3. 静默后台更新

静默更新的目标是让图数据和索引在用户没有显式运行 `update` 时也能保持新鲜，但它必须满足可控、可恢复、可降级。

### 3.1 用户控制

安装后必须提供这些配置:

| 配置 | 默认 | 含义 |
| --- | --- | --- |
| `background.enabled` | `false`，除非安装命令显式启用 | 是否允许后台常驻 |
| `background.silent_updates` | `false`，除非用户显式同意 | 是否允许静默刷新图和索引 |
| `background.sources` | 空 | 允许后台扫描和刷新哪些 source scope |
| `background.maintenance_window` | 无限制但受资源预算约束 | 允许重任务运行的时间窗口 |
| `background.max_cpu_percent` | 保守值 | 后台 CPU 预算 |
| `background.max_parallel_tasks` | 保守值 | 后台并发任务上限 |
| `background.max_disk_bytes` | 由安装目录和配置决定 | 图库、索引、日志和临时文件总预算 |
| `background.max_index_lag_versions` | 项目默认阈值 | 超过后 diagnostics 标记 degraded |

默认不应在用户未授权时静默扫描任意目录、私有仓库或大文件树。若用户通过安装参数传入 `--enable-background --enable-silent-updates --source <path>`，则可直接启用。

### 3.2 更新范围

静默更新只允许执行可恢复、可解释的后台任务:

- 读取已授权 source 的变更 fingerprint。
- 增量 ingest 新增或变化的文档、代码、图片 OCR 产物。
- 提交图 mutation，生成新的 `graph_version`。
- 从 mutation log 刷新 BM25、semantic、vector、summary、community 等派生索引。
- 做 WAL checkpoint、旧日志清理、过期索引分区清理。
- 汇报 health、metrics 和 diagnostics snapshot。

静默更新禁止执行:

- 未经用户确认的破坏性 schema 迁移。
- 未授权 source 的扫描或外发。
- 无上限的全量重建。
- 在查询热路径同步执行 embedding、OCR、大文件读取或 checkpoint。
- 静默删除唯一副本数据。

### 3.3 新鲜度语义

所有后台刷新都以 `graph_version` 和 `indexed_graph_version` 为准:

```text

source fingerprint changed
  -> ingest job created
  -> graph mutation committed at graph_version=N
  -> IndexRefreshRequested(graph_version=N, affected_scope=...)
  -> bm25 / semantic / vector workers refresh independently
  -> index_versions[index_kind].indexed_graph_version=N

```

查询响应必须继续暴露:

- `graph_version`
- `index_version`
- `indexed_graph_version`
- `stale`
- `degraded_reason`
- `retrieval_mode`

当索引落后时，调用方可以选择 `allow_stale`、`wait_until_fresh` 或 `graph_only`。后台服务不能用旧索引假装新鲜。

## 4. 资源治理

后台能力必须默认保守。用户不应该因为安装了 `relay-knowledge` 就遇到 CPU 飙升、磁盘写满或交互查询变慢。

### 4.1 CPU 和并发

后台 runtime 必须支持:

- 全局 worker 并发上限。
- 每类任务并发上限，例如 ingest、embedding、index refresh、maintenance。
- CPU 密集任务独立 worker pool，不运行在 async runtime 核心 executor 上。
- 任务预算耗尽后的 cooperative cancellation。
- 查询或前台命令优先级高于静默维护任务。
- 系统负载高、电池模式、用户活跃时自动降速或暂停低优先级任务。

Embedding、批量解析、OCR、社区摘要、全量索引 rebuild 都属于 CPU 或内存敏感任务，必须进入显式 worker 边界。

### 4.2 磁盘和 I/O

后台服务必须管理这些磁盘预算:

| 资源 | 要求 |
| --- | --- |
| SQLite database | 使用 WAL，但 checkpoint 不在查询热路径同步执行 |
| WAL 文件 | 设置 checkpoint 策略和告警阈值 |
| 索引分区 | 记录 scope、modality、model、graph version，支持 TTL 或手动清理 |
| 临时文件 | 写入受控 cache 目录，任务完成或失败后清理 |
| 日志 | 支持轮转、保留天数和最大大小 |
| dead-letter 记录 | 保留足够诊断信息，但有大小上限 |

大批写入要批量提交、幂等 upsert，并使用背压限制写队列。读路径要预算化，所有图遍历、路径搜索和混合检索都必须有 limit、timeout 和 truncated 标记。

### 4.3 背压和调度

事件管道必须使用有界队列:

```text

source watcher
  -> ingest queue [bounded]
  -> graph write queue [bounded, serialized where needed]
  -> index refresh queues [bounded per index kind]
  -> maintenance queue [low priority]

```

队列满时只能采取这些策略:

- 对外返回 retryable error。
- 合并同 scope 的重复 refresh request。
- 暂停低优先级 watcher。
- 降级为 stale 查询。

禁止无限增长内存队列，也禁止在队列满时静默丢弃图 mutation。

## 5. 假死检测和自愈

后台服务需要区分“进程崩溃”和“进程还在但不工作”。只看 PID 或端口不够。

### 5.1 失败模式

| 失败模式 | 检测信号 | 处理策略 |
| --- | --- | --- |
| process crash | OS service exit status | service manager 按策略重启 |
| event loop stalled | watchdog heartbeat 超时 | service manager 重启进程 |
| worker hung | task lease 过期且无进度 | 取消任务，重新入队，超过阈值进入 dead-letter |
| queue stuck | queue depth 长时间不降、oldest age 超阈值 | 暂停上游，提升 diagnostics 严重度，触发 reconciler |
| index lag runaway | `indexed_graph_version` 落后超阈值 | 标记 degraded，按 scope 合并 refresh，必要时 repair |
| storage busy | busy timeout、写入等待增长 | 降低写入并发，延后维护任务，暴露 retryable error |
| WAL runaway | WAL 大小或 checkpoint age 超阈值 | 维护窗口执行 checkpoint，失败时报警 |
| crash during commit | transaction 未提交或 mutation log 未完整 | 数据库事务回滚，启动后从最后 committed graph version 继续 |
| crash after graph commit before index event | graph version 领先 index cursor | reconciler 补发 refresh request |

### 5.2 Heartbeat 和 watchdog

应用内部必须维护两个信号:

1. **process heartbeat**: 表示主 runtime 仍能调度任务、响应 shutdown、读取配置。
2. **work progress heartbeat**: 表示关键队列仍在推进，例如 oldest task age、last completed task、last graph version、last indexed version。

systemd watchdog 或对应平台机制只能接收 process heartbeat。work progress 问题不应总是立即杀进程，应先进入 degraded 状态并尝试 reconciler 修复。

### 5.3 任务租约

所有长任务都必须使用持久化 lease:

```text

task_id
task_kind
scope_id
state: queued | running | succeeded | retrying | failed | dead_letter
lease_owner
lease_expires_at
attempt_count
next_retry_at
input_fingerprint
cursor_before
cursor_after
last_error_kind
last_error_message

```

任务开始时获取 lease，定期续租并写入进度。完成或失败上报必须携带当前
`lease_owner` 和 `attempt_count`，并只允许仍处于 `running` 且 lease 未过期的
拥有者推进状态，避免过期 worker 覆盖已重领或已完成的任务。进程崩溃后，新进程
可以把 lease 过期的 `running` 任务恢复为 `retrying`；超过 attempt 预算的过期
lease 必须进入 `dead_letter` 并把相关 cursor 标记为 failed。任务必须以
`input_fingerprint` 和稳定 ID 保证幂等。

### 5.4 调和器

后台服务启动后和周期性运行 reconciler:

- 扫描 graph version 与各索引 cursor 的差距。
- 补发遗漏的 `IndexRefreshRequested`。
- 恢复 lease 过期任务。
- 合并同 scope、同 index kind 的重复任务。
- 将反复失败且不可恢复的任务移入 dead-letter。
- 检查 orphan temp files、过期索引分区和过大 WAL。

Reconciler 不直接修复领域事实。它只恢复派生任务、索引进度和维护任务。

## 6. 配置、CLI 和诊断

CLI 至少预留这些命令能力:

```bash
relay-knowledge service install
relay-knowledge service uninstall
relay-knowledge service start
relay-knowledge service stop
relay-knowledge service restart
relay-knowledge service status
relay-knowledge service logs
relay-knowledge service doctor

relay-knowledge config set background.enabled true
relay-knowledge config set background.silent_updates true
relay-knowledge config set background.max_cpu_percent 25
relay-knowledge config set background.max_disk_bytes 20GB

relay-knowledge index status
relay-knowledge index repair --scope <scope-id>
relay-knowledge index pause --kind vector
relay-knowledge index resume --kind vector
```

`service status` 和 `doctor` 至少输出:

- OS service 状态、pid、启动时间、最近重启原因。
- 当前配置摘要，尤其是 silent update 是否启用。
- `graph_version`。
- 每类索引的 `indexed_graph_version`、lag、状态、最近错误。
- 事件队列深度、oldest task age、dead-letter 数量。
- 最近 heartbeat 和 work progress heartbeat。
- SQLite WAL 大小、checkpoint age、数据库 busy 次数。
- 数据目录、cache 目录、索引目录和日志目录磁盘占用。
- 最近自愈动作，例如 recovered leases、replayed refresh requests。

Web 界面可以调用同一组 application service 展示状态，但不能私自读取数据库或索引目录。

当前前台 `service run` 在接受 resident adapter work 前会执行 startup recovery pass:
读取当前 graph version 与 index cursors，计算最大 index lag，并刷新所有 stale v1
index kind。该最小 reconciler 不替代后续持久化 task lease、dead-letter 和平台
service manager 集成；它保证 crash 后 graph 已提交但 index cursor 落后时，常驻
进程启动会先恢复派生索引新鲜度再接收 MCP/ACP 请求。无 MCP transport 启用时，
`service run` 仍保持前台进程并等待平台 shutdown signal，便于 systemd、Windows
Service 或 launchd 管理进程生命周期。

## 7. 可观测性和告警

后台服务至少暴露这些 metrics:

| 指标 | 类型 | 含义 |
| --- | --- | --- |
| `relay_service_uptime_seconds` | gauge | 后台服务运行时长 |
| `relay_service_restarts_total` | counter | 服务重启次数 |
| `relay_background_silent_updates_enabled` | gauge | 静默更新是否启用 |
| `relay_background_task_queue_depth` | gauge | 后台任务队列深度 |
| `relay_background_task_oldest_age_seconds` | gauge | 最老未完成任务年龄 |
| `relay_background_task_retries_total` | counter | 后台任务重试次数 |
| `relay_background_dead_letter_total` | counter | dead-letter 任务数量 |
| `relay_index_lag_versions` | gauge | 各索引落后图版本数 |
| `relay_index_refresh_duration_ms` | histogram | 索引刷新耗时 |
| `relay_storage_wal_size_bytes` | gauge | WAL 文件大小 |
| `relay_storage_checkpoint_age_seconds` | gauge | 距离上次 checkpoint 时间 |
| `relay_reconciler_actions_total` | counter | reconciler 执行动作数 |

告警建议:

- service 反复重启。
- watchdog heartbeat 超时。
- task oldest age 超过阈值。
- index lag 持续超过阈值。
- dead-letter 持续增长。
- WAL 大小持续增长且 checkpoint 失败。
- 静默更新启用但授权 source 长时间没有成功刷新。

日志和 trace 必须包含 `task_id`、`task_kind`、`scope_id`、`graph_version`、`index_kind`、`indexed_graph_version`、`attempt_count`、`lease_owner`、`error_kind`。

## 8. 实施顺序

建议按以下顺序落地:

1. 定义后台配置模型和 diagnostics snapshot，不先写平台安装器。
2. 实现 foreground service runtime，让本地开发可直接观察 heartbeat、队列和 shutdown。
3. 引入持久化 task lease、retry backoff、dead-letter 和 reconciler。
4. 将 index refresh 改为从 mutation log 和持久化 cursor 恢复。
5. 增加资源预算和有界队列，把 CPU 密集任务移入 worker pool。
6. 实现 `service install|status|doctor|logs` 的平台适配，先 Linux systemd，再 Windows Service 和 launchd。
7. 增加磁盘清理、WAL checkpoint、旧索引 TTL 等维护任务。
8. 接入 metrics、告警示例和 Web 状态展示。

## 9. 测试和验收

必须覆盖这些测试场景:

- `service_status_reports_silent_update_configuration`
- `background_queue_rejects_when_capacity_is_exceeded`
- `health_queues_scoped_backlogs_larger_than_initial_budget`
- `refresh_indexes_drains_scoped_backlogs_larger_than_single_page`
- `index_refresh_resumes_after_process_restart`
- `expired_task_lease_is_requeued_once`
- `expired_task_lease_dead_letters_after_attempt_budget`
- `reconciler_replays_missing_index_refresh_after_graph_commit`
- `failed_vector_index_does_not_block_bm25_refresh`
- `query_reports_stale_when_index_lags`
- `maintenance_checkpoint_does_not_block_query_runtime`
- `dead_letter_records_repeated_non_retryable_failure`
- `resource_budget_pauses_low_priority_tasks`

当前 Rust v1 主路径已经覆盖 bounded index refresh queue、超过初始容量的 scoped
backlog 诊断降级、显式 refresh 的 queue-cap 错误、跨进程 enqueue 原子容量检查、
active lease/attempt 守卫、running task target 保护、superseded refresh attempt
重置、lease 过期恢复和 dead-letter、diagnostics reconciler
保留 dead-letter 隔离、mutation-log replay、scoped cursor freshness、
health/service stale diagnostics 和 foreground `refresh_indexes` drain。后续 service manager、silent update 配置、
maintenance checkpoint、资源预算暂停和完整 dead-letter operator 流程仍需按上表补齐。

验收标准:

- 用户可以明确开启、关闭和查看静默后台更新。
- 进程崩溃重启后，图事实不丢失，索引任务能从 cursor 恢复。
- 索引器假死或落后时，diagnostics 能显示 lag、oldest task age 和最近错误。
- CPU 密集任务不会阻塞 async runtime，也不会让查询热路径同步等待。
- WAL、日志、临时文件和旧索引分区都有上限或清理策略。
- 任何查询都能说明自己使用的新鲜或 stale 索引版本。

## 10. 参考实践

- systemd service unit 支持 restart policy、notify readiness 和 watchdog，适合作为 Linux 后台服务生命周期管理层: <https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html>
- `sd_notify` / `WATCHDOG=1` 是 systemd 下进程报告 ready、status 和 watchdog heartbeat 的标准机制: <https://www.freedesktop.org/software/systemd/man/latest/sd_notify.html>
- Windows Service 可以配置 failure actions，用于异常退出后的自动重启和恢复命令: <https://learn.microsoft.com/en-us/windows/win32/services/service-control-handler-function>
- `sc failure` 可配置 Windows 服务失败后的 restart、run command 等动作: <https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/sc-failure>
- launchd 使用 `KeepAlive`、`RunAtLoad` 和节流机制托管 macOS 后台 agent / daemon: <https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html>
- SQLite WAL 允许读写并发，但 checkpoint 策略会影响延迟和磁盘增长，后台维护任务必须显式治理: <https://www.sqlite.org/wal.html>
