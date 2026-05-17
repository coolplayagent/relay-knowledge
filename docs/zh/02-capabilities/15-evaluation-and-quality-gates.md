# 评估与质量门禁

[中文](./15-evaluation-and-quality-gates.md) | [English](../../en/02-capabilities/15-evaluation-and-quality-gates.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

评估能力确保基础功能和竞争力特性不是只在演示中成立。它覆盖 GraphRAG fixture、代码检索 E2E、浏览器集成和文档新鲜度。

## 用户可见行为

- Rust evaluation harness 覆盖 exact fact、multi-hop、temporal、negative rejection、stale index、ambiguous entity 和 code impact。
- relay-teams 和 Linux 代码图检索准确性记录保留在验证卷。
- Browser integration test 验证 Web diagnostics、GraphRAG readiness、operation composer、索引表、运行时面板和移动端布局。

## 竞争力特性

质量门禁把检索准确性、代码图结构、Web 操作和文档链接放在同一工程约束下，避免“功能已写但不可验证”。

## 命令/API 入口

```bash
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

## 降级与诊断

测试失败不能通过枚举已知 query、path、symbol 或 fixture 特例修复。优化必须来自通用 ranking signal、索引策略、数据结构、query planning 或并发边界。

## 关联验证记录

- [文档书架刷新审计](../06-verification/01-documentation-book-refresh-2026-05-17.md)
- [relay-teams E2E 验证](../06-verification/04-relay-teams-e2e-2026-05-14.md)
- [Linux 代码图检索准确性测试](../06-verification/06-code-graph-retrieval-accuracy-linux-2026-05-15.md)

---

导航: 上一章: [14. 运维与 Worker 能力](14-operations-and-worker-capabilities.md)
