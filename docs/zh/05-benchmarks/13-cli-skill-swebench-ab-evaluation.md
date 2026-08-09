# relay-knowledge CLI skill 的 SWE-bench A/B 测评

## 目标

`tools/relay_knowledge_skill_eval` 用配对实验回答一个问题：向同一个编码
Agent 提供已发布的 `relay-knowledge-cli` skill，是否能提高真实软件问题的
解决率，并以多少 token、费用和时间为代价。Pi 只是固定执行器，不是测评
对象。

## 固定条件

- 数据集：官方 `SWE-bench/SWE-bench_Verified`，固定 revision
  `91aa3ed51b709be6457e12d00300a6a596d4c6a3`，共 500 题；下载结果与缓存的
  规范 JSONL 都必须匹配 SHA256
  `de1e478b9b64b2d69a46bfe329273f3dc56f201307cd6dd0055f8d9a4de98841`。
- smoke 清单：与 `agent_teams` 相同的前 10 个 Astropy 实例，清单提交在
  `tools/relay_knowledge_skill_eval/src/relay_knowledge_skill_eval/data/smoke-10.txt`。
- Agent：Pi `0.80.3`。
- 模型：DeepSeek 官方 `deepseek-v4-flash`，`high` 推理档位。
- 每题每组只运行一次；完整实验为 1000 次 Agent 执行。
- 每个条件的 Agent 总时限为 1 小时；每个条件使用独立、原始的官方
  SWE-bench 容器。

baseline 禁用全部 skill；treatment 同样禁用自动 skill 发现，只显式加载
被测 `relay-knowledge-cli/SKILL.md`。两组使用相同 prompt、base commit、工具、
模型参数和容器运行环境，并用稳定哈希交替执行先后顺序。

强制使用变体通过 `--require-skill-use` 只在 treatment prompt 中要求执行
skill 自带的 CLI，并通过 `--parallel-conditions` 并行运行同题两组。配合
`--concurrency 2` 时最多同时运行 2 个 baseline 与 2 个 treatment Agent，
官方评分也由独立宿主 Python 子进程并行控制隔离的 Linux Docker 评分容器。
该变体衡量的是“加载 skill + 强制使用提示”的组合效果，其结果目录与
checkpoint 必须和默认受控协议隔离。

Pi 运行过程中如果发生网络、限流或进程中断，harness 会保留当前容器、工作树
和 Pi session，在同一小时总时限内最多发送 3 次继续指令。连续 10 分钟没有任何
输出也按可恢复中断处理。每次续跑都会写入 trace 和报告，不会重置已经完成的工作。
只有停滞、已识别传输故障或明确的临时进程退出码会触发续跑；普通非零退出属于最终
Agent 失败，不会被反复记成可重试基础设施故障。
传输识别同时覆盖 `request timed out`、`timeout` 等文本，不只识别 `ETIMEDOUT`。
如果进程在 checkpoint 最后一条 JSONL 写入中断，续跑会先截断残缺尾行再追加；
中间记录损坏仍会直接报错，避免静默跳过数据。
续跑还会确认已有 checkpoint 的每条结果都属于本次选择的 suite，且新目标结果数
不少于已有记录数；缩小或切换 suite 会直接拒绝，不会把旧结果标成新运行的数据。
SWE-bench 实例镜像使用固定的 Python 构建依赖，避免上游打包工具更新破坏旧仓库；
Windows 宿主生成的 Linux 构建脚本和 Dockerfile 强制使用 LF，避免 CRLF shebang
使环境镜像或实例镜像在 Agent 启动前失败。
无论官方构建脚本是否带 shebang，克隆命令和 Python 包安装命令都会执行相同转换。
SWE-bench constants 与 docker_build 模块内缓存的构建目录会同时指向评测 cache，
不会把脚本和日志写到进程当前目录。
单题镜像构建失败会写为可恢复的基础设施结果，不会中止其他题目。
本地 skill 与 release skill 使用相互隔离的内容寻址缓存，版本号相同也不会互相覆盖。
Pi runtime 镜像标签包含完整 skill SHA256，skill 内容变化后不会静默复用旧镜像。
Agent 容器以只读方式挂载 skill 和 CLI；每题索引仅写入该题容器隔离的 `/tmp`。
SWE-bench Agent 仅加入无默认外网的内部 Docker 网络，并通过固定 sidecar 访问
`api.deepseek.com:443`；DeepSWE 使用 Pier 的同域名网络白名单。
prompt 通过 stdin 跨宿主传输以避开命令行长度限制，再由 Linux wrapper 作为 Pi
JSON 模式的最后一个消息参数传入。
DeepSWE 仅在容器创建时通过 Compose 环境占位符注入 API key，后续
`docker compose exec` 的宿主进程参数不包含凭据。
DeepSWE 与 SWE-bench 解析同一份官方 release skill，并把包含完整 skill SHA256
的 runtime 镜像标签传给 Pier；缺失时构建该精确镜像，不使用固定或遗留标签。报告
记录实际解析的 skill 版本与 SHA256，重建报告时保留已有运行来源信息。
共享 Pi runtime 镜像显式以 `linux/amd64` 构建，使 ARM64 Docker 宿主也与
x86-64 CLI asset 和官方题目容器架构一致。
每次 Docker build 使用独立 UUID runtime context，并在 finally 中只清理自身目录。
未显式传入 `--tasks-dir` 时，runner 会在评测缓存中创建或复用官方
`datacurve-ai/deep-swe` checkout，并校验固定 commit
`435ee89ec2f2e2289f33b0da4f992f0b7b7266b9`；每次复用前重置 tracked 文件并
清理 untracked 文件，确认 113 个官方任务目录后再启动。
官方 HTTPS remote 比较会统一去掉可选的 `.git` 后缀。

DeepSWE 的两组使用相同的工程执行提示：先检查仓库和目标行为，在可行时建立
复现，再实现最小且通用的产品代码修改；聚焦测试失败后必须继续分析和迭代，并
检查边界情况、`git status` 与最终 diff。Skill 组只额外增加必须先执行
`relay-knowledge-cli` 查询并用查询证据指导实现的段落。DeepSWE 官方任务的
`tests` 和 `solution` 目录不挂载到 Agent 容器；`/logs/verifier` 只是 Agent 退出后
由独立 verifier 写入的结果目录，不能作为 held-out tests 或参考答案入口。

生产运行代码不得按 repository、instance ID、task name、题目文本或已知测试结果
分支。固定 smoke-10 选择只保存在打包的 manifest 中，不在 Python 里重复枚举。
DeepSWE 的 held-out `test.patch` 如果与候选 patch 冲突，应记为候选失败且不重跑；
只有 verifier 无法执行、候选 `model.patch` 未完成交接、传输中断等环境故障才进入
可恢复的 infra retry。官方 `[[verifier.collect]]` 由兼容脚本在 Agent 退出后执行，
避免旧 Pier 静默忽略该字段并让独立 verifier 测试原始仓库。
DeepSWE 续跑会先归档已经确认的可重试基础设施状态或损坏结果，再用本次 Agent 和
runtime 镜像配置校验剩余 Pier job，最后改写报告来源信息；不兼容的旧 job 不会污染
磁盘上已有数据的 provenance，截断结果也不会在修复前阻断续跑。最终失败是否
属于传输故障只依据当前一次 stderr，之前续跑尝试中的网络错误不会把后续确定性
Agent 失败误标为可重试基础设施故障。
预校验创建的 Pier job 会立即关闭日志 handler，再由真实执行打开或归档相同文件。
Pier setup watchdog 比 900 秒预索引时限多 120 秒，覆盖前置两个 30 秒容器探测并
额外保留 60 秒余量，确保索引器先决定 setup 结果。Agent execution 尚未创建时发生
的 setup 异常按基础设施失败归档重试；同一 Skill trial 在较早一次尝试中已经成功
执行的知识库查询会跨 transport continuation 保留，不要求最后一次尝试重复查询。
stderr 的 64 MiB 上限按脱敏前的原始输入计算；即使输入恰好在换行处到达边界、后续
数据又因脱敏使落盘文件变小，仍会写入超限标记并按最终 Agent 错误处理。

## 计时边界

treatment 在 Agent 计时前注册并完成仓库索引。预索引时间单独报告，不计入
主 A/B Agent 时间；Pi 继承相同的 `RELAY_KNOWLEDGE_HOME`，确保查询复用这份
预索引。两套 runner 都会用有界的 status/index-worker 循环排空持久化索引任务，
不会把 queued、retrying 或 running 状态当作预索引完成。如果 Pi 在运行中主动
重复索引，则仍属于 Agent 时间。
报告同时保留镜像准备、容器启动、预索引、Agent、官方评分和端到端墙钟
时间，并统计 Pi tool-call 的累计执行时间。
runner 与实时看板并发刷新报告时，各自使用唯一临时文件并通过原子替换发布，
不会竞争同一个 `.tmp` 文件或暴露部分写入的 JSON、JSONL、CSV、HTML。
公网 Site 上传失败时不会推进已同步签名；包括最终完成快照在内，都会在下一轮
继续重试，而不是因本地结果已完成而提前退出。
DeepSWE 的 infra_error 只计入已记录数，不计入已完成数；只有全部预期结果均无
基础设施失败时才启用最终 bootstrap 区间并把报告标为 final。
DeepSWE 不把空 stderr 本身当作网络失败；只有明确 transport 标记或已知瞬态退出码
才会触发续跑和基础设施重试，确定性的空 stderr 非零退出按 Agent 错误记录。
看板只要存在 completed_results 字段就采用该值，包括显式 0，不回退到条件记录数。
看板知识库查询数与强制使用规则共享同一查询类型集合，包含 software、
feature-flags 和 impact，不会把合规查询显示为 0。
release 下载和解压使用每次调用唯一的 UUID staging 路径，并原子发布已校验 skill，
避免多个冷缓存 prepare/run 进程互相删除临时目录。

## 质量判定和安全

Agent 只接收 `problem_statement`，看不到 reference patch、test patch、提示或
gold answer。Agent 结束后由 harness 以题目原始 base commit 为基准提取完整工作树
patch，因此 Agent 已提交的修改也不会丢失。原始 patch 不经脱敏改写直接交给官方
SWE-bench 评分器判定 `resolved`、FAIL_TO_PASS 和 PASS_TO_PASS；仅落盘副本脱敏。
patch 提取本身受 5 分钟时限和 64 MiB 大小预算约束，超过预算的候选按 Agent 错误
记录且不进入评分。只有 Agent 正常完成且 verifier 通过才计入通过率；超时或 Agent
错误留下的部分 patch 即使通过测试，也不计为测评通过。强制 Skill 使用只接受成功
结束的 relay-knowledge 仓库查询，启动后失败的查询不能满足条件，SWE-bench 与
DeepSWE 采用相同规则。镜像准备与 Linux 直接评分使用的 Docker SDK client 均在
边界调用结束后关闭，避免全量长跑累积连接和文件描述符。
索引任务进入 retrying 时，harness 按持久化 `next_retry_at_ms` 等待后再尝试 worker，
不会在退避窗口到达前空转耗尽有界轮次。
Windows scorer worker 超过外层时限时，父进程会按 run-scoped 容器名执行有界清理，
再返回可重试评分基础设施失败。

API key 只从 `DEEPSEEK_API_KEY` 进程环境传递给容器，不进入命令参数、配置、
checkpoint 或报告。trace 与 stderr 都按精确值和通用 `sk-...` 模式脱敏。DeepSWE
的 Pier 外层时限比 3600 秒 Agent 时限多 300 秒，确保内部超时时能停止 Pi、提交并
收集部分工作。所有生成物
位于 gitignored 的 `.evals/relay-knowledge-skill/`。
同步到公开 Site 的报告按嵌套字段递归白名单重建，不上传评分日志路径、测试输出
路径、prompt、trace、patch、本地路径或错误详情。

索引性能 CI 将结构与延迟判定集中在 `.github/index-performance-gate.jq`。产品代码、
性能 fixture 或 self-iteration harness 变更使用实际运行指标强制预算；只修改门禁
workflow 或该 jq 文件时，CI 仍运行完整结构检查，并以同一 jq 程序验证预算边界值
必须通过、超预算值必须失败，并分别要求冷索引和增量索引完成命令成功，避免重复命令
掩盖缺失的增量验证。三项延迟指标也按名称逐项验证；产品代码与门禁文件同时修改时，
实际性能预算和门禁语义自检都会执行，避免共享 runner 的冷启动噪声掩盖或误报门禁语义。

## 解释限制

smoke-10 用于验证执行链、工件完整性和趋势，不用于给出最终总体结论。最终
结论以 500 个配对实例为准，并同时报告通过率差、skill-only/baseline-only、
McNemar 配对统计、token、费用、时间和行为指标。运行命令、恢复策略和工件
路径见 `tools/relay_knowledge_skill_eval/README.md`。

## 2026-08-13 实测结果

这次实测采用 Pi `0.80.3`、DeepSeek 官方 `deepseek-v4-flash`、`high` 推理档位、
每个 Agent 3600 秒上限，以及 `--require-skill-use` 强制使用协议。SWE-bench
Verified 运行官方顺序前 100 题，DeepSWE 运行全部 113 题；两套运行均完成全部
预期记录且 `infra_error=0`。因此这里衡量的是“加载 Skill 并要求实际调用 CLI”的
组合效果，不应解释成仅加载 Skill 的纯因果效应。

| 数据集 | 普通组 | Skill 组 | 通过率差 | Skill 独有通过 | 普通组独有通过 | 95% CI | McNemar p |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| SWE-bench Verified 前 100 题 | 78/100（78.0%） | 82/100（82.0%） | +4.0 个百分点 | 9 | 5 | [-3.0%, +11.0%] | 0.424 |
| DeepSWE 113 题 | 46/113（40.7%） | 46/113（40.7%） | 0.0 个百分点 | 15 | 15 | [-9.7%, +9.7%] | 1.000 |

SWE-bench 前 100 题中，Skill 组的点估计更高，但置信区间跨过 0，当前样本不能
证明统计显著提升。DeepSWE 的总通过率完全持平，不过两组解决的题目并不相同：
双方共同通过 31 题，双方共同失败 52 题，另外各有 15 题只被其中一组解决。

| 数据集与指标 | 普通组总计 | Skill 组总计 | Skill 相对变化 |
| --- | ---: | ---: | ---: |
| SWE-bench 输入 Token | 2,708,452 | 3,841,407 | +41.8% |
| SWE-bench 输出 Token | 3,545,361 | 3,925,069 | +10.7% |
| SWE-bench 缓存命中 Token | 301,310,720 | 372,316,288 | +23.6% |
| SWE-bench 总 Token | 307,564,533 | 380,082,764 | +23.6% |
| SWE-bench 费用 | $2.216 | $2.679 | +20.9% |
| SWE-bench Agent 时间 | 10 小时 39 分 | 12 小时 28 分 | +17.1% |
| SWE-bench 工具调用 | 7,029 | 7,514 | +6.9% |
| DeepSWE 输入 Token | 9,354,618 | 9,843,799 | +5.2% |
| DeepSWE 输出 Token | 13,615,501 | 12,955,719 | -4.8% |
| DeepSWE 缓存命中 Token | 2,201,952,384 | 2,091,647,232 | -5.0% |
| DeepSWE 总 Token | 2,224,922,503 | 2,114,446,750 | -5.0% |
| DeepSWE 费用 | $11.287 | $10.862 | -3.8% |
| DeepSWE Agent 时间 | 49 小时 40 分 | 47 小时 14 分 | -4.9% |
| DeepSWE 工具调用 | 17,664 | 17,076 | -3.3% |

Skill 组实际记录了 849 次 SWE-bench relay 命令和 841 次 DeepSWE relay 命令；
普通组为 0。提供方没有返回可用的缓存写入 Token，因此结果只报告输入、输出、
缓存命中和总 Token。原始 prompt、压缩 trace、patch、评分输出和报告包含本地路径及
大体积诊断信息，按仓库策略保存在 gitignored 评测归档中，不提交到 Git。
