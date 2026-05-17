# Code Repository Tree-sitter Retrieval Research

[English](../../en/04-research/05-code-repository-tree-sitter-retrieval-research.md) | [中文](../../zh/04-research/05-code-repository-tree-sitter-retrieval-research.md)

This is the English documentation page for `04-research/05-code-repository-tree-sitter-retrieval-research.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> 文档版本: 1.0
> 编制日期: 2026-05-12
> 研究范围: tree-sitter 结构化代码解析、Git 增量变更发现、代码知识图谱、高性能索引和检索
> 输出用途: 支撑 `docs/zh/03-architecture-specs/07-code-repository-tree-sitter-retrieval.md` 的设计取舍

## 1. 研究结论

代码仓库检索不能只靠全文索引。成熟方向是把 Git 版本边界、tree-sitter 语法结构、符号/引用图、文本/向量检索和增量索引组合起来:

1. tree-sitter 适合作为 v1 结构化解析基础。它能为多语言源码生成 concrete syntax tree，支持增量解析、query capture、错误恢复和代码导航 tags。
2. Git 必须作为变更发现和 snapshot 真源。`diff --name-status -z -M` 适合 commit-to-commit 更新，`status --porcelain=v2 -z` 适合 worktree overlay，commit-graph changed-path Bloom filters 可辅助历史路径查询。
3. 高性能来自文件级增量和索引局部刷新，而不是每次全仓重建。核心路径是 changed paths -> content hash skip -> reverse dependents -> bounded reparse -> scoped index refresh。
4. 查询层需要混合路线。符号、路径和错误码适合 BM25；概念性问题适合 semantic/vector；调用、引用、依赖和影响分析必须走 graph expansion。
5. v1 不应承诺编译器级语义解析。tree-sitter 能可靠抽取语法层定义和引用，但跨文件符号解析、动态语言调用解析、宏展开和类型推断需要后续语言服务或编译器 adapter 增强。

## 2. 资料来源

| 来源 | 关键信息 | 设计影响 |
| --- | --- | --- |
| Tree-sitter README [R1] | tree-sitter 是 parser generator 和 incremental parsing library，目标包括足够快、能处理语法错误 | 适合作为本地代码结构抽取基础 |
| Tree-sitter advanced parsing [R2] | 修改旧 tree 后可把 old tree 传回 parser；included ranges 支持多语言文档 | watch 热更新可用 old tree；嵌入语言用 ranges 或 region adapter |
| Tree-sitter code navigation [R3] | query captures 可标记 definitions、references、calls、doc comments | 规格采用 `@definition.*`、`@reference.*`、`@name`、`@doc` contract |
| Rust tree-sitter Parser docs [R4] | `parse` 接收 UTF-8 text 和 optional old tree；old tree 需要先 edit | Rust adapter 可以显式建模 full parse 与 incremental parse |
| Rust tree-sitter Query docs [R5] | Query 是匹配 syntax tree 节点的一组 pattern，并与语言绑定，可跨线程共享引用 | query registry 应按 language/version 管理并缓存 |
| Git diff options [R6] | `--name-status` 输出路径和状态，`-z` 使用 NUL 分隔，`-M` 检测 rename | commit-to-commit 增量使用 machine-readable diff |
| Git status docs [R7] | porcelain 格式保证脚本解析稳定，v2 更详细，`-z` 适合机器解析 | dirty worktree overlay 使用 porcelain v2 |
| Git commit-graph docs [R8] | changed-path Bloom filters 可写入 commit graph | 大历史范围路径查询可利用 Git 自身加速结构 |
| libgit2 diff API [R9] | 提供 tree-to-tree、tree-to-index、index-to-workdir diff API | 后续可用 Rust/libgit2 adapter 替代 Git CLI |
| GitHub code navigation docs [R10] | GitHub code navigation 使用 tree-sitter，支持 definitions/references | 验证 tree-sitter tags 路线适合仓库级导航 |
| Codebase-Memory paper [R11] | Tree-Sitter 代码图 + MCP，覆盖多语言、parallel workers、call graph、impact analysis | 支持本项目面向 agent 的代码知识图谱方向 |
| 本仓库 capability reference [R12] | code-review-graph 使用 tree-sitter、多语言、SQLite/FTS5、SHA-256 增量和影响分析 | 本规格继承可借鉴点，并补齐事件驱动、scope 和 QoS |

## 3. Tree-sitter 能力分析

### 3.1 适合做什么

tree-sitter 对 `relay-knowledge` 有四个直接价值:

- **结构化抽取**: 从源码中稳定抽取定义、引用、调用、导入、doc comment 和代码块范围。
- **多语言统一入口**: 不同语言可以通过 grammar + query 映射到同一组 `CodeSymbol` 和 `CodeReference`。
- **错误容忍**: 源码处于半编辑状态时仍能产生部分 tree，适合 dirty worktree 或 watch 模式。
- **编辑器式增量解析**: 有旧 tree 和 edit range 时，可复用旧 tree 结构，降低单文件热更新成本。

### 3.2 不适合单独做什么

tree-sitter 本身不提供完整语义:

- 不做类型推断。
- 不展开宏或模板。
- 不解析动态调用目标。
- 不知道构建系统、workspace、module resolution 的完整语义。
- 不保证跨文件引用一定能唯一解析。

因此 v1 应把 tree-sitter 输出标记为 syntax-level facts。跨文件目标解析必须携带 `resolution_state`，把 ambiguous/unresolved 当成正常状态。

### 3.3 Query capture 路线

Tree-sitter code navigation 文档已经使用 `@definition.class`、`@definition.function`、`@reference.call` 和 `@name` 的命名风格。本项目应沿用这个风格，并增加项目内 query metadata:

```text
query_identity = language_id + grammar_version + query_name + query_version
```

这样可以追踪同一 commit 因 grammar/query 升级导致的抽取差异。query 结果必须写入 extractor metadata，方便重建、回滚和解释。

### 3.4 增量解析使用边界

Tree-sitter 的 old tree 复用要求调用方知道文本编辑范围，并先 edit 旧 tree。Git commit-to-commit diff 常常只有前后 blob，而不是编辑器保存时的精确 edit operations。

因此推荐:

| 场景 | 推荐 |
| --- | --- |
| 文件保存 watch，有 edit event 和旧 tree | 使用 tree-sitter incremental parse |
| Git pull、checkout、commit-to-commit update | 对 changed files 重解析 |
| grammar/query 版本变化 | 对受影响语言重解析 |
| 服务重启后恢复 | 从 Git snapshot 和存储状态恢复，不依赖内存 tree |

这条边界能避免为了追求单文件复用而引入复杂、不可恢复的缓存一致性问题。

## 4. Git 增量更新分析

### 4.1 Snapshot 解析

Git 代码仓库检索必须先把用户输入解析成稳定 snapshot:

```text
selector: branch/tag/HEAD/sha
  -> resolved_commit_sha
  -> tree_hash
  -> scope_id
```

好处:

- rebase、force push 和 branch 移动不会污染旧结果。
- 同一 tree hash 可复用索引。
- 查询响应能明确说明结果来自哪个 commit。

### 4.2 Commit-to-commit diff

`git diff --name-status -z -M base head` 是适合机器解析的基础形式:

- `--name-status` 给出 changed file 的状态和路径。
- `-z` 避免路径转义问题，适合包含空格、换行或特殊字符的路径。
- `-M` 启用 rename detection，帮助保留符号 lineage candidate。

diff 输出只能告诉文件变化，不等于代码图变化。实现还需要:

- 读取 old/new blob hash。
- 对未变 content hash 跳过解析。
- 删除旧文件关联事实。
- 对 rename 生成 move/lineage 候选。
- 查反向依赖找受影响引用或调用边。

### 4.3 Worktree overlay

dirty worktree 适合 code review 和本地 agent 辅助，但不能污染 clean snapshot。`git status --porcelain=v2 -z` 提供稳定的机器解析格式，可用于构造 `worktree_overlay` 或 `git_changeset` 派生 scope。

设计原则:

- clean commit snapshot 仍是默认检索源。
- dirty changes 只能在用户显式选择时进入查询。
- overlay 结果必须标注 `uncommitted=true` 和 path status。
- overlay 索引可以短 TTL，不能当成长期事实真源。

### 4.4 Commit-graph 和历史路径查询

Git commit-graph 可存储 commit metadata 和可选 changed-path Bloom filters。它更适合:

- 大历史范围中快速判断某路径是否可能被 commit 修改。
- 辅助历史检索、blame-like 路径演化和大仓库优化。

v1 不需要直接解析 commit-graph 格式。更实际路线是允许 Git 自身维护 commit-graph，并在后续 Git adapter 中利用 Git 命令或库的加速能力。

## 5. 代码知识图谱经验

### 5.1 code-review-graph / better-code-review-graph

本仓库已有 `09-knowledge-graph-capability-reference.md` 调研，关键经验包括:

- 文件收集使用 Git tracked files 和 ignore 过滤。
- tree-sitter 多语言 AST 解析可覆盖主流语言；当前实现已把 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift 和 Bash 纳入受控 grammar registry。
- SQLite + FTS5 足够支撑中小规模本地代码图。
- SHA-256 hash 比对能跳过大量未变文件。
- 增量更新流程通常是 Git diff -> dependents -> hash skip -> reparse changed files。
- 递归 CTE 可实现 BFS 影响分析，避免在应用层载入全图。

这些经验可直接借鉴，但 relay-knowledge 需要更强的工程边界:

- 事件驱动和 async-first。
- scope 内索引版本，不使用仓库全局 freshness 代表某快照。
- background service 由平台 service manager 托管。
- bounded queues、QoS、dead-letter、observability 从 v1 就进入设计。
- 查询和更新共享统一 API，不让 MCP/CLI 直接碰 storage。

### 5.2 GitHub code navigation

GitHub 文档说明其代码导航用 tree-sitter 支持 definitions 和 references，并能在仓库内搜索符号。这证明 tree-sitter query tags 适合做大规模仓库级导航入口。

值得注意的是，GitHub 对 code navigation 设有仓库规模边界，例如文档中提到文件数限制。这对 relay-knowledge 的启示是: 大仓库必须有 path filters、分区、预算和降级策略，不能默认承诺无限全仓实时索引。

### 5.3 Codebase-Memory

Codebase-Memory 论文把 Tree-Sitter 代码图通过 MCP 暴露给 LLM coding agents，并强调 parallel worker pools、call-graph traversal、impact analysis 和 community discovery。它支持本项目的总体方向: 让 agent 从结构化代码图取上下文，减少重复文件探索。

但 relay-knowledge 不应复制其运行时边界。按照现有规格，core 应做 knowledge substrate，MCP 或其他 agent adapter 只能通过统一 API 访问图谱和检索能力。

## 6. Rust 生态与实现选择

### 6.1 tree-sitter crates

推荐路线:

- 使用 `tree-sitter` Rust binding 作为 parser API。
- 每种语言引入独立 grammar crate 或维护受控 grammar registry。
- query 文件作为版本化资源，按语言加载。
- `Parser` 作为 worker-local 对象，避免跨任务共享可变 parser。
- `Query` 可按语言和版本缓存，作为不可变资源复用。
- 当上游 grammar crate 没有暴露 tags query 时，仓库内必须维护最小受控 query，至少覆盖函数/类型定义和可识别调用，避免已配置语言只产生整文件 chunk。

需要注意:

- `unsafe_code = "forbid"` 已在本仓库启用。直接依赖的 crate 内部可以有 unsafe，但本项目代码不能写 unsafe adapter。
- grammar crate 版本变化必须进入 extractor metadata。
- query 编译失败是启动或配置错误，必须 fail fast 或把该语言标记 unavailable。

### 6.2 Git adapter

可选路线:

| 路线 | 优点 | 风险 |
| --- | --- | --- |
| Git CLI | 行为接近用户本地 Git，支持 worktree/submodule/rename，启动快 | 进程开销、输出解析、平台差异 |
| libgit2/git2 crate | 结构化 API，避免解析 CLI 输出 | 与 Git CLI 行为可能不完全一致，认证/worktree 边界复杂 |
| gix | Rust 原生方向，适合长期集成 | API 选择和行为兼容需要更多验证 |

v1 推荐先定义 adapter trait 和结构化 diff contract。具体实现可以从 Git CLI 开始，后续替换为 libgit2 或 gix 时不影响 application/retrieval。

### 6.3 存储和索引

现有 SQLite 设计可承载 v1:

- `code_files` 记录 snapshot file instances。
- `code_symbols` 记录 definitions。
- `code_references` 记录 references/calls/imports。
- `code_chunks` 记录检索文本和 ranges。
- `code_dependencies` 支持 reverse dependents。
- FTS5 支持 path/symbol/chunk/doc comment BM25。

向量和 semantic 索引应保持派生 read model，不能成为事实真源。embedding 只对 content hash 改变的 chunk 重新计算。

## 7. 高性能设计分析

### 7.1 成本模型

全量构建成本:

```text
O(tracked_files + parsed_bytes + extracted_captures + index_writes)
```

增量更新目标成本:

```text
O(changed_files + affected_files + changed_chunks + refreshed_index_entries)
```

关键优化点:

- 用 Git tree/diff 避免全目录扫描。
- 用 blob/content hash 避免未变文件 parse。
- 用 reverse dependency read model 限制影响扩散。
- 用 batch writes 降低事务开销。
- 用 content hash 缓存 embedding。
- 用 scoped index metadata 避免全局 stale。

### 7.2 并发模型

推荐流水线:

```text
diff producer
  -> bounded changed-file queue
  -> metadata/hash workers
  -> bounded parse queue
  -> parse/extract workers
  -> bounded mutation batch queue
  -> storage writer
  -> index refresh workers
```

约束:

- producer 在队列满时暂停或返回 backpressure。
- parser worker 数量不超过 CPU 和内存预算。
- storage writer 批量提交，避免多个 writer 抢锁。
- embedding 和 community rebuild 低优先级执行。
- 查询读取最新 fresh index 或按 freshness policy 降级，不等待后台低优先级任务。

### 7.3 大仓库策略

大仓库风险来自文件数、生成代码、vendor 目录、锁文件、二进制资产和多语言 grammar 数量。默认策略:

- 优先 Git tracked files，不扫 untracked。
- 默认排除 common generated/vendor directories，可配置覆盖。
- 单文件大小上限，超限 text-only 或 skipped。
- language filters 和 path filters 可强制要求。
- 对超过预算的仓库返回 `requires_partitioning`，引导用户选择路径 scope。
- 历史查询和 cross-repo 查询走后台预计算，不阻塞交互查询。

## 8. 检索质量分析

### 8.1 BM25 强项

BM25 对这些代码问题很强:

- 精确函数名、类名、trait 名。
- 路径、模块名、配置 key。
- 错误码、日志字符串、feature flag。
- 具体 API 调用。

因此 v1 必须先保证 symbol/path/chunk/doc comment 的 BM25 字段质量。

### 8.2 Semantic/vector 强项

semantic/vector 对这些问题有价值:

- "哪里实现了重试退避" 这类概念查询。
- 不知道准确命名的新开发者 onboarding。
- 相关逻辑散布在多个文件时的主题召回。
- doc comment、README、ADR 和代码 chunk 的跨模态语义连接。

风险是相似但 scope 错误的结果。所有向量召回必须先过滤 scope 或在 ANN 后强制 scope post-filter，并在 metadata 中返回 index freshness。

### 8.3 Graph expansion 强项

graph expansion 适合:

- 定义到引用。
- 调用者/被调用者。
- import reverse dependents。
- changeset impact radius。
- 测试影响和架构热点。

graph traversal 必须限制深度、节点数、时间和输出预算。超限结果要返回 `truncated=true`，不能静默截断。

## 9. 风险与缓解

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| Grammar/query 版本漂移 | 同一源码抽取结果变化 | 记录 grammar/query version，升级触发 scoped re-index |
| 动态语言引用不准 | CALLS/REFERENCES 噪声 | resolution_state，语言服务后续增强 |
| rename 检测成本高 | 大 diff 更新变慢 | rename threshold 可配置，大 diff 降级为 delete+add |
| generated/vendor 文件过多 | parse/index 爆炸 | 默认过滤、path budgets、显式 opt-in |
| dirty worktree 污染 snapshot | 查询结果不稳定 | worktree_overlay scope，与 clean snapshot 隔离 |
| parser worker 占满 CPU | 查询延迟上升 | QoS 优先级、worker budget、维护窗口 |
| SQLite 写锁竞争 | 更新吞吐下降 | 单 writer batch commit，查询读 snapshot |
| embedding 成本过高 | 增量刷新慢 | content hash cache，后台低优先级，BM25 降级 |
| 子模块和多 worktree | scope 混乱 | repository_id 分层，submodule 显式注册或作为 external dependency |
| 大仓库一次性全量 | 内存和时间超预算 | path/language partition，requires_partitioning 状态 |

## 10. 推荐规格落点

根据以上研究，推荐把代码仓库能力拆成四层:

1. **Git source adapter**: 只负责 snapshot resolution、diff/status、blob metadata 和 tracked file enumeration。
2. **Tree-sitter extraction adapter**: 只负责 parse、query capture、range mapping 和 parse diagnostics。
3. **Code graph domain/storage**: 保存 versioned code facts、dependencies、chunks 和 changesets。
4. **Retrieval/indexing**: 维护 scoped BM25/semantic/vector/code graph read models，并通过统一 hybrid retrieval 返回 context pack。

v1 最小可用能力:

- 注册本地 Git 仓库。
- 对 HEAD snapshot 全量构建主流 grammar registry 语言的 definitions/imports/chunks。
- 使用 BM25 搜索 path/symbol/chunk。
- 支持 commit-to-commit 增量更新。
- 返回 scope、commit、tree、line range、stale/degraded metadata。

v1 不做:

- 编译器级类型推断。
- 跨仓库精确调用解析。
- 自动修改代码。
- 无限制全仓实时索引。
- MCP runtime 内置编排。

## 11. Benchmark 建议

建立三组 fixture:

| 规模 | 内容 | 用途 |
| --- | --- | --- |
| small | 100-500 files，Rust/TS/Python 混合 | CI 快速回归 |
| medium | 5k-20k files，含 generated/vendor 样本 | 本地性能门禁 |
| large | 50k-100k files，可选下载 | 手动/夜间 benchmark |

记录指标:

- full build files/sec 和 MB/sec。
- parse success/partial/failure 比例。
- 单文件、十文件、百文件增量更新 p50/p95。
- changed files 到 fresh BM25 的延迟。
- changed chunks 到 vector fresh 的延迟。
- 并发查询 p95/p99。
- graph impact traversal p95 和 truncation rate。
- worker queue depth、CPU、内存、SQLite transaction time。

## 12. References

- [R1] Tree-sitter README. <https://github.com/tree-sitter/tree-sitter>
- [R2] Tree-sitter documentation, "Advanced Parsing." <https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html>
- [R3] Tree-sitter documentation, "Code Navigation Systems." <https://tree-sitter.github.io/tree-sitter/4-code-navigation.html>
- [R4] Rust `tree_sitter::Parser` docs. <https://docs.rs/tree-sitter/latest/tree_sitter/struct.Parser.html>
- [R5] Rust `tree_sitter::Query` docs. <https://docs.rs/tree-sitter/latest/tree_sitter/struct.Query.html>
- [R6] Git documentation, "diff-options." <https://git-scm.com/docs/diff-options>
- [R7] Git documentation, "`git status`." <https://git-scm.com/docs/git-status>
- [R8] Git documentation, "`git commit-graph`." <https://git-scm.com/docs/git-commit-graph>
- [R9] libgit2 documentation, "diff APIs." <https://libgit2.org/docs/reference/main/diff/index.html>
- [R10] GitHub Docs, "Navigating code on GitHub." <https://docs.github.com/en/repositories/working-with-files/using-files/navigating-code-on-github>
- [R11] Martin Vogel et al., "Codebase-Memory: Tree-Sitter-Based Knowledge Graphs for LLM Code Exploration via MCP." <https://arxiv.org/abs/2603.27277>
- [R12] `relay-knowledge` local research, `docs/zh/03-architecture-specs/09-knowledge-graph-capability-reference.md`.
