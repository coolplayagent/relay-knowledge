# 代码索引保留策略

## 1. 目标

代码索引清理由两层策略和一种物理删除机制组成：

- Scope retention（作用域保留）限制单个仓库内历史索引代数。
- Repository retention（仓库级保留）限制多少个合格仓库继续持有已发布索引。
- Durable phased scope GC（持久化分阶段作用域垃圾回收）执行两种策略选中的全部物理索引删除。

仓库级 retention 是自动 maintenance（维护），不是 repository removal（仓库删除）。它保留仓库注册和 alias（别名），后续索引无需重新注册即可发布新的 active scope（活动作用域）。

## 2. Scope Retention（作用域保留）

普通 retention 保持不变。成功发布后保护以下集合的并集：

- active scope（活动作用域）与最近两个成功 scope；active 通常已包含在这两个 scope 的窗口内；
- 最近一次成功 incremental predecessor（增量前驱）；
- 每个 active worktree overlay（活动工作树覆盖）的 clean base（干净基线）；
- 每个 unfinished task（未完成任务）的 target（目标）和 base（基线）；
- 每个 repository-set member pin（仓库集合成员固定引用）。

未受保护的旧 scope 会原子标记为 `retiring`（退役中），并创建持久化 scope-GC job（作用域垃圾回收任务）。Reader（读取方）和 incremental-base selection（增量基线选择）立即排除 retiring scope。后续有界 maintenance transaction（维护事务）按阶段删除 fact（事实）、search document（搜索文档）、software projection（软件投影）、checkpoint（检查点）、workspace state（工作区状态）和 scope metadata（作用域元数据）。

## 3. Repository Retention（仓库级保留）

`RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES` 的取值范围是 `1..=i64::MAX`，默认值为 10。该上限与 SQLite 持久化整数范围一致，并在维护启动前校验。具有当前已发布 scope 的仓库计入数量。属于用户管理 repository set（仓库集合）的仓库既不计数，也不会成为候选。自动 workspace set（工作区集合）通过由仓库标识确定性生成的 `set_id`（集合标识）识别；可编辑 alias（别名）不参与授权或豁免判断。

成功发布后，以及 resident service（常驻服务）或 `repo index-worker`（仓库索引工作器）执行每次有界 retention pass（保留处理轮次）前，都会运行调度。全局最多存在一个活动 repository-retention parent job（仓库保留父任务）。合格已索引仓库数超过上限时，调度器选择当前成功发布时间最旧的仓库，并持久化：

- `repository_id`（仓库标识）；
- `initial_scope`（初始作用域），即选择时观测到的 active scope；
- `cutoff_ms`（截止时间），即调度时间；
- `cutoff_publication_generation`（截止发布代次），即初始作用域对应的成功发布代次；
- phase（阶段）、时间戳和 last error（最近错误）。

Candidate discovery（候选发现）在一次调度事务中通过 `(activity_ms, repository_id)` 顺序索引，从持久化 `code_repository_retention_activity`（仓库保留活动）投影中最多读取 64 条记录。可能影响仓库 current scope 或成功发布时间的变更会把仓库加入 `code_repository_retention_activity_dirty`（仓库保留活动脏队列）。每次调度事务在扫描前通过有索引的点查询最多刷新 64 个脏仓库；如果仍有脏数据，维护保持 pending（待处理），候选选择会等待，而不会读取陈旧投影。Schema marker（模式标记）版本 6 会在升级时创建并回填该投影。

Dirty activity（脏活动）的入队通过显式的索引存在性检查保证幂等，而不依赖 trigger（触发器）局部的冲突策略。SQLite 可能把外层语句的 UPSERT 冲突策略传播到 trigger 语句，因此相同 tree 的 commit 发布和终态任务复用不能依赖 `INSERT OR IGNORE` 对同一仓库的 dirty marker 去重。Schema marker 版本 7 会在下次打开数据库时替换旧版 dirty-activity trigger 定义，使现有数据库无需重写已索引事实即可获得该不变量。

如果候选页不足以判断是否超过上限，调度器会持久化单例 scan cursor（扫描游标）、合格数量、最旧候选和 catalog revision（目录修订号），并在后续维护轮次或进程重启后从游标继续。上限改变、已有活动父任务、投影刷新，或影响候选排序和资格的目录变更，都会丢弃游标并从第一页重启。未完成扫描或活动刷新属于活动维护，因此 `repo index-worker` 会持续报告 `maintenance_active=true`，直到处理完成或创建父任务。创建父任务前会重新校验所选仓库的 current scope、retirement state（退役状态）和用户管理 set 成员关系。

持久化父任务可以跨进程重启恢复。Maintenance pass 会加载父任务，并通过既有 scope-GC state machine（作用域垃圾回收状态机）选择和执行子 scope。仓库模式会有意跳过普通 active/latest-two protection（活动/最近两个保护），以清理 cutoff 前已存在的 scope。

## 4. 并发与保护

仓库级清理不阻止 index admission（索引准入），也不取消 queued（排队中）、retrying（重试中）或 running task（运行中任务）。它保护：

- 未完成任务引用的 target scope 和 base scope；
- 高于非零 `cutoff_publication_generation` 的成功发布代次；水位为零的旧父任务、发布代次为零的旧数据和 checkpoint（检查点）使用包含边界的 `cutoff_ms` 兼容判断；
- 与 `initial_scope` 不同的 active scope，包括同一毫秒发生的并发发布；
- 该并发发布需要的最近 incremental predecessor；
- active worktree base（活动工作树基线）。

初始 active scope 开始 retiring 时，事务会原子清空仓库的 current scope pointer（当前作用域指针），仓库状态返回 `registered`（已注册）和 stale（陈旧）。Repository row（仓库记录）、root（根路径）、alias、task history（任务历史）与父任务继续保留。旧子 GC phase 尚未完成时，新任务仍可发布。

仓库在调度后加入用户管理 set 时，maintenance 会删除父任务并停止退役更多 scope。Partitioned maintenance 会在调用 shard 前根据 control 结果刷新仓库模式，因此同一轮处理不会向分片转发已经失效的仓库 cutoff。已经标记为 `retiring` 的子 scope 仍会完成，因为 reader 已经不再把它视为 live（可用）。

在 partitioned storage（分区存储）中，如果 `initial_scope` 在 cutoff 所在毫秒以更高 publication generation（发布代次）重新发布，control database（控制数据库）也会识别该情况，并在父任务收敛前将该 scope 作为 shard retention pin（分片保留固定引用）传递，避免分片清理把这次同毫秒发布误判为 cutoff 前数据。

## 5. 完成与可观测性

Single-SQLite（单 SQLite）仅在没有仓库模式可退役 scope、且没有子 scope-GC job 时完成父任务。Partitioned SQLite（分片 SQLite）合并 control（控制面）与 shard（分片）retention state（保留状态），仅在两侧都收敛后完成父任务；catalog route（目录路由）继续遵循既有最终阶段顺序。

`repo status`（仓库状态）的 retention 输出会同时包含可选 repository-retention parent job 与子 scope-GC job。任一任务存在时，`maintenance_pending`（维护待处理）保持为 true。父任务报告仓库、初始作用域、时间和发布代次截止点、当前子 GC 阶段、时间戳和最近子任务错误。

cutoff（截止点）之后的成功 scope 会先跨 task（任务）与 checkpoint（检查点）发布记录去重，再应用有界历史上限。如果去重后的不同 scope 仍超过上限，则暂停退役并保持父任务待处理，不会根据不完整证据完成任务。Partitioned completion（分片完成判断）还要求合并后的 scope listing（作用域列表）未被截断。

## 6. 必须覆盖的测试

回归测试必须验证：

- 默认值为 10，正数 override（覆盖值）生效，0 被拒绝；
- 用户管理 set 成员被排除，automatic-workspace 成员仍计数，且不受 alias 或候选分页位置影响；
- 活动投影刷新和候选发现每次调度事务都限制为 64 条记录，候选读取使用活动顺序索引且不产生临时排序，并可在重新打开 SQLite 后续跑；
- 外层 scope/task UPSERT 内重复 dirty activity 入队仍保持幂等，包括相同 tree 的 commit 发布；
- 选择成功发布时间最旧的合格仓库；
- 重新打开 SQLite 后父任务和子任务可恢复；
- 首轮逻辑退役先于物理删除；
- 整仓索引清理后仓库注册与 alias 仍存在；
- 未完成任务、cutoff 后发布代次、同毫秒 incremental base 和同毫秒更高代次的重新发布均保留；
- task/checkpoint 的重复发布记录在历史上限前完成去重；
- 父任务 phase 和 last error 跟随当前子 GC 任务；
- 加入用户管理 set 后停止新增退役；
- 发布历史不完整时父任务保持待处理；
- Partitioned control 与 shard 收敛后才完成父任务。
