# relay-knowledge 自迭代

中文 | [English](README.md)

`tools/self_iteration` 是独立的 Rust 自迭代 harness，用 Codex 生成候选补丁，并用固定评估集判断它是否真正改进代码仓库检索、semantic/vector 检索、性能、稳定性或研究质量。它不属于产品 crate 的 `src/` 模块树，运行状态统一写入 `.git/relay-knowledge-self-iteration/`。旧的 tracked Python harness 已在功能对齐后移除，仓库根目录的 `self-iterate.sh` 会直接构建并运行 Rust binary。

## 快速路径

### 5 分钟上手

在仓库根目录运行：

```bash
./self-iterate.sh
```

启动脚本默认等价于：

```bash
cargo build --manifest-path tools/self_iteration/Cargo.toml --bin relay-knowledge-self-iterate
tools/self_iteration/target/debug/relay-knowledge-self-iterate loop --workspace . --yolo --profile fast
```

`self-iterate.sh` 是稳定入口。它默认构建 debug harness，避免每次本地自迭代先做 release build；需要 release harness 时设置 `RELAY_KNOWLEDGE_SELF_ITERATION_RELEASE=1`。调用者不需要手动进入 `tools/self_iteration` 或把 binary 安装到 `PATH`。

### 常见任务

| 目标 | 命令 |
| --- | --- |
| 运行一轮候选生成和评估 | `./self-iterate.sh once --profile fast` |
| 连续运行最多 3 轮 | `./self-iterate.sh --max-iterations 3` |
| 评估当前工作树 diff，不调用 Codex | `./self-iterate.sh evaluate --use-current-candidate --profile fast` |
| 聚焦 semantic/vector | `./self-iterate.sh once --profile fast --categories semantic_vector` |
| 运行 coding-agent 工作流回归 | `./self-iterate.sh evaluate --use-current-candidate --profile fast --categories agent_workflows` |
| 聚焦多个类别 | `./self-iterate.sh once --profile fast --categories semantic_vector,competitive` |
| 运行完整旧门禁和 workload | `./self-iterate.sh once --profile full` |
| 只验证启动器和 prompt | `./self-iterate.sh once --profile smoke --dry-run-codex` |
| 长周期无人值守 | `./self-iterate.sh loop --strategy unattended-layered --max-wall-clock-hours 48 --stop-after-accepted 12` |
| 生成研究计划 | `./self-iterate.sh research-plan --research-topic "2026 graph database research" --research-slug graph-database-research --research-date 2026-06-05` |
| 导出分数图表 | `./self-iterate.sh chart` |

### 如何选择运行级别

| 选择项 | 什么时候用 | 代价和覆盖 |
| --- | --- | --- |
| `--profile smoke` | 检查启动器、prompt 或很早期候选 | 不跑仓库评估。 |
| `--profile fast` | 默认本地迭代和 PR 前快速验证 | 跑格式、release 产品二进制 build、harness check（含 hierarchical BM25 与受界 code-index persistence 不变量）、默认仓库子集、repo-set 护栏和 semantic/vector guardrail。 |
| `--profile full` | 需要完整产品和 harness rail 时 | 恢复 release build、clippy、test、具名 hierarchical BM25 gate、本地文件 fixture、完整仓库评估、semantic/vector fixture 和 research judge。 |
| `--profile exhaustive` | 长周期大仓、完整初始索引和压力验证 | 包含 exhaustive 仓库和更重的性能目标。 |
| `--categories ...` | 想让一轮聚焦某个分数族 | 仍保留显式 `guardrail=true` 底线 case。 |
| `--strategy unattended-layered` | 需要 1-2 天无人值守推进 | 用 smoke 探索、fast 验证、macro explore 升级和深度检查组合运行。 |

支持的 category：`foundational`、`competitive`、`semantic_vector`、`file_fixtures`、`repository_sets`、`agent_workflows`、`research_judge`、`performance`、`all`。`--exclude-categories` 会在 `all` 展开后移除指定类别，例如 `--categories all --exclude-categories research_judge`。

### 输出产物

| 产物 | 路径 | 用途 |
| --- | --- | --- |
| 候选 patch | `.git/relay-knowledge-self-iteration/patches-v2/` | 保存每轮候选净改动。 |
| 评估报告 | `.git/relay-knowledge-self-iteration/reports-v2/` | 保存 gate、case、metric 和命令输出摘要。 |
| 评分历史 | `.git/relay-knowledge-self-iteration/runs-v2.jsonl` | 记录每轮评分、采纳决策和优化计划。 |
| 长期记忆 | `.git/relay-knowledge-self-iteration/memory/` | 记录采纳/拒绝模式、退化和 patch 索引，供下一轮 prompt 使用。 |
| 无人值守状态 | `.git/relay-knowledge-self-iteration/unattended-state-v2.json` | 恢复 category rotation、失败计数、accepted 计数和 deep-check 调度。 |
| 图表 | `.git/relay-knowledge-self-iteration/score-v2.csv`、`score-v2.svg` | 查看 scored-run 历史；绿色为已提交采纳，琥珀色为手动评估可采纳，红色为拒绝。 |

### 运行可观测性

harness 会把实时进度写到 stderr，统一使用 `[self-iterate]` 前缀。每个子进程都会输出 `command start`、每 15 秒一次的 `command running` 心跳，以及带退出码和耗时的 `command done` 或 `command timeout`。评估阶段还会输出 profile、evaluation home、并发度、质量门禁 stage、仓库 workload 规模、repository-set workload 规模和最终 gate/case/command 计数。产品命令 stdout/stderr 仍捕获进 JSON 报告，长时间运行的 `fast` profile 不会处于无输出状态。

### 源码所有权

evaluator 根现在只声明模块并暴露 `evaluate_candidate` 与 `EvaluationRun`，所有行为和测试均归精确 owner。runtime 分离合同、有界并发、报告、finish 序列化和顶层 orchestration；workloads 依赖底层 runtime 服务，由 orchestration 向上组合，避免经 evaluator facade 形成反向依赖。evaluator 的 quality gate 合同归 `quality` 领域根，策略与执行也是真正的 owner 模块并分别直接挂载 UT。research judge 同样是真正的模块树：共享输入合同位于 `judge` 领域根，evaluation 单向组合各自拥有直属测试的 settings、prompt、backend 与 outcome。workload 执行已拆为显式的 agent、CLI、file、repository、repository-set、selection 与 semantic-vector 模块；共享 case scoring 拥有独立 owner，每个有行为的源码都直接挂载同级测试。fixture 源码族、仓库装配和文件写入同样由真正的 owner 模块负责；生成式 agent-workflow 源码常量归 fixture，而不再属于 workload 执行。Config、scoring、evaluator、workflow、内嵌 unattended 阶段、Case、进程适配器、history 和渐进记忆均使用真正的 Rust 模块，生产与测试代码都不再使用 `include!` 装配。无人值守运行嵌套在 `workflow` 下，因此可消费 workflow 服务而不形成顶层模块依赖环。

## 命令参考

### 语法和模式

```bash
./self-iterate.sh [mode] [options]
tools/self_iteration/target/debug/relay-knowledge-self-iterate [mode] [options]
```

| 模式 | 默认 | 行为 |
| --- | --- | --- |
| `loop` | 是 | 持续生成候选，直到循环限制触发；被采纳的候选由 harness 创建 commit。 |
| `once` | 否 | 只运行一轮生成和评估。 |
| `evaluate` | 否 | 不调用 Codex、不创建 commit，只给当前 diff 打分。 |
| `chart` | 否 | 导出 `score-v2.csv` 和 `score-v2.svg`。 |
| `research-plan` | 否 | 输出可复用的 Markdown research 自迭代计划，不调用 Codex、不运行评估、不写历史。 |

### 通用参数

| 参数 | 取值 / 默认值 | 作用 |
| --- | --- | --- |
| `--workspace PATH` | 启动脚本设为仓库根目录 | 传给 Codex 和评估器的工作区。 |
| `--strategy VALUE` | `single`；别名：`unattended-layered`、`unattended_layered`、`layered` | 选择普通单轮循环或长周期无人值守分层策略。 |
| `--profile VALUE` | `fast`；取值：`smoke`、`fast`、`full`、`exhaustive` | 选择质量门禁和评估 workload。 |
| `--categories LIST` | 未设置 | 聚焦一个或多个评分族，同时保留底线护栏。 |
| `--exclude-categories LIST` | 未设置 | 在 `all` 展开后移除指定类别；支持 `judge`、`semantic-vector`、`repo_sets` 等别名。 |
| `--max-iterations N` | 未设置 | 循环最多运行 N 轮。 |
| `--stop-after-accepted N` | 普通策略未设置；无人值守默认 `8` | 采纳 N 个 commit 后停止。 |
| `--sleep-seconds N` | `5` | 普通循环轮次之间等待；未覆盖时也会设置无人值守 cycle sleep。 |
| `--cycle-sleep-seconds N` | 无人值守默认 `120` | 无人值守 cycle 之间的等待时间。 |
| `--commit-message TEXT` | 根据分数生成 | 覆盖采纳候选的 commit subject。 |
| `--dry-run-codex` | false | 生成 prompt 并记录 dry generation，不真正调用 Codex。 |
| `--keep-workdirs` | false | 保留每轮 evaluation home。 |
| `--use-current-candidate` | false | 跳过 Codex，直接评估当前工作树 diff。 |
| `--fail-fast` | false | 首个迭代错误直接返回，而不是继续等循环限制。 |

### Codex、research 和并发参数

| 参数 | 取值 / 默认值 | 作用 |
| --- | --- | --- |
| `--research-topic TEXT` | `relay-knowledge research iteration` | 写入生成计划的人类可读研究主题。 |
| `--research-slug VALUE` | `research-iteration` | 用于归档、issue 或报告文件名的稳定 slug；只允许小写 ASCII、数字、`.`、`-`、`_`。 |
| `--research-date YYYY-MM-DD` | `YYYY-MM-DD` 占位值 | 写入生成计划的日期。 |
| `--yolo` | false；启动脚本默认传入 | 映射到非交互 Codex approvals 和 `danger-full-access` sandbox。 |
| `--model MODEL` | `gpt-5.6-sol` | 候选生成使用的 Codex 模型。 |
| `--codex-reasoning-effort VALUE` | `xhigh`；取值：`low`、`medium`、`high`、`xhigh` | 设置 `model_reasoning_effort`。 |
| `--codex-profile NAME` | 未设置 | 向 Codex 传入 `-p NAME`。 |
| `--codex-path PATH` | `codex` | Codex 可执行文件路径。 |
| `--codex-timeout-seconds N` | `3600` | 候选生成超时时间。 |
| `--command-timeout-seconds N` | `900` | 评估子进程和产品 CLI 命令超时时间。 |
| `--jobs auto|N` | `auto` | 全局 command limiter；`auto` 使用可用 CPU 数或 `RELAY_KNOWLEDGE_SELF_ITERATION_JOBS`。 |
| `--repo-jobs auto|N` | `auto` | 仓库级并发；`auto` 使用可用 CPU 数的一半。 |
| `--query-jobs auto|N` | `auto` | 查询子进程并发；`auto` 使用可用 CPU 数。 |

### 无人值守参数

| 参数 | 默认值 | 作用 |
| --- | --- | --- |
| `--max-wall-clock-hours N` | `36` | 无人值守总运行时长上限。 |
| `--explore-timeout-seconds N` | `900` | 短 explore Codex 尝试超时时间。 |
| `--macro-explore-timeout-seconds N` | `2700` | macro mutation 尝试超时时间。 |
| `--max-explore-attempts-per-cycle N` | `3` | 一个 cycle 内短 explore 的重试次数。 |
| `--max-consecutive-empty-candidates N` | `8` | 连续无 diff 生成达到上限后停止。 |
| `--max-consecutive-promotion-failures N` | `10` | 连续 screen/validate 失败达到上限后停止。 |
| `--macro-after-competitive-failures N` | `4` | competitive 连续失败后触发 macro mutation。 |
| `--macro-after-empty-candidates N` | `6` | 连续空候选后触发 macro mutation。 |
| `--cooldown-after-accept-seconds N` | `300` | 采纳 commit 后等待时间。 |
| `--cooldown-after-timeout-seconds N` | `900` | Codex timeout 后等待时间。 |
| `--deep-check-interval-accepts N` | `6` | 采纳达到该数量后运行 deeper validation。 |
| `--deep-check-interval-hours N` | `12` | 达到该小时间隔后运行 deeper validation。 |

### 环境变量

| 变量 | 作用 |
| --- | --- |
| `RELAY_KNOWLEDGE_SELF_ITERATION_RELEASE=1` | 让 `self-iterate.sh` 构建并运行 release harness binary。 |
| `RELAY_KNOWLEDGE_SELF_ITERATION_JOBS=N` | 只覆盖 `--jobs auto` 的全局并发默认值。 |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS` | 逗号分隔的 fast profile 仓库子集。 |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT` | fast profile 每仓 case 数量上限。 |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS` | 逗号分隔的 fast repository-set 子集。 |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT` | fast profile 每个 repository-set 的 case 数量上限。 |
| `RELAY_KNOWLEDGE_JUDGE_BACKEND` | `http`、`openai`、`openai_compatible`、`api`、`llm`、`cli`、`opencode`、`agent`、`none`；禁用别名：`off`、`disabled`、`skip`、`false`。 |
| `RELAY_KNOWLEDGE_JUDGE_BASE_URL`、`RELAY_KNOWLEDGE_JUDGE_API_KEY`、`RELAY_KNOWLEDGE_JUDGE_MODEL` | OpenAI-compatible HTTP judge 配置。 |
| `RELAY_KNOWLEDGE_JUDGE_COMMAND` | CLI judge 命令模板；别名：`RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND`、`RELAY_KNOWLEDGE_JUDGE_CLI_COMMAND`。 |
| `RELAY_KNOWLEDGE_JUDGE_TIMEOUT_SECONDS` | judge 通用超时时间，默认 `120`。 |

### YOLO 和 research-plan

本地 Codex CLI 没有字面意义上的 `--yolo` 参数。本框架会把 `--yolo` 映射到当前非交互、高权限 Codex 调用：

```bash
codex -a never exec --dangerously-bypass-approvals-and-sandbox -s danger-full-access -C /opt/workspace/relay-knowledge -m gpt-5.6-sol -c 'model_reasoning_effort="xhigh"' -
```

只应在外部可信的工作区中使用。默认生成模型为 `gpt-5.6-sol`，推理强度为 `model_reasoning_effort="xhigh"`；需要更低成本或不同生成模式时，用 `--model` 和 `--codex-reasoning-effort low|medium|high|xhigh` 覆盖。

`research-plan` 是只读模式：不调用 Codex、不运行评估、不创建历史记录。它会把图数据库、CodeGraph、X.com、Reddit 和 arXiv 深度研究中的可重复方法整理为 Markdown 计划，包含来源台账 checklist、综合矩阵模板、竞品 issue 提取规则、文档/归档产物、验证门禁和完成证据。

## 运行模型

### 单轮生命周期

每一轮迭代会：

1. 检查工作树是否干净，除非传入 `--use-current-candidate`。
2. 提示本地 Codex 做一个聚焦的代码检索改进。
3. 将候选补丁保存到 `patches-v2/`。
4. 按 profile 运行质量门禁和评估。
5. 将报告写入 `reports-v2/`。
6. 将评分历史追加到 `runs-v2.jsonl`。
7. 更新 `score-v2.csv` 和 `score-v2.svg`。
8. 采纳前，把优化思路、变更文件、指标改善和已知退化追加到 `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`。
9. 只有当采纳策略接受候选时，才把候选净改动和采纳记录 squash 成一个 commit。
10. 候选被拒绝时，恢复到本轮开始的 commit。

如果启动时工作树是 dirty 状态，循环会立即退出，而不是重复重试同一个不可重试的前置条件失败。实现类候选必须在评估前更新自迭代优化记录，写清算法、架构、不变量、预期 case/metric 影响和已知风险；`self_iteration_algorithm_documentation` gate 会拒绝没有这些说明的代码、测试、benchmark 或 harness 策略变更。

### 历史和长期记忆

v2 harness 将 `runs-v2.jsonl`、`reports-v2/` 和 `patches-v2/` 与早期格式隔离。每次评分还会写入 `memory/index.jsonl`、`memory/summaries/` 和 `memory/details/`，下一轮 prompt 会收到拒绝恢复记忆、受限记忆索引、按 profile 汇总的历史综合摘要和受限历史 patch 索引。被拒记忆会记录变更路径、score delta、局部改善、退化和连续拒绝簇，帮助 Codex 避免重复尝试已经输给采纳基线的小改动。

prompt 只注入有界摘要，长期迭代不会随历史长度线性填满 LLM 上下文。它还要求 Codex 做仓库检查时优先使用 `rg`；如果本机未安装 `rg`，则改用排除 VCS 和 build 目录的有界 `grep -RIn` 搜索。

### 默认 fast profile

`fast` 是默认 profile，目标是用较低成本覆盖最容易回归的路径：

| 分组 | 覆盖内容 |
| --- | --- |
| 基础质量门禁 | 产品与 harness 的 `fmt --check`、Linux GNU glibc 2.28 baseline 策略门禁、`cargo build --release --bin relay-knowledge`、harness `cargo check`。 |
| 产品 gate | `skill_metadata_policy_cases`、`business_knowledge_regression_cases`、`code_index_recovery_cases`、`code_index_health_isolation_cases`、`code_index_sqlite_lock_cases`，以及覆盖 index-worker 和强类型 CodeSpec/Knowledge map 的 CLI contract case。 |
| 默认仓库 | `index_performance_many_files`、`index_performance_c_fragment`、`c_syntax_fixture`、`cpp_syntax_fixture`、`cross_language_syntax_fixture`、`typescript_syntax_fixture`、`nonstandard_layout_fixture`、`software_global_fixture`、`project_alias_fixture`、`relay_teams`、`leveldb_cpp`、`temporal_samples_go`、`temporal_sdk_go`。 |
| 默认取样 | 普通仓库默认取前 8 条 query case，并始终保留显式 `guardrail=true` case。 |
| repository-set | 默认保留 `temporal_go_workspace` 的 2 条跨仓门槛 case。 |
| semantic/vector | 默认运行 1 条 guardrail query。 |
| coding-agent 工作流 | `fast` 默认跳过；通过 `--categories agent_workflows` 或 PR benchmark workflow 运行。 |
| 运行时状态 | 每次 evaluation 都使用新的 `.git/relay-knowledge-self-iteration/work-v2/<run-id>/home/`，不跨轮复用可变数据库或生成式 fixture 状态。`fast` 仍复用 Cargo 编译产物与历史/baseline，但仓库延迟属于冷启动测量，继续受已声明 key budget 约束。 |

所有非 `smoke` profile 都使用 `target/release/relay-knowledge` 运行仓库 workload；debug harness 仍可编排这个 release 产品二进制。evaluation report 会记录 `product_binary_profile` 与 `product_binary_path`，workload previous/best history 和 profile 范围的硬 acceptance floor 都只在相同产品二进制口径内比较。缺少该字段的历史记录保持旧语义（`fast=debug`，其他 profile 为 `release`），因此旧 fast debug 分数和耗时既不能拒绝 fast release candidate，也不能成为其 baseline。comparison metadata 把该硬 floor 标为 `evaluation_profile_and_product_binary_profile_acceptance_floor`；同时以 `evaluation_profile_diagnostic_only` 报告跨产品口径的历史最高分，但该诊断值不参与采纳。`smoke` 只运行格式门禁，不构建也不执行产品 workload。

`fast` 默认不跑全量 clippy、全量 test、本地文件 fixture、research judge 或 harness 自身的 release build。`full`/`exhaustive` 会恢复这些 rail，并运行完整仓库评估、repository-set case、本地文件 fixture、semantic/vector fixture 和 research judge。

关键 fast 护栏的责任边界：

| 护栏 | 保护点 |
| --- | --- |
| `skill_metadata_policy_cases` | 拒绝把 Windows 命令或资产示例放进 bash/POSIX code fence，保证 agent-facing 指令保持 shell-specific。 |
| CLI contract case | 验证 agent 可见 help 暴露 `repo index-worker`，验证 idle worker 与 streaming worker 输出可解析 JSON，并保护强类型 CodeSpec/Knowledge map 的 help、校验、目录过滤和业务路由查询。 |
| `code_index_recovery_cases` | 覆盖过期 task lease 恢复、旧 worker 完成拒绝、attempt-budget dead-letter、checkpoint batch 续租、每个 durable finalization step 的执行前后续租边界、有界 finalization-step 推导、query-index subphase 从下一 unit 恢复，以及 writer lock 获取后拒绝 caller 传入的陈旧续租观测时间。其 `code_index_task_` case 还要求超限 worktree overlay 先克隆不可变基线、把 dirty delta 划分为确定性的有界批次、在批次间 lease takeover 后精确续跑，并持久化准确的多批收据。同一 gate 继续冻结 v3 17-slot plan 与 grouped reference-search v2：cleanup/discover/build page count、occurrence-to-group 聚合、exact manifest、rollback/reopen replay、公平完整 occurrence expansion，以及带 v2 budget clamp 的 leased v1 restart。Retired query-index unit 1 不得重新创建或自动删除、既有同名 shape 继续严格校验、v1/v2 cursor 不得跳过物理 unit 1 且 token version 必须跨 writer quantum 保留，并且每个 fresh Restart 即使只有单 path，也只能在 empty owner 上预建 chunk unit 13/14、同时继续延后全部其他 heavy index。该 gate 在所有 non-smoke profile 中运行，包括 fast 与 performance-focused evaluation。 |
| `code_index_persistence_performance_suite` | 作为隔离的 `fast` stage 运行，timeout 为 120 秒，key budget 为 30,000 ms。直属 owner 与 SQLite trace 要求 1,025 条 reference、symbol、chunk 在默认 1,024-row 上限下各只使用两条受界 base statement；runtime variable-limit 边界测试强制各 owner 的精确单行下限；rollback/replay 测试保留 checkpoint、staging、FTS 与 fence ownership。Search-document trace/EQP 会在高位 raw orphan 之后跨越 runtime-clamped 1,024-document flush 边界，要求恰好两次主 FTS insert、每个 flush 一次带 equality constraint 的 `INT64_MAX` 点查，固定 12/6/5-variable 对应两行/单行/拒绝边界，拒绝 constructor 或 flush 执行任何 `max(rowid)` aggregate，并保持 post-insert FTS/metadata interval 精确。Grouped reference-search 测试禁止 nullable-range SQL，要求首条页/续页均使用 indexed keyset，证明 length-only lazy scan 会在 payload fetch 前拒绝超大 cursor，要求每个已接纳页面只点查最后一个 durable cursor，并以 SQLite VM-step measurement 证明 returning UPSERT 在不改变 page cap 的前提下移除了重复的 discovery-page grouped scan。其 build-page trace 还要求 1,025 个已接纳 group 只使用一条有序主 FTS `INSERT ... SELECT` 与一条 metadata insert，同时继续保护规范空字段内容和写入前 `INT64_MAX` 拒绝。普通 `finalizing:resolve_references:v1` 测试覆盖 multi-row keyset page、两条 control row 与完整记录 byte accounting、带每页 name/path cache 的 length-only owner probe、精确 budget 边界、rollback/reopen/fence replay，以及不随 hot symbol 尾部增长的 VM work。1,025 行 call-only 页面必须执行零 payload 点查和零 owner update、推进 exact count/cursor，且只点查末游标；专属 call-target 测试继续保护 stale-binding 校验。 |
| `code_index_health_isolation_cases` | 验证 no-language-filter 仓库更新时 health 查询有界，`repo query --freshness allow-stale` 能读取最新已提交 scope。 |
| `code_index_sqlite_lock_cases` | 保护重复进程 SQLite lock 避免、active-task 复用和不同 task fingerprint 的并发 claim。 |
| `bm25_hierarchy_build` | 在独立 stage 中用 `cargo test --no-run` 编译并链接精确的 `--lib --all-features` test target。1,200 秒硬超时与现有 root Rust gate 上界一致，可覆盖干净环境的冷构建，不依赖预热，也不允许无限等待；该 preparation gate 没有 latency budget，不产生 BM25 性能结论。 |
| `bm25_hierarchy_suite` | 在 Cargo build lock 已释放后的下一独立 stage 运行，保留原有 120 秒执行超时和 30 秒 non-key whole-suite 诊断预算；因此该指标只包含 Cargo freshness 检查与 50 条确定性产品测试，不包含冷编译/链接，超过 30 秒只影响诊断/评分，并不单独形成硬 gate 失败。测试保护 `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4` 合同：一个 synthetic 4,096-document production-write/query-path fixture 保证 Recall@10 至少 0.9，并把 planned-MATCH result domain 从 768 行减到 448 行；同 v4 routed/flat score parity、selected-document/coarse-score bound、在 SQL scope 保持权威的同时用一个 `graph_bm25 MATCH` 对 business term、zero-weight scope64 token 与 scope-qualified group token 求交、hidden rank 与 rowid-sidecar hydrate、有界 persisted-DF probe、version-leading unscoped historical index、可观察 oversized-label degradation 与 fuzzy-posting bound，以及带 durable owner/expiry、phase/cursor、semantic/vector plan、128-document/4-MiB/8,192-label/8,192-link transaction budget、oversize-document isolation warning、companion-read pause、fence/swap/rollback 的可续跑 shadow rebuild。移除任一 invariant 都会在不依赖 wall-clock timing 的情况下失败。448/768 result-domain invariant 不是 posting scan、VM step 或 query-latency measurement；该 gate 不证明自然语料 recall/performance、equal-score cutoff 的确定 membership 或整个 hybrid pipeline 的 end-to-end bound。 |
| syntax 与 layout fixture | 保护 external import unresolved metadata、C/C++ 可恢复 parser error、非顶层 `src/` 布局、project alias 复用同一 indexed scope 和 source/text fallback 底线。 |
| `software_global_fixture` | 确保 `repo software` 事实来自已索引证据，并保护 ontology 分类：普通 README heading 只保留为文档，Dockerfile 是 build definition，CI job 不是 IaC resource，Terraform、Kubernetes、Compose 和 systemd 保持 deployment/resource 类型。Fixture 还检查 systems/APIs/resources/tests/deployments/releases、statement provenance、conflict、ontology/schema version 与 completeness，不扫描包缓存、云 API、SDK 目录或未索引外部源码。 |
| `business_knowledge_regression_cases` | 每次 fast evaluation 运行，保护 acronym/alias 解析、跨 domain 同名词 ambiguity、竞争 definition 保留、mapping resolved/unresolved hint、route 授权与 business publication barrier。 |
| `agent_workflow_fixture` | 用生成式 Rust、TypeScript、Python、YAML 和 Markdown 证据重放 coding-agent issue 分析任务，并约束工具调用、源码读取、输出/context 大小、证据数量、fallback 比例和总延迟。 |

software lifecycle projection 先在 SQLite 中用当前支持的 manifest、CI/IaC 和 Markdown 路径语义超集过滤普通源码，再进入 Rust 物化边界。它预检固定的 32,768 candidate documents、262,144 chunks 和 256 MiB 上限，按 path 顺序一次流式物化一个文档并共同喂给 build/IaC/design collector，同时输出 candidate document/chunk/byte 计数；component、dependency usage、SDK、build、IaC、design、entity、statement 和 diagnostic 都走有界存储路径。单 SQLite 的 fenced full 或 incremental index 依次推进 software projection v2 的 reset、dependencies、SDK usages、lifecycle、files、topics、relationships、ontology、publish；中间 phase 释放 writer 供 lease 续期时 code scope 仍保持 stale。fence 再校验后，software status、code scope/repository freshness、checkpoint completion 与 publication receipt 同时可见。partitioned store 不宣称跨数据库原子：code/software facts 先在目标 shard 完成，而 catalog route 仍由当前 task 持有并保持 `staged`；随后一个 fenced control-database transaction 校验 owner，激活 repository/scope route，镜像 fresh status 并写入 receipt。active control route 尚未存在时，对外 checkpoint 仍保持未发布状态；control transaction 前后 crash 都可幂等收敛，不会重新解析已经耐久的 target。task `succeeded` 是之后的独立 fenced completion transaction，必须验证 receipt 与匹配的 fresh target，外部 worker response 仍等待该 task terminal 状态。

通用 library-test rail 会另行验证每篇文档 256 labels、每个 label 1,024 bytes、每篇文档 8,192 grams 的 fuzzy-index limit，以及 request-level disable 跳过全部 graph-search source family；这些 test 不属于按名称过滤的 `bm25_hierarchy_suite` fast gate。

若要调整默认子集，可设置：

```bash
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=index_performance_many_files,index_performance_c_fragment,c_syntax_fixture,cpp_syntax_fixture,cross_language_syntax_fixture,typescript_syntax_fixture,nonstandard_layout_fixture,software_global_fixture,project_alias_fixture,relay_teams,leveldb_cpp,temporal_samples_go,temporal_sdk_go
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT=12
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS=temporal_go_workspace
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT=2
```

`full` 和 `exhaustive` 额外运行 `index_performance_wide_mixed_files`，生成 2048 个 Rust 目标文件和跨 shard bridge 查询。其 finalize 后 guardrail 会在 bridge path 上执行真实 `references` 查询 `rk_wide_target_2047`；fixture 为同一 grouped identity 提供两个 occurrence，focused storage test 则要求确定性完整展开。每个仓库索引完成后，harness 都会针对固定 ref 执行只读 `repo scope preview`；preview 的精确 `selected_file_count`、repository/alias、requested/resolved ref、tree hash 及 path/language filter 必须与索引返回的 task、summary、checkpoint、scope 和 fresh status 全部一致；冷 full index 同时强制 `task.state=succeeded`、`checkpoint.state=completed` 及精确 committed/status 计数。harness 还会把固定父 Git tree 的原始条目数记录为独立诊断证据，但不会把它当作 selected count 上界：父 tree 会把 gitlink 记成一个条目，而授权的索引范围可能展开子模块内容。该 Git 观测经过 evaluation 全局命令 limiter，且 timeout 上限为 120 秒；普通 filesystem source 会记录 `source_kind=filesystem` 且不提供 raw Git count，不会因此失败。显式声明的 `expected_file_count` 会保留，若与 preview 的精确 selected count 冲突则硬失败，不会被观测值覆盖。生成式性能仓库随后创建包含修改、新增和删除文件的第二个提交并执行 `repo update`；incremental completion 要求 succeeded task 与精确 summary/scope/status count 和 identity，同时保留 changed-path/blob-read/parse budget。合法的已完成 incremental response 可以不返回 `checkpoint`；fenced durable-clone path 返回 checkpoint 时，它必须 completed、属于精确 target scope，并证明完整 selected target count。Delta 成本不再从 scope-wide checkpoint counter 推断，而由 task-bound `incremental_summary` receipt 精确保存 base identity 与 changed-path/blob-read/parse metrics。checkpoint payload 只暴露 repository 与 scope identity，没有 commit/tree 字段，因此 commit/tree identity 会在 scope、task、summary 和 status 之间强制校验。报告记录 `*_cold_index_ms`、`*_cold_register_index_ms`、`*_incremental_index_ms` 和 query p50/p95/max。

所有带 fence 的 clean incremental run 都使用 durable clone，因此即使 base 能装入 direct transaction，生成式性能仓库仍会执行同一恢复路径。Task-bound receipt 必须让 delta metrics 跨 clone 到 `indexing` finalization 及 response loss 保持不变；后续 task 采用相同 content 时则返回既有 neutral no-work summary。Benchmark CI 继续把 `index_performance_many_files` rail 固定在 3,000 ms，并要求精确 3 changed paths、2 blob reads、2 parsed files、succeeded task/completed checkpoint 与 named persistence gate。Full/exhaustive 继续把 `index_performance_wide_mixed_files` 固定在 5,000 ms，并保持同样的 2-read/2-parse 上限。Legacy fact proof 缺失或为零时可以走类型化 full staging，但不能被计作 incremental pass 或 target write。

对于显式声明的 `index_only_performance_target`，只有 `<repository>_cold_index_completion` 严格校验成功后，该仓库报告才会增加 `cold_index_result`。它完整保留冷启动 `repo index` 的原始 JSON，包括 `scope`、`task`、`summary`、`checkpoint` 和 `status`，因此零 retrieval-case target 仍有可独立审计的完成性、freshness、计数和 identity 证据。普通仓库报告保持既有 `index_summary` schema 并省略 `cold_index_result`；index-only target 的严格冷终态校验失败时也会省略该字段。双语弹性预算 benchmark 合同给出了最终 `jq` 验收断言。

隔离是单轮内部的测量与磁盘生命周期边界，不替代共享状态覆盖。配置 `isolated_index_home=true` 的仓库会在唯一 run home 下获得一个子 home；harness 收集完 commands、cases、metrics 与内存 report 后删除它，只有显式传入 `--keep-workdirs` 才保留，评估报错时也会清理。创建与递归清理要求 run/isolation/home 每级都是非 symlink 目录，并满足 canonical direct-parent containment。repository-set member 若申请隔离会在配置合并后被拒绝，因为 overlay 必须从本轮公共 home 读取全部成员。小型 LevelDB 与 OpenTelemetry set 仍在同一个全新 run 内共享状态，用于保留顺序与 overlay 覆盖，但不会把状态带到下一轮。

`.github/workflows/benchmark-checks.yml` 会在 pull request 上运行 1024 文件性能 fixture，并先断言 JSON 报告选择了 `target/release/relay-knowledge` release 产品二进制，再从同一报告直接验证冷任务/checkpoint 已完成、三路径增量 delta、两文件 blob/parse 预算、完成性命令和三项延迟预算。

### coding-agent 工作流门禁

`--categories agent_workflows` 会运行 `cases/agent_workflow_targets.json` 中的确定性端到端 coding-agent 场景。fixture 覆盖定义定位、one-call `repo context` 打包、跨语言影响追踪、配置到文档追踪和 freshness policy 检查。每个场景执行有界 `repo query` 或 `repo context` 步骤；当期望证据缺失、context/output 超过预算、需要读取的唯一源码文件过多、text fallback 在证据包中过高、工具调用数过多，或总查询延迟超过阈值时失败。

PR benchmark workflow 会以 `agent-workflow-regression` job 运行该 category，并通过 `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=agent_workflow_fixture` 将运行范围限制到生成式 fixture。evaluation 结束后，workflow 要求四个 `(repository, case_id)` observation 精确齐全且不重复，确认该 category 已选择而非跳过，并要求 agent metric 非空；任一 gate、case 或 agent workflow metric budget 失败都会让 CI 失败，因此空 observation 不能真空通过。该 CI 门禁不使用 score-vs-history 的采纳决策。这样能控制 CI 成本，同时覆盖 agent-facing 行为。

### category 聚焦

`--categories` 会执行显式 guardrail case 加上所选类别 case；guardrail case 失败会转成 quality gate 失败，即使聚焦分数提升也会拒绝候选。`--categories semantic_vector` 会运行完整 semantic/vector suite，并保留 repository 与 repo-set 底线 case；`--categories performance` 会保留产生性能指标的 repository、repo-set、semantic/vector 和 file-fixture workload，而不是只剩 guardrail。评分历史按 profile 和 category focus 隔离，采纳时还会检查同 profile 下跨 category 的最佳已提交 run，避免新 category 首次运行因为同类 baseline 为空而接纳低于 profile 水位的候选。

### 并发边界

并发默认使用 `--jobs auto`、`--repo-jobs auto` 和 `--query-jobs auto`。`auto` 会让全局 command limiter 和 query pool 使用可用 CPU 数，repository jobs 使用可用 CPU 数的一半。同一次 evaluation 内的全部仓库 register/index 与 repository-set create/add/refresh writer 命令共享本轮 writer lock，隔离 home 也不例外。不同 harness 进程使用不同 run-scoped home，因此不需要为可变数据库增加跨进程锁。该边界限制单轮磁盘与 I/O 压力，并避免并发 writer 污染冷延迟；写边界之后的查询子进程可并发运行。Command completion 使用操作系统的 child-exit 通知，并以剩余 timeout/progress deadline 为等待上界，因此报告的 query latency 不再被旧有 20 ms polling interval 向上量化。

### 无人值守分层策略

`--strategy unattended-layered` 面向 1-2 天无人值守运行；未显式传入时，普通 `loop`/`once` 行为保持不变。默认按 36 小时窗口设置，关键参数见上方“无人值守参数”表。

每个 cycle 先用 `smoke` profile 做短探索，按 `competitive -> semantic_vector -> performance -> repository_sets` 轮转 category。Codex 只在 explore 层运行；候选通过 smoke screen 后，复用同一个 patch 进入同 category 的 `fast` validate，只有 validate 通过才进入既有 accept/commit 路径。

当短探索持续没有产出时，策略会升级到竞争力能力的 `macro_explore`。触发条件包括 repeated competitive promotion failure、连续 empty candidate，或当前 competitive capability 相对 best accepted focused baseline 出现超过阈值的差距。macro prompt 注入当前能力快照、`cases.json` 中的 `research_judge_suite.competitive_feature_targets` 和 `implementation_guardrails`，要求 Codex 做 ranking、indexing、relationship extraction、query planning、context construction 或 retrieval evidence 这类较大的泛化改进。候选说明必须写清 mutation hypothesis、affected subsystem、expected capability jump 和 regression containment，并继续禁止 fixture/query/path/symbol 特化枚举。

## 评分和采纳

### 加权分数

research judge 被禁用或跳过时：

```text
foundational_capability * 0.22
+ competitive_capability * 0.22
+ semantic_vector * 0.13
+ performance * 0.18
+ stability * 0.25
```

启用 research judge 后：

```text
foundational_capability * 0.17
+ competitive_capability * 0.17
+ semantic_vector * 0.10
+ research_judge * 0.22
+ performance * 0.15
+ stability * 0.19
```

这些公式先得到 `base_score`。持久化的 `score` 是 `min(1.0, base_score + capability_ceiling_bonus)`。动态天花板 bonus 上限为 `0.06`，只使用 latest matching workload run 或同 profile best accepted run 中真实存在的 baseline component 字段；缺少 judge 输出不会产生 research bonus，bonus 也不能绕过失败 gate、缺失 diff 或受保护目标回退。缺失 diff 仍会拒绝采纳，且无 diff 的 loop 记录不会作为后续 workload baseline；但当所选质量门通过时不会把 `stability` 组件归零。手动 `evaluate --use-current-candidate` 因此能在只验证当前基线时保持性能和 gate 分数可读。

### research judge

research judge 判断研究对齐、竞争优势、架构合理性、性能泛化、实现可操作性、是否存在 fixture 特化以及 judge evidence quality。它必须返回严格 JSON，字段包括 `passed`、`confidence`、`overall_score`、`scores`、`summary`、`evidence`、`risks`、`recommended_cases`、`capability_delta` 和 `research_gaps`；每个配置的 rubric dimension 都必须出现在 `scores` 中并达到 `min_dimension_score`。

judge 可通过 OpenAI-compatible HTTP endpoint 运行，也可通过 coding-agent CLI 运行，例如 `opencode`、`relay-teams`、`codex`、`cc` 或 `copilot`。未提供 judge backend 或 HTTP 配置时，CLI judge 默认使用 `opencode`。HTTP API key 只从环境变量读取，不写入报告。设置 `RELAY_KNOWLEDGE_JUDGE_BACKEND=none` 时仍选择 suite 但记录 `judge_skipped`；需要完全不运行 suite 时使用 `--exclude-categories research_judge`。显式配置但缺少必需环境变量、返回非法 JSON、低置信度、低总分、低 anti-fixture-special-casing 分数、缺失维度分数或必需维度分数过低时会拒绝候选。

### case 和性能目标

case objective 是连续质量分，不是通过率计数。rank 1 通过时从 `1.0` 起算；rank `N > 1` 即使仍在 `max_rank` 阈值内，也只从 `1.0 / N` 起算。case 还可以声明 `expected_all`、`expected_sequence`、`min_score`、`require_expected_all`、`require_expected_sequence`、`forbidden_rank_penalty` 和 `forbidden_rank_penalty_only`。空结果负例以 `rank=0` 通过时仍得 `1.0`。缺失的 foundational、competitive 或 semantic/vector objective 默认 `0.0`；`accuracy` 只汇总实际存在的 foundational 与 competitive objective。

`performance` 使用 `budget_relative_v2`。没有兼容上一轮记录时，指标使用按预算归一化的分数。lower-is-better 指标的任何非负且不超过预算的值都获得完整预算适配分，包括 `0` 和 `text_fallback_ratio` 一类小于 `1` 的比例；只有超预算值才使用有界 `budget / value`。higher-is-better 的预算适配使用 `value / budget`。存在兼容上一轮时，相对进步对 lower-is-better 使用 `previous / current`，对 higher-is-better 使用 `current / previous`，并封顶 `1.25`；相等值（包括 `0 == 0`）以 `1.0` 为中性，正值降到零（或 higher-is-better 从零升到正值）获得上界。最终单项分数由 70% 预算适配与 30% 相对进步混合。

### 采纳策略

采纳策略是“带硬约束和加权分数决胜的 epsilon-Pareto 采纳策略”。build/test gate、候选 diff 存在性以及每个当前 key metric 的声明预算都是硬约束。lower-is-better 的 key metric 在值高于预算时失败，higher-is-better 的 key metric 在值低于预算时失败；non-key metric 仍只作为评分与诊断信号，不构成硬拒绝。foundational_capability、competitive_capability、semantic_vector、stability 和延迟观测是受保护目标；epsilon 阈值用于抑制测量噪声；加权分数是决胜项而不是唯一决策规则。

候选在以下条件满足时被采纳：

```text
hard_constraints_pass
and no_current_key_metric_budget_failure
and no_protected_foundational_competitive_semantic_vector_or_stability_regression
and (
  no_profile_best_accepted
  or weighted_score > profile_best_accepted_weighted_score + score_epsilon
  or bug_fix_priority_improved(candidate, previous)
)
and (
  bug_fix_priority_improved(candidate, previous)
  or
  weighted_score > previous_weighted_score + score_epsilon
  or epsilon_pareto_improved(candidate, previous)
)
```

`bug_fix_priority_improved` 表示候选修复了已观测到的程序失败：上一轮失败的 quality gate 变为通过，或上一轮失败的 evaluation case 变为通过。它可以越过加权分数决胜项、profile 级最佳已提交分数线和原始耗时退化，但不能越过缺少 diff、当前 gate 失败、当前 key metric 预算失败或受保护目标回退。每个 key metric 硬拒绝都会在 `reject_reasons` 中列出指标名、观测值和声明预算。

默认 epsilon：

| 阈值 | 默认值 | 用途 |
| --- | --- | --- |
| `score_epsilon` | `0.0005` | 总分比较。 |
| `ratio_epsilon` | `0.005` | foundational、competitive、semantic_vector、performance、stability 等分数组件。 |
| `metric_epsilon` | `max(1e-9, 0.03 * max(abs(previous), abs(current)), min(25, 0.03 * budget))`；没有预算时省略 budget 项 | 对称的原始指标变化检测。声明预算提供量纲相关的噪声下限，两端观测提供连续的相对尺度，数值跨过 `1.0` 时不会切换公式。 |

退化会被记录为下一轮 Codex prompt 的 degradation feedback；正向改善也会传给下一轮，让后续迭代知道哪些成果需要保持。被采纳的优化方案会进入 run history 的 `optimization_plan` 字段，并在下一轮 prompt 的 `Recent adopted optimization plans to build on` 段落中作为设计参考。

## 评估数据

`cases.json` 及其 `include_files` 定义自迭代目标 workload。根文件只维护有界 manifest 和全局 suite；基础 repository query target 按 project-alias、relay-teams、Linux、LevelDB、Spring Framework 和 Kubernetes 拆入具名 include 文件。它不是“当前已经全部实现”的能力清单；新增 case 可以代表下一轮候选需要补齐的竞争力目标。候选应改进通用 parser、图边、候选收缩、排序、service workflow 或可观测性，不能通过删除、放宽或枚举 case 获得分数。

### 生成式和本地 fixture

| 分组 | 覆盖 |
| --- | --- |
| 本地文件索引 fixture | 临时生成 user documents、Linux `/opt` 风格路径、Windows `D:` 风格路径、深层目录和高噪声文件集合，运行 `files index/query`，记录 `file_index_ms`、`file_query_p50_ms`、`file_query_p95_ms`。 |
| C/C++ 语法 fixture | 生成临时 git 仓库并走 `repo register/index/query`，覆盖 function pointer typedef、operation table、initializer、macro、本地 include、callback dispatch、namespace、template、override、operator、lambda、alias 和 header/source split。设计说明见 `docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md`。 |
| 跨语言语法 fixture | 覆盖 C 调 C++、C++ 调 C、Go cgo 调 C、Rust FFI 调 C，让默认 fast 不依赖额外大仓也能验证多语言调用图。 |
| 额外多语言 fixture | 覆盖 Python、JavaScript、TypeScript/TSX、Go、Java、Rust、Bash、C#、Kotlin、PHP、Ruby、Scala 和 Swift；矩阵见 `docs/zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md`。 |
| repository-set targets | 注册每个成员为 `scope=all` 仓库，创建显式 `repo-set`，刷新跨仓 overlay，再运行 `repo-set query`；case 可要求具体 member、source_scope、路径、行号和 excerpt 证据。 |
| 冷索引与增量索引性能 targets | `repository_index_performance_targets.json` 配置冷索引 `index_budget_ms`/`register_index_budget_ms`、增量 `incremental_index_budget_ms`、完成性证据和 delta 读/解析上限；默认 fast 包含 1024 文件 fixture，`full`/`exhaustive` 还包含 2048 文件 wide fixture。 |
| Hierarchical BM25 算法 gate | `fast`、`full`、`exhaustive` 先运行不带指标预算的 `bm25_hierarchy_build` preparation gate，以 1,200 秒有界超时覆盖冷构建；随后独占运行 `bm25_hierarchy_suite`，保留原有 120 秒超时与 30 秒 non-key 诊断预算。固定 SQLite fixture 校验 v4 fingerprint/scope partition、同 schema flat parity、synthetic production-write/query-path Recall@10 >= 0.9 floor、planned-MATCH result-domain reduction、hard SQL authorization、single-FTS hidden-rank/rowid-hydrate shape、persisted-DF 与 65,536-posting admission bound、route-document `fts_rowid`/version/label-state invariant、version-leading global fallback index、可观察 oversized-label degradation 与 8,192-posting exhaustion、durable checkpoint takeover、全部四类 rebuild work budget、oversize-document isolation 与 bounded warning identity、当前 writer fence、companion-read pause、complete-reader activation 与 swap rollback。报告把 build preparation 与 whole-suite duration、捕获的 `BM25_WORK` 分开保留；这些都不是 query latency 或 FTS posting/VM-step work，equal-score cutoff membership、自然语料和整个 pipeline 的结论不属于该 synthetic gate。 |
| 软件全域 ontology targets | `repository_software_global_targets.json` 运行全部兼容和类型化 `repo software` kind，检查 ontology version 1.0.0、projection schema 6、statement provenance 100% 完整以及关键禁止误分类。这些 case 位于 fast fixture，`--categories performance` 也会选中它们，使投影吞吐/查询预算与语义分类回归共同受保护，同时禁止在产品代码中加入仓库特判。 |
| Framework graph targets | `repository_framework_targets.json` 在锁定的 Angular/Vue 官方仓库上运行独立 `repo framework` surface。Case 同时评分 graph node/edge，并执行声明的冷索引、p50 与 p95 预算。 |
| CLI contract cases | 直接运行产品 CLI，不需要大仓；默认 fast 覆盖 `repo index-worker` help、idle/streaming JSON，以及强类型 CodeSpec/Knowledge map 的 help、校验、目录过滤和业务路由查询。 |
| semantic/vector suite | 写入小型 evidence，刷新 semantic/vector 索引，验证 query 命中 `retriever_sources`、`backend_statuses` 和相关排序；外部 provider 只从运行时环境继承。 |
| research_judge_suite | 把候选 diff、确定性评估摘要、文档片段、竞争力目标和实现护栏交给 LLM 或 coding-agent judge；它不替代确定性 gate。 |

多语言 repository retrieval targets 按 `cases/repository_*_targets.json` 拆分，每种语言可独立扩展。语言 case 覆盖真实 `symbol`、`definition`、`references`、`callers`、`callees`、`imports`、`hybrid` 场景，包括函数、方法、类、导出值、宏、include/import、callback/trait 关系和执行流。relationship targets 分为 regression 与 challenge，challenge case 通过 `expected_all` 或 `expected_sequence` 保留排序和覆盖率改进空间。全域高扇出关系查询必须验收所有等价正确结果共享的 edge kind、resolution state、target hint、retrieval layer 或 evidence surface；请求本身没有 path/importer context 时，不能强制某一个任意 importer path。携带上下文的 challenge 可以要求 importer、edge 与 evidence 属性出现在同一 hit；独立 scoped regression case 继续锁定直接过滤查询。Importer context 词项必须在剥离 imported target 与 local-binding identity 后仍然存在；只有 target FQN 的查询不能冒充 importer context。

### 真实仓库 targets

当规模或冷索引合同使执行顺序影响结果时，全量外部仓库显式配置 `isolated_index_home=true`。`relay_teams`、`opencode_typescript` 和下表 exhaustive 仓库按仓隔离并清理；LevelDB 是有界的 shared-order 回归。Temporal 与 OpenTelemetry 成员必须保持非隔离，因为 repository-set overlay 依赖共同 runtime home。隔离冷索引延迟和共享 preload/order 行为是两个独立信号，不能互相替代。

| 仓库 | profile | 目标 |
| --- | --- | --- |
| `/opt/workspace/relay-teams` | 默认 | Python 服务、connector、eval checkpoint、re-export 等查询。 |
| `/opt/workspace/opencode` | 默认 | TypeScript/TSX monorepo，覆盖 symbol、references、overload、exported const、TSX component、caller/callee、relative import、`@/` 和 `~/` alias、HTTP recorder redaction flow、LLM protocol streaming flow 和负例 symbol lookup。 |
| `/opt/workspace/leveldb` | 默认 | C/C++ 类方法、自由函数、头文件、table cache、recovery、callers、hybrid lookup 和 filters。 |
| `/opt/workspace/temporal-samples-go`、`/opt/workspace/temporal-sdk-go` | 默认 | Go 全仓索引和 Temporal sample 到 SDK 的 repository-set API 使用关系。 |
| `/opt/workspace/opentelemetry-collector-contrib`、`/opt/workspace/opentelemetry-collector` | 默认 | Go 全仓索引和 contrib 到 core 的 receiver factory、component type 使用关系。 |
| `/opt/workspace/angular`、`/opt/workspace/vue` | 默认 | 锁定官方 Angular layout 与 Vue SFC playground scope，通过 framework graph 覆盖 component、rendered selector、prop 与 template variable。 |
| `/opt/workspace/linux` | `exhaustive` | C 大仓 symbol、函数、syscall 风格宏、导出符号、include、references、callers、callees、mmap flow、epoll/eventfd；`linux_full` 重复测量完整初始索引时间。 |
| `/opt/workspace/kubernetes` | `exhaustive` | Go command constructor、kubelet flow、API types、clientset/generic client、authorizer、informer imports、callers、hybrid lookup 和 filters。 |
| `/opt/workspace/spring-framework` | `exhaustive` | Java context、bean factory、webmvc servlet/handler mapping、imports 和 filtered lookup。 |
| `/opt/workspace/rustfs` | `exhaustive` | Rust trait implementation、函数内 import、认证调用链和启动执行流。 |
| `/opt/workspace/codex` | `exhaustive` | Python 异常继承、relative import、retry 调用链和 app-server stdio 执行流。 |
| `/opt/workspace/nvm` | `exhaustive` | Bash 函数、命令引用、installer source hook 和 artifact download flow。 |
| `/opt/workspace/dotnet-runtime` | `exhaustive` | C# core library class、method、using directive 和 array-pool buffer flow。 |
| `/opt/workspace/okhttp` | `exhaustive` | Kotlin client class、method definition、Okio import 和 request dispatch flow。 |
| `/opt/workspace/laravel-framework` | `exhaustive` | PHP application class、constructor call、namespace use 和 service-provider bootstrapping。 |
| `/opt/workspace/rails` | `exhaustive` | Ruby controller class、singleton method、require target 和 module composition。 |
| `/opt/workspace/scala3` | `exhaustive` | Scala compiler context class、inline method、import 和 phase/mode flow。 |
| `/opt/workspace/alamofire` | `exhaustive` | Swift session class、request method、import 和 queue/delegate flow。 |

请从不存在的干净目标目录准备所有 fixed-ref 仓库，并使用 `cases.json`
记录的精确 commit。下面的 Bash 配方只 fetch 该 commit，以 detached HEAD
检出，并在 `HEAD` 与配置 SHA 不一致时失败：

```bash
set -eu

clone_pinned_repository() {
    repository_url=$1
    destination=$2
    commit=$3

    test ! -e "$destination"
    git init --quiet "$destination"
    git -C "$destination" remote add origin "$repository_url"
    git -C "$destination" fetch --quiet --depth 1 origin "$commit"
    git -C "$destination" checkout --quiet --detach "$commit"
    test "$(git -C "$destination" rev-parse HEAD)" = "$commit"
}

# 默认 profile 的多仓库 fixture。
clone_pinned_repository https://github.com/temporalio/samples-go.git /opt/workspace/temporal-samples-go 231564bebe0be78e78233ef14992158c623d1e86
clone_pinned_repository https://github.com/temporalio/sdk-go.git /opt/workspace/temporal-sdk-go ff47f19909ac85aacff89645360de0dba6f6f898
clone_pinned_repository https://github.com/open-telemetry/opentelemetry-collector-contrib.git /opt/workspace/opentelemetry-collector-contrib 84fe8df16c34efbb7e929310c955df8f4861d2f4
clone_pinned_repository https://github.com/open-telemetry/opentelemetry-collector.git /opt/workspace/opentelemetry-collector 31e51520f30fc5c4362949e41307ea57b7b45a9d
clone_pinned_repository https://github.com/angular/angular.git /opt/workspace/angular 133cafda42028fbd8efd7840d6ff3fea25223166
clone_pinned_repository https://github.com/vuejs/core.git /opt/workspace/vue d63616ca17de965ed32dcb449a4c5cd9982f15d2

# exhaustive profile 的 tree-sitter 语言真实仓库。
clone_pinned_repository https://github.com/nvm-sh/nvm.git /opt/workspace/nvm 53855417eb66b9c35b732ac39358f1aae3ee1977
clone_pinned_repository https://github.com/dotnet/runtime.git /opt/workspace/dotnet-runtime 86db03a9c145cefc46fbe9e0f0dc646f739c606c
clone_pinned_repository https://github.com/square/okhttp.git /opt/workspace/okhttp 1d9a8ba6c335355da9c71586abf82c9516e1bac5
clone_pinned_repository https://github.com/laravel/framework.git /opt/workspace/laravel-framework f05ef246c22eac49c7c7e9b2815449873ccd8a22
clone_pinned_repository https://github.com/rails/rails.git /opt/workspace/rails a78f8bcaac1d6f10a515aeccfb6553b895f126c3
clone_pinned_repository https://github.com/scala/scala3.git /opt/workspace/scala3 c101b01b41f8780122caffcc03e0f395edc8016e
clone_pinned_repository https://github.com/Alamofire/Alamofire.git /opt/workspace/alamofire 7595cbcf59809f9977c5f6378500de2ad73b7ddb
```

所有 repository target 都必须使用 `scope=all`，评估器会拒绝其他值。普通 full-scope 注册不会把 repository `path_filters` 或 `language_filters` 传给 `repo register`，默认 guardrail 会验证产品注册拒绝 `--language`；case 级 filter 继续用于验证查询端过滤能力。两个官方 framework target 使用独立 `registration_path_filters` 字段，只授权锁定的 Angular layout 与 Vue SFC playground 源码范围，同时在这些 scope 内保留全部索引阶段。缺失外部 dependency source 不是 parser、index、file、scope 或 response degradation，应暴露为 unresolved edge metadata，例如 `resolution_state` 和 `target_hint`，不能用 source/text fallback 掩盖授权范围、依赖覆盖或 parser 恢复问题。
