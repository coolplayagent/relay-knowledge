# 代码仓库基础能力

[中文](./08-code-repository-basics.md) | [English](../../en/02-capabilities/08-code-repository-basics.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

代码仓库基础能力让用户把 Git 仓库注册为一等 source，按 clean snapshot 索引，并通过同一 application service 查询代码上下文。

## 用户可见行为

```bash
relay-knowledge repo register /path/to/repo --alias core --path src --language rust
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo query core --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo status core --format json
```

`repo query` 支持 `--limit`、`--ref`、可重复 `--path`、可重复 `--language` 和 freshness policy。

## 竞争力特性

仓库索引绑定 repository id、resolved commit、tree hash、path filter 和 language filter。相同树可以复用 scope，rebase 或 force-moved head 需要新索引，dirty worktree 通过 worktree overlay 显式建模。

## 命令/API 入口

窄类型包括 `symbol`、`definition`、`references`、`callers`、`callees` 和 `imports`。`--kind hybrid` 同时检索 symbol、definition、reference、import、call 和 chunk。

## 降级与诊断

Unsupported、非法 UTF-8、二进制或超大文件会降级为 text-only chunk。包含 error node 的 syntax tree 以 partial 状态索引，并记录 file diagnostic。

## 关联架构章节

- [Source Scope 模型](../03-architecture-specs/04-source-scope-model.md)
- [Tree-sitter 抽取与增量索引](../03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)

---

导航: 上一章: [7. 多模态证据能力](07-multimodal-evidence-capability.md) | 下一章: [9. 代码图竞争力特性](09-code-graph-competitive-features.md)
