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

冷启动代码仓库 full indexing 采用同一恢复形态。`repo index` 会先做 tracked source-layout discovery，再持久化包含 source scope、input fingerprint、payload、resource budget、attempt count、retry cursor 和 lease 字段的 code-index task；前台 CLI 只启动有界单次 worker，常驻 `service run` 作为 master，在启动时先恢复过期 code-index lease 和孤儿 `code-index-worker-<pid>` lease，再用 `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` 控制的有界仓库索引 worker pool 消费持久队列。不同 fingerprint 的 task 可以并发排队，但 claim 必须保证每个仓库最多一个 live writer；不同仓库仍可 claim 独立 lease、checkpoint 和 retry 状态。完全相同的 full-index fingerprint 会复用未完成 task，避免同一 source scope 被重复 full rebuild。过期 running lease 会在 claim/status 路径报告 active work 前被恢复：可重试 attempt 进入 retry 并记录 `lease_expired` 诊断，耗尽 attempt 的任务进入 dead-letter，旧 worker 在 lease 过期、被接管、被判定为孤儿或显式 reset 后不能再 complete/fail task。服务启动还会检查 `code-index-worker-<pid>` lease owner，owner 进程已退出的 running task 会以 `lease_orphaned` 诊断恢复，仍存活 worker 持有的 lease 会保留。显式 repository index reset 可以把未完成 code-index task 重新排队，清空 lease owner、lease expiry、attempt count、retry cursor 和 last-error 字段，但同仓库存在未过期 running lease 时不能执行重排，不能删除已完成 indexed scope，不能复活 terminal dead-letter 历史，也不能绕过 lease-guarded completion。活跃 worker 会在昂贵 batch 解析前、每次提交 checkpoint batch 后、finalize 前后和完成 task 前续租；未实现可选 recovery/renewal hook 的 store 会将这些 hook 视为 no-op。冷启动 batch 的 Git blob 物化使用有界 `git cat-file` 命令，显式关闭 stdin 并设置超时；Git 子进程卡住时会返回 task failure，由 retry/dead-letter 处理，而不是永久持有 lease。checkpoint `updated_at_ms` 保持可见以诊断卡住任务。Repository-set overlay refresh task 采用同样的常驻服务模型：CLI/Web 的默认同步或 async 请求都先持久化到同一个有界队列。本地默认同步请求只在其精确 task 可被定向 claim 时 drain，否则返回 queued，`service run` 使用单个 repository-set overlay refresh worker 消费该队列。准入会在同一事务内 supersede 同 set 较旧的 queued/retrying task，unfinished task 每 set 最多 2 个、全局最多 128 个，claim 只允许同 set 存在一个 live writer。Overlay edge 与 member replacement 在同一个 attempt-scoped live-lease 事务内发布，takeover 使旧 attempt 回滚。入队和完成维护为每 set 保留 64 条 succeeded，并为 failed、dead-letter、cancelled 每个 state 各保留 32 条；每条审计清理 DELETE 最多删除 64 行。worker 失败时进入 retry 或 dead-letter。在跨 shard import/export 聚合实现前，`partitioned_sqlite` 仍不支持该 overlay 能力；成功发布 code index 后会启动下文所述的受保护 durable retention workflow。

手动 repository-set overlay 容量固定为每 set 最多 64 个 member、每次发布最多 64 个 fact-version replacement。一次 refresh 在所有 member 间共享 4,096 个 manifest chunk、16 MiB manifest path/content byte 和 32,768 个 manifest-derived item 上限；另行限制 8,192 条总 import、131,072 个 file/symbol export target、8,192 条总 edge 和 512 个 origin/target selector key。每个 import 最多检查 11 个 export 并记录最多 10 个 candidate ID，因此 `candidate_count` 是有界观测值。所有持久集合与读取路径都使用 cap-plus-one 探测，超限返回 `CapacityExceeded`/`qos_rejected`，不得截断后发布伪 `fresh` 结果；direct/selector read 最多检查 8,193 条 edge，并过滤任一端 scope 已 retiring 的 edge。Refresh/add/member remove 在任何删除前也最多检查 8,193 条已有 edge。整仓删除最多准入 64 个受影响 set，并预检查每个 overlay；set 更多或任一 overlay 超过 8,192 条 edge 时，事务保持不变并拒绝。遗留手动 overlay 超限时保持数据不变并拒绝请求，而不执行无界 `DELETE`；本版没有有界 repair 入口，只能由后续升级提供 repair tool，不能把它描述成已经闭环的清理能力。这些上限尚未覆盖显式启用的 automatic-workspace cross-edge materialization 路径；分阶段 scope GC 只限制其过期状态删除，单次 build 仍是已知资源有界性缺口，workspace detection 保持默认关闭。

Commit-driven indexing 是经过 reconciliation 的 durable workflow，不是 best-effort hook。watcher 启用时，窄 `.git` ref notification 会触发低延迟检查，resident loop 还会在启动时及每个配置 interval 受界对账 `HEAD`。periodic check 是 linked worktree、丢失 event 与重启恢复的 cursor。它把已解析 HEAD 与最近发布 clean commit 比较，固定不可变 base/head/tree，再以稳定 per-ref fingerprint 排入 Incremental task。重复 hint 会合并，而已有 queue claim 继续保证每仓库最多一个 active writer。手动 `repo update` 复用同一不可变解析与 durable task 路径。没有 clean base 时必须先 full index；delta 在应用注册 filter 前超过 512 个 changed path 时明确失败并提示 full index。

Partitioned SQLite 不把跨独立 WAL 文件的 `ATTACH` transaction 当作断电原子事务。Worktree snapshot 进入 shard transaction 前，control-only `BEGIN IMMEDIATE` transaction 会完整校验 live lease、attempt、publication generation、内容派生 target identity 与 64-slot capacity，再把 provisional task target 持久替换为 real scope。后续 shard publication 只会看到 exact、幂等 target；剩余的 attached authority update 仅承担 attempt lock，不再携带 rebind 业务状态。任一侧崩溃后，受界 task row 都是 durable handoff record，启动 lease recovery 与正常 retry 会重放同一 target，而 takeover generation 不能通过旧 fence prepare 或 publish。Single-SQLite 模式继续使用原有同事务 rebind。

成功发布后运行 durable retention：保留 active 与最近两个成功发布时间窗口的并集（窗口通常已包含 active）、最近一次成功增量的 predecessor、active worktree overlay 的 clean base，再加未完成 task 的 target/base 和 repository-set pin。一个事务先把一个旧 scope 原子标成 `retiring` 并记录 GC job；后续每个 maintenance transaction 推进一个 scope-GC phase，该 phase 在受影响的应用表之间合计最多删除 512 个物理行，包括 facts、FTS/search row、software projection、checkpoint、workspace state 或 scope metadata。同一 pass 另有 succeeded task audit、failure-class task audit 和 commit alias 各最多 512 行的固定配额，使主清理合计最多 2,048 个物理行，另加最多一个终态 GC-job bookkeeping 行。Retiring scope 对 reader 和 incremental base 不可用，常驻 worker 空闲时重试 job。当单仓库已经存在 64 个不同的 published、checkpoint 或 unfinished scope identity 时，准入会拒绝新 target，并返回 maintenance backpressure，直到 GC 释放槽位。这会限制保留的 live generation，并让 SQLite 复用释放页面，但不承诺数据库文件立即在 OS 层缩小；回收物理 high-water mark 需要另行执行显式、有界的 maintenance compaction。Retained/prunable scope 数组各最多 64 项；`scope_listing_truncated=true` 表示数组和显示计数只是有界诊断投影与可观察 lower bound，不是完整保护集合，control-plane pin 被截断时 partitioned shard maintenance 会暂停。Shard 进入最终 `scope_metadata` phase 前，maintenance 会先删除 control-plane route；崩溃后可重放确定性 shard job，不会暴露 stale route。同 tree commit 复用内容图，并使用每仓 256 条的 commit alias 窗口；升级前遗留 scope 尚无 alias row 时，会在下一次 same-content publication 事务内惰性保留旧 commit，而不是在数据库打开时执行无界 backfill。完成态 task history 按每仓库 128 条 successful 与 64 条 failed/dead-letter/cancelled 限制，同时为每个 protected scope 保留最新 success；`repo status` 暴露 job 进度和错误。被淘汰 ref 必须 full index。

即使在 `resident_single_process` 中，nginx-style 职责拆分也必须明确：master 拥有 runtime 配置、worker pool 尺寸、启动恢复、队列监督、service-manager shutdown 和诊断；worker 只拥有 leased task execution，不能绕过 application service 或 storage trait。这让本地运行时在不增加进程级部署成本的前提下获得 master-worker 架构的主要优势：有界并行、overload 时快速拒绝或返回 degraded status、崩溃后确定性恢复、queue depth 和 worker slot 可观测，以及索引运行时查询行为可预测。`service status` 必须暴露 code-index master-worker 诊断，包括 configured worker count、active worker slots、queue depth、queued/running/retrying/dead-letter task counts、running leases 和 last error。

大型 repository indexing 不能阻断服务 liveness 或普通读查询。SQLite 写入必须经过带有界 transient busy/locked retry 的单 writer lane；health、graph/status/report、file query 和代码查询应优先走有界只读连接读取 committed snapshot。锁竞争必须通过 task status/checkpoint 和有界 busy 诊断暴露，不能要求操作者杀掉竞争的 `relay-knowledge` 进程，也不能加入无界 SQLite wait。`health` 不执行 diagnostic reconcile 写入，不排队 refresh work，超过短预算时返回 stale/degraded `storage_busy`。代码查询的 `allow-stale` 策略在请求 ref 正在索引且新 scope 未 finalize 时读取上一个已完成 scope，并显式标记 stale/degraded；`wait-until-fresh` 才允许因为目标 scope 未完成而拒绝。

Overload 处理遵循 SRE 和 adaptive concurrency 原则：当队列、IO、CPU 或 provider budget 饱和时，系统优先拒绝新后台 work、延迟低优先级内容索引、保留查询热路径预算，并返回 retryable/paused/degraded 状态。

上文“active 加最近两个”的旧简写表示集合并集：latest-two window 通常已经包含 active，因此通常保留两个 clean successful scope，而不是三个。

没有常驻服务时，`repo index-worker` 同时是显式的有界 maintenance 恢复入口：每次最多推进一个 retention pass，并暴露 `maintenance_active` 与可选 `maintenance_error`。捕获到 error 时 false activity 值不能证明完成，code-index task 结果仍单独表示。这样仓库在 64-scope 准入边界被拒绝后，仍能先 drain GC 再重试，无需直接修改数据库。在 partitioned storage 中，control catalog route 会在有界 shard fact 删除期间一直作为计入容量的 reservation；该 route 由 coordinator 在最终 `scope_metadata` shard transaction 前删除，而不是由通用 control GC 提前删除。route 删除后若崩溃，会重放确定性 shard job，而不恢复 stale route。

## 6. 验收标准

- 崩溃重启后不会丢失必要刷新工作。
- dead-letter task 不被诊断路径自动复活。
- 后台 CPU/IO-heavy work 不阻塞 health liveness 和查询热路径。
- watcher lag、scan backlog、cursor invalidation 和 overload decision 可在 health/service doctor 中解释。
- 丢失 Git notification、linked-worktree metadata 和服务重启会通过受界 periodic HEAD reconciliation 收敛。
- scope 与完成态 task retention 保持有界，同时不删除未完成 task 或 repository set 引用的 base/target。
- split worker 部署保持 durable task lease、bounded retry/backoff、checkpoint replay、dead-letter isolation 和 per-repository active writer 约束。

---

导航: 上一章: [16. 统一 API 与交互层架构](16-unified-api-and-interface-architecture.md) | 下一章: [18. 可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md)
