# 文档刷新审计 2026-05-17

[中文](./documentation-refresh-audit-2026-05-17.md) | [英文](../../en/06-verification/documentation-refresh-audit-2026-05-17.md)

本审计记录 2026-05-17 针对代码仓库检索自迭代提交的文档刷新。对应代码变更集中在 SQLite 代码检索候选选择、ranking、call excerpt 查询计划、call FTS finalize 写入，以及 HTTP graceful shutdown 单测稳定性。

## 本轮刷新内容

| 范围 | 刷新内容 |
| --- | --- |
| README | 补充代码仓库 FTS 候选窗口、path filter 下推、BM25 排序、identifier-aware scoring、call excerpt line containment 和声明块 ranking 摘要。 |
| 代码仓库能力文档 | 记录 symbol/reference/call/import/chunk FTS 候选表行为、fuzzy symbol OR 召回、graph edge all-term 召回、声明 chunk bonus、call chunk lookup index 和集合式 call FTS finalize。 |
| 代码仓库架构规格 | 将词法候选窗口约束写入架构契约：effective path filters 必须在 FTS LIMIT 前生效，graph edge 不能无契约放宽到 OR 召回，ranking bonus 只能作用于已召回候选。 |
| 自迭代 benchmark 记录 | 保留 2026-05-16 的候选采纳记录、分数、case 变化、延迟指标、已知退化和优化说明，作为后续 Codex 迭代的长期记忆。 |
| HTTP 测试说明 | 当前文档无新增用户或运维行为；该改动只稳定 `serve_router_enforces_graceful_shutdown_timeout` 的测试同步边界。 |

## 已同步的实现行为

- FTS 候选窗口会同时应用 indexed scope path filters 与 request path filters，并在进入 Rust scoring 前按 BM25 取有界候选。
- Symbol 查询可用 OR term 做 fuzzy recall；reference、call、import 仍保持更窄召回，避免 graph edge fan-out 扩大。
- Scoring 支持 snake_case/CamelCase identifier part、多段 symbol name、调用方向上下文、相关 callee 名称、声明式 prototype chunk 和纯虚 interface chunk 的受限 bonus。
- Call excerpt 查询通过 `code_repository_chunks(source_scope, symbol_snapshot_id)` 索引和调用行包含条件定位实际调用点 chunk。
- Call FTS document 在 finalize 后由 `code_repository_calls` 集合重建，schema backfill 使用同一 content 字段集合。

## 验证命令

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

如果本轮变更进入 PR，还需要等待 GitHub Actions 的 `pr-checks` 结果，并处理 Codex/GitHub review 意见。
