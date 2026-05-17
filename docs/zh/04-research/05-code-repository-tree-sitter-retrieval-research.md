# 代码仓库 Tree-sitter 检索研究材料

[中文](../../zh/04-research/05-code-repository-tree-sitter-retrieval-research.md) | [英文](../../en/04-research/05-code-repository-tree-sitter-retrieval-research.md)

> 文档版本: 1.0
> 编制日期: 2026-05-12
> 研究范围: tree-sitter 结构化代码解析、Git 增量变更发现、代码知识图谱、高性能索引和检索
> 输出用途: 支撑 `docs/zh/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md` 的设计取舍

## 研究定位

| 维度 | 结论 |
| --- | --- |
| 研究来源 | 以 Tree-sitter、Git、libgit2、GitHub code navigation、Codebase-Memory 和本仓库代码图经验为主。 |
| 研究目标 | 把代码仓库检索从全文搜索升级为 Git snapshot、语法结构、符号/引用图和增量索引组合。 |
| 关键竞争力 | 结构化代码事实、文件级增量、作用域授权、混合召回、影响分析和可恢复索引是面向 agent 的核心壁垒。 |
| 场景与未来 | 面向大仓库理解、脏工作树查询、代码审查、影响报告、Agent 上下文打包和后续语言服务适配。 |

## 1. 研究结论

代码仓库检索不能只靠全文索引。成熟方向是把 Git 版本边界、tree-sitter 语法结构、符号/引用图、文本/向量检索和增量索引组合起来:

1. tree-sitter 适合作为 v1 结构化解析基础。它能够为多语言源码生成具体语法树，支持增量解析、查询捕获、错误恢复和代码导航标签。
2. Git 必须作为变更发现和快照的真实来源。`diff --name-status -z -M` 适合提交间更新，`status --porcelain=v2 -z` 适合工作树覆盖，commit-graph 的变更路径 Bloom 过滤器可辅助历史路径查询。
3. 高性能来自文件级增量和索引局部刷新，而非每次全仓重建。核心流程是变更路径 -> 内容哈希跳过 -> 反向依赖 -> 有界重解析 -> 范围索引刷新。
4. 查询层需要混合策略。符号、路径和错误码适合 BM25；概念性问题适合语义/向量检索；调用、引用、依赖和影响分析必须采用图扩展。
5. v1 不应承诺编译器级语义解析。tree-sitter 能可靠抽取语法层定义和引用，但跨文件符号解析、动态语言调用解析、宏展开和类型推断需要后续语言服务或编译器 adapter 增强。

## 2. 资料来源

| 来源 | 关键信息 | 设计影响 |
| --- | --- | --- |
| Tree-sitter README [R1] | tree-sitter 是解析器生成器和增量解析库，目标包括足够快、能处理语法错误 | 适合作为本地代码结构抽取基础 |
| Tree-sitter advanced parsing [R2] | 修改旧树后可将旧树传回解析器；included ranges 支持多语言文档 | watch 热更新可用旧树；嵌入语言用 ranges 或 region 适配器 |
| Tree-sitter code navigation [R3] | 查询捕获可标记定义、引用、调用、文档注释 | 规范采用 `@definition.*`、`@reference.*`、`@name`、`@doc` 合约 |
| Rust tree-sitter Parser docs [R4] | `parse` 接收 UTF-8 文本和可选旧树；旧树需先编辑 | Rust 适配器可以显式建模完整解析与增量解析 |
| Rust tree-sitter Query 文档 [R5] | Query 是匹配语法树节点的一组模式，并与语言绑定，可跨线程共享引用 | query 注册表应按 language/version 管理并缓存 |
| Git diff 选项 [R6] | `--name-status` 输出路径和状态，`-z` 使用 NUL 分隔，`-M` 检测重命名 | commit-to-commit 增量使用机器可读 diff |
| Git status 文档 [R7] | porcelain 格式保证脚本解析稳定，v2 更详细，`-z` 适合机器解析 | 脏工作树覆盖使用 porcelain v2 |
| Git commit-graph 文档 [R8] | changed-path Bloom 过滤器可写入 commit graph | 大历史范围路径查询可利用 Git 自身加速结构 |
| libgit2 diff API [R9] | 提供 tree-to-tree、tree-to-index、index-to-workdir diff API | 后续可用 Rust/libgit2 适配器替代 Git CLI |
| GitHub 代码导航文档 [R10] | GitHub 代码导航使用 tree-sitter，支持 definitions/references | 验证 tree-sitter tags 路线适合仓库级导航 |
| Codebase-Memory 论文 [R11] | Tree-Sitter 代码图 + MCP，覆盖多语言、并行工作线程、调用图、影响分析 | 支持本项目面向 agent 的代码知识图谱方向 |
| 本仓库能力参考 [R12] | code-review-graph 使用 tree-sitter、多语言、SQLite/FTS5、SHA-256 增量和影响分析 | 本规格继承可借鉴点，并补齐事件驱动、作用域和 QoS |

## 3. Tree-sitter 能力分析

### 3.1 适合做什么

tree-sitter 对 `relay-knowledge` 有四个直接价值:

- **结构化抽取**: 从源码中稳定抽取定义、引用、调用、导入、doc comment 和代码块范围。
- **多语言统一入口**: 不同语言可以通过 grammar + query 映射到同一组 `CodeSymbol` 和 `CodeReference`。
- **错误容忍**：源码处于半编辑状态时仍能生成部分树，适合脏工作区（dirty worktree）或监视（watch）模式。
- **编辑器式增量解析**：当有旧树和编辑范围时，可复用旧树结构，降低单文件热更新成本。

### 3.2 不适合单独做什么

tree-sitter 本身不提供完整语义:

- 不做类型推断。
- 不展开宏或模板。
- 不解析动态调用目标。
- 不知道构建系统、workspace、module resolution 的完整语义。
- 不保证跨文件引用一定能唯一解析。

因此 v1 应将 tree-sitter 输出标记为语法级事实（syntax-level facts）。跨文件目标解析必须携带 `resolution_state`，将 ambiguous/unresolved 视为正常状态。

### 3.3 Query capture 路线

Tree-sitter 代码导航文档已采用 `@definition.class`、`@definition.function`、`@reference.call` 和 `@name` 的命名风格。本项目应沿用此风格，并增加项目内查询元数据（query metadata）：

```text
query_identity = language_id + grammar_version + query_name + query_version
```

这样可以追踪同一提交因语法/查询升级导致的抽取差异。查询结果必须写入提取器元数据（extractor metadata），方便重建、回滚和解释。

### 3.4 增量解析使用边界

Tree-sitter 的旧树复用要求调用方知道文本编辑范围，并先编辑旧树。Git 提交间的差异通常只有前后 blob，而非编辑器保存时的精确编辑操作。

因此推荐:

| 场景 | 推荐 |
| --- | --- |
| 文件保存监视，有编辑事件和旧树 | 使用 tree-sitter 增量解析 |
| Git pull、checkout、提交间更新 | 对变更文件重新解析 |
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

- rebase、强制推送和分支移动不会污染旧的结果。
- 同一 tree hash 可复用索引。
- 查询响应能明确说明结果来自哪个 commit。

### 4.2 提交间差异（commit-to-commit diff）

`git diff --name-status -z -M base head` 是适合机器解析的基础形式:

- `--name-status` 给出 changed file 的状态和路径。
- `-z` 避免路径转义问题，适合包含空格、换行或特殊字符的路径。
- 使用 `-M` 启用重命名检测，有助于保留符号的 lineage 候选。

diff 输出只能告诉文件变化，不等于代码图变化。实现还需要:

- 读取旧/新 blob 哈希。
- 对未变 content hash 跳过解析。
- 删除旧文件关联事实。
- 对 rename 生成 move/lineage 候选。
- 查反向依赖找受影响引用或调用边。

### 4.3 工作树覆盖（Worktree overlay）

脏工作树适合代码审查和本地代理辅助，但不能污染干净的快照。`git status --porcelain=v2 -z` 提供稳定的机器解析格式，可用于构造 `worktree_overlay` 或 `git_changeset` 派生的作用域。

设计原则:

- clean commit snapshot 仍是默认检索源。
- dirty changes 只能在用户显式选择时进入查询。
- overlay 结果必须标注 `uncommitted=true` 和 path status。
- overlay 索引可以短 TTL，不能当成长期事实真源。

### 4.4 Commit-graph 和历史路径查询

Git commit-graph 可存储提交元数据和可选的变更路径 Bloom 过滤器。它更适合：

- 大历史范围中快速判断某路径是否可能被 commit 修改。
- 辅助历史检索、blame-like 路径演化和大仓库优化。

v1 不需要直接解析 commit-graph 格式。更实际路线是允许 Git 自身维护 commit-graph，并在后续 Git adapter 中利用 Git 命令或库的加速能力。

## 5. 代码知识图谱经验

### 5.1 code-review-graph / better-code-review-graph

本仓库已有 `11-code-knowledge-graph-model.md` 调研，关键经验包括:

- 文件收集使用 Git tracked files 和 ignore 过滤。
- tree-sitter 多语言 AST 解析覆盖主流语言；当前实现已将 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift 和 Bash 纳入受控的 grammar registry。
- SQLite + FTS5 足够支撑中小规模本地代码图。
- SHA-256 hash 比对能跳过大量未变文件。
- 增量更新流程通常是 Git diff -> dependents -> hash skip -> 重新解析变更文件。
- 递归 CTE 可实现 BFS 影响分析，避免在应用层载入全图。

这些经验可直接借鉴，但 relay-knowledge 需要更强的工程边界:

- 事件驱动和 async-first。
- scope 内索引版本，不使用仓库全局 freshness 代表某快照。
- 后台服务由平台服务管理器托管。
- 有界队列、QoS、死信队列、可观测性从 v1 版本起即纳入设计。
- 查询和更新共享统一 API，不让 MCP/CLI 直接碰 storage。

### 5.2 GitHub 代码导航

GitHub 文档说明其代码导航使用 tree-sitter 支持 definitions 和 references，并能在仓库内搜索符号。这证明 tree-sitter 查询标签适合用作大规模仓库级导航入口。

值得注意的是，GitHub 对代码导航设有仓库规模边界，例如文档中提到的文件数限制。这对 relay-knowledge 的启示是：大仓库必须有路径过滤、分区、预算和降级策略，不能默认承诺无限制的全仓实时索引。

### 5.3 Codebase-Memory

Codebase-Memory 论文通过 MCP 将 Tree-Sitter 代码图暴露给 LLM 编码代理，并强调并行工作池、调用图遍历、影响分析和社区发现。它支持本项目的总体方向：让代理从结构化代码图获取上下文，减少重复文件探索。

但 relay-knowledge 不应复制其运行时边界。按照现有规格，core 应作为 knowledge substrate，MCP 或其他 agent adapter 只能通过统一 API 访问图谱和检索能力。

## 6. Rust 生态与实现选择

### 6.1 tree-sitter crates

推荐路线:

- 使用 `tree-sitter` Rust binding 作为 parser API。
- 每种语言引入独立的 grammar crate 或维护受控的 grammar registry。
- query 文件作为版本化资源，按语言加载。
- `Parser` 作为 worker-local 对象，避免跨任务共享可变 parser。
- `Query` 可按语言和版本缓存，作为不可变资源复用。
- 当上游 grammar crate 未暴露 tags query 时，仓库内必须维护最小受控的 query，至少覆盖函数/类型定义和可识别调用，避免已配置语言只产生整文件 chunk。

需要注意:

- `unsafe_code = "forbid"` 已在本仓库启用。直接依赖的 crate 内部可以包含 unsafe，但本项目代码不能编写 unsafe adapter。
- grammar crate 版本变化必须记录在 extractor metadata 中。
- query 编译失败属于启动或配置错误，必须快速失败（fail fast）或将该语言标记为 unavailable。

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
- FTS5 支持 path/symbol/chunk/doc comment 的 BM25 排名。

向量和语义索引应保持派生的只读模型，不能成为事实真源。embedding 只对内容哈希发生变化的 chunk 重新计算。

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
- 用 blob/content hash 避免对未变文件进行解析。
- 用反向依赖只读模型限制影响扩散。
- 用 checkpointed batch writes 降低事务开销，并让大 scope 中断前已有持久进度。
- 用 content hash 缓存 embedding。
- 用作用域索引元数据避免全局陈旧。

### 7.2 并发模型

推荐流水线:

```text
diff producer
  -> bounded changed-file queue
  -> metadata/hash workers
  -> bounded parse queue
  -> parse/extract workers
  -> bounded SQLite batch writer
  -> checkpoint cursor
  -> cross-batch edge finalizer
  -> index refresh workers
```

约束:

- producer 在队列满时暂停或返回 backpressure。
- parser worker 数量不超过 CPU 和内存预算。
- storage writer 批量提交，避免多个 writer 抢锁；每批更新 checkpoint 和 `indexing`
  状态。
- reference/import/call resolution 在 finalize 阶段读取同一 scope 的完整已落库事实，避免
  大仓库分批后丢失跨批文件关系。
- embedding 和 community rebuild 低优先级执行。
- 查询时读取最新的 fresh index 或按新鲜度策略降级，不等待后台低优先级任务完成。

### 7.3 大仓库策略

大仓库风险来自文件数、生成代码、vendor 目录、锁文件、二进制资产和多语言 grammar 数量。默认策略:

- 优先 Git tracked files，不扫 untracked。
- 默认排除常见的生成文件夹/第三方目录，可配置覆盖。
- 单文件大小上限，超限 text-only 或 skipped。
- language filters 和 path filters 可强制要求。
- 默认先用 `CodeIndexResourceBudget` 分批解析和落盘；每批同时受文件数、字节数和写入行数
  约束。
- 对超过运行时总资源预算或服务窗口的仓库返回 `requires_partitioning`，引导用户选择路径
  scope；这属于运维预算决策，不再是索引器必须单次 snapshot 成功的架构限制。
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
| rename 检测成本高 | 大 diff 更新变慢 | rename 阈值可配置，大 diff 降级为删除+新增 |
| generated/vendor 文件过多 | parse/index 爆炸 | 默认过滤、路径预算、显式选择加入 |
| 脏工作树污染快照 | 查询结果不稳定 | worktree_overlay 范围，与干净快照隔离 |
| 解析器工作线程占满 CPU | 查询延迟上升 | QoS 优先级、工作线程预算、维护窗口 |
| SQLite 写锁竞争 | 更新吞吐下降 | 单写入者批量提交，查询读取快照 |
| embedding 成本过高 | 增量刷新慢 | 内容哈希缓存，后台低优先级，BM25 降级 |
| 子模块和多工作树 | 范围混乱 | repository_id 分层，子模块显式注册或作为外部依赖 |
| 大仓库一次性全量 | 内存和时间超预算 | 路径/语言分区，requires_partitioning 状态 |

## 10. 推荐规格落点

根据以上研究，推荐把代码仓库能力拆成四层:

1. **Git 源适配器**：仅负责快照解析、差异/状态、blob 元数据和已跟踪文件枚举。
2. **Tree-sitter 提取适配器**：仅负责解析、查询捕获、范围映射和解析诊断。
3. **代码图域/存储**：保存版本化代码事实、依赖关系、代码块和变更集。
4. **检索/索引**：维护范围限定的 BM25/语义/向量/代码图读取模型，并通过统一的混合检索返回上下文包。

v1 最小可用能力:

- 注册本地 Git 仓库。
- 对 HEAD 快照全量构建主流语法注册表语言的定义/导入/代码块。
- 使用 BM25 搜索 path/symbol/chunk。
- 支持 commit-to-commit 增量更新。
- 返回作用域、提交、树、行范围、过时/降级元数据。

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
| medium | 5k-20k 文件，含生成/供应商样本 | 本地性能门槛 |
| large | 50k-100k files，可选下载 | 手动/夜间 benchmark |

记录指标:

- 完整构建文件数/秒 和 MB/秒。
- 解析成功/部分成功/失败的比例。
- 单文件、十文件、百文件增量更新 p50/p95。
- changed files 到 fresh BM25 的延迟。
- 变更块到向量刷新（vector fresh）的延迟。
- 并发查询 p95/p99。
- 图影响遍历的 p95 和截断率。
- 工作线程队列深度、CPU、内存、SQLite 事务时间。

## 12. 参考资料

- [R1] Tree-sitter 自述文件。<https://github.com/tree-sitter/tree-sitter>
- [R2] Tree-sitter 文档，“高级解析”。<https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html>
- [R3] Tree-sitter 文档，“代码导航系统”。<https://tree-sitter.github.io/tree-sitter/4-code-navigation.html>
- [R4] Rust `tree_sitter::Parser` docs. <https://docs.rs/tree-sitter/latest/tree_sitter/struct.Parser.html>
- [R5] Rust `tree_sitter::Query` docs. <https://docs.rs/tree-sitter/latest/tree_sitter/struct.Query.html>
- [R6] Git 文档，“diff-options”。<https://git-scm.com/docs/diff-options>
- [R7] Git documentation, "`git status`." <https://git-scm.com/docs/git-status>
- [R8] Git documentation, "`git commit-graph`." <https://git-scm.com/docs/git-commit-graph>
- [R9] libgit2 文档，“diff API。” <https://libgit2.org/docs/reference/main/diff/index.html>
- [R10] GitHub 文档，“在 GitHub 上浏览代码。” <https://docs.github.com/en/repositories/working-with-files/using-files/navigating-code-on-github>
- [R11] Martin Vogel 等人，“Codebase-Memory：基于 Tree-Sitter 的知识图谱，用于通过 MCP 进行大语言模型代码探索。” <https://arxiv.org/abs/2603.27277>
- [R12] `relay-knowledge` 本地调研，`docs/zh/03-architecture-specs/11-code-knowledge-graph-model.md`。
