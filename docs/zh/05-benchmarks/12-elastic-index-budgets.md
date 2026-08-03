# 大仓库索引弹性长预算模型

## 目的

大仓库索引不再使用所有仓库共用的固定 180 秒硬超时。180 秒保留为历史性能基线，用于比较回归；实际执行预算根据目标仓库规模和观测吞吐率弹性计算。

## 预算计算

系统默认启用 `index_budget_mode=elastic`；未填写该字段等价于 `elastic`。只有显式选择固定/严格模式的目标才不使用弹性计算。启用后，评估器先在授权 Git 工作树上执行 `git ls-files`，得到实际文件数 `N`。配置中的 `expected_file_count` 只作为无法观测文件数时的回退值。

预算按以下优先级计算：

1. 配置 `baseline_files_per_second` 时：

   `index_budget_ms = N / baseline_files_per_second × 1000`

2. 否则使用历史基线比例：

   `index_budget_ms = baseline_index_budget_ms × N / baseline_file_count`

3. 最终预算限制在 `max_index_budget_ms` 以内。

注册加索引预算另外加入 `register_overhead_budget_ms`，避免注册阶段的固定成本污染索引吞吐基线。命令级 timeout 会在预算秒数上再增加有限的恢复余量，但不会取消业务预算、checkpoint 或 freshness 要求。

## 持久化与恢复不变量

弹性预算只改变等待时间，不改变数据一致性合同：

- 每个 batch 先写 durable staging manifest；事实、FTS、checkpoint 和 `published` 标记由唯一 writer 原子提交。
- worker 使用有界 attempt lease；异常退出后由 orphan recovery 安全回收，不能抢占仍存活的 lease。
- reset 或 worker 重启从已有 `indexing` checkpoint 继续，已发布事实不会被重复冷写入。
- 未完成的 staging、edge finalize 或查询索引构建不能把 scope 标记为 fresh；状态必须继续报告为 indexing/stale/degraded。
- parser、队列、batch、FTS 写入和 SQLite 事务仍有固定上限，弹性预算不允许无界内存或无界重试。

## 观测与判读

报告应同时记录：实际文件数、基线文件数、基线吞吐率、计算出的索引预算、注册开销、最大预算、冷索引耗时、checkpoint 文件数和最终 freshness 状态。评估时应区分：

- “预算内完成”：任务成功且 scope fresh；
- “预算内仍在运行”：checkpoint 持续前进，不能提前返回成功；
- “预算上限触发”：任务保持可恢复状态并暴露 stale/degraded，不得删除已发布事实；
- “租约或事务错误”：属于一致性/恢复回归，不是单纯性能结果。

## 当前测评示例

Linux kernel 93,601 文件目标使用历史 34,150 文件/180 秒基线和约 80 文件/秒吞吐率，配置最大预算 1,800 秒。该配置让预算随实际仓库规模增长，同时保留明确上限，避免固定 180 秒在大仓库上制造误报，也避免无限等待。

相关配置位于 `tools/self_iteration/cases/repository_index_performance_targets.json`；运行性能测评：

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```
