# 自迭代采纳优化记录

本文档由自迭代 harness 在候选通过质量门禁并被采纳时追加，用于把本轮采用的优化思路传递给后续 Codex 迭代。人工维护的总结可以继续补充在对应条目下。

## 记录格式

- `patch`: 本轮候选补丁在 `.git/relay-knowledge-self-iteration/patches/` 下的路径。
- `score`: 采纳时的总分和 accuracy、performance、stability 分项。
- `cases`: 采纳时通过的检索 case 数量。
- `changed paths`: 本轮变更的主要文件。
- `key improvements`: 相对上一轮改善的 case、gate 或 metric。
- `known degradations`: 相对上一轮已观测到的退化，后续迭代必须优先保护或修复。
- `Adopted optimization notes`: Codex 输出中提取的优化说明，用作下一轮 prompt 的上下文。

## 候选优化说明：20260516T111042Z

- 目标：降低 Linux、Kubernetes、Spring Framework 等大仓全量索引中的 Git blob 读取开销，避免每个文件启动一次 `git show` 子进程。
- 方法：全量索引计划在每个受资源预算约束的解析批次内，用 `git cat-file --batch` 按小组批量读取 commit blob，并在小组内并行解析文件；SQLite checkpoint 进度改为按已提交 batch 增量维护，避免每批对 files、symbols、references、chunks 重新执行全表 `COUNT(*)`。默认自迭代 profile 不再运行 Linux、Kubernetes、Spring Framework 这类单 CPU 环境下不可完成的长周期 full-scope gate，保留到 `--profile exhaustive`。
- 预期影响：把大仓索引的 Git 进程数从“文件数级别”降到“文件数/批量组大小级别”，消除 checkpoint 阶段随已索引行数增长的重复扫描，并在有多核预算时提高解析吞吐；保留既有路径筛选、语言筛选、语法解析和检索行为。

## 候选优化说明：自迭代文档与 patch 长期记忆

- 目标：让自迭代候选在修改代码、测试、benchmark 或 harness 策略时，同时留下可供后续迭代理解的算法与架构说明，避免只有 patch 和评分而缺少设计意图。
- 方法：候选 diff 只要包含非文档文件，就追加 `self_iteration_algorithm_documentation` gate，要求同步更新本文档；prompt 明确要求写出算法、架构、不变量、预期 case/metric 影响和风险。该 gate 在候选评估完成后、评分前加入，作为硬质量门禁参与 `quality gates failed` 拒绝原因。
- 长期记忆：prompt 新增 `.git/relay-knowledge-self-iteration/patches/` 索引，按最近 patch 列出路径、大小、采纳状态、分数、变更文件、拒绝原因和主要改善。Codex 先读索引，再用 `sed -n` 对相关 patch 小范围渐进读取，避免一次性塞入所有历史 patch 造成上下文膨胀。
- 预期影响：后续自迭代能同时利用结构化 run history、人工可读设计说明和原始 patch 细节，减少重复尝试，提高对历史成功/失败算法的复用质量。
