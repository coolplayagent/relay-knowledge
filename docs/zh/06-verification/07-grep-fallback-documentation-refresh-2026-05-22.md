# Grep 兜底文档刷新审计 2026-05-22

[中文](./07-grep-fallback-documentation-refresh-2026-05-22.md) | [English](../../en/06-verification/07-grep-fallback-documentation-refresh-2026-05-22.md)

本审计记录 2026-05-22 对代码检索 `ripgrep` 精确文本兜底的文档刷新。此次变更只更新文档，不改变 Rust、Web、CLI 或测试 harness 行为。

## 刷新范围

| 区域 | 刷新内容 |
| --- | --- |
| 用户手册 | 在使用指南总览、CLI 命令参考、代码仓库工作流和排障章节说明 `repo query` 的 `definition`、`references`、`hybrid` 查询会先走 tree-sitter/FTS，再按需使用有界内部 exact-text source fallback。 |
| 能力说明 | 在能力总览、混合检索、代码仓库基础和代码图竞争力章节补充 `text_fallback` provenance、缺失 `rg` 的降级语义和不能覆盖 resolved edge 的边界。 |
| 架构规格 | 在混合检索、Tree-sitter 索引和代码检索排序章节明确 fallback 继承 scope/path/language/freshness/authorization，运行在 blocking-worker 边界，并记录候选文件、物化字节、行长和 timeout 预算。 |
| 研究与基准 | 在 Tree-sitter 检索研究、实现落地路线、竞争力研究和 benchmark 目标中加入 grep fallback 的使用场景、风险、观测字段和回归原则。 |
| 书架索引 | 中英文文档书架增加本审计入口，便于后续回溯本次文档刷新。 |

## 关键约束

- `ripgrep` 兜底只补齐已索引 commit 上的精确源码行，不直接扫描当前脏工作树。
- 兜底命中必须标记 `lexical` 和 `text_fallback`；definition 兜底可以同时标记 `definition`。
- 兜底命中不返回 resolved edge confidence，也不能压过已有 exact symbol 或 resolved edge。
- 候选路径查询失败、候选文件预算耗尽、物化字节预算耗尽或单行长度限制只降级 exact-text fallback，并通过 `degraded_reason` 暴露。

## 验证命令

```bash
rg -n 'ripgrep|grep 兜底|text_fallback|exact-text fallback' docs/zh docs/en README.md
git diff --check
```
