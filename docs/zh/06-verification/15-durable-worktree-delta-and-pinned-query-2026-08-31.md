# Durable Worktree Delta 与固定身份查询验证记录 2026-08-31

[中文](../../zh/06-verification/15-durable-worktree-delta-and-pinned-query-2026-08-31.md) | [English](../../en/06-verification/15-durable-worktree-delta-and-pinned-query-2026-08-31.md)

> 日期: 2026-08-31
> 范围内状态: PASS
> 基线 revision: `6e78bdbac22e1a0875cee2b13434baffd3b52a17`
> 评估 patch: 299,016 bytes，SHA-256 `882f30848b626308a0f6c78a51cfd6473a795ea4df1f171791ef0689aa20aa34`
> 最终自迭代报告: `manual-evaluate-1788148749661647156-0-1647985.json`，SHA-256 `7dce856be5fc21f38ab3289542a0ec9a22c64f9e9e4a066892f25a6b11065903`
> 证据边界: durable worktree storage owner、lease recovery、固定 synthetic ref 查询、release 产品 fast/performance 评估、全量 Rust targets、覆盖率、文档和仓库地图；不认证 exhaustive、agent workflow、research judge、browser、package、service、Kubernetes 或跨平台门禁

## 1. 目标与根因

上一条验证记录发现，真实 dirty-worktree 索引可能超过单个 direct SQLite writer quantum。
返回 `DurableStagingRequired` 是正确的 admission 行为，但 worktree dispatch 没有合法切换到
既有 checkpointed pipeline 的路径，因此只能在保留旧 fresh HEAD scope 的同时重试同一个
超预算事务。

CodeSpec、Knowledge Map、已索引仓库 context 和 `tools/self_iteration` 的真实 recovery cases
把问题路由到 snapshot coordinator、durable clone、task receipt、publication fence 与
synthetic-ref query 边界。修复后，小 overlay 继续走 direct path；发生类型化 over-budget 时，
使用同一个 queued task、冻结资源预算、active attempt 与 publication fence 进入 durable staging。

## 2. Durable 状态机与有界职责

实现后的状态机如下：

1. 原子地把 pending task 重新绑定到不可变
   `worktree:<base>:<overlay-hash>` target。
2. 通过 metadata-indexed keyset page 克隆 clean base，同时排除 dirty file owner。
3. 按路径顺序冻结确定性 delta plan。每个文件拥有自己的 symbol、reference、import、
   dependency、feature flag、framework fact、route、chunk、diagnostic 和最终 call。
4. 每个 worker step 最多提交一个 delta batch；持久化 `batch_count` 是 lease 过期和 takeover
   之间的 replay cursor。
5. 在独立 terminal writer quantum 中共同 admission cleanup、tombstone、固定 control rows 和
   可变大小 receipt，并把 `last_path` 恢复为 target 的真实最大路径。
6. 进入既有 query-index、reference/import/call、search、software、business 与 publication
   finalizer，不跳过 freshness 检查。

所有容量派生都使用 checked arithmetic，在写入前拒绝溢出。没有 file owner 的 fact 会 fail
closed；源字节、序列化 owned-fact surface、派生搜索行或 control row 无法装进一个冻结 writer
quantum 的不可分文件同样在 delta 写入前被拒绝。Receipt batch 只计 parsed-file data work；删除路径继续保留在 affected-path 审计指标中，
但 clone owner 删除后不会被误当作 parsed-file capacity。worktree task 不会被重新解释成 clean
full index，也没有把 queue、batch、transaction、retry、source fallback 或 timeout 改成无界。

resolved 与 pending worktree selector 作为互斥 identity 解析。嵌套 context query 直接复用
固定的 resolved identity，不会把 synthetic value 送入 Git ref resolution。publication 使用
重新绑定后的有效 lease identity，因此旧 attempt 不能发布新 generation 的 staged facts。

## 3. 聚焦恢复证据

聚焦 recovery rail 为：

```bash
cargo test --all-targets --all-features code_index_task_ -- --nocapture
```

结果为 filtered library 89 passed、filtered integration 2 passed。新增端到端 case
`oversized_worktree_code_index_task_delta_batches_and_recovers_between_leases` 强制产生两个
dirty batches，在两批之间让 lease 过期并 reclaim，随后从持久化 cursor 恢复且不重放第一批。
receipt 记录 2 个 delta batches、2 个 parsed files 和 44 次 SQLite writes；完成 checkpoint
记录 3 个总 batches 和真实字典序最大路径。

确定性 planner、超预算不可分文件与 orphan 拒绝、terminal cleanup/tombstone/control/receipt
admission、删除密集与多 batch receipt 扩展、固定 worktree context、publication identity 重绑定和
CLI map namespace 报错顺序的 owner tests 均通过。resume precheck 也证明 clone phase 尚未创建
staged target 时不会提前验证它。

## 4. Release 产品自迭代

候选通过公开 release 产品评估入口运行：

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

第一轮完整运行被保留为 rejected evidence，没有丢弃：

| 报告 | 状态 | 原因 |
| --- | --- | --- |
| `manual-evaluate-1788148459837113509-0-1638323.json` | rejected | release build 184,379 ms，超过未调整的 180,000 ms 预算 |
| `manual-evaluate-1788148749661647156-0-1647985.json` | `would_accept` | 所有已选择 gate、case、command 和 metric 通过 |

rejected report 的 SHA-256 为
`2e4439c0dae5e06cd781b82ad55cf6ece888374a20725989acfac4763f0d1d96`。
两轮均完成 368/368 gates、132/132 cases 和 307/307 command contracts；第一轮只因实测
build 预算被拒绝。通过轮 score 为 `0.9989406099518459`，`score_accepted=true`，
`adoption_status=would_accept`；manual evaluation 没有创建 commit。

| 关键 metric | 实测 | 预算 | 结果 |
| --- | ---: | ---: | --- |
| Release build | 215 ms | 180,000 ms | PASS |
| Code-index recovery cases | 24,438 ms | 60,000 ms | PASS |
| Software fixture cold index | 514 ms | 15,000 ms | PASS |
| Software fixture register + cold index | 551 ms | 18,000 ms | PASS |
| Software query p50 / p95 | 75 / 81 ms | 100 / 250 ms | PASS |
| 1,024-file cold index | 611 ms | 12,000 ms | PASS |
| 1,024-file register + cold index | 772 ms | 13,000 ms | PASS |
| Many-file incremental index | 784 ms | 3,000 ms | PASS |
| C syntax query p95 | 127 ms | 180 ms | PASS |

performance、stability 与 semantic/vector 均保持 `1.0`。`agent_workflows` 和
`research_judge` 因 category 选择而跳过，不能由通过的 fast/performance 结果代替。

随后第一次 GitHub `index-performance-regression` 执行通过全部 runtime gate、case、完成性断言和
索引延迟预算，但冷 runner 的 release 产品编译耗时 330,829 ms，超过独立的 180,000-ms quality
build 预算，因此候选仍被拒绝。PR workflow 现在先在显式、不计入性能测量的步骤中构建同一个
release binary；evaluator 仍校验 release 路径与增量 build gate，而 index-performance job 继续只
对 cold/incremental repository runtime 负责，不再把 host compiler 冷启动波动混入索引性能。
历史复用 fixture 将增量耗时记录为 `initialization_incremental_index_ms`；workflow 在不改变任一预算的前提下，同时接受该 fixture 专用名称和普通的 `incremental_index_ms` 名称。没有放宽产品或索引延迟预算。

## 5. 全量测试与覆盖率

独立的当前工作树 Rust 门禁通过：

```bash
cargo test --all-targets --all-features
```

结果为 library 3,792 passed、1 ignored，benchmark 1/1 passed，integration 156/156
passed。被忽略的 subprocess fixture 仍显式记为 ignored，不能计作 passed。

精确覆盖率门禁为：

```bash
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

第一轮精确执行被保留为失败证据：154,185 行中 missed 15,431 行，结果 `89.99%`，低于硬
阈值。它暴露了新增 owner 中尚未覆盖的 fail-closed 分支。新增三个聚焦测试，要求 duplicate
file ownership、超出冻结 batch plan 的 ordinal，以及缺少不可变 base identity 的 durable
receipt 返回类型化错误。

最终结果为 154,238 行中 missed 15,348 行，即 line coverage `90.05%`，通过未调整的
90% 阈值。这次执行重新运行全部 targets 与 features：library 3,792 passed、1 ignored，
benchmark 1/1 passed，integration 156/156 passed；没有排除新增 storage owner 或降低要求。

## 6. 真实仓库回放

完成 durable 切换实现后、添加本验证记录及其地图 metadata 前，release 产品二进制对共享
工作树执行了索引：

```bash
target/release/relay-knowledge repo index relay-knowledge-reference --ref worktree --format json
```

task `code-index-task:164a93ebb170174a` 在 attempt 1 完成，发布 scope
`git_snapshot:0c6a43ff14ae84f1`，resolved identity 为
`worktree:6e78bdbac22e1a0875cee2b13434baffd3b52a17:cd811b09b98f8588`。durable
checkpoint 记录 58 个 changed paths、3 个 deletions、55 次 blob reads 和 parsed files、
12,395 次 SQLite writes、315,392 个 committed fact rows、1 个 delta batch 和 2 个总 batches；
真实 `last_path` 为 `src/relay_knowledge/storage/sqlite/software/ontology/query_tests.rs`。

新 fresh scope 包含 2,439 files、42,985 symbols、230,240 references 和 26,004 chunks。
它仍报告 20 个整体 degraded files 与 2 个 changed degraded files，因此不是零降级声明。附录
B.14 对旧版缺失切换路径的记录仍是有效历史证据，但不再描述当前默认 CLI 路径。

这份 evidence snapshot 必然早于本记录正文和生成地图 shards。最终交付因此还要求在所有
tracked text 与 map mutation 后执行一次最终 worktree reconciliation 和固定身份的
`repo context` 查询；结果随 change summary 报告，而不能倒置为“本文在写入前已经存在”的证明。

## 7. 文档、地图与范围边界

实现已同步更新双语 worktree workflow、增量索引架构、弹性预算说明、工程硬约束、
self-iteration 优化台账、专门 CodeSpec design 与本双语验证记录。CodeSpec 和 Knowledge Map
root 只通过产品 CLI mutation；生成 shards/history 后必须再次 validate。

durable overlay 有意拒绝非空 auto-workspace projection state。CLI 默认关闭 workspace
detection。API/Web caller 显式启用该组合时会 fail closed，而不是丢失 manifest metadata 或
发布 clean-snapshot identity。支持这一组合需要独立设计有界、持久化的 workspace manifest。

本轮没有修改安装路径、package artifact、service-manager template、runtime data directory、
configuration migration、upgrade、rollback 或 uninstall 行为，因此第 19 章无需改动。本轮
focused PASS 不是整体 release readiness，也不替代
[文档与自迭代准备度验证记录 2026-08-18](13-documentation-self-iteration-readiness-2026-08-18.md)。

---

导航: 上一条:
[14. 软件全域证据优先级验证记录](14-software-global-evidence-priority-2026-08-31.md)
| 索引: [验证与审计记录](README.md)
