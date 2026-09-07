# 地图与图关系存储验收 2026-09-07

[中文](../../zh/06-verification/16-map-graph-storage-acceptance-2026-09-07.md) | [English](../../en/06-verification/16-map-graph-storage-acceptance-2026-09-07.md)

> 日期：2026-09-07
> 基线：`a93406aeaa79b382d6a8357fb3ad986df472bbe5`
> 范围：Repository Map 治理、软件关系、持久索引、跨维度来源证据、存储和索引性能。

## 1. 存储决策

兼容关系不携带独立事实：文件到主题、依赖、SDK 和配置的关系可以从绑定同一
snapshot 的源行重建。Schema 8 删除逐条关系写入和反复 OFFSET 扫描物化，
读取通过一个 SQL 投影应用 scope、path、language 和结果预算；持久化
relationships 阶段逐行复用 domain 校验，包括未选中的配置重复项，再记录精确
计数；无效事实不能发布 fresh checkpoint。返回值保留稳定 identity、evidence、未解析目标和
graph version。配置的重复 identity 按最高置信度、最宽结束行和最小 usage ID
确定唯一结果，不同 relationship kind 保持独立。

类型化 ontology statement 继续持久化来源、事实状态和 reconciliation 语义。
代码调用、引用、导入和检索索引保持完整。旧兼容表暂留，以支持历史 import 和
retention cursor；成功刷新 scope 后回收其旧行，打开数据库不会全量删除或
压缩 SQLite。升级与回滚见[安装、发布与升级合同](../03-architecture-specs/19-installation-release-and-upgrade.md)。

## 2. 验收 Cases

| 要求 | 证据 |
| --- | --- |
| 双地图兼容 | 当前源码与已发布 1.1.17 CLI 均验证 CodeSpec、Knowledge Map v4；8 个 CLI 治理合同覆盖目录可见性、source 顺序、保留 route 和保留历史。 |
| 大规模主题无重复写入 | Owner 测试在新增 4,096 个 topic 后计数 4,101 条关系，读取有界 1,000 条窗口，要求 SQLite write/page counter 不变且兼容表 0 行；另有 513-topic refresh 跨过旧 512 行分页边界。 |
| 恢复路径保留优化 | 12,000 文件性能 case 从 fenced software publication 阶段继续，报告 12,000 条 SDK 关系，完成 checkpoint 前必须确认兼容表 0 行。 |
| 迁移可恢复 | 注入 ontology insert 失败后旧行清理整体回滚，成功刷新只回收自身 scope；schema 初始化标记旧状态 stale，但不删除旧 payload。 |
| 高维 map 索引 | 4 个 repository-map case 验证 8 个授权 topic、孤立 shard 排除、软件维度组合和类型化 `documents`、`derived_from`、`depends_on`、`deploys`、`runs_as` 来源。 |
| 增量变化保留不可变历史 | CLI integration 删除 source，检查空 route 与替换 shard，排除已退役 shard 的关系，验证图端点，并回放完全相同的 base-commit topic 和 relationship。 |
| 业务和架构绑定同一快照 | Bootstrap integration 检查 authored business mapping、software/context 与 architecture/business-domain view 的固定 commit 和 source scope。 |

存储 case 进入 fast self-iteration 强制门禁。配置去重、无效 public feature-flag
发布、Unicode 空白字段、未解析 SDK hint、先过滤后
limit、跨 scope 排除、无效事实及 SQL 错误均有 focused owner tests。

## 3. 真实仓库快照

隔离 runtime 通过一个持久任务、5 个 batch 索引基线仓库。Completed checkpoint
包含 2,471 文件、44,292 symbol、237,775 reference、26,554 chunk；software、
business、architecture、business-domain 和 context 均读取同一个固定 commit。

Software projection 包含 2,409 topic、12,832 entity、15,240 typed statement，
报告 9,982 条兼容关系。只读数据库检查确认兼容表 0 行，并完整包含 root 授权的
6 个地图主题：architecture、benchmarks、business-knowledge、cli、
release-documentation、software-model。Statement 来源完整度为 10,000 basis
points，ontology diagnostic 为 0。

此快照有 20 个 `text_only` 文件，主要是 CSS、lockfile、ignore rule 和 previous
map root。因此即使 snapshot 已完成且不 stale，投影仍明确报告 degraded freshness。
仓库 authored business glossary 无 domain/term，索引也没有 IaC resource；业务和
部署维度的正向证据由确定性 fixture 提供。Context 有界且报告 truncated。
普通架构 Markdown 不是 OKF concept bundle，不能充当成功的 `repo graph`
neighborhood fixture。这些是明确的证据边界，不会把缺少的事实静默宣称为完整。

## 4. 复现

```bash
cargo test --lib software_relationship_storage -- --nocapture
cargo test --lib code_index_persistence_performance_suite -- --nocapture
cargo test --test relay_knowledge knowledge_development_loop
cargo test --manifest-path tools/self_iteration/Cargo.toml
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

存储字节测量须使用独立全新 runtime home、同一个 Git fixture，以及经 hash 验证
不同的 baseline/candidate 二进制。在解读耗时前，必须检查 completed checkpoint、
projection schema 7/8、非兼容图事实计数不变、有界兼容关系 payload 相同。
用 SQLite `dbstat` 统计表及其索引的占用页；空表仍占 root page。新旧构建交替
运行，冷索引与重复有界查询分别计时。

## 5. 存储与索引实测

Release 对比在生成的 `repository_map_graph_v4` fixture 上增加 256 个 Markdown
文件，每个含 16 个 heading，以及 256 个 Rust 文件，每个含 4 个 function。
两个构建分别在全新 runtime home 索引同一 commit
`c53e0fa0c0a28e63d3924a84186a44d98e9b7a19`，使用本地 semantic/vector backend。
每个构建交替运行 5 轮，每轮执行 7 次 limit 100 的关系查询。下表为中位数，
耗时只代表此 workload，不宣称所有仓库都具有相同比例的收益。

| 指标 | Baseline | Schema 8 | 减少 |
| --- | ---: | ---: | ---: |
| 兼容关系持久行数 | 4,120 | 0 | 100% |
| 兼容表及索引占用字节 | 1,507,328 | 12,288 | 99.18% |
| 数据库分配字节 | 35,438,592 | 33,943,552 | 4.22% |
| 冷索引 | 1,355.2 ms | 1,332.5 ms | 1.67% |
| 有界关系查询 | 72.8 ms | 70.5 ms | 3.08% |

两个构建均保留 546 文件、5,860 code symbol、2,048 reference、2,048 call、
1,314 chunk、4,118 topic、8,776 ontology entity 和 12,893 typed statement。
检查的 12 张非兼容表计数、有界关系 payload 均相同；都报告 4,120 条兼容关系和
completed、非 stale 的 snapshot。

Baseline 二进制 SHA-256：
`f2b2a3fe38bc77c620ca0c1e536a80bdce607475221e2028b9234d3d97de3904`。
Candidate 二进制 SHA-256：
`e913ac3ee26f7a1cddec2fefc6f89770fd3ca0eb099c70c57ff74a4cb3c778ed`。
Baseline 源码已与基线 revision 的全部 1,825 个 tracked Cargo/source 文件逐字节
比较。此前发现的新旧二进制 hash 相同的一组产物已作为无效对比证据排除；
与测试编译重叠的 pilot 轮次也不计入上表耗时。

## 6. 本地质量门禁

| 门禁 | 结果 |
| --- | --- |
| Cargo check、Clippy，全 targets/features | 通过，warnings denied |
| Rust formatting、文档检查 | 通过，216 个 Markdown 文件 |
| Rust unit tests | 3,908 通过；1 个 subprocess fixture 按设计 ignored，由父测试调用 |
| Rust integration tests | 157 通过 |
| 确定性 benchmark target | 1 通过 |
| Self-iteration harness unit tests | 240 通过 |
| Current/stable map compatibility 与 CLI contracts | 两个 reader 兼容，8 个合同通过 |
| Release map graph matrix | 4 个 case 通过 |
| 首次本地 LLVM coverage，全 targets/features | `9d2363394b` 行覆盖率 90.09%，90% 门禁通过；最终评审修复仍须通过 PR coverage 门禁 |
| Playwright Chromium browser test | 1 通过 |
| Fast/performance evaluation | `would_accept`；392/392 gate、139/139 case、327 command contract、86 metric；performance 和 stability 均为 1.0 |

完整评估使用 `--jobs 2 --repo-jobs 1 --query-jobs 2`，耗时 134,542 ms。
报告 `manual-evaluate-1788784416972397547-0-518296.json` 的 SHA-256 为
`06df94a4a92af9051198d1c8c77fa3d91193fa448ae41b9f0a9278dc035c19a8`。
生成的 map fixture 通过 harness 实际评分路径的全部 4 个 case；evaluate 模式没有
自动创建 commit。

Chromium 已通过 Playwright 安装并使用现有 Linux library 成功执行；
`--with-deps` 的系统依赖安装尝试需要当前不可用的 sudo 凭据。Browser test 使用
API fixture，不承担 SQLite 存储证明。Miri 和 AddressSanitizer 仍是独立 PR
必需 job；此处本地结果不宣称通过 nightly 门禁或完成跨平台发版认证。
