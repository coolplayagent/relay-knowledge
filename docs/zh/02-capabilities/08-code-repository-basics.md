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

`definition`、`references` 和 `hybrid` 查询会先使用已索引代码图与 SQLite FTS；当这些层存在明确召回缺口时，才在已索引 commit 的候选文件上执行有界 `ripgrep` 兜底。兜底结果以 `lexical` 和 `text_fallback` layer 暴露，不能替代 resolved reference/call/import edge。

冷启动 full `repo index` 会返回 queued task handle，由后台 code-index worker 在 lease 下执行解析和 SQLite 写入。`repo status` 暴露 active task、checkpoint 进度和 retention 摘要；worker 成功后会保留 active scope、最近两个完成 scope 和未完成任务 scope。

## 竞争力特性

仓库索引绑定 repository id、resolved commit、tree hash、path filter 和 language filter。相同树可以复用 scope，rebase 或 force-moved head 需要新索引，dirty worktree 通过 worktree overlay 显式建模。

代码源码布局识别不再局限于顶层 `src/` 目录。索引器和 import 解析会识别
`external_deps/`、`packages/`、`modules/`、`plugins/`、`extensions/`、
`Sources/`、`lib/` 等目录下的真实源码，也支持
`modules/<name>/src/main/java` 这类嵌套 JVM source root。普通 `vendor/`
和 `third_party/` 这类重型依赖转储仍由 source preset 默认排除，除非用户通过
path filter 显式 opt in。

## 命令/API 入口

窄类型包括 `symbol`、`definition`、`references`、`callers`、`callees` 和 `imports`。`--kind hybrid` 同时检索 symbol、definition、reference、import、call 和 chunk。调用图检索会对跨语言调用目标做规范化：C/C++ 互调、Go cgo 的 `C.*` 调用和 Rust FFI/bindings 路径会解析到同仓库里的 C/C++ 符号；当 header 声明、FFI scoped 声明和实现共享同一个叶子名时，唯一实现优先作为 resolved call target。`C.*` leaf fallback 只用于 Go cgo 文件，call target 只会解析到 callable 符号。普通命名空间调用不会只按叶子名合并，例如 `module::connect` 或 `module::sys::connect` 不会被当作 `connect` 的 FFI 调用别名；已解析 FFI 调用保留原始 scoped hint，因此 `rk_c_decode` 和 `ffi::rk_c_decode` 查询都能匹配对应调用边。

`repo feature-flags` 是独立只读入口，用于枚举或过滤 indexed scope 内的配置驱动特性开关图。它返回按开关分组的配置来源和 `defines_config`、`reads_config`、`guards_code` 关系，而不是把 feature flag 作为普通 `repo query --kind` 值。

## 降级与诊断

Unsupported、非法 UTF-8、二进制或超大文件会降级为 text-only chunk。包含 error node 的 syntax tree 以 partial 状态索引，并记录 file diagnostic。`rg` 缺失、超时或候选预算耗尽只降级 query-time exact-text fallback，响应必须保留 `degraded_reason`，已有结构化命中仍然可用。

## 关联架构章节

- [Source Scope 模型](../03-architecture-specs/04-source-scope-model.md)
- [Tree-sitter 抽取与增量索引](../03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)
- [代码检索排序与影响分析](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

导航: 上一章: [7. 多模态证据能力](07-multimodal-evidence-capability.md) | 下一章: [9. 代码图竞争力特性](09-code-graph-competitive-features.md)
