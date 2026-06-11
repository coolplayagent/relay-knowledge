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
| `--profile fast` | 默认本地迭代和 PR 前快速验证 | 跑格式、debug build、harness check、关键 product gate、默认仓库子集、repo-set 护栏和 semantic/vector guardrail。 |
| `--profile full` | 需要完整产品和 harness rail 时 | 恢复 release build、clippy、test、本地文件 fixture、完整仓库评估、semantic/vector fixture 和 research judge。 |
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
| `--model MODEL` | `gpt-5.5` | 候选生成使用的 Codex 模型。 |
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
codex -a never exec --dangerously-bypass-approvals-and-sandbox -s danger-full-access -C /opt/workspace/relay-knowledge -m gpt-5.5 -c 'model_reasoning_effort="xhigh"' -
```

只应在外部可信的工作区中使用。默认生成模型为 `gpt-5.5`，推理强度为 `model_reasoning_effort="xhigh"`；需要更低成本或不同生成模式时，用 `--model` 和 `--codex-reasoning-effort low|medium|high|xhigh` 覆盖。

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
| 基础质量门禁 | 产品与 harness 的 `fmt --check`、Linux GNU glibc 2.31 baseline 策略门禁、产品 debug build、harness `cargo check`。 |
| 产品 gate | `skill_metadata_policy_cases`、`code_index_recovery_cases`、`code_index_health_isolation_cases`、`code_index_sqlite_lock_cases`、CLI contract case。 |
| 默认仓库 | `index_performance_many_files`、`c_syntax_fixture`、`cpp_syntax_fixture`、`cross_language_syntax_fixture`、`typescript_syntax_fixture`、`nonstandard_layout_fixture`、`software_global_fixture`、`project_alias_fixture`、`relay_teams`、`leveldb_cpp`、`temporal_samples_go`、`temporal_sdk_go`。 |
| 默认取样 | 普通仓库默认取前 8 条 query case，并始终保留显式 `guardrail=true` case。 |
| repository-set | 默认保留 `temporal_go_workspace` 的 2 条跨仓门槛 case。 |
| semantic/vector | 默认运行 1 条 guardrail query。 |
| coding-agent 工作流 | `fast` 默认跳过；通过 `--categories agent_workflows` 或 PR benchmark workflow 运行。 |
| 复用缓存 | 复用 `.git/relay-knowledge-self-iteration/cache-v2/fast-evaluation-home/`，减少重复注册和索引成本。 |

`fast` 默认不跑产品 release build、全量 clippy、全量 test、本地文件 fixture 或 research judge。`full`/`exhaustive` 会恢复这些 rail，并运行完整仓库评估、repository-set case、本地文件 fixture、semantic/vector fixture 和 research judge。

关键 fast 护栏的责任边界：

| 护栏 | 保护点 |
| --- | --- |
| `skill_metadata_policy_cases` | 拒绝把 Windows 命令或资产示例放进 bash/POSIX code fence，保证 agent-facing 指令保持 shell-specific。 |
| CLI contract case | 验证 agent 可见 help 暴露 `repo index-worker`，并验证 idle worker 与 streaming worker 输出可解析 JSON。 |
| `code_index_recovery_cases` | 覆盖过期 task lease 恢复、旧 worker 完成拒绝、attempt budget dead-letter 和 checkpoint batch 续租。 |
| `code_index_health_isolation_cases` | 验证 no-language-filter 仓库更新时 health 查询有界，`repo query --freshness allow-stale` 能读取最新已提交 scope。 |
| `code_index_sqlite_lock_cases` | 保护重复进程 SQLite lock 避免、active-task 复用和不同 task fingerprint 的并发 claim。 |
| syntax 与 layout fixture | 保护 external import unresolved metadata、C/C++ 可恢复 parser error、非顶层 `src/` 布局、project alias 复用同一 indexed scope 和 source/text fallback 底线。 |
| `software_global_fixture` | 确保 `repo software` 投影事实来自已索引证据，不扫描包缓存、云 API、SDK 目录或未索引外部源码。 |
| `agent_workflow_fixture` | 用生成式 Rust、TypeScript、Python、YAML 和 Markdown 证据重放 coding-agent issue 分析任务，并约束工具调用、源码读取、输出/context 大小、证据数量、fallback 比例和总延迟。 |

若要调整默认子集，可设置：

```bash
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=index_performance_many_files,c_syntax_fixture,cpp_syntax_fixture,cross_language_syntax_fixture,typescript_syntax_fixture,nonstandard_layout_fixture,software_global_fixture,project_alias_fixture,relay_teams,leveldb_cpp,temporal_samples_go,temporal_sdk_go
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT=12
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS=temporal_go_workspace
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT=2
```

`full` 和 `exhaustive` 额外运行 `index_performance_wide_mixed_files`，它生成 2048 个 Rust 目标文件与跨 shard bridge 查询，并记录 cold `*_index_ms`、`*_register_index_ms`、query p50/p95/max 指标，用更宽的 workload 提高性能门槛。

### coding-agent 工作流门禁

`--categories agent_workflows` 会运行 `cases/agent_workflow_targets.json` 中的确定性端到端 coding-agent 场景。fixture 覆盖定义定位、跨语言影响追踪、配置到文档追踪和 freshness policy 检查。每个场景执行有界 `repo query` 步骤；当期望证据缺失、context/output 超过预算、需要读取的唯一源码文件过多、text fallback 在证据包中过高，或总查询延迟超过阈值时失败。

PR benchmark workflow 会以 `agent-workflow-regression` job 运行该 category，并通过 `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=agent_workflow_fixture` 将运行范围限制到生成式 fixture。evaluation 结束后，workflow 会检查生成的 JSON report，只要任一 gate、case 或 agent workflow metric budget 失败就让 CI 失败；该 CI 门禁不使用 score-vs-history 的采纳决策。这样能控制 CI 成本，同时覆盖 agent-facing 行为。

### category 聚焦

`--categories` 会执行显式 guardrail case 加上所选类别 case；guardrail case 失败会转成 quality gate 失败，即使聚焦分数提升也会拒绝候选。`--categories semantic_vector` 会运行完整 semantic/vector suite，并保留 repository 与 repo-set 底线 case；`--categories performance` 会保留产生性能指标的 repository、repo-set、semantic/vector 和 file-fixture workload，而不是只剩 guardrail。评分历史按 profile 和 category focus 隔离，采纳时还会检查同 profile 下跨 category 的最佳已提交 run，避免新 category 首次运行因为同类 baseline 为空而接纳低于 profile 水位的候选。

### 并发边界

并发默认使用 `--jobs auto`、`--repo-jobs auto` 和 `--query-jobs auto`。`auto` 会让全局 command limiter 和 query pool 使用可用 CPU 数，repository jobs 使用可用 CPU 数的一半。仓库 register/index 以及 repository-set create/add/refresh 这类共享评估库写命令仍会串行化，写边界之后的查询子进程可并发运行。

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

`performance` 使用 `budget_relative_v2`。没有兼容上一轮记录时，指标使用按预算归一化的分数；一旦上一轮也使用该策略，每个指标会混合预算适配度和相对上一轮的进步幅度。因此耗时只是在预算内不会长期保持 `1.0`，真实延迟优化仍会持续产生有界评分信号。

### 采纳策略

采纳策略是“带硬约束和加权分数决胜的 epsilon-Pareto 采纳策略”。build/test gate 和候选 diff 存在性是硬约束；foundational_capability、competitive_capability、semantic_vector、stability 和延迟观测是受保护目标；epsilon 阈值用于抑制测量噪声；加权分数是决胜项而不是唯一决策规则。

候选在以下条件满足时被采纳：

```text
hard_constraints_pass
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

`bug_fix_priority_improved` 表示候选修复了已观测到的程序失败：上一轮失败的 quality gate 变为通过，或上一轮失败的 evaluation case 变为通过。它可以越过加权分数决胜项、profile 级最佳已提交分数线和原始耗时退化，但不能越过缺少 diff、当前 gate 失败或受保护目标回退。

默认 epsilon：

| 阈值 | 默认值 | 用途 |
| --- | --- | --- |
| `score_epsilon` | `0.0005` | 总分比较。 |
| `ratio_epsilon` | `0.005` | foundational、competitive、semantic_vector、performance、stability 等分数组件。 |
| `metric_epsilon` | `max(25ms, previous_metric * 0.03)` | 原始耗时指标。 |

退化会被记录为下一轮 Codex prompt 的 degradation feedback；正向改善也会传给下一轮，让后续迭代知道哪些成果需要保持。被采纳的优化方案会进入 run history 的 `optimization_plan` 字段，并在下一轮 prompt 的 `Recent adopted optimization plans to build on` 段落中作为设计参考。

## 评估数据

`cases.json` 及其 `include_files` 定义自迭代目标 workload。它不是“当前已经全部实现”的能力清单；新增 case 可以代表下一轮候选需要补齐的竞争力目标。候选应改进通用 parser、图边、候选收缩、排序、service workflow 或可观测性，不能通过删除、放宽或枚举 case 获得分数。

### 生成式和本地 fixture

| 分组 | 覆盖 |
| --- | --- |
| 本地文件索引 fixture | 临时生成 user documents、Linux `/opt` 风格路径、Windows `D:` 风格路径、深层目录和高噪声文件集合，运行 `files index/query`，记录 `file_index_ms`、`file_query_p50_ms`、`file_query_p95_ms`。 |
| C/C++ 语法 fixture | 生成临时 git 仓库并走 `repo register/index/query`，覆盖 function pointer typedef、operation table、initializer、macro、本地 include、callback dispatch、namespace、template、override、operator、lambda、alias 和 header/source split。设计说明见 `docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md`。 |
| 跨语言语法 fixture | 覆盖 C 调 C++、C++ 调 C、Go cgo 调 C、Rust FFI 调 C，让默认 fast 不依赖额外大仓也能验证多语言调用图。 |
| 额外多语言 fixture | 覆盖 Python、JavaScript、TypeScript/TSX、Go、Java、Rust、Bash、C#、Kotlin、PHP、Ruby、Scala 和 Swift；矩阵见 `docs/zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md`。 |
| repository-set targets | 注册每个成员为 `scope=all` 仓库，创建显式 `repo-set`，刷新跨仓 overlay，再运行 `repo-set query`；case 可要求具体 member、source_scope、路径、行号和 excerpt 证据。 |
| register-to-index 性能 targets | `repository_index_performance_targets.json` 收紧 `index_budget_ms` 并新增 `register_index_budget_ms`；默认 fast 包含 1024 文件 fixture，`full`/`exhaustive` 还包含 2048 文件 wide fixture。 |
| 软件全域投影 targets | `repository_software_global_targets.json` 运行 `repo software`，覆盖 dependencies、sdks、files、topics、relationships、build、iac、design 和 all 投影 kind，且事实只能来自已索引证据。 |
| CLI contract cases | 直接运行产品 CLI，不需要大仓；默认 fast 覆盖 `repo index-worker` help、idle JSON 和 streaming JSON。 |
| semantic/vector suite | 写入小型 evidence，刷新 semantic/vector 索引，验证 query 命中 `retriever_sources`、`backend_statuses` 和相关排序；外部 provider 只从运行时环境继承。 |
| research_judge_suite | 把候选 diff、确定性评估摘要、文档片段、竞争力目标和实现护栏交给 LLM 或 coding-agent judge；它不替代确定性 gate。 |

多语言 repository retrieval targets 按 `cases/repository_*_targets.json` 拆分，每种语言可独立扩展。语言 case 覆盖真实 `symbol`、`definition`、`references`、`callers`、`callees`、`imports`、`hybrid` 场景，包括函数、方法、类、导出值、宏、include/import、callback/trait 关系和执行流。relationship targets 分为 regression 与 challenge，challenge case 通过 `expected_all` 或 `expected_sequence` 保留排序和覆盖率改进空间。

### 真实仓库 targets

| 仓库 | profile | 目标 |
| --- | --- | --- |
| `/opt/workspace/relay-teams` | 默认 | Python 服务、connector、eval checkpoint、re-export 等查询。 |
| `/opt/workspace/opencode` | 默认 | TypeScript/TSX monorepo，覆盖 symbol、references、overload、exported const、TSX component、caller/callee、relative import、`@/` 和 `~/` alias、HTTP recorder redaction flow、LLM protocol streaming flow 和负例 symbol lookup。 |
| `/opt/workspace/leveldb` | 默认 | C/C++ 类方法、自由函数、头文件、table cache、recovery、callers、hybrid lookup 和 filters。 |
| `/opt/workspace/temporal-samples-go`、`/opt/workspace/temporal-sdk-go` | 默认 | Go 全仓索引和 Temporal sample 到 SDK 的 repository-set API 使用关系。 |
| `/opt/workspace/opentelemetry-collector-contrib`、`/opt/workspace/opentelemetry-collector` | 默认 | Go 全仓索引和 contrib 到 core 的 receiver factory、component type 使用关系。 |
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

准备默认 profile 的多仓库 fixture：

```bash
git clone --depth 1 https://github.com/temporalio/samples-go.git /opt/workspace/temporal-samples-go
git clone --depth 1 https://github.com/temporalio/sdk-go.git /opt/workspace/temporal-sdk-go
git clone --depth 1 https://github.com/open-telemetry/opentelemetry-collector-contrib.git /opt/workspace/opentelemetry-collector-contrib
git clone --depth 1 https://github.com/open-telemetry/opentelemetry-collector.git /opt/workspace/opentelemetry-collector
```

准备新增 tree-sitter 语言真实仓库：

```bash
git clone --depth 1 https://github.com/nvm-sh/nvm.git /opt/workspace/nvm
git clone --depth 1 https://github.com/dotnet/runtime.git /opt/workspace/dotnet-runtime
git clone --depth 1 https://github.com/square/okhttp.git /opt/workspace/okhttp
git clone --depth 1 https://github.com/laravel/framework.git /opt/workspace/laravel-framework
git clone --depth 1 https://github.com/rails/rails.git /opt/workspace/rails
git clone --depth 1 https://github.com/scala/scala3.git /opt/workspace/scala3
git clone --depth 1 https://github.com/Alamofire/Alamofire.git /opt/workspace/alamofire
```

所有 repository target 都必须使用 `scope=all`。评估器会拒绝非全量 scope，full-scope 注册不会向 `repo register` 传递 path 或 language filter，并且默认 guardrail 会验证产品注册拒绝 `--language`；case 级 filter 只用于验证查询端过滤能力。缺失外部 dependency source 不是 parser、index、file、scope 或 response degradation，应暴露为 unresolved edge metadata，例如 `resolution_state` 和 `target_hint`，不能用 source/text fallback 掩盖授权范围、依赖覆盖或 parser 恢复问题。
