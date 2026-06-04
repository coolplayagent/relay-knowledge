# 后台服务、恢复与自愈

[中文](../../zh/03-architecture-specs/17-background-service-recovery-and-self-healing.md) | [English](../../en/03-architecture-specs/17-background-service-recovery-and-self-healing.md)

> 文档版本: 2.1
> 编制日期: 2026-06-04
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

后台服务不是 unmanaged CLI loop。长运行刷新、索引、维护、诊断和静默更新必须托管在平台 service manager 之下，并以有界资源、持久租约、启动调和和 dead-letter 保证可恢复。

## 2. 运行模式

| 平台 | 管理器 |
| --- | --- |
| Linux | systemd |
| macOS | launchd |
| Windows | Windows Service |

CLI 可以生成服务定义和执行 doctor，但不应伪装成后台常驻管理器。

服务化部署支持 `resident_single_process` 和未来 `split_worker_preview`。单进程模式中控制面 API、startup reconciler、operator 和 worker 同进程运行；split worker 模式只允许独立 worker 从控制面 claim 持久 task 后工作，不能自建调度循环、直接读写 shard、跳过 QoS 或绕过 application service。

## 3. 工作队列

所有后台任务都有 kind、scope、priority、budget、attempt、lease owner、lease expiry、target graph version、payload hash 和 last error。队列容量是硬上限；入队失败返回 overload/retryable error。

跨进程 worker 的 lease 是数据面写入授权。worker 在未持有有效 lease、lease 过期、attempt count 不匹配、task 被 reset、task 被接管或 task 进入 dead-letter 后，不能 complete、fail、续租或提交数据面写入。控制面 status 必须能解释 active/running/retrying/dead-letter 的来源，不能用进程存在性推断任务成功。

## 4. Reconciler

启动调和器负责：

- 重放 mutation log 中未完成的 index refresh。
- 回收过期 lease。
- 保留 dead-letter 隔离。
- 报告 index lag、queue depth、stale scope 和 failed cursor。
- 修正运行中 task 完成时 graph version 已前进的 cursor 状态。

## 5. 静默更新

静默更新必须用户可配置、可暂停、可观测、可回滚。它只能在授权 scope 内刷新图数据和派生索引，并暴露 fresh、stale、paused、degraded、failed 状态。
常驻本地文件索引遵守同一规则：扫描器只处理已配置的绝对路径 root，
在扫描前拒绝相对路径配置，持久化 cursor 和诊断，执行扫描/查询 timeout 预算，
报告被截断 root、扫描错误、新鲜度和 lag，不能阻塞查询路径，也不能静默扩大到未授权磁盘。

文件系统 watcher 和 scan worker 必须按平台能力降级：Windows 可使用 USN cursor，macOS 可使用 FSEvents cursor，Linux 可使用 inotify/fanotify 或定期 bounded rescan。事件 overflow、journal reset、权限变化、root missing 和 cursor invalidation 都进入可恢复诊断状态，而不是触发无界全盘扫描。

冷启动代码仓库 full indexing 采用同一恢复形态。`repo index` 会先做 tracked source-layout discovery，再持久化包含 source scope、input fingerprint、payload、resource budget、attempt count、retry cursor 和 lease 字段的 code-index task；前台 CLI 只启动有界单次 worker，常驻 `service run` 在启动时先恢复过期 code-index lease 和孤儿 `code-index-worker-<pid>` lease，再用 `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` 控制的有界仓库索引 worker pool 消费持久队列。不同 fingerprint 的 task 可以并发排队，但 claim 必须保证每个仓库最多一个 live writer；不同仓库仍可 claim 独立 lease、checkpoint 和 retry 状态。完全相同的 full-index fingerprint 会复用未完成 task，避免同一 source scope 被重复 full rebuild。过期 running lease 会在 claim/status 路径报告 active work 前被恢复：可重试 attempt 进入 retry 并记录 `lease_expired` 诊断，耗尽 attempt 的任务进入 dead-letter，旧 worker 在 lease 过期、被接管、被判定为孤儿或显式 reset 后不能再 complete/fail task。服务启动还会检查 `code-index-worker-<pid>` lease owner，owner 进程已退出的 running task 会以 `lease_orphaned` 诊断恢复，仍存活 worker 持有的 lease 会保留。显式 repository index reset 可以把未完成 code-index task 重新排队，清空 lease owner、lease expiry、attempt count、retry cursor 和 last-error 字段，但同仓库存在未过期 running lease 时不能执行重排，不能删除已完成 indexed scope，不能复活 terminal dead-letter 历史，也不能绕过 lease-guarded completion。活跃 worker 会在昂贵 batch 解析前、每次提交 checkpoint batch 后、finalize 前后和完成 task 前续租；未实现可选 recovery/renewal hook 的 store 会将这些 hook 视为 no-op。冷启动 batch 的 Git blob 物化使用有界 `git cat-file` 命令，显式关闭 stdin 并设置超时；Git 子进程卡住时会返回 task failure，由 retry/dead-letter 处理，而不是永久持有 lease。checkpoint `updated_at_ms` 保持可见以诊断卡住任务。Repository-set overlay refresh task 采用同样的常驻服务模型：异步 refresh 请求会持久化带 lease 的任务，`service run` 用单个 repository-set overlay refresh worker 消费该队列。worker 失败时进入 retry 或 dead-letter；code-index 成功后还会保留 active scope、最近两个完成 scope 和未完成任务 scope，并淘汰更旧的代码 scope。

大型 repository indexing 不能阻断服务 liveness 或普通读查询。SQLite 写入必须经过带有界 transient busy/locked retry 的单 writer lane；health、graph/status/report、file query 和代码查询应优先走有界只读连接读取 committed snapshot。锁竞争必须通过 task status/checkpoint 和有界 busy 诊断暴露，不能要求操作者杀掉竞争的 `relay-knowledge` 进程，也不能加入无界 SQLite wait。`health` 不执行 diagnostic reconcile 写入，不排队 refresh work，超过短预算时返回 stale/degraded `storage_busy`。代码查询的 `allow-stale` 策略在请求 ref 正在索引且新 scope 未 finalize 时读取上一个已完成 scope，并显式标记 stale/degraded；`wait-until-fresh` 才允许因为目标 scope 未完成而拒绝。

Overload 处理遵循 SRE 和 adaptive concurrency 原则：当队列、IO、CPU 或 provider budget 饱和时，系统优先拒绝新后台 work、延迟低优先级内容索引、保留查询热路径预算，并返回 retryable/paused/degraded 状态。

## 6. 验收标准

- 崩溃重启后不会丢失必要刷新工作。
- dead-letter task 不被诊断路径自动复活。
- 后台 CPU/IO-heavy work 不阻塞 health liveness 和查询热路径。
- watcher lag、scan backlog、cursor invalidation 和 overload decision 可在 health/service doctor 中解释。
- split worker 部署保持 durable task lease、bounded retry/backoff、checkpoint replay、dead-letter isolation 和 per-repository active writer 约束。

---

导航: 上一章: [16. 统一 API 与交互层架构](16-unified-api-and-interface-architecture.md) | 下一章: [18. 可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md)
