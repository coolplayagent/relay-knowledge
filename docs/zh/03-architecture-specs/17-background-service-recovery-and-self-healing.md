# 后台服务、恢复与自愈

[中文](../../zh/03-architecture-specs/17-background-service-recovery-and-self-healing.md) | [English](../../en/03-architecture-specs/17-background-service-recovery-and-self-healing.md)

> 文档版本: 2.4
> 编制日期: 2026-08-17
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

跨进程 worker 的 lease 是数据面写入授权。续租、完成、失败和定向 orphan recovery 都比较已观察到的 owner、attempt 与单调递增 publication generation；orphan recovery 还比较已观察到的 expiry，防止延迟扫描撤销期间已经续租的 lease。worker 在未持有有效 lease、lease 过期、任一 attempt token 不匹配、task 被 reset、task 被接管或 task 进入 dead-letter 后，不能 complete、fail、续租或提交数据面写入。expiry 对该 attempt 仍不可逆；必须先恢复并重新 claim，推进 attempt 与 generation 后才能从 checkpoint 重放。控制面 status 必须能解释 active/running/retrying/dead-letter 的来源，不能用进程存在性推断任务成功。

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

手动 repository-set overlay 容量固定为每 set 最多 64 个 member、每次发布最多 64 个 fact-version replacement。一次 refresh 在所有 member 间共享 4,096 个 manifest chunk、16 MiB manifest path/content byte 和 32,768 个 manifest-derived item 上限。durable leased task 从不可变 member scope 按 `(source_scope, import_id)` 主键 cursor 以每页 512 条扫描 import row，最多扫描 262,144 条，再在内存中过滤 unresolved external candidate；resolved 和 local row 同样消耗扫描预算。另行限制 131,072 个 file/symbol export target、8,192 条跨 member candidate edge 和 512 个 origin/target selector key。没有 member export candidate 的 import 继续作为 unresolved source-scope fact，不会重复写入 overlay。每个 import 最多检查 11 个 export 并记录最多 10 个 candidate ID，因此 `candidate_count` 是有界观测值。所有扫描、持久集合与读取路径都使用 cap-plus-one 探测，超限返回 `CapacityExceeded`/`qos_rejected`，不得截断后发布伪 `fresh` 结果；最终事务在发布前重新验证 task lease 与 member-version snapshot。Direct/selector read 最多检查 8,193 条 edge，并过滤任一端 scope 已 retiring 的 edge。Refresh/add/member remove 在任何删除前也最多检查 8,193 条已有 edge。整仓删除最多准入 64 个受影响 set，并预检查每个 overlay；set 更多或任一 overlay 超过 8,192 条 edge 时，事务保持不变并拒绝。遗留手动 overlay 超限时保持数据不变并拒绝请求，而不执行无界 `DELETE`；本版没有有界 repair 入口，只能由后续升级提供 repair tool，不能把它描述成已经闭环的清理能力。这些上限尚未覆盖显式启用的 automatic-workspace cross-edge materialization 路径；分阶段 scope GC 只限制其过期状态删除，单次 build 仍是已知资源有界性缺口，workspace detection 保持默认关闭。

被重新 claim 的 full-index attempt 会重新准备 source plan，并且只在 checkpoint 证明 repository/scope/commit/tree/filter identity、task 的 durable resource budget、parsed 与 committed 计数相等、受界确定性路径前缀、合法 state/batch count 和精确 prefix last path 全部匹配时恢复 cursor。过期 attempt 的内存 parse overflow 从不视为 durable。只读 plan 校验会捕获完整 expected checkpoint，也会明确捕获“checkpoint 不存在”；session `begin` 在写事务内、修改 repository status 或 checkpoint row 前比较该 token，因此 preflight 与 `begin` 之间插入或推进的 checkpoint 会原样失败，不会把此前 fresh scope 改成 stale。损坏会返回内部 5xx 类一致性错误，把已 claim task 立即以 `checkpoint_invariant` 持久转入 dead-letter，并保留 repository/checkpoint 状态以供观察，不会伪装成用户输入，也不会猜测 cursor 后静默重试。另有一种窄例外：已完成的同内容 checkpoint 可能只携带旧 commit alias。若该内容 scope 仍处于 published、non-retiring、software-current 且 query-index-current 状态，fenced reconcile 会在一个只改 metadata 的事务内重新激活它，保留此前 active commit alias，更新 target checkpoint/receipt，并且绝不让 finalization page 修改仍可查询的 facts；partitioned reconcile 可以先通过 repository shard 定位 retained scope，再原子切换 catalog route。否则 Plan 才把该精确 checkpoint 情形分类为 `ContentEquivalentRestart` 而不是 `Resume`，SQLite 与 partitioned SQLite 都比较完整 preflight token。若 partitioned API 把 active shard 的 raw `finalizing:partitioned_publish` 投影为 `completed`，它必须先在精确 raw-token CAS 与当前 publication fence 下把 completed 状态物化，并保持 checkpoint timestamp 不变，再启动受 CAS 保护的 restart。Storage 随后返回绑定新 commit、零进度的 `indexing` checkpoint，application 校验后才能继续。该例外要求 completed state，以及 repository/scope/tree/filters/path count/resource budget 全部一致；partial/finalizing commit mismatch 和其他任何 mismatch 仍是 `checkpoint_invariant`，并立即进入 dead-letter。每个已知 `finalizing:*` checkpoint 都保留 facts，并从最后已提交 phase 之后继续；fenced 路径会在 software projection 前把 scope staging 与 workspace-import resolution 作为两个 stale phase 分别提交，因此任一 phase 都能恢复而不会提前发布，software-projection、partitioned-publication 与 completed 状态继续使用 publication shortcut，receipt 恢复响应也会报告 task 的 durable resource budget。Batch 跳号在 staging 前失败。checkpoint 已覆盖的 batch index 只执行 fence 校验并安全 no-op；即使 conflicting duplicate 内容不同，也不能重置 published staging manifest、替换 facts 或回退 progress。因此 retry 可以避免重复解析已提交工作，同时完整保留 lease、single-writer fence、bounded batch、FTS 写入、finalization 与 freshness 约束。该恢复优化不影响首次 cold-index attempt 的耗时。

仅为修复 active completed scope 而排队的 task 会先执行同样的 exact publication reconciliation。若 reconcile 仅因 software projection 缺失或 stale 而失败，对外 `completed` checkpoint 必须继续进入 fenced automatic-workspace cleanup 与 software projection refresh，不能误报 partitioned handoff failure；真正 raw `finalizing:partitioned_publish` checkpoint 若仍无法 reconcile，则继续 fail closed。该区分使 active-scope derived repair 保持有 lease、可重放，同时不弱化 raw shard-to-catalog publication barrier。

Commit-driven indexing 是经过 reconciliation 的 durable workflow，不是 best-effort hook。watcher 启用时，窄 `.git` ref notification 会触发低延迟检查，resident loop 还会在启动时及每个配置 interval 受界对账 `HEAD`。periodic check 是 linked worktree、丢失 event 与重启恢复的 cursor。它把已解析 HEAD 与最近发布 clean commit 比较，固定不可变 base/head/tree，再以稳定 per-ref fingerprint 排入 Incremental task。重复 hint 会合并，而已有 queue claim 继续保证每仓库最多一个 active writer。手动 `repo update` 复用同一不可变解析与 durable task 路径。没有 clean base 时必须先 full index；delta 在应用注册 filter 前超过 512 个 changed path 时明确失败并提示 full index。

Partitioned SQLite 不把跨独立 WAL 文件的 `ATTACH` transaction 当作断电原子事务。Worktree snapshot 进入 shard transaction 前，control-only `BEGIN IMMEDIATE` transaction 会完整校验 live lease、attempt、publication generation、内容派生 target identity 与 64-slot capacity，再把 provisional task target 持久替换为 real scope。后续 shard publication 只会看到 exact、幂等 target；剩余的 attached authority update 仅承担 attempt lock，不再携带 rebind 业务状态。任一侧崩溃后，受界 task row 都是 durable handoff record，启动 lease recovery 与正常 retry 会重放同一 target，而 takeover generation 不能通过旧 fence prepare 或 publish。Single-SQLite 模式继续使用原有同事务 rebind。

Partitioned checkpoint route 会在 shard 提交 session begin 或 batch 前，在当前 task fence 下幂等 staged。该 staged route 是 crash-recovery locator，而不是 publication claim：active-only read 会忽略它，checkpoint lookup 则能在 reopen 后定位 shard。若在 shard commit 前崩溃，则没有 checkpoint；若在其后崩溃，则会暴露 exact checkpoint，并由同一 task/fence 通过 compare-and-swap resume。

成功发布后运行 durable retention：保留 active 与最近两个成功发布时间窗口的并集（窗口通常已包含 active）、最近一次成功增量的 predecessor、active worktree overlay 的 clean base，再加未完成 task 的 target/base 和 repository-set pin。一个事务先把一个旧 scope 原子标成 `retiring` 并记录 GC job；后续每个 maintenance transaction 推进一个 scope-GC phase，该 phase 在受影响的应用表之间合计最多删除 512 个物理行，包括 facts、FTS/search row、software projection、checkpoint、workspace state 或 scope metadata。同一 pass 另有 succeeded task audit、failure-class task audit 和 commit alias 各最多 512 行的固定配额，使主清理合计最多 2,048 个物理行，另加最多一个终态 GC-job bookkeeping 行。Retiring scope 对 reader 和 incremental base 不可用，常驻 worker 空闲时重试 job。当单仓库已经存在 64 个不同的 published、checkpoint 或 unfinished scope identity 时，准入会拒绝新 target，并返回 maintenance backpressure，直到 GC 释放槽位。这会限制保留的 live generation，并让 SQLite 复用释放页面，但不承诺数据库文件立即在 OS 层缩小；回收物理 high-water mark 需要另行执行显式、有界的 maintenance compaction。Retained/prunable scope 数组各最多 64 项；`scope_listing_truncated=true` 表示数组和显示计数只是有界诊断投影与可观察 lower bound，不是完整保护集合，control-plane pin 被截断时 partitioned shard maintenance 会暂停。Shard 进入最终 `scope_metadata` phase 前，maintenance 会先删除 control-plane route；崩溃后可重放确定性 shard job，不会暴露 stale route。同 tree commit 复用内容图，并使用每仓 256 条的 commit alias 窗口；升级前遗留 scope 尚无 alias row 时，会在下一次 same-content publication 事务内惰性保留旧 commit，而不是在数据库打开时执行无界 backfill。完成态 task history 按每仓库 128 条 successful 与 64 条 failed/dead-letter/cancelled 限制，同时为每个 protected scope 保留最新 success；`repo status` 暴露 job 进度和错误。被淘汰 ref 必须 full index。

Search cleanup 状态机先删除带 metadata owner 的 row，再以 durable `search_orphans` phase 按全局 FTS rowid 空间逐页推进，每页最多检查并删除 512 行。它不会对 FTS 的 `UNINDEXED` scope 字段执行无界 predicate scan：每页先按 rowid 受界物化，只删除属于 retiring scope 且没有 exact rowid/scope/kind/record/path metadata owner 的 row；若目标 row 仍有 metadata owner，则整页在删除或推进 cursor 前 fail closed。即使交错页零命中，job 也会持久化最后检查的 rowid。数据删除、deleted-row 计数、cursor 推进和 phase 切换处于同一事务；rollback/reopen 重放同一 cursor，EOF 在继续后续 fact phase 前清空 cursor。该 phase 安装时只会原子 rewind 已越过它的 pre-capability job，避免 crash 使 legacy FTS orphan 在 scope metadata 消失后永久滞留。

即使在 `resident_single_process` 中，nginx-style 职责拆分也必须明确：master 拥有 runtime 配置、worker pool 尺寸、启动恢复、队列监督、service-manager shutdown 和诊断；worker 只拥有 leased task execution，不能绕过 application service 或 storage trait。这让本地运行时在不增加进程级部署成本的前提下获得 master-worker 架构的主要优势：有界并行、overload 时快速拒绝或返回 degraded status、崩溃后确定性恢复、queue depth 和 worker slot 可观测，以及索引运行时查询行为可预测。`service status` 必须暴露 code-index master-worker 诊断，包括 configured worker count、active worker slots、queue depth、queued/running/retrying/dead-letter task counts、running leases 和 last error。

大型 repository indexing 不能阻断服务 liveness 或普通读查询。SQLite 写入必须经过带有界 transient busy/locked retry 的单 writer lane；health、graph/status/report、file query 和代码查询应优先走有界只读连接读取 committed snapshot。锁竞争必须通过 task status/checkpoint 和有界 busy 诊断暴露，不能要求操作者杀掉竞争的 `relay-knowledge` 进程，也不能加入无界 SQLite wait。`health` 不执行 diagnostic reconcile 写入，不排队 refresh work，超过短预算时返回 stale/degraded `storage_busy`。代码查询的 `allow-stale` 策略在请求 ref 正在索引且新 scope 未 finalize 时读取上一个已完成 scope，并显式标记 stale/degraded；`wait-until-fresh` 才允许因为目标 scope 未完成而拒绝。

Overload 处理遵循 SRE 和 adaptive concurrency 原则：当队列、IO、CPU 或 provider budget 饱和时，系统优先拒绝新后台 work、延迟低优先级内容索引、保留查询热路径预算，并返回 retryable/paused/degraded 状态。

上文“active 加最近两个”的旧简写表示集合并集：latest-two window 通常已经包含 active，因此通常保留两个 clean successful scope，而不是三个。

没有常驻服务时，`repo index-worker` 同时是显式的有界 maintenance 恢复入口：每次最多推进一个 retention pass，并暴露 `maintenance_active` 与可选 `maintenance_error`。捕获到 error 时 false activity 值不能证明完成，code-index task 结果仍单独表示。这样仓库在 64-scope 准入边界被拒绝后，仍能先 drain GC 再重试，无需直接修改数据库。在 partitioned storage 中，control catalog route 会在有界 shard fact 删除期间一直作为计入容量的 reservation；该 route 由 coordinator 在最终 `scope_metadata` shard transaction 前删除，而不是由通用 control GC 提前删除。route 删除后若崩溃，会重放确定性 shard job，而不恢复 stale route。

带 lease 的 code-index finalization 使用显式 step-boundary protocol，不依赖并发 heartbeat 抢回 SQLite writer mutex。Parser 与 writer drain 后，Application 会重新读取 durable checkpoint，要求完整 repository/scope/commit/tree/filter/budget identity 和已提交 file prefix 全部匹配，并根据重新读取的 reference count 派生 hard step bound，而不是沿用 begin-time count；checkpoint 缺失或漂移会在首个 quantum 前 fail closed。随后 Application 先按权威时间严格续租，只推进一个 durable finalization quantum，再续租一次后才处理 `Pending` 或 `Ready`；每个 `Pending` token 必须不同于前一个 token，且该派生 hard step bound 防止无界循环。SQLite 每个 quantum 后释放 writer mutex，partitioned storage 把同一 one-step contract 委派给 staged shard。每个 quantum 都在 operation 前后校验 publication fence；校验先取得精确 authority writer lock，再采样 execution time。即使单个 index build 本身超过 lease，operation 后校验也会拒绝已过期 attempt，并把该 index 与 checkpoint cursor 一并回滚，不能复活 lease 或发布部分进度。Lease claim、全局过期回收、运维 reset、renewal、completion 与 failure transition 同样只在 `BEGIN IMMEDIATE` 已取得 writer lock 后采样 execution time；caller `now_ms` 只表示观测时间，未来观测会 fail closed，存活/过期判定、lease deadline、retry deadline 和写入时间戳全部使用权威 execution time。权威时间读取失败或无法表示时必须返回 invariant error 并中止 publication transaction；publication fence 不得在时钟采样失败时回退为 epoch zero 或截断值。Expiry 永不复活，owner/attempt/generation/authority CAS 仍保持严格。Post-index optimize/checkpoint maintenance 只在 task completion 已 terminal 后 best-effort 运行并记录诊断，不能反转 completed 结果。

大仓 staged finalization 把同一 one-step contract 先用于普通 reference resolution，再用于 grouped-v2 reference-search 的受界 cleanup、discover 与 build page。普通 resolution 记录 `finalizing:resolve_references:v1:resolve:{page}:{count}:{cursor_digest}`；progress row 保存 exact cursor 与从 resource budget 派生的 limit，direct 或 nested query-index-repair reopen 必须在任何写入前让 page、count、digest、冻结 committed count 与 cursor 全部匹配。Grouped cleanup 按 record ID 对 exact metadata owner 做 keyset；discover 按 reference fact 做 keyset并为每个 exact identity 持久化一个 owner 与 occurrence count；build 按 group ID 做 keyset并使用 canonical encoder。Grouped checkpoint token 携带 protocol version、stage 与 completed page ordinal，record/reference/group cursor 和冻结 totals 保存在 checkpoint-owned progress row 中，直至精确 group total 完成并发布 current manifest。两套协议都为 progress/checkpoint control mutation 留位，把完整记录的保守上界计入同一 byte quantum，并使用 length-only lazy scan，在获取超预算 cursor 或构造大 payload text 前停止。Rollback 或 reopen 因而按同一 keyset page 重放，不从 FTS metadata 推断进度，也不重启 parser/blob work。每页前后 shard 都必须仍在本地不可查询；partitioned storage 还必须在同一 live fence 下证明 catalog route 仍由 exact task staged。若两个 page 之间遇到 append-only query-index repair，wrapper token 保存 exact inner token，每事务推进一个 descriptor，且只在完整 current-plan validation 后恢复该 token。Stale fence、route 改变、query-index collision、count/cursor mismatch、畸形 token、首行超过预算或 checked `CODE_INDEX_FINALIZATION_MAX_STEPS + 4 * references + 6` bound 耗尽都会 fail closed，且不能推进任一 cursor。保留的 grouped v1 cleanup/build 状态只允许在 live fence 下恢复：重新校验 durable budget、把 page limit 钳制到更严格的 v2 上限，并从 v2 cleanup page zero 重启。Publication 与 fresh serving 必须具有 current grouped manifest；没有 manifest 的 legacy post-progress scope fail closed，并由新的 fact-versioned full task 替换。

Version-3 query-index finalization plan 保持 version 1/2 建立的 17 个 ordinal identity 不变。Ordinal 16 仍是 v2 追加的 import `(source_scope, path, line_start, line_end)` lookup；ordinal 1 保留 legacy name、owner 与 columns，但成为 retired stable slot，不再新建也不自动删除。只有 v3 cursor 或 current coarse scan 才把缺失 unit 1 视为 complete。Parser 会在每个 writer quantum 保留规范 v1/v2 token 的 version，因此 legacy cursor 只有在物理 unit 1 的 exact shape 存在时才能越过该 ordinal；普通 v2 repair 与 reference-search v2 wrapper 遵守同一规则。Current formatter 输出 `finalizing:build_query_indexes:v3:N`、`finalizing:query_index_repair:v3:N:resume:P` 和对应 v3 reference-search wrapper。每个非终态 coarse checkpoint 在后续工作或 Ready 前重新校验 current plan，每 quantum 最多修复一个缺失 required descriptor。Repair checkpoint 保留完整 file prefix 并跳过 parser/blob/batch restart；稳定 `P` code 恢复 exact coarse state，partitioned repair 继续保持 raw/pending。终态 `completed` 校验既有 shape，但允许 required descriptor 缺失且不由该兼容路径改写。Startup 只做只读 exact-shape validation。每个 fresh Restart 都不再根据 path count 推断 batch 数，只在完整 chunks owner 为空时预建 ordinal 13/14；resume 与 populated-owner 路径都不创建，其他 missing heavy descriptor 继续延后。

Receipt recovery 必须服从 raw/public checkpoint eligibility 和精确 software projection。Receipt 绝不能绕过 repository/scope identity 或 software-projection freshness 校验；projection 缺失或 stale 时必须返回 pending，让同一个未完成 worker task 先执行 fenced refresh 再恢复响应。只有 raw/query-plan、scope status、精确 fresh projection 与最终 checkpoint gate 全部通过后，exact task/repository/scope receipt 才能直接返回成功而不重新发布 catalog；receipt 不存在时才执行该幂等发布。即使 receipt 已存在，raw repair token 和 current plan 不完整的 raw partitioned-publication token 也同样返回 pending。对于 catalog inactive、由当前 task 持有的 staged partition，遗留 raw `completed` 只能由 exact begin 在 live fence 与 control-authority lock 下 CAS 恢复为 raw partitioned publication，从而进入 durable repair；不得放宽 catalog-active 历史终态例外。

Durable incremental delta 在交接到 `indexing` 前，还会把 canonical `incremental_summary_json` 写入 scope checkpoint。Receipt 受冻结 byte/file/row budget 约束，并绑定 task、base commit、changed/skipped/deleted/affected 计数、blob/parse/write/degraded 计数与单个 delta batch。同一 task 只有在 repository、scope、commit、tree、filters、resource budget、live fence 与 checkpoint state 全部匹配后才能消费；因此 completed checkpoint 的 software projection 若 stale，会继续执行 receipt-owned finalizer，在 fenced projection repair 后恢复原始 delta metrics。另一个 task 可以采用相同 terminal content scope，但绝不能复用第一个 task 的指标：terminal checkpoint CAS 必须在同一 fenced adoption 或 generic-repair transaction 中清除旧 receipt，采用方响应使用既有 no-work summary。非终态 task mismatch 必须零写失败。Storage 会在每个 finalizer transaction 内再次校验 ownership；只有 terminal `completed` 或 raw partitioned-publication checkpoint 可以把 receipt 转移给没有 incremental base 的 session，其 CAS 必须绑定精确旧 state 与 canonical receipt。CAS 前 crash 保留旧 receipt，CAS 后 crash 留下 generic terminal checkpoint，两条路径都不能伪造或重放另一 task 的 delta summary。

Ad hoc direct writer 不能绕过 recovery ownership。单库 direct checkpoint 本身就是 durable queue reservation：其他 target 不能越过未被 task 持有的非终态 checkpoint 入队，而同 scope task 接管后会立即 fence 后续 direct batch、finalization 与 automatic-workspace cleanup quantum。Partitioned data plane 不提供 unfenced recovery mode；snapshot、checkpoint、batch、finalization 与 workspace cleanup 都必须携带当前 durable task fence，避免 control authority 与 shard progress 在取消、接管或重启后分叉。

可选 renewal no-op 兼容只适用于未声明支持 fenced single-step finalization 的 store。实现 single-step contract 的 store 必须同时实现 strict renewal，否则 leased task 会在下一 writer quantum 前 fail closed。

## 6. 验收标准

- 崩溃重启后不会丢失必要刷新工作。
- dead-letter task 不被诊断路径自动复活。
- 后台 CPU/IO-heavy work 不阻塞 health liveness 和查询热路径。
- watcher lag、scan backlog、cursor invalidation 和 overload decision 可在 health/service doctor 中解释。
- 丢失 Git notification、linked-worktree metadata 和服务重启会通过受界 periodic HEAD reconciliation 收敛。
- scope 与完成态 task retention 保持有界，同时不删除未完成 task 或 repository set 引用的 base/target。
- 仓库级 retention 保留仓库注册和并发索引工作，且绝不选择用户管理 repository-set member。
- split worker 部署保持 durable task lease、bounded retry/backoff、checkpoint replay、dead-letter isolation 和 per-repository active writer 约束。
- Partial full-index retry 只跳过经过严格校验的 committed prefix；无效 checkpoint 不能被推断为进度，也不宣称改善首次 cold-index 性能。

---

导航: 上一章: [16. 统一 API 与交互层架构](16-unified-api-and-interface-architecture.md) | 下一章: [18. 可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md)
