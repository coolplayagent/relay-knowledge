# 评估与质量门禁

[中文](./15-evaluation-and-quality-gates.md) | [English](../../en/02-capabilities/15-evaluation-and-quality-gates.md)

> 文档版本: 2.2
> 编制日期: 2026-09-01
> 适用范围: 第二卷能力说明

## 能力定位

评估能力确保基础功能和竞争力特性不是只在演示中成立。它覆盖 GraphRAG fixture、代码检索 E2E、浏览器集成和文档新鲜度。

## 用户可见行为

- Rust evaluation harness 覆盖 exact fact、multi-hop、temporal、negative rejection、stale index、ambiguous entity 和 code impact。
- relay-teams 和 Linux 代码图检索准确性记录保留在验证卷。
- Browser integration test 验证 Web diagnostics、GraphRAG readiness、knowledge/code graph canvas、software ontology graph、冲突与 shape diagnostics、operation composer、索引表、运行时面板和移动端布局。

## 竞争力特性

质量门禁把检索准确性、代码图结构、Web 操作和文档链接放在同一工程约束下，避免“功能已写但不可验证”。

## 命令/API 入口

```bash
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

## Commit 与 Rust 深检门禁

Issue #358 采用分层合同落地，不让每次 Git commit 都重建 nightly 插桩产物：

| 门禁 | 日常 commit 证据 | deep/PR 证据 |
| --- | --- | --- |
| Cargo check | pre-commit 与 PR CI 执行 `cargo check --all-targets --all-features` | `./check.sh --deep` 在插桩前再次执行 |
| Clippy | pre-commit 与 PR CI 对所有 target/feature 拒绝 warning | deep profile 再次执行 |
| Tests | pre-commit 执行所有 target/feature；PR CI 拆分 UT 与集成测试 | library/binary tests 在 AddressSanitizer 下再次执行 |
| Miri | stable commit hook 不执行 | nightly 对 `domain::core::` 执行 strict provenance、symbolic alignment 与 deterministic concurrency |
| Sanitizer | stable commit hook 不执行 | Linux x86_64 CI 使用带插桩标准库的 nightly AddressSanitizer |
| Benchmark | pre-commit 的 `--all-targets` 已包含 | 独立确定性 benchmark jobs 与 `--deep` 诊断 |

普通 commit hook 固定使用仓库 stable 工具链。Miri 与 AddressSanitizer 依赖
nightly，并有显著编译或解释成本，因此作为必跑 pull-request jobs，同时提供显式
本地 deep profile：

```bash
rustup toolchain install nightly --profile minimal --component miri,rust-src
./check.sh --deep
```

Miri 只运行核心领域测试面，因为产品 SQLite 与网络边界使用 Miri 不支持的 FFI
或 host API。这是显式覆盖边界，不是跳过失败：普通测试继续覆盖这些路径，
AddressSanitizer 则在受支持的原生 target 上执行 library 与 binary tests。参见
[Miri 支持与 CI 指南](https://github.com/rust-lang/miri#using-miri)及
[Rust sanitizer target 与插桩合同](https://doc.rust-lang.org/stable/unstable-book/compiler-flags/sanitizer.html)。

## 降级与诊断

测试失败不能通过枚举已知 query、path、symbol 或 fixture 特例修复。优化必须来自通用 ranking signal、索引策略、数据结构、query planning 或并发边界。

## GitHub 自动化策略

仓库继续在 pull request 上执行确定性的文档、格式、Cargo check、Clippy、单元测试、
集成测试、benchmark、Miri、AddressSanitizer、架构、兼容性、覆盖率、构建、runtime
和浏览器门禁。Qodana 是可选云端诊断，仅允许通过
`workflow_dispatch` 手动执行；pull request 与 push 不再自动触发。外部服务 quota 或可用性
不能成为产品正确性的合并门禁。

Pull request 的 index-performance job 会先在独立 prerequisite step 中构建 release 产品，再启动
计时的 self-iteration workload。报告仍必须选择 `target/release/relay-knowledge`、通过增量 build
gate、完成 cold/incremental task，并满足所有已声明索引延迟预算。这样只把冷 runner 编译器波动
移出索引 runtime 信号，不会削弱编译或产品性能检查。

## 文件监听 (fs.watch) 验收

文件监听功能需要满足以下验收条件：

- **跨平台支持**：`notify` crate 集成，覆盖 Linux（inotify）、macOS（FSEvents）、Windows（ReadDirectoryChangesW）
- **事件去抖**：可配置 debounce 窗口（默认 3s）合并高频文件变更事件
- **内容哈希过滤**：`ContentHashCache`（FNV-1a）过滤无内容变化的保存操作
- **路径过滤**：自动忽略 `.git/`、`target/`、`node_modules/` 等目录和二进制文件
- **资源有界**：`max_watch_dirs` 限制最大监听目录数，防止 fd/inotify 资源耗尽
- **降级恢复**：监听失败时自动降级为 `Degraded` 状态，不影响查询热路径
- **诊断暴露**：watcher 状态通过 `service status` API 暴露（state、事件计数、降级原因）
- **持久化任务**：增量索引任务通过 `CodeIndexTaskSeed`（WorktreeOverlay 模式）进入持久化队列
- **Worker 兼容**：watcher 生成的 payload 可以反序列化为 `CodeIndexRequest`，worker claim `WorktreeOverlay` 任务时保留 payload 中的 ref selector
- **单元测试覆盖**：config 解析、路径过滤、确定性内容哈希淘汰、状态管理、动态 watch/unwatch、任务生成、事件丢弃诊断、worker overlay 任务执行、诊断序列化

## 关联验证记录

- [文档书架结构审计](../06-verification/05-documentation-book-structure-audit-2026-05-17.md)
- [relay-teams E2E 验证](../06-verification/01-relay-teams-e2e-2026-05-14.md)
- [Linux 代码图检索准确性测试](../06-verification/04-code-graph-retrieval-accuracy-linux-2026-05-15.md)

---

导航: 上一章: [14. 运维与 Worker 能力](14-operations-and-worker-capabilities.md)
