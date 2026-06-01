# relay-teams 优化问题 2026-05-14

[中文](../../zh/05-benchmarks/02-relay-teams-optimization-issues-2026-05-14.md) | [英文](../../en/05-benchmarks/02-relay-teams-optimization-issues-2026-05-14.md)

日期：2026-05-14

来源基线：[relay-teams baseline](01-relay-teams-baseline-2026-05-14.md)

## RK-PERF-001：无操作全量索引重建整个仓库

- 基线：86.56 秒，`changed_path_count=1658`，`skipped_unchanged_count=0`。
- 根因：full index mode 在解析和替换 row 前没有检查是否已有新鲜的 matching scope。
- 修复：增加快速路径，先解析 commit/tree；当请求的 full index 已经 fresh 时，直接返回持久化 scope metadata。
- 验收：重复 full index 返回 `changed_path_count=0`，blob read、parse 和 SQLite write 都为 0，并在 relay-teams 上 300ms 内完成。
- 测试：application service 覆盖同一 HEAD 的重复 full index。
- 状态：正确性已实现并重新验证，blob read、parse 和 SQLite write 都为 0。最新样本为 380ms，因此本轮未达到原 300ms 延迟目标，仍应继续作为性能观察项。

## RK-PERF-002：混合代码查询物化过多候选

- 基线：CLI hybrid query 1.46 秒；Web hybrid query 1.50 秒。
- 根因：hybrid search 在大型 scope table 上执行多组 `LIKE '%token%'` 查询。
- 修复：索引时填充 code-repository FTS5 candidate table，并用它为 typed query-layer lookup 提供 seed，再进入 Rust scoring/dedupe。
- 验收：relay-teams `project` hybrid query 在 500ms 内完成，且不改变响应 schema。
- 测试：现有 code query 行为测试继续通过。
- 状态：已实现并重新验证；验收口径的 `project` hybrid query 最新样本为 CLI 160ms、Web 64ms。

## RK-PERF-003：影响分析扫描完整 scope table

- 基线：CLI impact 2.47 秒；Web impact 2.51 秒。
- 根因：chunk、call 和 import 先按 source scope 全量取出，再在 Rust 中过滤。
- 修复：把 changed-path、callee-symbol、deleted-name 和 broad module filter 下推到 SQL。
- 验收：relay-teams base-to-HEAD impact 在 500ms 内完成。
- 测试：现有 impact 行为测试继续通过。
- 状态：已实现并重新验证；最新样本为 CLI 521ms、Web 269ms。该轮 CLI 单样本略高于 500ms 目标，Web 路径仍低于目标；后续复测应继续观察抖动。

## RK-PERF-004：仓库报告默认运行昂贵延迟样本

- 基线：JSON report 4.22 秒；Markdown report 3.56 秒。
- 根因：report generation 默认运行最多 3 个 hybrid query 作为 latency sample。
- 修复：默认 report 只输出 metadata，latency sampling 留给显式 benchmark workflow。
- 验收：relay-teams `repo report --format json` 在 300ms 内完成，并默认返回空 `latency_samples`。
- 测试：application service 验证 report 保留 representative query name，但省略 latency sample。
- 状态：已实现并重新验证；最新样本为 400ms，返回 `latency_samples=[]`。默认报告仍不运行昂贵 latency sample，但本机单样本高于原 300ms 目标。

## RK-PERF-005：Web 代码索引超时

- 基线：Web `code.repo.index` 30.015 秒后返回 HTTP 408。
- 根因：Web execute 同步等待仓库索引完成，且 no-op 情况仍执行 full rebuild。
- 修复：no-op fast path 让重复 Web index request 提前完成。
- 验收：已 fresh 的 relay-teams scope 重复 Web index 返回 HTTP 200 且耗时小于 1 秒。
- 后续：cold full indexing 仍应迁移为 queue/progress handle，而不是单个 blocking request。
- 状态：no-op Web indexing 已实现并重新验证；最新样本为 HTTP 200、162ms。

## RK-PERF-006：顶层 GraphRAG CLI 查询拒绝多词输入

- 基线：`query relay-teams benchmark --source ...` 以退出码 2 失败。
- 根因：顶层 `query` parser 只接受第一个位置参数。
- 修复：收集连续位置参数作为 query text，并保持 flag parsing 的歧义保护。
- 验收：多词 query 通过 CLI 和 help contract 验证。
- 状态：已实现。

## RK-PERF-007：默认 scope 包含大型数据和 lock 文件

- 根因：默认 file preset 未排除大型 JSONL 数据集 dump，并曾把 `uv.lock` 当 unknown source file 展开。
- 修复：默认排除常见二进制/媒体 asset 与 dataset dump；`uv.lock` 只作为 SBOM metadata 参与依赖建模，不产生 source chunk；显式 path filter 仍可 opt in。
- 验收：relay-teams 默认 source retrieval bytes 从 32,888,900 降到 22,063,153。
- 状态：已实现。

## RK-PERF-008：重复注册同一仓库根会覆盖 alias

- 根因：重复 registration 更新既有 repository row，但没有把新 alias 追加为同一 repository id 的持久 alias。
- 修复：同一 Git root 的重复注册保留旧 alias，并将新 alias 解析到同一 repository id。
- 验收：`fixture` 和 `fixture-web` 同时可查询同一 repository id。
- 状态：已实现。

## RK-PERF-009：增量更新前置条件容易从默认路径触发

- 根因：在 active status 已经指向 HEAD 时，`repo update --base main --head HEAD` 不能从持久 base scope 读取 previous fingerprint。
- 修复：incremental snapshot 携带 resolved base commit；storage 复制匹配的 persisted base scope；service 从 base scope 读取旧 fingerprint。
- 验收：先索引 base commit，再索引不同 active HEAD 后，仍可从 persisted base snapshot 成功 update。
- 状态：已实现。最新复测中，未在同一运行时索引 base scope 时仍会按前置条件失败：CLI 134ms、Web HTTP 400 / 4ms；单独运行时先索引 base 后再更新到 HEAD 成功，耗时 7.56s。

## RK-PERF-010：health 代码计数可能显示为空

- 影响：API consumer 只读 graph counter 时，可能误判代码索引未运行。
- 修复：service-level `health` 与 `graph inspect` 在 graph code counter 中包含 repository code totals，同时保留 `repository_code_totals` 作为仓库维度拆分。
- 验收：仓库索引后，`health.graph.code_file_count` 至少等于 `repository_code_totals.indexed_file_count`，parse-status count 与 graph counter 一致。
- 状态：已通过 application regression coverage 实现。
