# relay-knowledge 自迭代

中文 | [English](README.md)

本目录包含一个独立的 Codex 驱动优化循环，用于改进代码仓库检索质量。它有意放在 Rust crate 之外，所有运行状态都存放在 `.git/relay-knowledge-self-iteration/` 下。

## 启动

在仓库根目录运行：

```bash
./self-iterate.sh
```

启动脚本默认等价于：

```bash
python3 tools/self_iteration/self_iterate.py loop --yolo
```

常用变体：

```bash
./self-iterate.sh once
./self-iterate.sh --max-iterations 3
./self-iterate.sh chart
./self-iterate.sh once --profile smoke --dry-run-codex
```

## YOLO 模式

本地 Codex CLI 没有字面意义上的 `--yolo` 参数。本框架会把 `--yolo` 映射到当前非交互、高权限 Codex 调用：

```bash
codex -a never exec --dangerously-bypass-approvals-and-sandbox -s danger-full-access -C /opt/workspace/relay-knowledge -
```

只应在外部可信的工作区中使用。该循环按无人值守运行设计。

## 循环行为

每一轮迭代会：

1. 检查工作树是否干净，除非传入 `--use-current-candidate`。
2. 提示本地 Codex 做一个聚焦的代码检索改进。
3. 将候选补丁保存到 `.git/relay-knowledge-self-iteration/patches/`。
4. 运行 build、lint、tests 和代码仓库检索评估。
5. 将报告写入 `.git/relay-knowledge-self-iteration/reports/`。
6. 将评分历史追加到 `.git/relay-knowledge-self-iteration/runs.jsonl`。
7. 只有当上一轮改进采纳策略接受候选时，才把候选净改动 squash 成一个 commit。
8. 候选被拒绝时，恢复到本轮开始的 commit。

如果启动时工作树是 dirty 状态，循环会立即退出，而不是重复重试同一个不可重试的前置条件失败。

## 评分和采纳

加权分数为：

```text
accuracy * 0.55 + performance * 0.30 + stability * 0.15
```

采纳策略使用 `带硬约束和加权分数决胜的 epsilon-Pareto 采纳策略`。从多目标优化角度看，build/test gate 和候选 diff 存在性是硬约束，检索质量与延迟观测是目标，epsilon 阈值用于抑制测量噪声，加权分数是决胜项而不是唯一决策规则。

候选在以下条件满足时被采纳：

```text
hard_constraints_pass
and (
  weighted_score > previous_weighted_score + score_epsilon
  or epsilon_pareto_improved(candidate, previous)
)
```

`epsilon_pareto_improved(candidate, previous)` 表示：至少一个被跟踪目标的改善超过其 epsilon 阈值，并且没有任何被跟踪目标的退化超过其 epsilon 阈值。默认阈值为：

- `score_epsilon = 0.0005`
- `ratio_epsilon = 0.005`，用于 accuracy、performance、stability 等分数组件
- `metric_epsilon = max(25ms, previous_metric * 0.03)`，用于原始耗时指标

这可以避免真实 case/rank 改善因为某个耗时指标在正常噪声范围内波动而被拒绝，也能避免只靠噪声获胜、同时悄悄回退受保护目标的候选被采纳。accuracy、case、gate 和 metric 的退化会被记录为下一轮 Codex prompt 的 degradation feedback。正向的 score、case、gate 和 metric 改善也会被记录并传给下一轮 Codex prompt，方便后续迭代知道哪些成果需要保持。

`chart` 命令会写入：

- `.git/relay-knowledge-self-iteration/score.csv`
- `.git/relay-knowledge-self-iteration/score.svg`

## 评估数据

`cases.json` 定义 benchmark targets：

- `/opt/workspace/relay-teams`：仓库索引和代表性代码图查询。
- `/opt/workspace/linux`：默认 profile 下的 C 语言采样索引，覆盖函数、syscall 风格宏、导出符号、include、callers 和 callees。
- `/opt/workspace/linux`：`exhaustive` profile 下通过 `linux_full` 目标评估完整 C 仓库初始索引时间。
- `/opt/workspace/leveldb`：C++ 采样索引与查询，覆盖类方法、自由函数、头文件、callers、hybrid lookup 和 filters。
- `/opt/workspace/kubernetes`：Go 采样索引与查询，覆盖 command constructor、kubelet flow、API types、clientset constructor、callers、hybrid lookup 和 filters。

使用 `--profile smoke` 可验证启动器而不运行仓库评估。需要测量 Linux 全量初始索引时间时使用 `--profile exhaustive`。
