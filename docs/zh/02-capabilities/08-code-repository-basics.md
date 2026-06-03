# 代码仓库基础能力

[中文](./08-code-repository-basics.md) | [English](../../en/02-capabilities/08-code-repository-basics.md)

> 文档版本: 2.0
> 编制日期: 2026-05-30
> 适用范围: 第二卷能力说明

## 能力定位

代码仓库基础能力让用户把 Git 仓库或非 Git source directory 注册为一等 source，按 clean snapshot 或 filesystem synthetic snapshot 索引，并通过同一 application service 查询代码上下文。

## 用户可见行为

```bash
relay-knowledge repo register /path/to/relay-knowledge --path src
relay-knowledge repo index relay-knowledge --ref HEAD --format json
relay-knowledge repo query relay-knowledge --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo query relay-knowledge --query serde --kind sbom --ref HEAD --format json
relay-knowledge repo update relay-knowledge --base main --head HEAD --format json
relay-knowledge repo status relay-knowledge --format json
```

省略 `--alias` 或传入空 alias 时，注册会使用解析后的 Git root 或 filesystem root 目录名作为稳定仓库 alias。agent 首次注册项目时应优先使用这个默认值，让后续 session 复用同一索引；`--alias` 仍可作为显式覆盖。

`repo query` 支持 `--limit`、`--ref`、可重复 `--path`、可重复 `--language` 和 freshness policy。`repo register` 会拒绝 language filter，确保混合语言仓库保留完整语言面；需要收窄结果时在查询期使用 `--language`。

`definition`、`references` 和 `hybrid` 查询会先使用已索引代码图与 SQLite FTS；当这些层存在明确召回缺口时，才在已索引 commit 的候选文件上执行有界内部 exact-text source fallback。兜底结果以 `lexical` 和 `text_fallback` layer 暴露，不能替代 resolved reference/call/import edge。

冷启动 full `repo index` 会返回 queued task handle，由后台 code-index worker 在 lease 下执行解析和 SQLite 写入。`service run` 会在启动时恢复过期 code-index lease，`repo index <alias> --reset` 可以把未完成 task 重新排队，且不删除已完成 indexed scope，也不复活 terminal dead-letter 历史。`repo status` 暴露 active task、checkpoint 进度、finalization 阶段和 retention 摘要；worker 成功后会保留 active scope、最近两个完成 scope 和未完成任务 scope。如果任务已经不再 active 但仓库仍处于 `indexing`，status 会报告最近 checkpoint，便于区分 finalization 慢和进度缺失。

## 竞争力特性

仓库索引绑定 repository id、resolved commit、tree hash 和 path filter。查询期 language filter 会在完整语言 scope 上继续收窄。相同树可以复用 scope，rebase 或 force-moved head 需要新索引，dirty worktree 通过 worktree overlay 显式建模。

代码源码布局识别不再局限于顶层 `src/` 目录。索引器和 import 解析会识别
`external_deps/`、`packages/`、`modules/`、`plugins/`、`extensions/`、
`Sources/`、`lib/` 等目录下的真实源码，也支持
`modules/<name>/src/main/java` 这类嵌套 JVM source root。当注册 scope 覆盖整个仓库时，
Git tree 决定哪些目录是源码证据：被 Git 跟踪的 `.cloudbuild/`、`.cid/`、
`.build_config/`、`build/`、`dist/`、`vendor/` 和 `third_party/` 路径都会按
普通索引候选处理，而不会只因目录名被拒绝。文件级保护仍会跳过二进制/媒体资产和
`*.jsonl` 数据集转储，除非显式 path filter opt in。脏工作树 overlay 仍通过
Git status 处理 untracked 文件，且不会递归展开未跟踪的宽泛依赖、缓存或构建目录，
除非显式 path filter opt in。

非 Git source directory 没有 tracked tree 作为目录权威，因此默认使用白名单扫描：
根层支持的 source/config/docs 文件，以及 `src/`、`include/`、`lib/`、`Sources/`、
`packages/`、`modules/`、`plugins/`、`extensions/`、`docs/`、`config/` 等
source-like roots 会进入索引；`build/`、`dist/`、`target/`、`node_modules/`、
`vendor/`、`third_party/`、cache、virtualenv 和 coverage 目录默认跳过，只有显式
`--path` opt in 时才会扫描。非 Git 的 `HEAD` 或其他 ref selector 会解析到当前
filesystem synthetic snapshot，full、incremental 和 worktree overlay 共享同一套
filesystem fingerprint 语义。

例如，混合布局仓库可以注册 `--path external_deps/python_sdk`、
`--path plugins/example.com/nonstandard/session` 或
`--path modules/payment/src/main/java` 来索引这些授权源码；如果注册时刻意收窄为
`--path src`，那么 `vendor/pkg`、`third_party/pkg` 或 `build/` 会继续处于注册
scope 之外，直到用户显式放宽 path filter。

## 命令/API 入口

窄类型包括 `symbol`、`definition`、`references`、`callers`、`callees`、`imports` 和 `sbom`。`--kind hybrid` 同时检索 symbol、definition、reference、import、call 和 chunk。`--kind sbom` 检索索引期从 Cargo、npm、Go、Python、Maven effective `pom.xml`/BOM、Gradle 和 Conan manifest/lockfile 提取的依赖清单；它是本地 inventory，不执行包管理器、不解析传递依赖，也不做漏洞或许可证合规分析。import resolution 覆盖 parser 支持语言的同仓库本地 import/use/module 关系，包括 JavaScript/JSX、Kotlin、Scala、C#、PHP、Rust 和 Swift；没有授权索引源码的包管理器或 SDK import 保持 unresolved edge metadata，而不是 parser degradation。调用图检索会对跨语言调用目标做规范化：C/C++ 互调、Go cgo 的 `C.*` 调用和 Rust FFI/bindings 路径会解析到同仓库里的 C/C++ 符号；当 header 声明、FFI scoped 声明和实现共享同一个叶子名时，唯一实现优先作为 resolved call target，signature-only 声明不会阻断后续实现候选。`C.*` leaf fallback 只用于 Go cgo 文件，call target 只会解析到 callable 符号。普通命名空间调用不会只按叶子名合并，例如 `module::connect` 或 `module::sys::connect` 不会被当作 `connect` 的 FFI 调用别名；已解析 FFI 调用保留原始 scoped hint，因此 `rk_c_decode` 和 `ffi::rk_c_decode` 查询都能匹配对应调用边。

Rust enum variant 和 C/C++ enumerator 会作为挂在 enum owner 下的结构化 `enum_member` symbol 写入索引，因此 `--kind symbol` 和 `--kind definition` 可以解析 `Color.Red` 或 `Direction.kForward` 这类身份，而不依赖 text fallback。其他语言的 enum case 形态应按语言补充 parser fixture 后再纳入结构化 enum-member 覆盖范围。

`repo feature-flags` 是独立只读入口，用于枚举或过滤 indexed scope 内的配置驱动特性开关图。它返回按开关分组的配置来源和 `defines_config`、`reads_config`、`guards_code` 关系，而不是把 feature flag 作为普通 `repo query --kind` 值。

通用配置和文档文件会进入同一代码图，而不是单独的文档索引。`.conf` 复用 INI/key-value 语法面，输出 section、config 和布尔 feature-flag facts；Markdown 输出 heading symbol，并把本地 inline link、图片链接和引用式链接定义写为 import facts；JSON 输出稳定点分配置路径，数组统一使用 `[]`。这些文件仍保留文件级 chunk，因此正文、配置值和局部 partial parse 内容可被 `hybrid` 与 BM25 召回。

## 降级与诊断

Unsupported、非法 UTF-8、二进制或超大文件会降级为 text-only chunk。包含不可恢复 error node 的 syntax tree 以 partial 状态索引，并记录 file diagnostic；C/C++ 宏密集文件如果 error node 局限在 macro expansion、有界 preprocessor directive 或 decorator declaration，且仍能抽取可靠 symbol、reference 或 import，可以保持 parsed。外部依赖源码缺失只作为 unresolved edge coverage metadata 暴露，不写入 `degraded_reason`。source fallback 候选路径、候选文件、物化字节或单行长度预算问题只降级 query-time exact-text fallback，响应必须保留 `degraded_reason`，已有结构化命中仍然可用。

## 关联架构章节

- [Source Scope 模型](../03-architecture-specs/04-source-scope-model.md)
- [Tree-sitter 抽取与增量索引](../03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)
- [代码检索排序与影响分析](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

导航: 上一章: [7. 多模态证据能力](07-multimodal-evidence-capability.md) | 下一章: [9. 代码图竞争力特性](09-code-graph-competitive-features.md)
