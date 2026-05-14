# relay-teams 优化研究 2026-05-14

[中文](../../zh/benchmarks/relay-teams-optimization-study-2026-05-14.md) | [英文](../../en/benchmarks/relay-teams-optimization-study-2026-05-14.md)

本文记录 relay-teams benchmark 中慢路径的实现导向性能分析，用作优化前后的对照材料。

## 1. 重复 full index 的 no-op 快速路径

问题：`build_full_snapshot` 会解析 ref、列出 tracked file、通过 `git show` 读取每个 blob、解析所有选中文件并执行 full replacement。已有 fingerprint 只服务 incremental 和 worktree overlay，无法让相同 ref 的 full index 提前返回。

优化：

- full indexing 前先解析目标 commit 和 tree hash。
- 如果 storage 已经存在 matching repository scope，且 commit、effective filter 和 indexed status 都 fresh，则直接从持久 status 构造 response。
- no-op full index 应报告 `changed_path_count=0`、`skipped_unchanged_count=indexed_file_count`、0 blob read、0 parse、0 SQLite write。
- relay-teams no-op full index 目标小于 300ms。

## 2. Hybrid query 候选集过大

问题：hybrid query 会在 symbol、reference、call、import 和 chunk 多个层面执行查询。基线 SQL 使用 `lower(...) LIKE '%token%'`，很难利用索引；relay-teams 基线包含 28,125 个 symbol、187,993 个 reference、187,992 个 call、11,534 个 import 和 28,441 个 chunk。

优化：

- 索引时填充 code-repository FTS5 candidate table。
- 先用 FTS candidate 为每个 code query layer 生成候选，再回到 typed table 构造响应。
- 保持现有行为和响应 shape，避免大范围 `%token%` scan。
- relay-teams `project` hybrid query 目标小于 500ms。

## 3. Impact analysis 扫描全 scope

问题：`chunks_for_paths`、`callers_for_symbols` 和 `importers_for_modules` 会先取出 indexed scope 的大量 row，再在 Rust 中过滤 changed path、callee symbol、deleted name 或 module。

优化：

- 把 changed path filter 下推到 chunk SQL query。
- 把 callee symbol/deleted-name filter 下推到 call SQL query。
- 把 broad module candidate filter 下推到 import SQL，并保留精确匹配。
- Impact analysis 应随 changed path 和 seed symbol 扩展，而不是随完整 scope table size 扩展。
- relay-teams base-to-HEAD impact 目标小于 500ms。

## 4. Repository report 默认运行延迟样本

问题：storage report 本身只是 aggregate metadata，但 application code 默认执行最多 3 个 hybrid repository search 来填充 latency sample，嵌入式样本耗时约 1.21s 到 1.32s。

优化：

- 默认不运行 latency sample。
- 保留 representative query name，方便 operator 显式运行 query benchmark。
- 默认 report 应只输出 metadata 并保持快速。
- relay-teams report generation 目标小于 300ms。

## 5. Web index request 超时

问题：Web operation endpoint 在同一个 request 中同步执行 `code.repo.index`。no-op full index rebuild 会放大超时。

优化：

- no-op fast path 让重复同 scope Web index request 快速返回 HTTP 200。
- cold full indexing 后续应迁移为 queue/progress handle，而不是单个 blocking request。
- 已 fresh 的 relay-teams scope 重复 Web index 目标小于 1 秒。

## 6. 优化后验证

优化后使用相同 release binary 和 runtime pattern 重新运行基准。关键结果：

| 场景 | 基线 | 优化后 |
| --- | ---: | ---: |
| 重复 full index | 86.56s | 0.39s |
| 混合查询 | 1.46s | 0.10s |
| 影响分析 | 2.47s | 0.34s |
| JSON 报告 | 4.22s | 0.26s |
| Web 无操作索引 | HTTP 408 / 30.015s | HTTP 200 / 0.17s |
| 顶层多词 `query` | exit 2 | exit 0 |

Cold full index 在该轮从 82.57s 变为 90.43s，因为优化后索引会额外填充 code-repository FTS candidate table。这是为查询延迟做出的有意取舍；如果 cold indexing budget 成为主要瓶颈，需要重新评估。
