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
4. 运行 build、lint、tests、代码仓库检索评估和自迭代文档 gate。
5. 将报告写入 `.git/relay-knowledge-self-iteration/reports/`。
6. 将评分历史追加到 `.git/relay-knowledge-self-iteration/runs.jsonl`。
7. 采纳候选前，将本轮采用的优化思路、变更文件、指标改善和已知退化追加到 `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`。
8. 只有当上一轮改进采纳策略接受候选时，才把候选净改动和采纳记录 squash 成一个 commit。
9. 候选被拒绝时，恢复到本轮开始的 commit。

如果启动时工作树是 dirty 状态，循环会立即退出，而不是重复重试同一个不可重试的前置条件失败。

实现类候选必须在评估前更新
`docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`，写清算法、架构、不变量、预期 case/metric 影响和已知风险。harness 会追加
`self_iteration_algorithm_documentation` gate，拒绝没有携带这些说明的代码、测试、benchmark 或 harness 策略变更。prompt 也会把
`.git/relay-knowledge-self-iteration/patches/` 当作长期记忆：先列出有界 patch 索引，再要求 Codex 只对相关历史 patch 做小范围渐进读取。

## 评分和采纳

加权分数为：

```text
accuracy * 0.60 + performance * 0.15 + stability * 0.25
```

采纳策略使用 `带硬约束和加权分数决胜的 epsilon-Pareto 采纳策略`。从多目标优化角度看，build/test gate 和候选 diff 存在性是硬约束，accuracy 和 stability 是保证基础功能可用的受保护目标，检索质量与延迟观测是目标，epsilon 阈值用于抑制测量噪声，加权分数是决胜项而不是唯一决策规则。

候选在以下条件满足时被采纳：

```text
hard_constraints_pass
and no_protected_accuracy_or_stability_regression
and (
  weighted_score > previous_weighted_score + score_epsilon
  or epsilon_pareto_improved(candidate, previous)
)
```

`epsilon_pareto_improved(candidate, previous)` 表示：至少一个被跟踪目标的改善超过其 epsilon 阈值，并且没有任何被跟踪目标的退化超过其 epsilon 阈值。默认阈值为：

- `score_epsilon = 0.0005`
- `ratio_epsilon = 0.005`，用于 accuracy、performance、stability 等分数组件
- `metric_epsilon = max(25ms, previous_metric * 0.03)`，用于原始耗时指标

这可以避免真实 case/rank 改善因为某个耗时指标在正常噪声范围内波动而被拒绝，也能避免只靠噪声获胜、同时悄悄回退受保护目标的候选被采纳。accuracy、case、gate 和 metric 的退化会被记录为下一轮 Codex prompt 的 degradation feedback。正向的 score、case、gate 和 metric 改善也会被记录并传给下一轮 Codex prompt，方便后续迭代知道哪些成果需要保持。被采纳的优化方案还会进入 run history 的 `optimization_plan` 字段，并在下一轮 prompt 的 `Recent adopted optimization plans to build on` 段落中作为设计参考。

`chart` 命令会写入：

- `.git/relay-knowledge-self-iteration/score.csv`
- `.git/relay-knowledge-self-iteration/score.svg`

## 评估数据

`cases.json` 定义 benchmark targets：

- 本地文件索引 fixture：在临时目录中生成 `user documents`、Linux `/opt`
  风格路径、Windows `D:` 风格路径、深层目录和高噪声文件集合，运行
  `relay-knowledge files index/query`，记录 `file_index_ms`、
  `file_query_p50_ms` 和 `file_query_p95_ms`。每条文件查询都使用
  subprocess timeout，防止候选实现卡死 evaluator。
- `/opt/workspace/relay-teams`：`scope=all` 全仓索引和 Python 服务、connector、eval checkpoint、re-export 等查询。
- `/opt/workspace/linux`：`exhaustive` profile 下 `scope=all` 全仓索引，覆盖函数、syscall 风格宏、导出符号、include、callers、callees、mmap flow、epoll/eventfd 等大仓检索场景。
- `/opt/workspace/linux`：`exhaustive` profile 下通过 `linux_full` 目标重复测量完整仓库初始索引时间，用于长周期基线。
- `/opt/workspace/leveldb`：`scope=all` 全仓 C/C++ 索引与查询，覆盖类方法、自由函数、头文件、table cache、recovery、callers、hybrid lookup 和 filters。
- `/opt/workspace/kubernetes`：`exhaustive` profile 下 `scope=all` 全仓 Go 索引与查询，覆盖 command constructor、kubelet flow、API types、clientset/generic client、authorizer、informer imports、callers、hybrid lookup 和 filters。
- `/opt/workspace/spring-framework`：`exhaustive` profile 下 `scope=all` 全仓 Java 索引与查询，覆盖 context、bean factory、webmvc servlet/handler mapping、imports 和 filtered lookup。

所有 repository target 都必须使用 `scope=all`。评估器会拒绝非全量 scope，并且 full-scope 注册不会向 `repo register` 传递 path 或 language filter；case 级 filter 只用于验证查询端过滤能力。使用 `--profile smoke` 可验证启动器而不运行仓库评估。需要运行 Linux、Kubernetes 或 Spring Framework 长周期全量初始索引 gate 时使用 `--profile exhaustive`；这些 gate 有意不放在默认 profile，避免单 CPU 自迭代 worker 在收集可操作检索反馈前就拒绝每个候选。
