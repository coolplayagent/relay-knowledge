# relay-knowledge 自迭代

中文 | [English](README.md)

本目录包含一个独立的 Codex 驱动优化循环，用于改进代码仓库检索质量，以及图谱 semantic/vector 检索质量。它有意作为 `tools/self_iteration` 下的独立 Rust harness 演进，不放入产品 crate 的 `src/` 模块树；所有运行状态都存放在 `.git/relay-knowledge-self-iteration/` 下。旧的 tracked Python harness 已在功能对齐后清理，`self-iterate.sh` 会直接构建并运行 Rust binary。

## 启动

在仓库根目录运行：

```bash
./self-iterate.sh
```

启动脚本默认等价于：

```bash
cargo build --manifest-path tools/self_iteration/Cargo.toml --bin relay-knowledge-self-iterate
tools/self_iteration/target/debug/relay-knowledge-self-iterate loop --workspace . --yolo --profile fast
```

`self-iterate.sh` 是稳定入口。它默认构建 debug harness，避免每次本地自迭代先做 release build；需要 release harness 时设置 `RELAY_KNOWLEDGE_SELF_ITERATION_RELEASE=1`。调用者不需要手动进入该目录或安装 binary 到 `PATH`。

常用变体：

```bash
./self-iterate.sh once
./self-iterate.sh --max-iterations 3
./self-iterate.sh chart
./self-iterate.sh once --profile full
./self-iterate.sh once --profile fast --categories semantic_vector
./self-iterate.sh once --profile fast --categories semantic_vector,competitive
./self-iterate.sh once --profile smoke --dry-run-codex
./self-iterate.sh loop --strategy unattended-layered
./self-iterate.sh loop --strategy unattended-layered --max-wall-clock-hours 48 --stop-after-accepted 12
```

## 进度日志

harness 会把实时进度写到 stderr，统一使用 `[self-iterate]` 前缀。每个子进程都会输出
`command start`、每 15 秒一次的 `command running` 心跳，以及带退出码和耗时的
`command done` 或 `command timeout`。评估阶段还会输出当前 profile、evaluation
home、解析后的并发度、质量门禁 stage、仓库 workload 规模、repository-set workload
规模，以及最终 gate/case/command 计数。产品命令的 stdout/stderr 仍会被捕获进 JSON
报告，但长时间运行的 `fast` profile 不再处于无输出状态。

## YOLO 模式

本地 Codex CLI 没有字面意义上的 `--yolo` 参数。本框架会把 `--yolo` 映射到当前非交互、高权限 Codex 调用：

```bash
codex -a never exec --dangerously-bypass-approvals-and-sandbox -s danger-full-access -C /opt/workspace/relay-knowledge -m gpt-5.5 -c 'model_reasoning_effort="xhigh"' -
```

只应在外部可信的工作区中使用。该循环按无人值守运行设计。

自迭代生成阶段默认使用 `gpt-5.5`，并为 Codex 设置
`model_reasoning_effort="xhigh"`，让无人值守候选默认走最高本地推理档位。需要更低成本或不同生成模式时，可用 `--model` 覆盖模型，用
`--codex-reasoning-effort low|medium|high|xhigh` 覆盖推理强度。

## 循环行为

每一轮迭代会：

1. 检查工作树是否干净，除非传入 `--use-current-candidate`。
2. 提示本地 Codex 做一个聚焦的代码检索改进。
3. 将候选补丁保存到 `.git/relay-knowledge-self-iteration/patches-v2/`。
4. 按 profile 运行质量门禁和评估。默认 `fast` 只跑格式检查、Linux glibc 兼容性策略门禁、产品 debug build、harness `cargo check`、扩展后的普通仓库子集、repository-set 护栏和一条 semantic/vector guardrail query；`full`/`exhaustive` 会恢复产品和独立 harness 的 release build、产品 `clippy -> test` 与 harness `clippy -> test` 两条 rail，并运行完整仓库评估、repository-set case、本地文件 fixture、semantic/vector fixture 和 research judge。
5. 将报告写入 `.git/relay-knowledge-self-iteration/reports-v2/`。
6. 将评分历史追加到 `.git/relay-knowledge-self-iteration/runs-v2.jsonl`。
7. 将 v2 图表写入 `.git/relay-knowledge-self-iteration/score-v2.csv` 和 `.git/relay-knowledge-self-iteration/score-v2.svg`；`accepted` 只表示已经创建 git commit。
8. 采纳候选前，将本轮采用的优化思路、变更文件、指标改善和已知退化追加到 `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`。
9. 只有当上一轮改进采纳策略接受候选时，才把候选净改动和采纳记录 squash 成一个 commit。
10. 候选被拒绝时，恢复到本轮开始的 commit。

如果启动时工作树是 dirty 状态，循环会立即退出，而不是重复重试同一个不可重试的前置条件失败。

实现类候选必须在评估前更新
`docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`，写清算法、架构、不变量、预期 case/metric 影响和已知风险。harness 会追加
`self_iteration_algorithm_documentation` gate，拒绝没有携带这些说明的代码、测试、benchmark 或 harness 策略变更。prompt 会把 v2 run history、历史综合摘要和 patch 路径作为有界上下文，避免一次性读取全部历史产物。

v2 harness 将 `runs-v2.jsonl`、`reports-v2/` 和 `patches-v2/` 与早期 run/report/patch 格式隔离；既有工作树中的旧文件可保留为历史资料。渐进式长期记忆保留在共享的 `.git/relay-knowledge-self-iteration/memory/` 树下：每次评分都会写入 `memory/index.jsonl`、`memory/summaries/` 和 `memory/details/`，下一轮生成 prompt 会收到拒绝恢复记忆、受限记忆索引、按 profile 汇总的历史综合摘要和受限历史 patch 索引。被拒记忆会记录变更路径、score delta、局部改善、退化和连续拒绝簇，帮助 Codex 避免重复尝试已经输给采纳基线的小改动。Codex 只应在条目匹配当前 gate、metric、case、path 或算法目标时打开对应 summary、detail 或 patch 文件。直接注入 prompt 的历史综合摘要带硬字符上限，长期迭代不会随历史长度线性填满 LLM 上下文。生成 prompt 还会要求 Codex 做仓库检查时优先使用 `rg`，如果本机未安装 `rg`，则改用排除 VCS 和 build 目录的有界 `grep -RIn` 搜索，不能因为缺少 ripgrep 就停止源码检索。

默认 profile 是 `fast`。它只跑产品与 harness 的 `fmt --check`，检查 release workflow 仍强制 Linux GNU glibc 2.31 baseline，再跑产品 debug build、harness `cargo check` 和 targeted `code_index_recovery_cases`、`code_index_sqlite_lock_cases` gate，然后用 `target/debug/relay-knowledge` 执行评估；默认不跑产品 release build、全量 clippy、全量 test、本地文件 fixture 或 research judge。`code_index_recovery_cases` 覆盖 code-index 过期 task lease 恢复、旧 worker 完成拒绝、attempt budget dead-letter 和 checkpoint batch 续租，不需要索引 exhaustive 大仓。SQLite lock gate 会用独立 file-backed store 打开同一个数据库，验证重复 full-index start 复用 running task，同仓 follow-up task 会等待 active lease。`fast` 还会运行注册 guardrail，验证 `repo register --language` 被拒绝，避免 C/C++ 混合仓库在注册期被错误收窄。`fast` 评估 `c_syntax_fixture`、`cpp_syntax_fixture`、`cross_language_syntax_fixture`、`typescript_syntax_fixture`、`nonstandard_layout_fixture`、`project_alias_fixture`、`relay_teams`、`leveldb_cpp`、`temporal_samples_go` 和 `temporal_sdk_go`，每个普通仓库默认取前 8 条 query case，并始终保留显式 `guardrail=true` 底线 case；project alias fixture 会先无 `--alias` 注册生成仓库，再给同一个 root 增加 session 风格显式 alias，并验证两个 alias 都复用同一 indexed scope；TypeScript、C 和 C++ syntax fixture 会把 external import source fallback 底线 case 纳入默认 fast 循环，并要求外部依赖源码缺失保持 unresolved edge metadata 而不是 `degraded_reason`；C/C++ fixture 还覆盖 macro-generated handler 和 export-macro-decorated class，避免可恢复 parser error 迫使查询退到 source fallback；nonstandard layout fixture 会把 Python、TypeScript、Go、Java、C++ 和 Swift 中不在顶层 `src/` 下的源码纳入 fast 底线，并用 Cargo、npm、Go、Python、Maven BOM、Gradle 和 Conan manifest/lockfile case 守护 `repo query --kind sbom` 依赖清单能力；C fixture 在 fast case window 前段包含显式 source/text-fallback case，因此 fast 运行不需要额外索引大仓也能覆盖精确源码文本兜底，并且不依赖外部 `rg` 二进制；repo-set 默认保留 `temporal_go_workspace` 的 2 条跨仓门槛 case；semantic/vector 默认运行 1 条 guardrail query。它复用 `.git/relay-knowledge-self-iteration/cache-v2/fast-evaluation-home/` 作为评估 home，减少重复注册和索引成本。评分历史按 profile 和 category focus 隔离，`fast --categories semantic_vector` 只和同类 fast 历史比较，不会把 full/exhaustive 的 judge 分数或默认 fast workload 当作回归基线；采纳时还会检查同 profile 下跨 category 的最佳已提交 run，避免新 category 首次运行因为同类 baseline 为空而接纳低于 profile 水位的候选。可用 `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=c_syntax_fixture,cpp_syntax_fixture,cross_language_syntax_fixture,typescript_syntax_fixture,nonstandard_layout_fixture,project_alias_fixture,relay_teams,leveldb_cpp,temporal_samples_go,temporal_sdk_go`、`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT=12`、`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS=temporal_go_workspace` 和 `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT=2` 调整默认子集。需要完整旧门禁和完整 workload 时显式传 `--profile full`；长周期大仓扩展仍使用 `--profile exhaustive`。

可用 `--categories` 聚焦某个分数族，但不会移除底线保护。支持的类别包括 `foundational`、`competitive`、`semantic_vector`、`file_fixtures`、`repository_sets`、`research_judge`、`performance` 和 `all`。聚焦运行会执行显式 `guardrail=true` case 加上所选类别的 case；guardrail case 失败会转成 quality gate 失败，即使聚焦分数提升也会拒绝候选。例如 `--categories semantic_vector` 会运行完整 semantic/vector suite，并同时保留 repository 与 repo-set 底线 case；`--categories competitive` 会运行 competitive repository cases 和相同底线 case；`--categories performance` 会保留 repository、repo-set、semantic/vector 和 file-fixture 中会产生性能指标的 workload，而不是只剩 guardrail。可用 `--exclude-categories` 在 `all` 展开后减去指定类别，例如 `--categories all --exclude-categories research_judge` 会保留完整聚焦 workload，但不运行 research judge suite。

并发默认使用 `--jobs auto`、`--repo-jobs auto` 和 `--query-jobs auto`。`auto` 会更积极地使用本机：全局 command limiter 和 query pool 默认等于可用 CPU 数，repository jobs 默认等于可用 CPU 数的一半。仓库 register/index 以及 repository-set create/add/refresh 这类共享评估库写命令仍会串行化，写边界之后的查询子进程可并发运行。可用 `--jobs N` 或 `RELAY_KNOWLEDGE_SELF_ITERATION_JOBS=N` 覆盖全局并发。

## 无人值守分层策略

`--strategy unattended-layered` 是面向 1-2 天无人值守运行的长周期策略。未显式传入该策略时，现有单轮 loop/once 行为保持不变。

默认参数按 36 小时窗口设置：`--max-wall-clock-hours 36`、`--stop-after-accepted 8`、`--explore-timeout-seconds 900`、`--macro-explore-timeout-seconds 2700`、`--max-explore-attempts-per-cycle 3`、`--max-consecutive-empty-candidates 8`、`--max-consecutive-promotion-failures 10`、`--macro-after-competitive-failures 4`、`--macro-after-empty-candidates 6`、`--cycle-sleep-seconds 120`、`--cooldown-after-accept-seconds 300` 和 `--cooldown-after-timeout-seconds 900`。

每个 cycle 先用 `smoke` profile 做短探索，按 `competitive -> semantic_vector -> performance -> repository_sets` 轮转 category。Codex 只在 explore 层运行；候选通过 smoke screen 后，复用同一个 patch 进入同 category 的 `fast` validate，只有 validate 通过才进入既有 accept/commit 路径。这样不会为了同一个候选重复调用 Codex。

当短探索持续没有产出时，策略会升级到竞争力能力的 `macro_explore`。触发条件包括 repeated competitive promotion failure、连续 empty candidate，或当前 competitive capability 相对 best accepted focused baseline 出现超过阈值的差距。macro prompt 会注入 `cases.json` 中 `research_judge_suite.competitive_feature_targets` 和 `implementation_guardrails`，要求 Codex 做 ranking、indexing、relationship extraction、query planning、context construction 或 retrieval evidence 这类较大的泛化改进，同时继续禁止 fixture/query/path/symbol 特化枚举。

运行状态写入 `.git/relay-knowledge-self-iteration/unattended-state-v2.json`，用于崩溃后恢复 category rotation、失败计数、accepted 计数和 deep-check 调度。分层 run history 会额外记录 `strategy`、`layer`、`parent_run_id`、`promoted_from_run_id`、`macro_trigger`、`promotion_decision` 和 wall-clock 字段。传入 `--use-current-candidate` 时会跳过 Codex，直接从当前 diff 的 smoke screen 和 fast validate 开始。

## 评分和采纳

research judge 被禁用或跳过时，加权分数为：

```text
foundational_capability * 0.22
+ competitive_capability * 0.22
+ semantic_vector * 0.13
+ performance * 0.18
+ stability * 0.25
```

启用 research judge 后，`research_judge` 成为受保护目标，分数权重切换为：

```text
foundational_capability * 0.17
+ competitive_capability * 0.17
+ semantic_vector * 0.10
+ research_judge * 0.22
+ performance * 0.15
+ stability * 0.19
```

这个策略有意提高 research 质量和性能对采纳分数的影响，同时继续通过回退检查保护其他目标。

research judge 用于判断研究对齐、架构合理性、可靠性推理、性能泛化、实现可操作性和是否存在 fixture 特化。它可以通过 OpenAI-compatible HTTP endpoint 运行，也可以通过开放 coding agent CLI 运行，例如 `opencode`、`relay-teams`、`codex`、`cc` 或 `copilot`。未提供 judge backend 或 HTTP 配置时，CLI judge 默认使用 `opencode`。所有 judge 覆盖配置都来自运行时环境变量：

`cases.json` 也可以配置 judge workload。`documents` 选择有界的 02/03/04 文档片段，`competitive_feature_targets` 列出候选补丁应推进的研究竞争力能力，`implementation_guardrails` 列出反 fixture 特化、async 边界、freshness/version 证据和同变更文档更新等不可放松约束。

- `RELAY_KNOWLEDGE_JUDGE_BACKEND=http|cli|opencode|none`；`opencode` 是 CLI alias，未设置自定义命令时使用默认 opencode 命令
- HTTP: `RELAY_KNOWLEDGE_JUDGE_BASE_URL`、`RELAY_KNOWLEDGE_JUDGE_API_KEY`、`RELAY_KNOWLEDGE_JUDGE_MODEL`；独立 harness 用 `curl` 发起请求，API key 只从环境变量读取，不写入报告
- CLI: `RELAY_KNOWLEDGE_JUDGE_COMMAND`，也支持别名 `RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND` 和 `RELAY_KNOWLEDGE_JUDGE_CLI_COMMAND`；未设置时默认命令为 `opencode run "Read the attached relay-knowledge judge prompt and return only the strict JSON object it requests." --file {prompt_file}`
- 通用 timeout: `RELAY_KNOWLEDGE_JUDGE_TIMEOUT_SECONDS`

自定义 CLI command 默认通过 stdin 接收 judge prompt；命令模板也可以使用 `{workspace}`、`{prompt_file}` 或 `{prompt}` 占位符。harness 要求 HTTP 或 CLI judge 返回严格 JSON。设置 `RELAY_KNOWLEDGE_JUDGE_BACKEND=none` 时仍选择 suite 但记录 `judge_skipped`；`off`、`disabled`、`skip` 和 `false` 也作为禁用别名。需要完全不运行 suite 时使用 `--exclude-categories research_judge`。显式配置但缺少必需环境变量、返回非法 JSON、低置信度、低总分或低 anti-fixture-special-casing 分数时会拒绝候选。

case objective 是连续质量分，不是通过率计数。case 在 rank 1 通过时从
`1.0` 起算；rank `N > 1` 即使仍在该 case 的 `max_rank` 采纳阈值内，也只从
`1.0 / N` 起算。case 还可以声明 `expected_all`、`expected_sequence`、
`min_score`、`require_expected_all`、`require_expected_sequence`、
`forbidden_rank_penalty` 和 `forbidden_rank_penalty_only`。这些字段允许 case
在通过的同时因为只覆盖部分关系集合、缺少执行流步骤或把禁止命中排得过高而低于满分。空结果负例以 `rank=0` 通过时仍得 `1.0`。缺失的 foundational、competitive 或 semantic/vector objective 默认 `0.0`，不会因为没有 case 而显示为满分；`accuracy` 只汇总实际存在的 foundational 与 competitive objective。超出预算的耗时指标会进入 `metric_budget_failures` 诊断字段。

`performance` 使用 `budget_relative_v2`。没有兼容上一轮记录时，指标仍使用按预算归一化的分数；一旦上一轮也使用该策略，每个指标会混合预算适配度和相对上一轮的进步幅度。因此耗时只是在预算内不会长期保持 `1.0`，真实延迟优化仍会持续产生有界评分信号，普通测量噪声继续由 epsilon 策略过滤。

`accuracy` 保留为 foundational 与 competitive case 分数的兼容汇总。采纳策略使用 `带硬约束和加权分数决胜的 epsilon-Pareto 采纳策略`。从多目标优化角度看，build/test gate 和候选 diff 存在性是硬约束，foundational_capability、competitive_capability、semantic_vector 和 stability 是保证基础可用性、高阶检索质量、semantic/vector 来源覆盖和后端可用性的受保护目标，延迟观测也是目标，epsilon 阈值用于抑制测量噪声，加权分数是决胜项而不是唯一决策规则。

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

`bug_fix_priority_improved(candidate, previous)` 表示候选修复了已观测到的程序失败：上一轮失败的 quality gate 变为通过，或上一轮失败的 evaluation case 变为通过。这个优先级可以越过加权分数决胜项、profile 级最佳已提交分数线和原始耗时退化，但不能越过缺少 diff、当前 quality gate 失败或受保护目标回退。profile 级最佳已提交分数线只使用同 profile 的已提交 run，不按 category focus 隔离，用于防止新聚焦类别在同类 baseline 为空时接纳低分候选。

`epsilon_pareto_improved(candidate, previous)` 表示：至少一个被跟踪目标的改善超过其 epsilon 阈值，并且没有任何被跟踪目标的退化超过其 epsilon 阈值。默认阈值为：

- `score_epsilon = 0.0005`
- `ratio_epsilon = 0.005`，用于 foundational_capability、competitive_capability、semantic_vector、performance、stability 等分数组件
- `metric_epsilon = max(25ms, previous_metric * 0.03)`，用于原始耗时指标

这可以避免真实 case/rank 改善因为某个耗时指标在正常噪声范围内波动而被拒绝，也能避免只靠噪声获胜、同时悄悄回退受保护目标的候选被采纳。当局部 metric 改善仍未超过当前 profile 的 latest baseline 时，拒绝原因会额外记录该诊断和 score delta。foundational、competitive、semantic_vector、research_judge、performance、case、gate 和 metric 的退化会被记录为下一轮 Codex prompt 的 degradation feedback。正向的 score、research_judge、performance、case、gate 和 metric 改善也会被记录并传给下一轮 Codex prompt，方便后续迭代知道哪些成果需要保持。被采纳的优化方案还会进入 run history 的 `optimization_plan` 字段，并在下一轮 prompt 的 `Recent adopted optimization plans to build on` 段落中作为设计参考。

`chart` 命令会写入：

- `.git/relay-knowledge-self-iteration/score-v2.csv`
- `.git/relay-knowledge-self-iteration/score-v2.svg`

CSV 是 scored-run history，不是 patch 目录清单；它包含 run mode、patch path、`score_accepted` 和 `committed` 字段，方便区分手动评估和 loop 迭代。手动 `evaluate` run 会使用唯一的 `manual-evaluate-*` patch/report 名称，可以记录 `score_accepted=true`，但除非创建了 git commit，否则绝不会是 `accepted=true`。SVG 中绿色点表示已提交的 accepted run，琥珀色点表示评分会通过的手动评估，红色点表示 rejected run。

## 评估数据

`cases.json` 及其 `include_files` 定义自迭代目标工作负载。它不是“当前已经全部实现”的能力清单；新增 case 可以代表下一轮候选需要补齐的竞争力目标。候选应改进通用 parser、图边、候选收缩、排序、service workflow 或可观测性，不能通过删除、放宽或枚举 case 获得分数。

- 本地文件索引 fixture：在临时目录中生成 `user documents`、Linux `/opt`
  风格路径、Windows `D:` 风格路径、深层目录和高噪声文件集合，运行
  `relay-knowledge files index/query`，记录 `file_index_ms`、
  `file_query_p50_ms` 和 `file_query_p95_ms`。文件 case 可声明
  `objective`、`max_results`、`truncated`、`degraded_reason` 和更细粒度的命中字段，用来表达路径/内容分离、scope 前置、候选收缩、后台索引和诊断目标。每条文件查询都使用 subprocess timeout，防止候选实现卡死 evaluator。
- 多语言代码仓库检索 targets：语言 case 已按
  `cases/repository_*_targets.json` 拆分，让每种语言能独立扩展。默认
  profile 覆盖生成式 C/C++ 语法 fixture、生成式 C/C++/Go/Rust 跨语言调用
  fixture、relay-teams Python/JavaScript、opencode TypeScript/TSX 和 LevelDB C++；Linux C、Kubernetes Go、Spring Framework
  Java、RustFS Rust、Codex Python、nvm Bash、dotnet/runtime C#、OkHttp Kotlin、
  Laravel PHP、Rails Ruby、Scala 3 和 Alamofire Swift 继续由 repository 级
  `profile=exhaustive` 控制。语言
  case 覆盖真实 `symbol`、`definition`、`references`、`callers`、
  `callees`、`imports`、`hybrid` 场景，包括函数、方法、类、导出值、宏、
  include/import、callback/trait 关系和执行流。import case 可以要求
  external-dependency source fallback 诊断：当 import target 因依赖库未纳入代码图谱而保持
  unresolved 时，产品只在当前已索引仓库源码内搜索，并返回 `text_fallback` 证据供 LLM 推理。
  fast C fixture case 还覆盖 header、实现文件和 generated-table 源文件里的注释-only
  reference 与 hybrid 精确文本兜底，确保 source/text-fallback 能在默认 profile 中持续被验证。
  relationship targets 仍拆成
  regression 与 challenge 两组，并通过 extended relationship 文件为 Rust、
  Go、C、C++、Java、Python、JavaScript、TypeScript 显式补充实现、别名和
  inline callback/closure 场景。regression cases 保留 path filter 和较宽 rank
  阈值，作为稳定回归护栏；challenge cases 去掉 path filter、降低 limit 与
  max rank，并用 `expected_all` 或 `expected_sequence` 让继承、实现、依赖、
  别名、内联、调用链和执行流 case 即使通过也继续保留排序和覆盖率改进空间。
- C/C++ 语法 fixture 会在 evaluation home 下生成临时 git 仓库，再走普通
  `repo register/index/query` 路径。C fixture 覆盖 function pointer typedef、
  operation table、designated/compound initializer、function-like macro、本地
  include 和 callback dispatch；C++ fixture 覆盖 namespace、template class、
  out-of-line template method、virtual override、operator、lambda capture、
  namespace alias、using alias 和 header/source split。设计说明与外部仓库固定
  commit 见 `docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md`。
- 跨语言语法 fixture 同样在本地生成，并保留在默认 fast profile 中。它覆盖
  C 调 C++、C++ 调 C、Go cgo 调 C、Rust FFI 调 C 的 caller/callee 查询，让默认
  自迭代无需额外 clone 大仓也能持续验证多语言调用图检索。
- 额外生成式语法 fixture 覆盖 Python、JavaScript、TypeScript/TSX、Go、Java、
  Rust、Bash、C#、Kotlin、PHP、Ruby、Scala 和 Swift。它们用于紧凑、可复现地
  覆盖语言特有语法，真实固定仓库继续承担规模、噪声和性能压力；fixture 矩阵见
  `docs/zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md`。
- 多仓库 repository-set targets 位于
  `cases/repository_multi_repository_targets.json`。评估器会先把每个成员作为普通
  `scope=all` 仓库注册和索引，再创建显式 `repo-set`、刷新跨仓 overlay，并运行
  `repo-set query`。打分前会把 `results[*].member` 与 `results[*].hit`
  展平，让 case 能要求具体的 `repository_alias`、`source_scope`、路径、行号和
  excerpt 证据，而不会把 repository-set 命中伪装成单仓事实。默认 profile 覆盖
  Temporal `samples-go` 到 `sdk-go`，以及 OpenTelemetry
  `opentelemetry-collector-contrib` 到 `opentelemetry-collector` 的真实跨仓引用。
- 仓库注册后索引性能 targets：`cases/repository_index_performance_targets.json` 收紧 `index_budget_ms`，并新增 `register_index_budget_ms` 组合预算。评估器会同时记录 `*_index_ms` 与 `*_register_index_ms`，让自迭代优先优化 `repo register` 后 cold index 的批处理、解析吞吐、SQLite 写入、finalize 和增量复用路径。
- 内置 `semantic_vector_suite`：在自迭代专用 source scope 中写入小型 evidence，刷新 semantic/vector 索引，并验证 query 命中的 `retriever_sources` 覆盖 semantic/vector、`backend_statuses` 可用以及相关内容排序。启用 `RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external` 或 `RELAY_KNOWLEDGE_VECTOR_BACKEND=external` 时，评估器会直接继承运行时环境变量并先执行 `provider probe`；不在 cases 或命令行中保存 provider URL、API key、模型名或维度。
- `research_judge_suite`：把候选 diff、确定性评估摘要、选定的 02/03/04 文档片段、竞争力特性目标和实现护栏交给 LLM 或 coding-agent judge，输出 `research_judge` objective。默认使用 `opencode` CLI judge，也可以指向 OpenAI-compatible HTTP；`RELAY_KNOWLEDGE_JUDGE_BACKEND=none` 会保留 suite 选择但禁用 backend，需要完全不运行 suite 时使用 `--exclude-categories research_judge`。不支持的 backend 名称会让 judge gate 失败；显式 CLI judge command 会选择 CLI backend，除非 `RELAY_KNOWLEDGE_JUDGE_BACKEND` 明确要求 HTTP。该 suite 不替代确定性 gate，只负责研究性质和开放式质量判断。
- `/opt/workspace/relay-teams`：`scope=all` 全仓索引和 Python 服务、connector、eval checkpoint、re-export 等查询。
- `/opt/workspace/opencode`：`scope=all` 全仓 TypeScript/TSX monorepo 索引与查询，覆盖 symbol、references、overloaded function、exported const、TSX component、caller/callee、relative import、`@/` 与 `~/` alias import、HTTP recorder redaction flow、LLM protocol streaming flow 和负例 symbol lookup。该目标有意选择 import-heavy 代码，让自迭代循环演进稳定 TypeScript import identity 和重复 edge 处理，而不是只优化小 fixture。
- `/opt/workspace/linux`：`exhaustive` profile 下 `scope=all` 全仓索引，覆盖 symbol、函数、syscall 风格宏、导出符号、include、references、callers、callees、mmap flow、epoll/eventfd 等大仓检索场景。
- `/opt/workspace/linux`：`exhaustive` profile 下通过 `linux_full` 目标重复测量完整仓库初始索引时间，用于长周期基线。
- `/opt/workspace/leveldb`：`scope=all` 全仓 C/C++ 索引与查询，覆盖类方法、自由函数、头文件、table cache、recovery、callers、hybrid lookup 和 filters。
- `/opt/workspace/temporal-samples-go` 与 `/opt/workspace/temporal-sdk-go`：
  默认 profile 下 `scope=all` 全仓 Go 索引，并通过 repository set 覆盖样例仓库到
  SDK 仓库的 worker/client API 使用关系。
- `/opt/workspace/opentelemetry-collector-contrib` 与
  `/opt/workspace/opentelemetry-collector`：默认 profile 下 `scope=all` 全仓 Go
  索引，并通过 repository set 覆盖 contrib 与 core 仓库之间的 receiver factory
  和 component type 使用关系。
- `/opt/workspace/kubernetes`：`exhaustive` profile 下 `scope=all` 全仓 Go 索引与查询，覆盖 command constructor、kubelet flow、API types、clientset/generic client、authorizer、informer imports、callers、hybrid lookup 和 filters。
- `/opt/workspace/spring-framework`：`exhaustive` profile 下 `scope=all` 全仓 Java 索引与查询，覆盖 context、bean factory、webmvc servlet/handler mapping、imports 和 filtered lookup。
- `/opt/workspace/rustfs`：`exhaustive` profile 下 `scope=all` 全仓 Rust 索引与查询，覆盖 trait implementation、函数内 import、认证调用链和启动执行流。
- `/opt/workspace/codex`：`exhaustive` profile 下 `scope=all` 全仓 Python 索引与查询，覆盖异常继承、relative import、retry 调用链和 app-server stdio 执行流。
- `/opt/workspace/nvm`：`exhaustive` profile 下 `scope=all` 全仓 Bash 索引与查询，覆盖 shell 函数、命令引用、installer source hook 和 artifact download flow。
- `/opt/workspace/dotnet-runtime`：`exhaustive` profile 下 `scope=all` 全仓 C# 索引与查询，覆盖 core library class、method、using directive 和 array-pool buffer flow。
- `/opt/workspace/okhttp`：`exhaustive` profile 下 `scope=all` 全仓 Kotlin 索引与查询，覆盖 client class、method definition、Okio import 和 request dispatch flow。
- `/opt/workspace/laravel-framework`：`exhaustive` profile 下 `scope=all` 全仓 PHP 索引与查询，覆盖 application class、constructor call、namespace use 和 service-provider bootstrapping。
- `/opt/workspace/rails`：`exhaustive` profile 下 `scope=all` 全仓 Ruby 索引与查询，覆盖 controller class、singleton method、require target 和 module composition。
- `/opt/workspace/scala3`：`exhaustive` profile 下 `scope=all` 全仓 Scala 索引与查询，覆盖 compiler context class、inline method、import 和 phase/mode flow。
- `/opt/workspace/alamofire`：`exhaustive` profile 下 `scope=all` 全仓 Swift 索引与查询，覆盖 session class、request method、import 和 queue/delegate flow。

默认 profile 的多仓库 fixture 可用以下命令准备：

```bash
git clone --depth 1 https://github.com/temporalio/samples-go.git /opt/workspace/temporal-samples-go
git clone --depth 1 https://github.com/temporalio/sdk-go.git /opt/workspace/temporal-sdk-go
git clone --depth 1 https://github.com/open-telemetry/opentelemetry-collector-contrib.git /opt/workspace/opentelemetry-collector-contrib
git clone --depth 1 https://github.com/open-telemetry/opentelemetry-collector.git /opt/workspace/opentelemetry-collector
```

新增 tree-sitter 语言 fixture 可用以下命令准备：

```bash
git clone --depth 1 https://github.com/nvm-sh/nvm.git /opt/workspace/nvm
git clone --depth 1 https://github.com/dotnet/runtime.git /opt/workspace/dotnet-runtime
git clone --depth 1 https://github.com/square/okhttp.git /opt/workspace/okhttp
git clone --depth 1 https://github.com/laravel/framework.git /opt/workspace/laravel-framework
git clone --depth 1 https://github.com/rails/rails.git /opt/workspace/rails
git clone --depth 1 https://github.com/scala/scala3.git /opt/workspace/scala3
git clone --depth 1 https://github.com/Alamofire/Alamofire.git /opt/workspace/alamofire
```

所有 repository target 都必须使用 `scope=all`。评估器会拒绝非全量 scope，full-scope 注册不会向 `repo register` 传递 path 或 language filter，并且默认 guardrail 会验证产品注册拒绝 `--language`；case 级 filter 只用于验证查询端过滤能力。使用 `--profile smoke` 可验证启动器而不运行仓库评估。需要运行长周期全量初始索引 gate 时使用 `--profile exhaustive`；这些 gate 有意不放在默认 profile，避免单 CPU 自迭代 worker 在收集可操作检索反馈前就拒绝每个候选。
