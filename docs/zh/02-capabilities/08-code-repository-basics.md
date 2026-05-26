# 代码仓库基础能力

[中文](./08-code-repository-basics.md) | [English](../../en/02-capabilities/08-code-repository-basics.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

代码仓库基础能力让用户把 Git 仓库注册为一等 source，按 clean snapshot 索引，并通过同一 application service 查询代码上下文。

## 用户可见行为

```bash
relay-knowledge repo register /path/to/relay-knowledge --path src --language rust
relay-knowledge repo index relay-knowledge --ref HEAD --format json
relay-knowledge repo query relay-knowledge --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo query relay-knowledge --query serde --kind sbom --ref HEAD --format json
relay-knowledge repo update relay-knowledge --base main --head HEAD --format json
relay-knowledge repo status relay-knowledge --format json
```

省略 `--alias` 或传入空 alias 时，注册会使用解析后的 Git root 目录名作为稳定仓库 alias。agent 首次注册项目时应优先使用这个默认值，让后续 session 复用同一索引；`--alias` 仍可作为显式覆盖。

`repo query` 支持 `--limit`、`--ref`、可重复 `--path`、可重复 `--language` 和 freshness policy。

`definition`、`references` 和 `hybrid` 查询会先使用已索引代码图与 SQLite FTS；当这些层存在明确召回缺口时，才在已索引 commit 的候选文件上执行有界内部 exact-text source fallback。兜底结果以 `lexical` 和 `text_fallback` layer 暴露，不能替代 resolved reference/call/import edge。

冷启动 full `repo index` 会返回 queued task handle，由后台 code-index worker 在 lease 下执行解析和 SQLite 写入。`repo status` 暴露 active task、checkpoint 进度和 retention 摘要；worker 成功后会保留 active scope、最近两个完成 scope 和未完成任务 scope。

## 竞争力特性

仓库索引绑定 repository id、resolved commit、tree hash、path filter 和 language filter。相同树可以复用 scope，rebase 或 force-moved head 需要新索引，dirty worktree 通过 worktree overlay 显式建模。

代码源码布局识别不再局限于顶层 `src/` 目录。索引器和 import 解析会识别
`external_deps/`、`packages/`、`modules/`、`plugins/`、`extensions/`、
`Sources/`、`lib/` 等目录下的真实源码，也支持
`modules/<name>/src/main/java` 这类嵌套 JVM source root。普通 `vendor/`
和 `third_party/` 这类重型依赖转储仍由 source preset 默认排除，除非用户通过
path filter 显式 opt in。

例如，混合布局仓库可以注册 `--path external_deps/python_sdk`、
`--path plugins/example.com/nonstandard/session` 或
`--path modules/payment/src/main/java` 来索引这些授权源码；如果确实需要
`vendor/pkg` 或 `third_party/pkg` 中的源码，必须显式传入对应 `--path`，避免把大
容量依赖转储误纳入默认 scope。

## 命令/API 入口

窄类型包括 `symbol`、`definition`、`references`、`callers`、`callees`、`imports` 和 `sbom`。`--kind hybrid` 同时检索 symbol、definition、reference、import、call 和 chunk。`--kind sbom` 检索索引期从 Cargo、npm、Go、Python、Maven BOM、Gradle 和 Conan manifest/lockfile 提取的依赖清单；它是本地 inventory，不执行包管理器、不解析传递依赖，也不做漏洞或许可证合规分析。调用图检索会对跨语言调用目标做规范化：C/C++ 互调、Go cgo 的 `C.*` 调用和 Rust FFI/bindings 路径会解析到同仓库里的 C/C++ 符号；当 header 声明、FFI scoped 声明和实现共享同一个叶子名时，唯一实现优先作为 resolved call target，signature-only 声明不会阻断后续实现候选。`C.*` leaf fallback 只用于 Go cgo 文件，call target 只会解析到 callable 符号。普通命名空间调用不会只按叶子名合并，例如 `module::connect` 或 `module::sys::connect` 不会被当作 `connect` 的 FFI 调用别名；已解析 FFI 调用保留原始 scoped hint，因此 `rk_c_decode` 和 `ffi::rk_c_decode` 查询都能匹配对应调用边。

`repo feature-flags` 是独立只读入口，用于枚举或过滤 indexed scope 内的配置驱动特性开关图。它返回按开关分组的配置来源和 `defines_config`、`reads_config`、`guards_code` 关系，而不是把 feature flag 作为普通 `repo query --kind` 值。

## 降级与诊断

Unsupported、非法 UTF-8、二进制或超大文件会降级为 text-only chunk。包含不可恢复 error node 的 syntax tree 以 partial 状态索引，并记录 file diagnostic；C/C++ 宏密集文件如果 error node 局限在 macro expansion、有界 preprocessor directive 或 decorator declaration，且仍能抽取可靠 symbol、reference 或 import，可以保持 parsed。外部依赖源码缺失只作为 unresolved edge coverage metadata 暴露，不写入 `degraded_reason`。source fallback 候选路径、候选文件、物化字节或单行长度预算问题只降级 query-time exact-text fallback，响应必须保留 `degraded_reason`，已有结构化命中仍然可用。

## 关联架构章节

- [Source Scope 模型](../03-architecture-specs/04-source-scope-model.md)
- [Tree-sitter 抽取与增量索引](../03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)
- [代码检索排序与影响分析](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

导航: 上一章: [7. 多模态证据能力](07-multimodal-evidence-capability.md) | 下一章: [9. 代码图竞争力特性](09-code-graph-competitive-features.md)
