# 软件全域证据优先级验证记录 2026-08-31

[中文](../../zh/06-verification/14-software-global-evidence-priority-2026-08-31.md) | [English](../../en/06-verification/14-software-global-evidence-priority-2026-08-31.md)

> 日期: 2026-08-31
> 范围内状态: PASS
> 基线 revision: `6e78bdbac22e1a0875cee2b13434baffd3b52a17`
> 评估 patch: 50,981 bytes，SHA-256 `19e85c4ed920d998100c1a5533258525ad965024a38b1e4ac0f194152298dd55`
> 最终报告: `manual-evaluate-1788132786594001932-0-1437486.json`，SHA-256 `91bcee863e5b9e62a7cd6dcd89ca6fb16cb26440827a3ae9d26cff10629e7fc6`
> 证据边界: release 产品二进制、`fast --categories performance`、当前变更的 Rust 与文档门禁；不认证 exhaustive、agent workflow、research judge、Kubernetes 或整体发版就绪

## 1. 目标与分析范围

本轮以 CodeSpec、Knowledge Map、仓库代码图和 `tools/self_iteration` 的真实 case 为路由依据，
分析 `software_global_fixture` 中已经通过但主证据排序不理想的查询。基线的 368 个 gate、
132 个 case 和 307 个 command 全部通过，说明问题不是召回缺失或 schema 不完整，而是相同
候选集合中的可操作证据没有排在前面。

代码图把 API/resource/deployment 查询路由到
`storage::sqlite::software::ontology::query`，把 topic 路由到
`storage::sqlite::software::graph::topics`，把 design 路由到
`storage::sqlite::software::lifecycle::design`。实现因此只调整对应读取边界的确定性排序，
没有修改 ingestion、projection、schema、durable task、lease、checkpoint、freshness 或 writer
路径。

## 2. 基线、算法与结果

| 观测 | 基线 rank | 最终 rank | 排序信号 |
| --- | ---: | ---: | --- |
| API 主证据 | 2 | 1 | API schema，其次 code，再次 documentation |
| Resource 主证据 | 3 | 1 | Kubernetes、Terraform、Compose、systemd、launchd、Helm |
| Deployment 主证据 | 4 | 1 | service definition、IaC、runtime；deployment unit 先于 runtime service |
| Topic 主证据 | 3 | 1 | nested document、Knowledge Map topic、根 README heading |
| Design 主证据 | 2 | 1 | architecture、capability、module、API、software system |
| Statement provenance | 7 | 7 | 本轮不改动；保留为后续独立查询计划缺口 |

所有规则都只使用已经物化的 kind、source、provider、path、language、name、confidence 和
identity 字段，并以稳定字段完成 tie-break。没有增加 query limit、读取 live source、枚举
fixture/repository/query/path/symbol，也没有把无界排序引入 statement 全 scope 热路径。

最终 score 从基线 `0.990520666` 提升至 `0.998940610`；accuracy 从 `0.978456059`
提升至 `0.997592295`，foundational capability 从 `0.968750000` 提升至 `1.000000000`，
competitive capability 从 `0.988162119` 提升至 `0.995184591`。semantic/vector、performance
和 stability 均保持 `1.0`。

## 3. Release-binary 自迭代证据

执行入口为：

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

最终报告使用 `target/release/relay-knowledge`，`cached_home=false`，隔离 evaluation home，
并发上界为 global 16、repository 8、query 16。368/368 gates、132/132 cases、307 个
commands 全部通过，共记录 80 个 metrics；`score_accepted=true`、
`adoption_status=would_accept`，manual evaluation 没有创建 commit。

| 关键 metric | 实测 | 预算 | 结果 |
| --- | ---: | ---: | --- |
| Release build | 277 ms | 180,000 ms | PASS |
| Software fixture cold index | 651 ms | 15,000 ms | PASS |
| Software fixture register + cold index | 701 ms | 18,000 ms | PASS |
| Software query p50 | 80 ms | 100 ms | PASS |
| Software query p95 | 90 ms | 250 ms | PASS |
| C syntax query p95 | 180 ms | 180 ms | PASS，触及上界 |

三轮候选复测没有放宽预算，也没有丢弃失败样本：

| 报告 | 状态 | Software cold / p50 / p95 | 拒绝原因 |
| --- | --- | --- | --- |
| `manual-evaluate-1788132101088846588-0-1415938` | rejected | 427 / 83 / 98 ms | cold release link 230,130 ms，超过 180,000 ms |
| `manual-evaluate-1788132525019340332-0-1427696` | rejected | 575 / 85 / 93 ms | 无关 C fixture p95 184 ms，超过 180 ms |
| `manual-evaluate-1788132786594001932-0-1437486` | would_accept | 651 / 80 / 90 ms | 无 reject reason；所有预算通过 |

前两轮报告 SHA-256 分别为
`f2cb8001954bbc305345968ebdfd5361a4f1c66fbb641c7172a213a8877af6df` 和
`533f14bdb926a27e41c28477492ac05de1addc7a4cdc00986e50b8b07bb89e45`。
基线报告 `manual-evaluate-1788131353759667735-0-1401277.json` 的 SHA-256 为
`dfba514d400bb6374bc7344d2fd717f4adb8faab770ad86277d152ca77075a2e`。

`agent_workflows` 与 `research_judge` 因本轮只选择 performance category 而未执行；这两个
suite 的 skipped 状态不能被 368 个已通过 gate 替代。

## 4. Owner 测试、覆盖率与工作树边界

新增 owner 测试直接固定 API code、Kubernetes resource、systemd service definition、
architecture/capability/module 和 nested-document topic 的相对顺序。聚焦排序测试 16/16
通过；架构边界聚焦测试 6/6 通过。

全量覆盖率命令为：

```bash
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

结果为 153,584 行中 missed 15,340 行，line coverage `90.01%`，通过 90% 硬门禁。
同轮 library target 为 3,768 passed、1 ignored，benchmark target 为 1 passed。

覆盖率执行还暴露了一个与产品排序无关、但会妨碍真实 dirty-worktree 验证的测试问题：旧的
行数预算测试直接读取 `git ls-files` 返回的缓存路径，Knowledge Map 更新删除旧 shard 后会因
该路径已不存在而失败。修复后的边界统一枚举“现存的已跟踪与未跟踪常规文件”，忽略只存在于
Git index 的已删除路径，但其他 metadata I/O 错误仍 fail closed。新测试在隔离 Git 仓库中同时
固定 retained、deleted 和 untracked 三种路径行为，因此没有削弱文件长度门禁。

最终共享工作树还执行以下独立门禁；它们验证自迭代报告生成后新增的验证文档、地图更新和
测试基础设施修复，不反向扩大评估 patch 的范围：

- `cargo fmt --all -- --check`：PASS。
- `cargo clippy --all-targets --all-features -- -D warnings`：PASS。
- `cargo test --all-targets --all-features`：PASS。
- `python3 tools/docs/check_docs.py`：PASS。
- CodeSpec 与 Knowledge Map validation：PASS。
- `git diff --check`：PASS。

最终全量测试汇总为 library 3,768 passed、1 ignored，benchmark 1/1 passed，integration
156/156 passed；该命令在 Knowledge Map v20 的新增、替换和已删除 shard 同时存在的工作树上
完成，直接验证了修复后的路径枚举边界。

## 5. 代码图最终一致性边界

基线 HEAD 的持久化代码图固定到 scope `git_snapshot:cd18dbccb30f1b8a` 与 commit
`6e78bdbac22e1a0875cee2b13434baffd3b52a17`，状态为 fresh，包含 2,426 files、
42,557 symbols、229,444 references 和 25,935 chunks。快照绑定的 context、software、
business、architecture 与 dependency view 均可读取；context 把本轮主题路由到第 21 章架构、
software request contract 和真实 software-global case。该历史索引仍显式报告 20 个 degraded
files，不能把它描述为零降级索引。

在本记录生成时，最终 worktree 通过以下 CLI 请求增量索引：

```bash
relay-knowledge repo index relay-knowledge-reference --ref worktree --format json
relay-knowledge repo index-worker --task-id code-index-task:1a8b0c09a0d91999 --format json
```

初次 attempt 和一次遵守 retry backoff 的 durable worker 复试都在相同边界失败：direct
snapshot 超出 writer quantum，要求使用 checkpointed full-index pipeline。任务保持
`retrying`、attempt count 2，既有 fresh HEAD scope 未被覆盖。没有增大 512-file/16 MiB/
150,000-row 资源预算，没有绕过 lease/checkpoint，也没有直接修改 SQLite。因而本记录不声称
最终 worktree code graph fresh；最终变更由 snapshot-bound HEAD 路由、有界 `git diff` 审阅、
owner tests 和全量门禁共同验证。以上内容保留 B.14 当时实际观测到的历史状态。后续
[Durable Worktree Delta 与固定身份查询验证记录](15-durable-worktree-delta-and-pinned-query-2026-08-31.md)
实现缺失的有界切换并记录真实 worktree 成功回放；它取代“问题仍开放”的当前结论，但不改写
早期证据。

## 6. 结论与未关闭风险

`software-global-typed-evidence-priority` 在本记录限定的 focused performance 范围内采纳：
五类目标证据都从非首位提升到第 1 位，全部 case/gate 和未调整预算保持通过，查询 p95 仍远低于
software fixture 的 250 ms 上界。采纳的是可泛化的物化证据排序，不是某个 fixture 的名称或
路径特判。

本记录不关闭 statement rank 7。对最多 524,288 条 statement 的查询，直接加入全 scope
`CASE` sort 可能放大 SQLite 热路径成本；后续改进必须先给出有界索引或查询计划以及相同
performance case 的回归证据。最终轮 C syntax p95 恰好等于预算，且前两轮存在独立预算波动，
也应继续作为稳定性监测信号。

本记录不是整体 release-readiness 证明，也不替代 exhaustive、Kubernetes、browser、package、
service 或跨平台门禁。整体准备度仍以
[文档与自迭代准备度验证记录 2026-08-18](13-documentation-self-iteration-readiness-2026-08-18.md)
及其后续专门记录为准。

---

导航: 上一条:
[13. 文档与自迭代准备度验证记录](13-documentation-self-iteration-readiness-2026-08-18.md)
| 索引: [验证与审计记录](README.md)
| 下一条: [15. Durable Worktree Delta 与固定身份查询验证记录](15-durable-worktree-delta-and-pinned-query-2026-08-31.md)
