# 冷索引无损优化验证 2026-09-17

[中文](17-lossless-cold-index-2026-09-17.md) | [English](../../en/06-verification/17-lossless-cold-index-2026-09-17.md)

## 1. 结论与范围

本轮保留索引事实和查询行为，减少配置扫描、行号计算和 FTS 参数的临时复制。
真实仓库三轮冷索引中位数由 31.585 秒降至 30.017 秒，减少 4.96%；完整
诊断预览由 8.827 秒降至 6.413 秒，减少 27.35%。冷索引样本范围有重叠，
不能把 4.96% 外推为所有仓库的稳定收益。预览的全部优化后样本均快于基线。

这是指定快照的 focused A/B 验证，不是完整 self-iteration fast/exhaustive
采纳或跨平台发布认证。此前 v1.1.17 的历史耗时没有混入本轮样本。
本轮选择八项耗时场景，执行八项、跳过零项；每版每项三次，共 48 个计时样本。
十八项查询各执行一对行为比较；未执行的完整 evaluator 不计为通过。

## 2. 实现与不变量

- 配置词法扫描先检查剩余字符串是否包含目标 API 模式；没有匹配时跳过
  字符解码及引号状态扫描。命中时沿用原有转义、引号和 UTF-8 偏移规则；
  引号谓词改用静态分派。
- 文件首次需要配置事实行号时，缓存 CR/LF/CRLF 的换行位置，随后二分定位。
  缓存属于当前文件输入；生产输入继续受 512 KiB 解析上限约束。
- FTS 批量参数借用 pending document 的字符串，避免复制六列内容。
  每批仍最多 1,024 条，并受实际 SQLite variable limit 约束。

保留全部配置支持事实、getter 证据、正文和全文索引，不改持久化格式、事实版本、
排序、未解析状态、任务租约、单仓库单写者、检查点恢复、发布屏障或预算。
没有新增运行时配置、安装依赖或服务；该优化本身不要求已有索引迁移或重建。
文件级行号缓存增加少量临时存储，因此本轮不宣称内存降低。

## 3. 二进制与环境

| 项目 | 身份 |
| --- | --- |
| 优化前 | `main df33178641576afcda6b47724ff1f780d4679174` 的既有 release 包 |
| 优化后 | `perf/lossless-cold-index` 工作区，基于同一 commit |
| 优化前 binary SHA-256 | `6d8547cc29a9387d6b64cee6547e1d3801a053a91909162201a8c72e57037b69` |
| 优化后 binary SHA-256 | `ee0329e5732e1f875c708b9b8f5597f7408bcbae08f9bc27e4accf537fe03e4d` |
| 源码输入摘要 | `37ca9294c213efaf9048a81d2d893a93bac49258dad83e83ea62d96264d097ae` |
| 编译 | Rust 1.98.1，Linux x86_64，release；优化后使用 `--locked --all-features` |
| 运行 | Ubuntu 24.04 / WSL2，glibc 2.39，Intel i7-1260P，16 个逻辑 CPU |
| 真实仓库 | `relay-teams`，`fa3c0ddc9d81400b8d5e58ab7600dd557a056816` |

源码摘要取 2,010 个 `src` 及 Cargo 输入文件，CRLF 规范化为 LF 后逐文件计算
SHA-256，再对按路径排序的 `{path, sha256}` JSON 数组计算摘要；已逐项核对
工作区和实际构建快照。报告、测试驱动和文档不属于产品源码摘要。

构建产物放在 Windows C 盘以控制磁盘占用；计时用二进制、仓库和独立 runtime
均放在 WSL 原生 ext4。计时期间没有本次编译或测试负载并行运行。
每版使用三个全新 runtime，依次按 before/after、after/before、before/after
运行；不清空操作系统页缓存。因此“冷”指没有应用索引，不指清空系统缓存。
每条命令使用单调时钟及 GNU time 记录墙钟、CPU 和峰值 RSS，限时 300 秒，
并保留磁盘余量保护。无样本被挑选删除。

## 4. 耗时与资源

以下为三轮中位数，单位秒；变化率为 `(优化后 / 优化前 - 1) × 100%`。

| 用例 | 优化前 | 优化后 | 变化 |
| --- | ---: | ---: | ---: |
| 真实仓库冷索引 | 31.585 | 30.017 | -4.96% |
| 真实仓库完整 scope preview | 8.827 | 6.413 | -27.35% |
| 真实仓库无变化重复 index | 0.1133 | 0.1107 | -2.28% |
| C fragment 冷索引 | 0.3568 | 0.3487 | -2.28% |
| 1,024 文件冷索引 | 0.7080 | 0.5494 | -22.41% |
| 1,024 文件增量索引 | 0.8461 | 0.7660 | -9.46% |
| 2,048 文件冷索引 | 3.3796 | 2.8562 | -15.49% |
| 2,048 文件增量索引 | 3.5673 | 3.1079 | -12.88% |

1,024/2,048 是既有 fixture 名称中的源码规模；注册完整范围后，实际索引分别
包含 1,025/2,084 个文件，另含清单等文件，两版范围相同。

真实仓库各轮冷索引为 before `[26.791, 34.888, 31.585]`、after
`[29.235, 33.090, 30.017]`。预览为 before `[8.663, 8.827, 9.648]`、after
`[6.420, 6.017, 6.413]`。小用例也有运行调度和存储波动，不能由三个样本建立
统计显著性结论。查询探针只有一对样本，用于行为对比，不用于延迟分位数认证。

冷索引 user CPU 中位数从 145.39 降至 90.36 CPU 秒（-37.85%），预览从
100.24 降至 64.90 CPU 秒（-35.26%）。多线程累计 CPU 秒不能直接当作墙钟秒。
配置扫描工作减少，而持久化、FTS 写入和 finalization 仍完整执行；这与完整
预览收益更明显、冷索引总体收益较小相符，但本轮没有逐阶段剖析或单项消融。

冷索引峰值 RSS 中位数反而从 938.4 升至 1,011.8 MiB（+7.81%），预览从
315.4 降至 302.7 MiB；没有单独归因峰值变化。数据库与 WAL 合计约 1.475 GiB，
两版仅有页级差异；文件系统写出量约相同。不能宣称数据库缩小或冷索引内存改善。

## 5. 行为一致性与预算

真实仓库首轮对 16 张事实和检索表的全部 1,545,613 行进行规范化 JSON 行摘要
及排序后的整表 SHA-256 对比；时间列及运行 I/O 诊断不参与比较。两版全部一致，
包含类型归属 JSON、配置 metadata、正文、诊断、FTS 文档和 search rowid 映射。

| 内容 | 两版相同的数量 |
| --- | ---: |
| 文件 / symbol | 1,835 / 40,720 |
| reference / call / import | 263,817 / 237,634 / 12,136 |
| chunk | 39,057 |
| 配置证据 / 配置绑定 | 33,006 / 60,887 |
| FTS 文档 / metadata | 427,980 / 427,980 |
| 文件诊断 | 91 |

其余比较表为 dependency、framework node/edge、route 和 path tombstone。
没有删除诊断或支持事实来换取性能。七种查询（hybrid、symbol、definition、
references、callers、callees、imports）及三个现有环境变量配置查询的有序结果相同；
配置响应仅排除 scope、freshness 和 metadata 外壳后比较。

三个既有 self-iteration fixture 的五对完整/增量快照及八项有序查询也一致。
全量及增量索引执行都验证 succeeded task、completed checkpoint 和精确 commit；
真实仓库另验证 fresh 状态。无变化重复 index 的响应只返回既有 scope/status/summary 等信息，
六份响应均为 fresh 且 commit 正确，没有返回 task/checkpoint，不将其计为新的任务完成证据。
增量用例保持最多两次 blob read、两个 parsed file。
原有预算不变：C 冷索引 5 秒、1,024 文件冷/增量 12/3 秒、2,048 文件冷/增量
30/5 秒；全部样本通过。未执行整轮 evaluator，因此这些结果不等同于其评分。

## 6. 质量验证

已通过 release 构建、全目标 check、全目标 Clippy、格式、218 个 Markdown 文档
校验、skill metadata 校验和 Chromium 浏览器测试。UT 重跑为 4,493 passed、
0 failed、1 ignored；忽略项是由有界 Git 测试显式调用的子进程 fixture。
集成测试 171/171 通过，测试执行 81.79 秒；self-iteration harness 为 243/243，
harness Clippy 通过。扫描工作量门禁 2/2、持久化性能门禁 20/20、确定性 benchmark
1/1 均通过，命令墙钟分别为 5.79、8.92、15.25 秒（含 Cargo 检查或编译），
不能把这些门禁耗时作为产品索引耗时。

`cargo llvm-cov --lib --bins --all-features --fail-under-lines 90` 的单元测试行覆盖率
为 **90.04445%（156,590/173,903 行）**，没有合入集成测试覆盖率。词法扫描和
行号缓存模块分别为 56/56、18/18 行，SQLite search owner 为 347/378 行。
覆盖率运行同样通过 4,493 项测试。Miri 在 strict provenance、symbolic alignment
和 deterministic concurrency 设置下通过 17/17 项 `domain::core::` 测试。
AddressSanitizer 最终完整通过同一套 4,493 项测试，零失败、一个既有 ignored fixture；
测试执行 1,683.99 秒，完整命令 1,712.63 秒，退出码为 0，启用泄漏检查。

本地 ASan 首轮在 6 GiB 内存 scope 内编译时触发 OOM，尚未启动测试；保留内核及 scope
诊断。改用 1,024 个 codegen units 并临时增加 1 GiB swap 后编译成功，但编译与测试合计
达到 2,400 秒上限，退出 124，不能计为通过。后续复用同一编译产物，并将
`malloc_context_size` 从默认 30 改为 5；这仅缩短分配/释放诊断栈，仍启用 AddressSanitizer
和 `detect_leaks=1`，不修改断言或产品预算。最终轮沿用 6 GiB 内存、3 GiB swap scope，
八个测试线程，限时 3,600 秒；正常完成后已停用并删除临时 swap 文件。
本地设置不替代 PR 中原生 Linux 的默认 ASan 门禁。

第一轮以两个测试线程执行的 UT 在 1,800 秒本地进程上限处中断，此前没有断言失败；
保留原日志，使用同一已编译测试程序和八个测试线程完整重跑。测试线程调整不改变
产品资源预算，也不参与上面的性能计时。

挂载目录中的八线程集成测试有一个 health/query isolation case 失败，该轮在
保留日志后停止。同一个测试程序在挂载目录和原生目录单独执行时都通过（整项
约 1.88/1.84 秒）；两秒响应断言未放宽。未取得该次失败的完整断言信息，不能
据此确定具体超时或代码根因。随后定位到 Cargo 给测试进程注入 38 个动态库搜索
目录，其中 36 个位于 C 盘；子进程反复进行跨系统路径探测。`ldd` 确认测试程序
只需要 Linux 系统库。仅移动源码仍慢，因此停止该次未完成验证，最终在原生源码
目录、两测试线程下，以 `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER='env -u LD_LIBRARY_PATH'`
启动测试，完整通过。此 runner 只清理测试子进程的冗余动态库路径，不改产品代码
或断言。性能 A/B 直接启动 release binary，没有继承 Cargo 的测试搜索路径。

## 7. 证据与复现边界

本机原始证据位于忽略的 `target/lossless-cold-index-20260917/`，本报告保存可随
仓库发布的关键结果；原始目录不是可供其他机器访问的共享位置。

| 原始报告 | SHA-256 |
| --- | --- |
| `comparison.json` | `831fd008c25f54dea75547cc33ee533e926cb058ba91edd0a96b121fbca42fb5` |
| `fixtures/comparison.json` | `1cb84c573804b38549fc0c5bf8ed041dfd456548fcdbfe00d4b49a54669cf431` |
| `summary.json` | `d86e3facc99f6acf14c1823436ce7e6e2218c8178727505df7bc6f1bbf467411` |

可在 POSIX 环境按上述快照和顺序分别为两个 release 二进制建立独立 runtime，
执行 `repo register`、`repo index --ref <commit>`、`repo scope preview --ref <commit>`
及无变化的重复 index。Fixture 定义来自
`tools/self_iteration/cases/repository_index_performance_targets.json`，沿用既有生成器。
新增确定性 `configuration_scan_work_suite` 已接入 fast 门禁，防止无匹配行重新进行
逐字符引号扫描；原有持久化门禁继续保护 FTS 顺序、边界、回滚与恢复。

---

导航：[验证记录](README.md) | 上一篇：[16. 地图与图关系存储验收](16-map-graph-storage-acceptance-2026-09-07.md)
