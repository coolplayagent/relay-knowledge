# 大仓库索引弹性长预算模型

[中文](12-elastic-index-budgets.md) | [English](../../en/05-benchmarks/12-elastic-index-budgets.md)

## 目的

大仓库索引不再使用所有仓库共用的固定 180 秒硬超时。180 秒保留为历史性能基线，用于比较回归；实际执行预算根据目标仓库规模和观测吞吐率弹性计算。

## 预算计算

系统默认启用 `index_budget_mode=elastic`；未填写该字段等价于 `elastic`。只有显式选择固定/严格模式的目标才不使用弹性计算。对 Git source，评估器在固定 ref 上执行 `git ls-tree -r -z --name-only <ref>`，记录父 tree 的原始条目数。显式声明的 `expected_file_count` 始终保持权威，并且必须与产品 scope preview 的精确 selected count 相等；原始 Git 观测不会覆盖它。未声明 expected count 时，原始观测才作为弹性缩放输入。由于产品 scope 会排除 preset 文件，也可能展开可用 gitlink，这两个计数可以合理不同。

这里的 `N` 优先取显式 expected count，其次取固定 tree 的原始观测，二者都不存在时才取配置的 baseline count。预算按以下优先级计算：

1. 配置 `baseline_files_per_second` 时：

   `index_budget_ms = N / baseline_files_per_second × 1000`

2. 否则使用历史基线比例：

   `index_budget_ms = baseline_index_budget_ms × N / baseline_file_count`

3. 最终预算限制在 `max_index_budget_ms` 以内。

注册加索引预算另外加入 `register_overhead_budget_ms`，避免注册阶段的固定成本污染索引吞吐基线。命令级 timeout 会在预算秒数上再增加有限的恢复余量，但不会取消业务预算、checkpoint 或 freshness 要求。

## 持久化与恢复不变量

弹性预算只改变等待时间，不改变数据一致性合同：

- 每个 batch 先写 durable staging manifest，再由唯一 writer 提交 facts、FTS 与 checkpoint progress；最终 freshness 必须继续由 fenced code + software publication barrier 扣留。
- worker 使用有界 attempt lease；异常退出后由 orphan recovery 安全回收，不能抢占仍存活的 lease。
- reset 或 worker 重启只会在 session `begin` 内完成 exact checkpoint CAS 后继续；已提交 batch index 只执行 fence 校验并安全 no-op，不能替换已发布事实或重置 manifest。
- 未完成的 staging、edge finalize 或查询索引构建不能把 scope 标记为 fresh；状态必须继续报告为 indexing/stale/degraded。
- parser、队列、batch、FTS 写入和 SQLite 事务仍有固定上限，弹性预算不允许无界内存或无界重试。

### 查询索引写放大策略

稳定查询索引计划当前为 version 3，共 17 个有序 slot。Unit 1 保留 legacy identity
但已 retired：产品不创建也不删除 `code_repository_symbols_lookup`；同名索引若存在，
完整 shape 仍严格校验。只有 current v3 cursor 或 coarse scan 才把缺失 unit 1 视为
stable skip。规范 v1/v2 cursor 会跨 writer quantum 保留解析出的 version，若没有物理
legacy unit 1 就不能越过该 ordinal。Database startup 只校验既有 index，不执行
query-index DDL。每个 fresh Restart 无论 path count 或后续 byte/row 分批如何，都仅在
完整共享 chunks owner 为空时预建 unit 13/14；owner 已 populated 或任何 resume 都不
预建两者，其他 heavy descriptor 继续延后。

完整文件前缀 durable 后，finalization 每事务最多创建一个缺失 required descriptor，
并在同一事务推进对应的 `v3:<ordinal>` state。V1/v2 subphase 与 v2 repair token 继续
可读且不重新解释 ordinal；current formatter 始终输出 v3。FTS 写入、facts、staging
manifest、lease 与 freshness gate 均不改变。Direct snapshot 与 database import 没有
durable finalizer，因此必须在 fact mutation 前预建 required empty-owner index，随后
要求所有 required slot；populated owner 缺 required index 时 fail closed。

用于定位的 inactive Kubernetes snapshot 中，retired unit-1 index 占
420,921,344 bytes；`EXPLAIN QUERY PLAN` 对 identity、scoped-identity 与 reference
resolution grouping 均选择 `code_repository_symbols_name_path_lookup`。该 snapshot 有
486,702 个 symbol name/kind/path/hint group 与 2,879,261 条 reference。这些是诊断观测，
不是变更后的 wall-time 结论。Non-smoke `code_index_recovery_cases` self-iteration gate
会在 fast 与 performance-focused evaluation 中运行 `code_index_task_` 结构测试；若
unit 1 被重新创建、v1/v2 completed-prefix strictness 或跨 quantum version preservation
丢失，或 fresh Restart 不再于 empty owner 预建 chunk unit 13/14、并在单 path session
中继续延后全部其他 heavy index，门禁即失败。

已保留的 Kubernetes `finalizing:build_query_indexes:v2:12` cursor 维持原证明边界：
v3 继续进入 chunk unit 13/14 前，unit 0 到 12（包括物理 unit 1）仍必须 exact validate。
本次升级不会删除 420,921,344-byte legacy index，也不会把 inactive snapshot 伪装成完成；
chunk early-build 收益只适用于未来 chunks owner 为空的 fresh Restart。

### Durable incremental clone 与 finalization rail

所有带 fence 的 clean incremental 测量现在都走 durable clone，而不再按 base 大小选择 direct transaction。Base checkpoint 提供实际累计 `committed_fact_row_count`；proof 为零或不存在会在写入前转入 full staging。每个 clone page 受 task 冻结 row/byte quantum 约束，在同一 fence 下推进 metadata-indexed keyset 与 checkpoint/progress CAS，并在跳过 affected owner 时仍计入其 scan cost。Search copy 以 metadata 为权威；只有通过长度准入后才获取 payload，每个已接纳 page 会批量写入 exact contiguous FTS/metadata interval。Checked step proof 为 `5F + table_count + 4`，其中 `F` 是持久 fact proof，而不是 `batch_count × row_limit`。Terminal delta admission 还必须计入 affected-owner cascade cleanup 与 task-bound checkpoint receipt。

不带 workspace projection 的 worktree overlay 仅在完整 overlay mutation 装得进同一冻结 writer quantum 时保留 direct fast path。超预算 overlay 继续使用原 task、fence、target scope 与 synthetic identity：先建立 content-addressed worktree staging，再 clone immutable clean base，并按 file ownership 确定性划分 dirty delta。每个 worker step 最多提交一个 dirty batch；lease takeover 从持久 batch count 继续。任一 delta batch 启动前，terminal quantum 必须共同准入 owner cleanup、tombstone、固定 control row 与多批 receipt。一个受全局上限约束、不可再拆的文件可以单独占用一个 batch，但不得再吸收另一个文件。`code_index_task_` fast/performance recovery filter 包含 `oversized_worktree_code_index_task_delta_batches_and_recovers_between_leases`：该用例强制形成两个 dirty batch，在两批之间使 lease 过期并重新 claim，并断言精确 receipt、terminal path 和 completed checkpoint。这是 worktree fallback 的回归 rail；提高 queue、transaction 或 retry limit 不能替代它。在 auto-workspace manifest 具备单独的持久 recovery 协议前，对应 worktree projection 保持 fail closed。

最后一个 dirty-batch handoff 把完整但尚未发布的 target 交接到 `indexing`；之后 ordinary full finalizer 必须完成 query-index work、reference/import/call resolution、按精确 `F' = F - D + I` 更新 proof 的 Maven effective dependency replacement、grouped reference search、call rebuild、publication、workspace 与 software projection。响应丢失时从该 checkpoint 继续，不重新 clone 或 parse。直属 regression 覆盖 missing-proof 零写 fallback、受界 multi-page reopen、stale-fence takeover 拒绝、确定性 dirty-batch lease takeover、terminal cleanup 与 terminal-path restoration、query-index repair、receipt byte boundary、same-task response recovery、different-task neutral adoption，以及 Maven/reference 语义等价。Release-binary `index_performance_many_files` fast target 是性能采纳 rail：benchmark CI 直接断言其 1,024-file base、3 changed paths、2 blob reads、2 parsed files、completed task/checkpoint 与 3,000-ms incremental budget。`index_performance_wide_mixed_files` 保留对应 2,048-file、5,000-ms full/exhaustive rail。Clone 协议不会放宽这些预算。

## 观测与判读

报告应同时记录：实际文件数、基线文件数、基线吞吐率、计算出的索引预算、注册开销、最大预算、冷索引耗时、checkpoint 文件数和最终 freshness 状态。评估时应区分：

- “预算内完成”：任务成功且 scope fresh；
- “预算内仍在运行”：checkpoint 持续前进，不能提前返回成功；
- “预算上限触发”：任务保持可恢复状态并暴露 stale/degraded，不得删除已发布事实；
- “租约或事务错误”：属于一致性/恢复回归，不是单纯性能结果。

## 冷索引隔离与共享预加载

冷索引 target 不能继承前序仓库留下的图或 SQLite 状态。每次 evaluation 必须创建唯一且不可复用的 run root/home；heavyweight 和全量外部 target 在该 run home 下再分配仓库专属 `RELAY_KNOWLEDGE_HOME`，共享与隔离 home 仍共用一个全局 writer 锁。全局锁阻止默认仓库并发一次启动多个 disk-heavy 冷 writer，避免跨 store I/O 竞争污染延迟。收集完 commands、cases、metrics 和 report 证据后再清理。递归清理只能接受通过 lexical parent 与 canonical parent 双重校验的精确、非 symlink run/repository descendant，报错时同样执行，只有显式使用诊断参数 `--keep-workdirs` 才保留；不得删除复用或共享 root。

隔离结果回答“仓库能否在弹性预算内完成真实冷索引”，不覆盖共享 preload、alias 复用或顺序敏感性。小型 LevelDB workload 刻意保留在共享评估 home 上，负责这一回归面。反过来，共享 preload case 通过也不能证明冷索引吞吐，因为前序状态可能减少实际工作量。两类信号必须分别测量和报告，不能互相替代。

repository-set member 之间不得隔离。Temporal 与 OpenTelemetry set member 必须在同一个共享评估 home 中注册和索引，set overlay 才能解析全部成员。case 配置合并后会拒绝任何 member 上的 `isolated_index_home=true`。

冷索引完成性采用严格终态：checkpoint total 必须达到配置下限，committed-file count 必须等于 total-path count，仓库 indexed-file count 必须覆盖该 total，durable task 必须为 `succeeded`，checkpoint 必须为 `completed`，仓库状态必须为 `fresh` 且 `stale=false`。Task transition 本身也受 receipt gate 约束：必须存在同一 task 的 publication evidence 和匹配的 fresh target scope，因此 stale attempt、部分计数的 terminal label 或足够 parsed-file 数量都不能证明成功。每个隔离仓库若未声明更高下限，会隐式使用一文件下限；共享的 OpenTelemetry member 显式配置该下限。当前 `repo index` JSON 已提供 task、checkpoint 和仓库状态字段，但尚未暴露独立的 software projection 状态对象；在产品响应提供 projection state、stale 标记和 last error 前，harness 无法把 projection 完成性设为独立断言。

## Software projection 尾延迟合同

Code facts 完成不等于一次仓库索引已经完成。fenced full/incremental 流程必须继续构建 software projection；单 SQLite 只有在 projection 成功后，software status、code scope/repository freshness、checkpoint completion 与 publication receipt 才能一起可见。Partitioned store 先把 code/software facts 提交到目标 shard；其 catalog route 仍为 `staged`，由 durable task 的 `staged_task_id` 持有，active-only read 不会看见它。随后一个 control-database transaction 重新校验 fence 与 staged owner，并在同一事务中激活 repository/scope route、镜像 repository freshness/status、插入 publication receipt。这是围绕 shard/control 边界的可重试收敛，不是跨数据库原子：control transaction 前 crash 从 staged shard 继续，transaction 后 crash 复用 receipt。Durable task 的 `succeeded` 是紧随其后的独立 fenced completion，必须验证 receipt 与匹配的 fresh publication state；外部 worker 响应必须等待该 task terminal。

Lifecycle loader 的回归信号是 `candidate_document_count`、`candidate_chunk_count` 与 `candidate_materialized_bytes`。候选集合由既有 build、IaC、design parser 的支持集合推导；普通源码在 SQLite 内过滤，候选上限为 32,768 documents、262,144 chunks 和 256 MiB。单个候选文档按稳定行序物化一次并同时供三个 collector 使用，写入复用 prepared statements。单测以 2,000 个各 4,096 字节的无关 Rust chunk 加一个 Cargo manifest 固定回归面：只允许 1 document、1 chunk 和少于 128 字节跨过 loader 边界。该指标保护 I/O/物化放大，不替代真实仓库端到端延迟、FTS、edge、checkpoint 与 task-terminal 验收。

Software-file projection 另有直属存储放大 guard：owner test 投影 1,025 条有序 path，跨越两个完整的 512-row page 与一个 tail row，要求 path/role/status/version 序列逐条完全一致且得到 1,025 个不同 stable id；SQLite prepare-time authorizer 对整个 refresh 必须只观察到一次 `software_files` insert prepare。该 guard 在 owner 层固定 prepared-statement 与 `OFFSET` 回归；端到端采纳仍必须通过既有 isolated release-binary performance target 与未放宽的 freshness 检查。

当前 `repo index` JSON 尚未暴露独立的 software projection status 对象，因此 harness 不能单列 projection fact 数与 last error；publication barrier 已保证 projection 失败时 checkpoint 不会成为 `completed`、scope 不会成为 `fresh`，严格终态仍能间接拒绝提前成功。

## Parse 阶段放大回归线

变更前 exhaustive report `manual-evaluate-1786623651584251770-0-2786323` 只作为诊断证据，不是变更后的采纳结论。在隔离 home 中，两个 93,601-file Linux target 都触及有限 command timeout（`linux_sample` 1,201.124 秒，`linux_full` 1,201.224 秒）；Kubernetes 为 300.159 秒，dotnet/runtime 为 330.166 秒。同轮较小仓库能够完成，因此这些 timeout 保留了真实大仓性能回归面，不能据此增大 timeout 或跳过索引阶段。

三组直属 guard 固定 parse-stage amplification。Row cap 小于单文件事实数时，该文件仍必须原子输出；同一 fetch group 已解析的其余文件按 FIFO 保留，五个文件最终只允许一次 Git batch read。相同静态语言的重复调用必须返回同一 compiled tag-query allocation，不同语言不得共享，非法 query 编译后不能留下 cache entry。worker-thread-local Parser cache 还必须证明同一语言复用同一实例、不同语言隔离、每线程 64-entry hard cap，以及 zero-budget callback 取消后经 reset 能成功解析下一份独立文档。端到端采纳仍由既有 release-binary `--categories performance` target 及其未调整预算负责；query cache 主要减少非 C 的 tag-query 编译，Parser 实例复用预期减少同一 worker 多文件场景的 `Parser::new`/`set_language` 固定成本，parsed-overflow 复用则直接保护 fact-dense C/C++ 大仓。直属 guard 不提供 wall-time 结论。

一次 retained Kubernetes candidate 提供了 phase evidence，但不是已采纳的 latency 结果。360 秒截止时，隔离 clone 已用 61 个 batch 提交全部 30,353 个文件，并持久化 1,434,001 个 symbols、2,879,261 个 references 和 215,501 个 chunks；durable checkpoint 仍为 `finalizing:refresh_dependencies`。该 checkpoint 约在 296 秒更新，距 timeout 约 64 秒；按 checkpoint 表示“最后完成 phase”的语义，下一 reference-search rebuild 很可能正在执行，但尚未 durable 推进。该证据把 reference-search finalization 单独列为受界量化对象；缺少内部 phase timer 时，不能据此断言 rebuild 的精确耗时、证明新分页已经提速，或声称它超过 task lease。

第一段受界 persistence 优化只收窄基础 reference 的 statement 数：在 bundled SQLite 变量上限下，每个既有 code-index batch 把每条 reference 一次 16-bind execute 改为 `ceil(reference_count / 1,024)` 次 multi-values execute，每次不超过 16,384 个 bind；connection 变量上限更低时，实际行分组随之收窄。Bind vector 借用 record field 而不 clone 其中的字符串，固定满组 SQL 使用 `prepare_cached`，只有 tail shape 单独 prepare。直属 owner regression 用 1,025 条有序 reference 跨过默认边界，要求两个分组的基础事实与 intermediate search document 都保持输入顺序，验证已发布 batch replay 幂等，并在第二组注入 unique 冲突，要求整个 fact transaction 回滚且 staged manifest 仍可 replay。Lower-limit case 要求 31-variable 上限动态退化为单行分组，接受单行所需 16 个变量的 inclusive 边界，并在更低上限时 fail closed。该切片不改变 FTS finalization 或任何其他 fact owner。这些 statement-count、allocation 与恢复 guard 不构成 wall-time 加速结论；采纳仍由未调整的隔离 release-binary performance target 负责。

第二段受界 persistence 优化只对基础 symbol 应用相同的 statement-count 收窄。固定满组 statement 包含按输入顺序排列的 1,024 行、每行 17 列及 17,408 个借用 bind；较低的 runtime variable limit 会缩小该分组，一次调用最多另行 prepare 一个更小的 tail shape。Optional role JSON 是每个受界分组中唯一为 fact bind 物化的字符串，symbol-search 内容和输入顺序继续由既有 inserter 保持。直属 owner regression 用 1,025 条 symbol 跨过默认边界，并覆盖降低后的单行 variable limit、route-role JSON、有文档与 null field、第二组失败，以及 caller 对 facts、FTS rows 与 metadata 的整体 rollback。这些 guard 保护受界 statement amplification 与 transaction ownership，不构成 wall-time 结论；采纳仍以未调整的隔离 release-binary performance target 为准。

第三段受界 persistence 优化移除共享 search-document writer 反复构造和 prepare SQL 的开销，并把每个六列分组按 runtime SQLite variable limit 钳制到最多 1,024 个 document/6,144 个 bind。默认 full-shape FTS insert 只做一次进程级 SQL allocation 并使用 `prepare_cached`；较低 runtime full shape 进入 connection cache，唯一较小的 tail shape 则单次 prepare，不写入按 row count 扩张的 cache。直属 owner regression 要求 1,025 个 document 恰好执行两次主 FTS insert，覆盖 12/6/5-variable 对应两行/单行/拒绝的精确边界，从最高 orphan FTS rowid 之后开始，保留 affected-row/连续 interval/metadata affected-row 三重校验，并要求 tail ownership conflict 回滚已经 flush 的完整分组。该证据只保护受界 prepare/statement amplification 与原子恢复，不声称已经改善 Kubernetes 或其他真实仓库的 wall-time；采纳仍以未调整的隔离 release-binary performance target 为准。

当前 release candidate 的 focused 报告 `manual-evaluate-1787657485515273930-0-3038475.json` 对 1,024-file fixture 实测冷索引 382 ms、register 加冷索引 453 ms、incremental 423 ms，均在未放宽的产品预算内；release build 为 321/180,000 ms，named persistence suite 为 739/30,000 ms。346 个 gate、119 个 case 与 293 个 command 全部通过，报告记录 score 1.0、`score_accepted=true` 和 `adoption_status=would_accept`；手工 evaluation 未创建 commit。它关闭了前一工作树的 focused-fast 拒绝，但不能关闭 Kubernetes 失败 rail，也不能替代 exhaustive 证据。

第四段受界 persistence 优化只把 12 列基础 chunk owner 从逐行 execute 改为按 runtime variable limit 收窄、最多 1,024 行、12,288 个 bind 的 multi-values 分组。直属 SQLite trace 要求 1,025 条 fact 恰好执行两条 base-chunk insert，同时 FTS rowid 保持输入顺序。边界 regression 把 runtime limit 设为 24 个 variable、接受精确的 12-variable 单行上限，并要求 11-variable 上限在零写入时拒绝。Tail 中的 unique failure 必须回滚此前 1,024-row 分组、FTS row、metadata 和 checkpoint advancement，同时保留 staged batch 供 exactly-once replay。这些是 statement-count 与恢复 invariant，不构成真实仓库 latency 结论。

第五段受界 persistence 优化只改变已经通过准入的 grouped reference-search build page：用一条有序 `INSERT ... SELECT` 取代 Rust 侧六字段 document 物化和反复 FTS `VALUES` flush，再校验精确的 `last_insert_rowid` interval，并用一条带 scope 约束的 statement 建立全部 metadata owner。既有 page row/byte limit、lazy admission、checkpoint/progress CAS、transaction 与 publication fence 均不改变；既有 `INT64_MAX` row 会在 owner 写入前拒绝。Named gate trace 要求 1,025-group page 只执行一条主 FTS insert 和一条 metadata insert，并另行验证规范空字段内容与 rollback-safe rowid 拒绝。该机制尚未通过 Kubernetes 210 秒 rail，在隔离 release run 通过之前不得报告为端到端提速。

Grouped reference-search finalization 具有直属的 plan 与 work 证据。Cleanup、discovery 与 build 的所有 range 都分别使用静态首条页/续页 SQL，不含 nullable-parameter `OR`，并由 `EXPLAIN QUERY PLAN` 强制 indexed keyset range。每页预留两次 control mutation，并计入 owner/progress/checkpoint 的完整记录保守上界。单条 lazy scan 只返回整数 lookup key、cursor 长度与 row-byte 上界；4 KiB budget 下的 8 KiB cursor 会在 cursor fetch 前被拒绝，已接纳页面只点查最后一个 durable cursor。Build admission 只做字段长度加法，不拼接 search content。Discovery page 的 returning UPSERT 取代此前额外的 grouped count scan，不改变既有 cap。在包含 2,048 条 reference、128 个已存在首条页 group owner 的确定性 fixture 上，SQLite progress-handler 对 legacy nullable-range/count/upsert 路径记录 126,790 个 VM step，对 production static-range/streaming/returning-upsert 路径记录 56,472 个 VM step，减少 70,318。该 fixture 直接证明被移除的 SQLite work，不替代 Kubernetes wall time。

普通 reference resolution 为 `finalizing:resolve_references:v1` 提供独立的 production-mechanism gate。它要求 multi-row 静态首条页/续页 keyset、按每页 name 与 `(name,path)` 缓存的两类 indexed `LIMIT 2` owner-length probe、单次已接纳非 call range UPDATE，以及计入两次 control mutation 的完整 owner/progress/checkpoint 记录保守字节。Tiny-byte fixture 证明超大 reference 或 symbol payload 会在 payload materialization 前被拒绝。1,025 行 call-only 页面必须推进 exact durable cursor，同时执行零次 reference owner update、零次 path/name 点查和恰好一次末游标点查；后续 call-target phase 另行证明 stale 的 pre-resolved non-callable target 仍会降级。十倍 hot-symbol-tail fixture 在两个规模下记录相同 scan/probe VM work：首条页/续页 plan 分别为 130/136 step，对应 range UPDATE 为 351/358 step。Cursor digest/count 漂移、放大的 persisted limit、伪 zero count、非 tail EOF、rollback、reopen 与 stale fence 测试都必须 fail closed 或精确 replay。Driver bound 保守保持 `CODE_INDEX_FINALIZATION_MAX_STEPS + 4R + 6`；该 gate 不放宽 row、byte、timeout、lease、FTS 或 freshness budget。

`fast` profile 在共享 library test target build 完成后，以隔离 stage 运行这些机制对应的 `code_index_persistence_performance_suite`。其 hard timeout 保持 120 秒，key metric `code_index_persistence_performance_suite_ms` 的 budget 为 30,000 ms。Benchmark CI 同时要求 named gate 通过且 key metric 位于预算内，因此删除 chunk grouping、重新引入 nullable-range SQL、失去 indexed plan、恢复重复 discovery scan 或逐行 grouped cursor fetch、在预算前获取 grouped cursor/content，或让普通 resolution 对 call-only 页面重新物化或更新 owner，都会使 fast performance rail 失败，而无需扩大任何产品 resource budget。

阶段归因不得与 live writer 竞争。诊断运行可以使用 `--keep-workdirs`，但必须等待产品命令退出，并用操作系统 handle 检查确认 main/WAL/SHM 不再被打开，之后把三者复制到隔离临时目录，只以 SQLite read-only URI 与 `query_only=ON` 查询副本。Checkpoint 的 `state`、committed/total file count、batch count、last path 与 finalization phase 可区分 ingest amplification 和 projection/finalization 尾延迟，同时不会 checkpoint 或修改源库。

2026-08-25 的隔离 release-binary 诊断使用精确 Kubernetes target 配置：commit `016a2bcfa48d4a56059ee5e878eb208ffccdb773`、全文件 scope、无 path/language filter、全新 runtime home。Attempt 1 在未调整的 210,000-ms budget 后仍在运行，随后 host 墙钟向前跳约三小时，在 `finalizing:rebuild_reference_search:v2:discover:22` fail closed；因此产生的 10,861,839-ms 墙钟差不是有效 latency 证据。Generation 2 未重放 facts，从该 checkpoint 恢复并达到 `completed`/`fresh`，精确包含 30,353 files、1,434,001 symbols、2,879,261 references、268,075 chunks 与 4,771,612 committed fact rows。该恢复段自身约 244 秒，也单独超过 210 秒。该结果证明 durable fence/reopen 正确，但 Kubernetes performance rail 仍失败；它不能替代仍 pending 的 exhaustive query-case report。

完成受界 shared-FTS 与 grouped-build 变更后，对同一 release target 再做一次 fresh-home 实测；单个 attempt 正常完成，`/usr/bin/time` 报告 592.72 秒、exit code 0。Task=`succeeded`、checkpoint=`completed`、status=`fresh`，计数与上文 30,353 files、1,434,001 symbols、2,879,261 references、268,075 chunks 和 4,771,612 base facts 精确一致。命令在 210 秒观察点仍未完成，因此 key metric 仍以预算 2.82 倍明确失败；正常完成和正确发布不能把该失败改写为采纳。该索引上 7 条此前失败的 Kubernetes focused query 已通过当前 rank/evidence 合同，但完整 Kubernetes/exhaustive report 仍 pending。

把受界 reference、symbol、chunk 基础事实分组提升到 1,024 行后，对相同 target 做第三次 fresh-home release 实测；单个 attempt 再次以完全相同的 fact 计数和终态正常完成。实测耗时 607.03 秒，210 秒观察点仍在运行，以未调整 rail 的 2.89 倍失败；相对上一条单样本增加 14.31 秒、约 2.4%。每个候选仅一个样本，不能区分 scheduler/storage 波动与小幅回归，但足以证明更大的受界基础事实分组没有关闭端到端瓶颈。1,025-row trace test 仍只属于 mechanism evidence。

另一次 fresh-home 运行约每 10 秒轮询一次只读 task status，以获得粗粒度 phase 证据。轮询会与 live workload 竞争，因此总耗时 612.08 秒只属于诊断，不能作为性能样本。相对 task 创建时间，首次观察到 `build_query_indexes` 约为 158 秒；query-index 加普通 reference 区间约在 258 秒进入 `resolve_imports`；imports 约在 286 秒进入 `resolve_call_targets`；随后 grouped discovery 与 build 分别约占 107 秒和 109 秒；call rebuild 约 30 秒；完成前 software projection 约占 67 秒。轮询粒度与 coarse checkpoint 语义不允许把这些数值当成 exact phase timer，但能把普通与 grouped reference-wide finalization 确认为主要受界测量面。该诊断不改变 210 秒失败 rail，也不允许跳过 phase、放宽预算，或预先宣称 cursor-fetch 变更带来 wall-time 改善。

完成末游标与普通 call ownership 变更后，首次 fresh-home attempt 又被 host 墙钟不连续污染：task 创建约 153.5 秒后，30,353 个文件与 4,771,612 条基础 fact 已全部到达 `indexing` checkpoint，随后墙钟向前跳约 2,830 秒，publication fence 正确拒绝过期 generation。Generation 2 没有重放 61 个 fact batch，以单调时钟 386.35 秒完成剩余 finalization。这两个数值都不是有效的单-attempt latency 结果，但保留了精确的 fail-closed 与 checkpoint recovery 证据。

随后第二个 fresh home 在不轮询 status 的情况下产生有效验收样本。单个 release-binary attempt 以单调时钟 564.99 秒正常完成，task=`succeeded`、checkpoint=`completed`、status=`fresh`，并保持相同的 30,353 files、1,434,001 symbols、2,879,261 references、268,075 chunks 与 4,771,612 committed base facts。它比紧邻的 607.03 秒样本快 42.04 秒、约 6.9%，比此前 592.72 秒样本快 27.73 秒、约 4.7%。每个候选单样本不能把差异归因于 cursor/call 变更，且命令在 210 秒时仍在运行；564.99 秒仍是未调整 key budget 的 2.69 倍，因此 Kubernetes rail 保持失败。

## 当前测评示例

性能耗时只在报告记录的产品二进制口径内有效。所有非 smoke 自迭代 profile（包括聚焦 `performance` 的 `fast` 运行）都会构建并执行 `target/release/relay-knowledge`，harness 自身仍可使用 debug 二进制。workload previous/best baseline 按 `product_binary_profile` 过滤；历史记录缺字段时按旧语义解释为 `fast=debug`、非 fast=`release`。benchmark CI 会拒绝没有记录 release profile 与 release 产品路径的性能报告，避免旧 fast debug 测量被静默用作 release 性能证据。

Linux kernel 目标显式声明精确 selected scope 为 93,601 个文件，使用历史 34,150 文件/180 秒基线和约 80 文件/秒吞吐率，配置最大预算 1,800 秒。固定 ref 的原始 parent-tree 观测可以更大，并作为独立诊断证据保留。该配置让预算随目标规模增长，同时保留明确上限，避免固定 180 秒在大仓库上制造误报，也避免无限等待。

相关配置位于 `tools/self_iteration/cases/repository_index_performance_targets.json`。`linux_full` 声明 `index_only_performance_target=true`，因此 exhaustive 会执行它的冷索引测量，但它有意不产生 retrieval case observation。运行性能测评：

```bash
./self-iterate.sh evaluate --use-current-candidate --profile exhaustive --categories performance
```

index-only 仓库有意不提供 retrieval case，因此不能只凭 case count 判断它是否真正执行。严格冷完成校验通过后，其 repository report 会加入 `cold_index_result`，原样保留冷启动 `repo index` payload 中的 `scope`、`task`、`summary`、`checkpoint` 和 `status`。普通仓库报告继续保持既有紧凑 `index_summary` 语义并省略该可选字段；严格完成校验失败时同样省略，不能用部分工作伪造 terminal evidence。

最终验收断言以 repository 参数化，不依赖任何 query 名称，同时检查 target 被选中、零 case 执行、两项 key budget、严格完成命令、durable 终态、精确计数、freshness 和 identity：

```bash
report_path="$(ls -t .git/relay-knowledge-self-iteration/reports-v2/manual-evaluate-*.json | head -n 1)"
repository=linux_full
jq --arg repository "$repository" -e '
  ([.evaluation.gates[] | select(.passed | not)] | length) == 0 and
  ([.evaluation.cases[] | select(.passed | not)] | length) == 0 and
  ([.evaluation.repositories[] | select(.repository == $repository)] as $reports |
    ($reports | length) == 1 and
    ($reports[0] |
      (.cases | length) == 0 and
      ([.commands[] |
        select(.name == ($repository + "_cold_index_completion") and
               .exit_code == 0)] | length) == 1 and
      ([.metrics[] |
        select((.name == ($repository + "_cold_index_ms") or
                .name == ($repository + "_cold_register_index_ms")) and
               .key == true and .budget != null and .value <= .budget)] |
        length) == 2 and
      (.cold_index_result as $cold |
        $cold.task.state == "succeeded" and
        $cold.task.mode == "full" and
        $cold.checkpoint.state == "completed" and
        $cold.checkpoint.total_path_count > 0 and
        $cold.checkpoint.committed_file_count == $cold.checkpoint.total_path_count and
        $cold.status.state == "fresh" and
        $cold.status.stale == false and
        $cold.scope.indexed_file_count == $cold.checkpoint.total_path_count and
        $cold.summary.indexed_file_count == $cold.checkpoint.total_path_count and
        $cold.status.indexed_file_count == $cold.checkpoint.total_path_count and
        $cold.scope.repository_id != null and
        $cold.scope.repository_id == $cold.task.repository_id and
        $cold.scope.repository_id == $cold.summary.repository_id and
        $cold.scope.repository_id == $cold.checkpoint.repository_id and
        $cold.scope.repository_id == $cold.status.repository_id and
        $cold.scope.alias == $cold.task.alias and
        $cold.scope.alias == $cold.status.alias and
        $cold.scope.requested_ref == $cold.task.ref_selector and
        $cold.scope.scope_id != null and
        $cold.scope.scope_id == $cold.task.source_scope and
        $cold.scope.scope_id == $cold.summary.source_scope and
        $cold.scope.scope_id == $cold.checkpoint.source_scope and
        $cold.scope.scope_id == $cold.status.last_indexed_scope_id and
        $cold.scope.resolved_commit_sha != null and
        $cold.scope.resolved_commit_sha == $cold.task.resolved_commit_sha and
        $cold.scope.resolved_commit_sha == $cold.summary.resolved_commit_sha and
        $cold.scope.resolved_commit_sha == $cold.status.last_indexed_commit and
        $cold.scope.tree_hash != null and
        $cold.scope.tree_hash == $cold.task.tree_hash and
        $cold.scope.tree_hash == $cold.summary.tree_hash and
        $cold.scope.tree_hash == $cold.status.tree_hash and
        $cold.scope.path_filters == $cold.task.path_filters and
        $cold.scope.path_filters == $cold.status.path_filters and
        $cold.scope.language_filters == $cold.task.language_filters and
        $cold.scope.language_filters == $cold.status.language_filters)))
' "$report_path"
```
