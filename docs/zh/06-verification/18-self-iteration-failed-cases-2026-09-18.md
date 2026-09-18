# 自迭代失败用例修复验证 2026-09-18

[中文](18-self-iteration-failed-cases-2026-09-18.md) | [English](../../en/06-verification/18-self-iteration-failed-cases-2026-09-18.md)

## 范围与基线

本次在同步远端后的 `main fcedd99e` 上修复 fast 自迭代的四个失败用例。
基线使用 release 产品二进制、`--profile fast --jobs 8 --repo-jobs 4 --query-jobs 8`，
通过 396/399 gates 与 128/132 cases；失败项是
`software_global_statements_keep_complete_provenance`、
`repository_map_v4_keeps_typed_cross_dimension_provenance`、
`java_class_member_callees` 和 `c_syntax_callers_function_pointer_read`。
项目别名查询 p95 为 353 ms，超过既有 150 ms 预算。
基线报告为本机 `manual-evaluate-1789692382483866536-0-2568916.json`；
它只证明该次环境与并发参数下的结果。

## 修复与不变量

快照物化与 SQLite durable finalization 共用调用名规则：已解析调用采用目标
symbol 名；未解析调用采用 parser 的成员名，只有源码限定名包含较短的接收者
提示时才保留源码形式。较长的字段接收者或下标表达式继续存在 `target_hint` 中，
并用于查询结果 excerpt。这样 C 函数指针字段 `read` 与 Java 成员 `println`
可由现有调用边索引命中，同时保留 Java 未知类型及跨语言调用的显示形式。
两个软件投影用例的版本断言与产品当前 schema 版本 9 对齐；provenance、
ontology 与 completeness 断言保持不变。

第一轮修复评估还暴露两处 evaluator 合同差异：Java callee 用例原先要求
`processItem calls println`，而已有集成测试要求保留源码表达式
`processItem calls System.out.println`；现在统一为后者，仍要求第一名、
精确路径和 call graph 层。软件依赖投影按包含 scope 的 opaque component ID
分页，十项结果中目标事实的序位会随 scope 变化；原先的前六名断言与该
分页合同不符。用例改为检查十项范围内的完整事实，并继续核对声明、锁定
版本、使用关系、生态、证据路径及状态，没有删减事实断言。

未扩大候选窗口、队列、批次、租约或重试预算；reference、call、FTS 写入、
持久任务检查点、单仓库单写者与发布屏障继续由现有路径执行。
本次没有安装、配置、服务或数据迁移变化。

## 验证结果

环境为 Ubuntu 24.04.4、Linux x86_64、16 个逻辑 CPU、Rust 1.97.1。
验证时本地 `main` 与 `origin/main` 均为
`fcedd99ef1db79f3e56ef95854b74403a759d395`；修复当时尚在工作树，
报告的 patch digest 指向该基线上的候选变更。
release 产品二进制为 `target/release/relay-knowledge`，SHA-256 为
`4afd450d9bcc91024eb671ff43f8d4bdb207f832a218a7403c32ae78503a0601`。

```bash
cargo build --release --bin relay-knowledge
./self-iterate.sh evaluate --use-current-candidate --profile fast --jobs 4 --repo-jobs 1 --query-jobs 2
```

上面的并发参数与仓库 benchmark CI 的 fast 作业一致；预构建 release 二进制
使 evaluator 中的 `cargo_build_release_ms` 为 517/180,000 ms，
不能用该缓存编译耗时估计全新编译。最终报告
`manual-evaluate-1789698241433448978-0-3022855.json` 的 SHA-256 为
`5ed58b19c72e4a0a6276f856211895e7c6b1ce8578aee3a51633cb4885472456`；
候选 patch SHA-256 为
`056c2ed30a4e1e846d12edddce726b3a65d45df8297a9ddb9e4b803b2be9c199`。
报告与 patch 原件在本机忽略的 `.git/relay-knowledge-self-iteration/` 内；
上述摘要及本记录是可随仓库发布的证据索引，记录后补的验证文字不在候选 patch 中。

最终报告状态为 `would_accept`，score `0.9872394591255969`：
399/399 gates、132/132 selected cases、332 条命令合同、86 项指标；
18 个仓库工作量被选中。唯一非零命令是按合同预期拒绝无效 C++ 语言筛选的
registration negative case，退出码 1，不是质量门禁失败。三个 suite
`file_fixtures`、`agent_workflows`、`research_judge` 未在 fast profile 执行。
四个基线失败用例均通过：Java callee 与 C 函数指针 caller 都位列第一，
两个软件投影用例保留完整 provenance 及类型化跨维度断言。

| 固定指标 | 实测 / 预算 |
| --- | ---: |
| 项目别名查询 p95 | 56 / 150 ms |
| 非标准布局查询 p95 | 113 / 200 ms |
| Java class calls 查询 p95 | 77 / 2,000 ms |
| C syntax 查询 p95 | 126 / 180 ms |
| `relay-teams` 冷索引 | 40,682 / 45,000 ms |

完整 `cargo test --all-targets --all-features` 在最后的空 `target_hint`
边界保护前通过 4,502 项单元测试、171 项集成测试及 1 项额外测试，
0 失败、1 项既有 ignored fixture。随后新增的空提示分支以定向单测通过，
最终代码的 `cargo test --test relay_knowledge --all-features` 再通过
171/171 项集成测试。
最终代码的 `cargo check --all-targets --all-features`、
`cargo clippy --all-targets --all-features -- -D warnings`、
`cargo fmt --all -- --check` 和 220 篇 Markdown 文档检查均通过。
最终代码的 `cargo llvm-cov --lib --bins --all-features --fail-under-lines 90`
通过：4,502 项单元测试通过、1 项既有 ignored fixture，
行覆盖率 **90.05443%**（156,674/173,977 行），高于 90% 门禁。

本记录只覆盖本机 fast profile 与上述本地质量门禁。未执行 full/exhaustive、
浏览器集成测试、Miri、AddressSanitizer、跨平台打包或发布验收；
此前 8/4/8 并发的探索轮曾超出查询 p95 预算，不能用本次 4/1/2
通过结果推断该高并发配置已合格。

---

导航：[验证记录](README.md) | 上一篇：[17. 冷索引无损优化验证](17-lossless-cold-index-2026-09-17.md)
