# 文档与自迭代准备度验证记录 2026-08-18

[中文](../../zh/06-verification/13-documentation-self-iteration-readiness-2026-08-18.md) | [English](../../en/06-verification/13-documentation-self-iteration-readiness-2026-08-18.md)

> 日期: 2026-08-18
> 总体状态: BLOCKED
> 证据截止: 本记录编写前已经确认最终结果的检查
> Revision 范围: 当前共享工作树快照；最终不可变 revision 尚待记录
> 证据更新: 2026-08-25 focused performance 与 Kubernetes 冷索引诊断
> 历史前序: [文档发版准备审计 2026-06-05](11-documentation-release-readiness-2026-06-05.md)

## 1. 目的与证据边界

本记录是当前 documentation 与 self-iteration readiness 的优先入口。
2026-06-05 审计继续作为历史证据保留，但不能证明当前工作树状态。

只有在证据截止前已确认最终 PASS 的完整命令才标记为 PASS。PENDING 表示本记录尚未
获得该门禁的最终确认结果，不等于通过、失败、跳过或豁免。局部日志、成功的前置步骤
或更窄测试不能上推为完整门禁结论。

## 2. 已确认 PASS 证据

| 门禁 | 命令或范围 | 状态 | 证据边界 |
| --- | --- | --- | --- |
| Rust 全量测试 | `cargo test --all-targets --all-features` | PASS | 已确认 full-suite 最终结果 |
| Rust 类型/构建检查 | `cargo check --all-targets --all-features` | PASS | 已确认最终结果 |
| Rust lint | `cargo clippy --all-targets --all-features -- -D warnings` | PASS | 已确认 warnings denied 的最终结果 |
| Rust 格式 | `cargo fmt --all -- --check` | PASS | 已确认最终结果 |
| Package 组装 | `cargo package --allow-dirty --offline` | PASS | 当前共享工作树打包 1,974 个文件（14.6 MiB，压缩后 2.9 MiB），并从解包 crate 编译成功；`--allow-dirty` 只接纳已审阅的共享工作树，`--offline` 隔离了此前一次 crates.io TLS 瞬时失败 |
| 发布校验 | `cargo publish --dry-run --allow-dirty` | PASS | 当前 crates.io 发布 dry-run 对同一 crate 完成打包与编译，到达上传边界后未实际发布 |
| Web 生产构建 | 在 `web/` 中运行 `npm run build` | PASS | 已确认 Web build 最终结果 |
| Runtime smoke | `sh tests/runtime/run_sh_smoke.sh` | PASS | Exit code 0；覆盖实际 release binary service 启动与退出 |
| Browser 依赖环境 | `uv sync --extra dev --no-default-groups` | PASS | Browser test 的 Python 依赖已同步 |
| 不修改系统依赖的 Chromium 安装 | `uv run --extra dev python -m playwright install chromium` | PASS | 未使用 `sudo`，Chromium 安装完成 |
| Browser integration | `uv run --extra dev pytest tests/browser` | PASS | 使用已安装 Chromium 与现有系统库，1/1 test PASS，耗时 3.52 秒 |
| 单元测试覆盖率 | `CARGO_BUILD_JOBS=1 cargo llvm-cov --all-targets --all-features --fail-under-lines 90` | PASS | 当前工作树 exit code 0；139,267 行中 missed 13,405 行，line coverage 90.37%，阈值 90% |
| Focused fast performance evaluation | release binary 对 `index_performance_many_files` 执行 `fast --categories performance` | PASS | 报告 `manual-evaluate-1787657485515273930-0-3038475.json`：346/346 gates、119/119 cases、293 commands、score 1.0、`score_accepted=true`、`adoption_status=would_accept`；手工 evaluation 未创建 commit |

以上状态记录协调验证轮已经确认的结果。当前轮重新执行了 Rust 全量与 coverage 命令；
两者均报告 library tests 3,603 passed、1 ignored，benchmark integration 1 passed，
primary integration 203 passed。当前轮还按上表的共享工作树参数重新执行了 package
组装与 publication dry-run。Web build、runtime 与 browser 行保留各自已确认的证据，
本次更新没有重跑这些命令。

Focused performance 报告中的 named metric 全部位于未调整预算内：release build
321/180,000 ms、persistence suite 739/30,000 ms、1,024-file 冷索引
382/12,000 ms、register 加冷索引 453/13,000 ms、incremental 423/3,000 ms。

标准本地/CI 准备命令
`uv run --extra dev python -m playwright install --with-deps chromium` 在当前环境中未能
完成，因为操作系统依赖安装步骤请求 `sudo`，但没有可用 TTY。因此本记录不把该命令
标为 PASS。当前已有系统依赖足以让实际 Chromium browser test 通过。CI 仍保留并执行
`--with-deps` 命令；当前环境限制不删除也不放宽该 CI 步骤。

## 3. 待确认的证据

| 门禁 | 预期证据 | 状态 | 当前边界 |
| --- | --- | --- | --- |
| Self-iteration 评估 | 所需 `tools/self_iteration` profile 与 category 的最终报告 | PENDING | Harness build、局部 case 或中间输出均不足 |
| Kubernetes accuracy workload | Kubernetes 评估终态、实际执行 case 数与最终报告 | PENDING | 新隔离索引上，7 条此前失败的 focused query 已通过当前精确 rank/evidence 合同，但尚未运行完整 Kubernetes case 集及最终报告 |
| Kubernetes strict 冷索引性能 | release binary、commit `016a2bcfa48d4a56059ee5e878eb208ffccdb773`、精确全文件 scope、isolated home、210,000-ms budget | FAIL | 最新干净单 attempt 使用单调时钟计量，正常完成于 564.99 秒，task=`succeeded`、checkpoint=`completed`、status=`fresh`，scope 精确包含 30,353 files；耗时是未调整预算的 2.69 倍。其 fact 与此前 592.72 秒和 607.03 秒实测完全一致。相对紧邻样本改善 42.04 秒，但每个候选单样本既不能证明因果，也没有关闭 rail。另一次 host 墙钟跳变/恢复运行仅保留为诊断证据。 |

所有 PENDING 项与失败的 Kubernetes performance evaluation 都是 release blocker。
已通过的 focused-fast 证据不证明 exhaustive self-iteration 或 Kubernetes accuracy，
Kubernetes 210 秒性能预算则明确未满足。

## 4. 当前结论与更新规则

当前快照已有 Rust 核心门禁、package、publication dry-run、Web build、实际 release
binary runtime smoke 与 Chromium browser test 的已确认证据，但
focused-fast performance 报告也已通过；但 documentation/self-iteration 总体准备度
仍为 **BLOCKED**。Coverage 结果为 90.37%，
已超过 90% 阈值，但 exhaustive 证据仍 pending，Kubernetes 冷索引性能 rail 已失败，
因此不得描述为完整 release-ready。即使 browser test 已通过，上述本地 `--with-deps` 环境限制仍
必须保留披露。

主代理提供最终结果后，应在本记录中补充精确命令、环境、不可变 revision、最终状态，
以及失败或跳过原因。不得用预期结果或不完整日志把 PENDING 改成 PASS。若证据对应更晚
revision，应新增带日期记录，而不是静默扩大本快照的证明范围。

## 5. 记录维护校验

本记录及其路由改动使用独立于产品门禁的文档检查完成验证：

- `python3 tools/docs/check_docs.py`: PASS。
- 对受影响文档与 knowledge-map 文件运行 `git diff --check`: PASS。

这些检查只证明文档结构和 patch 空白正确，不能关闭任何 PENDING 的 self-iteration
或 Kubernetes 门禁。

---

导航: 上一条:
[12. 图数据库、知识图谱与 CodeGraph 深度研究归档](12-graph-database-codegraph-deep-research-archive-2026-06-05.md)
| 索引: [验证与审计记录](README.md)
