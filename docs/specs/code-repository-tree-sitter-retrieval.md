# 代码仓库 Tree-sitter 检索规格

> 文档版本: 1.0
> 编制日期: 2026-05-12
> 适用范围: Git 代码仓库摄取、tree-sitter 结构化解析、存量构建、增量更新、高并发检索和代码影响分析
> 默认路线: Git snapshot scope + tree-sitter 语法图 + scoped hybrid retrieval + event-driven bounded workers

## 1. 设计结论

`relay-knowledge` 需要把代码仓库视为一类一等 source，而不是普通文本目录。代码搜索必须同时理解 Git 版本、路径、符号、引用、调用、导入、文档注释和变更集，否则 agent 只能退化成反复 `grep` 和读文件。

核心结论:

1. **Git 快照是真源**: 默认代码检索必须绑定 `repository_id + resolved_commit_sha + tree_hash + path_filters`，继承 `Source Scope 与多模态摄取规格` 的 snapshot 隔离规则。
2. **tree-sitter 是结构入口**: 使用 tree-sitter 生成语法树，并通过 query captures 提取定义、引用、调用、导入、doc comment 和代码 chunk。
3. **全量和增量同一套图模型**: 存量构建和增量更新都产出同一类 `CodeFile`、`CodeSymbol`、`CodeReference`、`CodeDependency`、`CodeChunk` 和 `CodeChangeSet` 事实。
4. **高性能来自缩小工作集**: 增量更新先用 Git diff/status 和 blob/content hash 缩小候选，再用反向依赖扩散找受影响文件，最后局部刷新图和索引。
5. **查询和更新隔离**: 大规模 rebuild、embedding、索引刷新和影响分析必须运行在有界 worker 或维护边界后面，不阻塞查询 hot path 或 async runtime executor。
6. **索引必须按 scope 分区**: BM25、semantic、vector 和代码结构索引都必须能按 `scope_id`、`repository_id`、`tree_hash`、语言和路径过滤。
7. **失败可降级**: 某语言 grammar 缺失、解析错误或引用解析失败时，文件级文本 chunk 和 BM25 检索仍要可用，结果携带 degraded metadata。

## 2. 架构边界

本规格不替代现有架构文档，而是在这些边界内补充代码仓库能力:

| 已有规格 | 本规格继承的约束 |
| --- | --- |
| `source-scope-and-multimodal-ingestion.md` | Git snapshot、changeset、scope 内索引版本和 rebase 隔离 |
| `open-agent-runtime-and-hybrid-retrieval-architecture.md` | BM25、semantic、vector、graph expansion 和 RRF 融合 |
| `storage-layer-design.md` | GraphStore 是事实真源，IndexStore 是派生读模型 |
| `background-service-and-self-healing.md` | 后台服务、静默更新、自愈、dead-letter 和资源预算 |
| `engineering-hard-constraints.md` | async-first、无阻塞 hot path、QoS、文件长度、测试覆盖和文档完整性 |

依赖方向保持:

```text
interfaces / agent_adapter
  -> api
  -> application
  -> domain contracts
  -> storage / indexing / retrieval traits
  -> adapters: git, tree_sitter, index backends
```

禁止事项:

- CLI、Web、MCP 或 HTTP adapter 直接访问 Git 仓库、SQLite、tree-sitter parser 或索引 writer。
- retrieval 层直接启动 Git 命令、读取 worktree 文件或重建索引。
- tree-sitter adapter 读取环境变量、构造运行时路径或创建网络 client。
- 在 async 查询路径中同步读取大文件、递归扫描仓库、运行 embedding 或执行全图重算。

## 3. Source 和仓库模型

### 3.1 Repository identity

每个仓库注册后产生稳定 `repository_id`。推荐来源:

```text
repository_id = hash(normalized_origin_url, absolute_repo_identity, install_instance_salt)
```

同一个 remote 的不同本地 checkout 必须保持不同 `repository_id`，避免
alias、status lookup 或本地授权范围互相混淆。查询键必须先按
`repository_id` 精确查找；未命中时再按 alias 查找，所以 alias 可使用
`repo:` 前缀，但与 `repository_id` 冲突时必须由 `repository_id` 优先。

仓库元数据至少包含:

| 字段 | 含义 |
| --- | --- |
| `repository_id` | 本地稳定仓库 ID |
| `worktree_root` | 由 `paths` 或显式授权路径解析后的根目录 |
| `git_dir` | Git metadata 目录，支持 worktree |
| `default_branch` | 审计信息，不作为事实真源 |
| `remote_url_fingerprint` | remote URL 的脱敏指纹 |
| `registered_at` | 注册时间 |
| `authorization_scope` | 允许摄取的路径、分支和文件类型范围 |

`worktree_root` 只能由路径边界模块管理。运行时状态、索引、缓存、dead-letter 和日志不能默认写入仓库目录。

### 3.2 Snapshot scope

代码检索默认使用:

```text
CodeSnapshotScope {
  repository_id,
  resolved_commit_sha,
  tree_hash,
  path_filters,
  language_filters,
}
```

规则:

- branch、tag、HEAD、PR ref 和 worktree selector 必须先解析为 commit/tree，再构造 scope。用户输入的 ref 不能以 `-` 开头，必须在调用 Git 前拒绝，避免被解释为 Git 选项。
- 同一 tree hash 可复用索引分区，即使来自不同 branch 名。
- rebase 后的新 head 必须产生新 scope；旧 scope 只能用于历史审计或显式 diff。
- dirty worktree 必须显式建模为 `git_changeset` 或 `worktree_overlay`，不能混入 clean snapshot。`worktree_overlay` 必须有显式 overlay identity；查询 clean commit ref 时不能返回 overlay 内容。

### 3.3 Changeset scope

代码审查和影响分析使用:

```text
CodeChangeSetScope {
  repository_id,
  base_commit_sha,
  head_commit_sha,
  base_tree_hash,
  head_tree_hash,
  changed_paths_hash,
  path_filters,
}
```

`git_changeset` 是派生视图，不是事实真源。返回的定义、引用和 chunk 仍要标记所属 snapshot；diff、risk score、affected symbols 和 test impact 属于 changeset evidence。

## 4. 代码图模型

### 4.1 核心实体

| 实体 | 主键建议 | 说明 |
| --- | --- | --- |
| `CodeFile` | hash(`repository_id`, `tree_hash`, `path`, `blob_hash`) | 某快照中的文件实例 |
| `CodeSymbol` | `symbol_snapshot_id` | 类、函数、方法、接口、变量、常量、模块等 |
| `CodeReference` | hash(`file_id`, `range`, `target_hint`, `capture_kind`) | 引用、调用、继承、实现、导入等语法引用 |
| `CodeDependency` | hash(`from_file_id`, `to_path_or_symbol`, `kind`) | 文件、模块、包或符号级依赖 |
| `CodeChunk` | hash(`file_id`, `range`, `chunk_strategy`, `content_hash`) | 检索用代码片段 |
| `CodeChangeSet` | hash(`repository_id`, `base`, `head`, `changed_paths_hash`) | diff 视图和影响分析入口 |

`canonical_symbol_id` 和 `symbol_snapshot_id` 必须分离:

```text
canonical_symbol_id = repo://{repository_id}/{logical_path}::{qualified_name}
symbol_snapshot_id = hash(repository_id, tree_hash, language, qualified_name, blob_hash, range)
```

规则:

- accepted graph facts 默认绑定 `symbol_snapshot_id`。
- 跨快照、跨 branch 或跨 rebase 的同名符号只能通过显式 `SAME_LOGICAL_SYMBOL_AS` 或 lineage 关系连接。
- rename/move 只能在相似度、Git rename 和符号指纹足够时生成候选 lineage；不能直接覆盖旧事实。

### 4.2 边类型

v1 必须支持的边:

| 边 | 来源 | 用途 |
| --- | --- | --- |
| `DEFINED_IN` | definition capture | 符号到文件 |
| `CONTAINS` | AST range nesting | 文件、类、函数和 chunk 层级 |
| `REFERENCES` | reference capture | name usage 到候选符号 |
| `CALLS` | call capture | 调用图和影响分析 |
| `IMPORTS_FROM` | import/module capture | 依赖扩散 |
| `IMPLEMENTS` | language-specific capture | interface/trait 关系 |
| `EXTENDS` | class/type capture | 继承关系 |
| `DOCUMENTED_BY` | doc capture | doc comment 到符号 |
| `CHANGED_IN` | Git diff | changeset 到文件/符号 |

如果目标符号无法唯一解析，边必须保留 `target_hint`、`resolution_state=unresolved|ambiguous|resolved` 和候选列表。不能把猜测写成确定调用。

## 5. Tree-sitter 解析与抽取

### 5.1 语言注册

语言选择按以下顺序:

1. 显式 repository config 或 workspace config。
2. 文件扩展名和 shebang。
3. GitHub Linguist 风格 heuristics 的后续 adapter。
4. fallback 为 text-only chunk，不进入 AST graph。

每个语言定义:

```text
LanguageDefinition {
  language_id,
  grammar_name,
  grammar_version,
  file_patterns,
  injection_rules,
  tag_query,
  reference_query,
  chunk_query,
}
```

grammar 版本必须写入 extractor metadata。grammar 升级后，同一文件的抽取结果可能变化，必须触发 scoped re-index 或标记 extractor drift。

### 5.2 Query capture contract

tree-sitter query capture 推荐命名:

| Capture | 含义 |
| --- | --- |
| `@definition.function` | 函数定义 |
| `@definition.method` | 方法定义 |
| `@definition.class` | 类定义 |
| `@definition.interface` | 接口、trait 或 protocol |
| `@definition.module` | 模块、namespace 或 package |
| `@reference.call` | 函数/方法调用 |
| `@reference.type` | 类型引用 |
| `@reference.import` | import/use/require |
| `@reference.implementation` | 实现或继承引用 |
| `@name` | 定义或引用名称 |
| `@doc` | 文档注释或 docstring |
| `@chunk` | 可检索代码块 |

每个 capture 结果必须记录:

- `language_id`
- `grammar_version`
- `query_name`
- `query_version`
- `byte_range`
- `line_range`
- `node_kind`
- `capture_kind`
- `content_hash`

### 5.3 多语言和嵌入语言

TSX、Vue、Svelte、Markdown fenced code、Jupyter notebook 和模板文件可能包含多语言区域。处理规则:

- 主文件产生一个 `CodeFile`。
- 每个 embedded language region 产生 `CodeChunk` 和可选 `EmbeddedCodeRegion`。
- parser 使用 included ranges 或 adapter 切分策略；range 必须保留到原始文件坐标。
- 嵌入语言解析失败只影响该 region，不让整个文件变为 failed。

### 5.4 解析错误降级

tree-sitter 可在语法错误存在时返回部分树。系统必须区分:

| 状态 | 行为 |
| --- | --- |
| `parsed` | AST、符号和 chunk 都可用 |
| `partial` | 存在 error node，保留可靠 capture，标记 degraded |
| `text_only` | grammar 缺失、非法 UTF-8、二进制或超限，只有 text chunk/BM25 |
| `failed` | 读取、解码或 parser 异常，写 diagnostics 和 dead-letter |

解析失败不能回滚整个仓库的更新批次。失败文件必须可重试，并在 health/status 中可见。

## 6. 存量全量构建

全量构建用于新仓库注册、grammar 升级、索引损坏恢复或用户显式 rebuild。

```text
resolve snapshot
  -> enumerate tracked files
  -> apply authorization/path/language filters
  -> load blob metadata and content hash
  -> skip generated/binary/oversized files
  -> parse/extract in bounded worker pool
  -> batch graph mutation commit
  -> scoped BM25/semantic/vector refresh
  -> publish repository index status
```

要求:

- 文件枚举以 Git tracked files 为基础，不能默认递归扫描整个目录。
- `.gitignore` 只影响 worktree overlay；clean snapshot 使用 Git tree。
- generated/vendor/lockfile/large file 规则必须可配置、可观测、可解释。
- 单文件读取、解析和抽取必须有大小上限、时间上限和取消点。
- 写入按批次提交；批次失败只回滚该批次，成功批次保留。
- 全量构建完成前，旧 scope 查询继续可用；新 scope 可按 freshness policy 返回 stale/partial。

## 7. 增量更新

### 7.1 变更发现

commit-to-commit 更新:

```text
git diff --name-status -z -M {old_commit} {new_commit}
```

增量更新复用上一版文件指纹前，`old_commit` 必须解析到当前已索引
snapshot 的 commit；不匹配时必须拒绝本次增量更新，要求调用方先执行
full index 或从当前 indexed commit 继续。

worktree overlay 更新:

```text
git status --porcelain=v2 -z --untracked-files=all
```

worktree overlay 必须绑定当前 checked-out `HEAD`，不能把其它 ref 标记成
当前工作区内容。overlay 变更发现必须显式启用 untracked 文件，并把未跟踪
目录展开为文件级变更后再应用 path/language filters。

实现可以通过 Git CLI、libgit2 或 Rust Git adapter 提供统一 diff contract。无论底层实现如何，application 层只接收结构化 `ChangedPath`。

```text
ChangedPath {
  path,
  old_path,
  status: added | modified | deleted | renamed | copied | type_changed,
  old_blob_hash,
  new_blob_hash,
  similarity,
}
```

### 7.2 候选缩小

增量更新必须按顺序缩小工作集:

1. 解析 Git diff/status 得到 changed paths。
2. 按授权 scope、path filters、language filters 过滤；`.` 和 `./` path
   filter 表示仓库根，`./src` 等前缀必须规范化为 `src` 后匹配。
3. 用 blob/content hash 跳过内容未变文件。
4. 对删除文件产生 tombstone mutation。
5. 对 rename/move 保留 lineage candidate。
6. 查 `IMPORTS_FROM`、`REFERENCES` 和 package/module dependency read model 找 reverse dependents。
7. 对受影响文件执行局部重解析或引用重解析。

`find_dependents` 必须有深度、节点数、时间和队列预算。超过预算时标记 `impact_truncated=true`，不能无界扩散。

### 7.3 Tree 复用策略

tree-sitter 的旧 tree 复用适合编辑器式热更新，但 Git commit-to-commit 更新通常只知道文件前后内容。v1 默认策略:

| 场景 | 策略 |
| --- | --- |
| commit-to-commit 或 pull 后更新 | changed files 按文件重解析 |
| watch 模式且有 edit range | 对单文件使用 `old_tree + InputEdit` 增量解析 |
| grammar/query 版本升级 | 全量重解析受影响语言 |
| parser cache miss | 文件重解析 |

旧 tree 和 query 结果缓存只能作为性能优化，不能作为事实真源。重启后必须能从 Git snapshot 和存储状态恢复。

### 7.4 索引刷新

增量更新提交后:

- `CodeFile`、`CodeSymbol`、`CodeReference` 和 `CodeChunk` 的变更推进 graph version。
- 受影响 `scope_id + index_kind + modality + language` 标记 stale。
- BM25 可按 changed chunks 局部 upsert/delete。
- semantic/vector embedding 只刷新 content hash 改变的 chunks。
- graph expansion read model 刷新受影响邻域。
- 大批量 embedding 或 community rebuild 进入后台维护队列。

索引刷新失败不得回滚已提交图事实。响应必须携带 `index_refresh_error` 或 scoped stale metadata。

## 8. 检索能力

### 8.1 查询类型

v1 代码仓库检索至少覆盖:

| 查询 | 召回路径 |
| --- | --- |
| 符号名搜索 | BM25 path/symbol field + symbol index |
| 定义跳转 | `CodeSymbol` exact match + scope filter |
| 引用搜索 | `CodeReference` + target resolution |
| 调用者/被调用者 | `CALLS` graph traversal |
| import 依赖 | `IMPORTS_FROM` graph traversal |
| 变更影响半径 | `CodeChangeSet` seeds + reverse dependents |
| 代码片段问答 | BM25 + semantic + vector + graph expansion |
| onboarding/架构概览 | symbols + dependencies + communities/summaries |

### 8.2 Hybrid retrieval

代码检索必须参与统一 hybrid pipeline:

```text
query
  -> source scope resolution
  -> freshness policy
  -> lexical recall: path, symbol, chunk, doc comment
  -> semantic recall: symbol summary, chunk summary, doc comment
  -> vector recall: code/doc/comment embeddings
  -> graph expansion: call/import/containment/definition/reference
  -> fusion and rerank
  -> context pack with citations and ranges
```

代码结果必须返回:

- `repository_id`
- `scope_id`
- `resolved_commit_sha`
- `tree_hash`
- `path`
- `language_id`
- `byte_range`
- `line_range`
- `symbol_snapshot_id` 或 `file_id`
- `retrieval_layers`
- `index_versions`
- `stale` 和 `degraded_reason`

### 8.3 Context packing

代码 context pack 以可引用、可审计为目标:

- 优先包含定义签名、doc comment、调用片段和邻近 imports。
- 不直接塞入整文件，除非用户显式请求且预算允许。
- 相同文件多个片段应合并为带 range 的 compact excerpt。
- 输出必须保留足够路径和行号，方便 agent 或人类打开源文件。
- 对 stale scope 或 partial parse 要明确标记，不能隐藏为正常结果。

## 9. 高性能与并发约束

### 9.1 资源预算

每个仓库维护独立预算:

| 预算 | 默认策略 |
| --- | --- |
| parse workers | `min(cpu_count, repo_config.max_parse_workers)` |
| file queue | 有界队列，满时暂停 diff 消费或返回 retryable overload |
| write batch | 按文件数、mutation 数或字节数封顶 |
| embedding workers | 低优先级后台 worker，不能抢占查询 |
| graph traversal | 深度、节点数、时间和输出条数封顶 |
| parser cache | 按语言和最近文件 LRU，受内存预算控制 |

所有预算必须可观测，并可被后台服务暂停、恢复或降级。

### 9.2 更新路径目标

性能目标按仓库规模分层记录，具体数值应通过后续 benchmark 校准:

| 规模 | 目标 |
| --- | --- |
| 小型 `<1k` tracked files | clean full build 秒级，单文件更新亚秒到秒级 |
| 中型 `1k-50k` tracked files | full build 可后台完成；常见增量更新只重解析 changed + affected files |
| 大型 `50k-100k` tracked files | 默认需要 path filters、language filters 和后台索引；查询不等待 full rebuild |
| 超大型 `>100k` tracked files | 必须显式启用分区策略，不承诺一次性全仓导航 |

高性能定义:

- 更新速度: changed files 小时，处理量主要与 changed/affected files 成正比，而不是与全仓文件数成正比。
- 并发: 多个查询可与后台更新并行；查询优先级高于 embedding、community rebuild 和低优先级扫描。
- 可恢复: 服务重启后从 cursor、leases 和 graph/index version 继续，不重做已提交批次。

### 9.3 查询并发

查询路径要求:

- 读操作使用 snapshot/index version，不等待写锁。
- 召回层并发执行，但每层有 timeout 和 result budget。
- graph traversal 使用 bounded frontier，不产生无界 BFS。
- fusion 前先按 scope、language、path 和 freshness 过滤。
- 返回结果分页，禁止无界输出。

## 10. 后台服务与恢复

代码仓库更新属于 installed background operation:

- 常驻服务由 systemd、Windows Service 或 launchd 管理。
- watch、pull 后刷新、定时 refresh 和 maintenance 都进入同一任务队列。
- 每个任务有 lease、deadline、retry backoff、attempt count 和 dead-letter。
- 启动 reconciler 检查 stale scopes、expired leases、failed batches 和 index lag。
- 用户可暂停某仓库或某类索引，状态必须在 CLI/Web/API 可见。

故障处理:

| 故障 | 行为 |
| --- | --- |
| Git 仓库不可读 | 标记 repository degraded，不删除旧索引 |
| diff 解析失败 | 退化为 scoped full scan 或等待人工诊断 |
| parser panic/异常 | 文件级 dead-letter，worker 隔离恢复 |
| 索引 writer 失败 | graph facts 保留，index status failed/stale |
| embedding 后端不可用 | BM25 + graph retrieval 继续 |
| 队列超预算 | 限流、暂停低优先级任务或返回 retryable overload |

## 11. API 和 CLI 落点

当前实现已经在统一 application service、CLI 和 SQLite storage boundary 中落地 v1 code repository API。接口仍保持可演进，但 CLI/Web/未来 HTTP adapter 必须继续通过统一 service 调用，不能绕过 API contract。

```rust
pub struct CodeRepositorySelector {
    pub repository_id: String,
    pub ref_selector: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
}

pub struct CodeIndexRequest {
    pub repository: CodeRepositorySelector,
    pub mode: CodeIndexMode,
    pub freshness_policy: FreshnessPolicy,
}

pub enum CodeIndexMode {
    Full,
    Incremental { base_ref: String, head_ref: String },
    WorktreeOverlay,
}

pub struct CodeRetrievalRequest {
    pub query: String,
    pub repository: CodeRepositorySelector,
    pub code_query_kind: CodeQueryKind,
    pub limit: u32,
    pub freshness_policy: FreshnessPolicy,
}
```

CLI 形态建议:

```bash
relay-knowledge repo register <path> --alias <name>
relay-knowledge repo index <alias> --ref HEAD
relay-knowledge repo update <alias> --base main --head HEAD
relay-knowledge repo query <alias> --query "where is RetryPolicy used?"
relay-knowledge repo impact <alias> --base main --head HEAD
relay-knowledge repo status <alias> --format json
```

所有命令都必须调用统一 application service，不能绕过 API contract。

当前 v1 支持:

- `repo register`: 解析 Git root，持久化 `repository_id`、alias、root path、path/language filters。
- `repo index`: 对 clean Git tree 做 full build，写入 code files、symbols、references、imports、calls 和 chunks。
- `repo update`: 解析 `git diff --name-status --find-renames -z`，仅重解析 changed/copied/renamed/type-changed path，删除 selected deleted/renamed old path，并记录 rename tombstone。copy source path 不能作为 impact changed seed。worktree overlay 必须删除 selected rename source path，synthetic tree hash 只由 selector 范围内的 changed path/content 计算；clean 或 out-of-scope-only overlay 必须回到 clean snapshot，不得重标记旧数据。
- `repo query`: 支持 `hybrid`、`symbol`、`definition`、`references`、`callers`、`callees` 和 `imports` query kind。`impact` 不是普通查询模式，必须通过 `repo impact` 执行。
- `repo query`: 请求 ref 必须解析到当前 indexed commit；显式 `worktree` ref 才能读取 worktree overlay。查询旧 commit、branch 或 tag 前必须先对该 ref 建索引，避免返回错误 revision 的 code context。
- `repo query`: request path/language filters 只能收窄 registration scope，不能替代或扩大注册时授权的 path/language filters。`wait-until-fresh` 必须拒绝 stale code index；`graph-only` 不返回 repository-index rows。
- `repo impact`: 根据 Git diff changed paths，从 changed chunks、call graph 和 import graph 返回有界影响结果。
- `repo impact`: changed path seed 必须先按 registration/request selector 过滤；删除文件没有 active file row 时，必须根据路径扩展名推断 Rust、Python、TypeScript 或 TSX language id，再执行 language filters；`head_ref` 必须解析到当前 indexed snapshot；caller expansion 必须优先使用 resolved symbol identity，删除文件的 symbol names 必须进入 impact seed，避免漏报 removed API 的调用方。
- `repo impact`: import graph seed 必须包含 changed path module key、语言原生 module key、symbol qualified name 和 symbol name。Rust 路径必须能生成 `crate::...` key，例如 `src/lib.rs` 中的 `retry_policy` 影响 `use crate::retry_policy;`。import graph 匹配必须按 module boundary 判断，不能用裸 substring 扩大影响面；underscore 和 hyphen 不能被视为 module boundary。
- `repo status`: 返回当前 indexed commit/tree、fresh/stale/degraded state 和计数。

当前 v1 语言包覆盖 Rust、Python、TypeScript 和 TSX。grammar 缺失、非法 UTF-8、二进制或超预算文件会降级为 text-only 或 diagnostic，不阻塞其他文件入库。

## 12. 可观测性

日志和 trace attributes:

- `repository_id`
- `scope_id`
- `resolved_commit_sha`
- `tree_hash`
- `path`
- `language_id`
- `grammar_version`
- `query_version`
- `changed_path_count`
- `affected_file_count`
- `parse_status`
- `index_kind`
- `indexed_graph_version`
- `degraded_reason`

Metrics:

| 指标 | 类型 | 含义 |
| --- | --- | --- |
| `relay_code_files_discovered_total` | counter | 发现的 tracked files |
| `relay_code_files_parsed_total` | counter | 解析成功/部分/失败文件数 |
| `relay_code_parse_duration_ms` | histogram | 单文件 parse + extract 耗时 |
| `relay_code_update_duration_ms` | histogram | 增量任务总耗时 |
| `relay_code_changed_files_total` | counter | diff/status 发现变更数 |
| `relay_code_skipped_unchanged_total` | counter | hash 未变跳过数 |
| `relay_code_affected_files_total` | counter | 依赖扩散后的受影响文件数 |
| `relay_code_worker_queue_depth` | gauge | parse/index worker 队列深度 |
| `relay_code_index_lag_versions` | gauge | scope 内代码索引落后图版本 |
| `relay_code_query_duration_ms` | histogram | 代码检索延迟 |
| `relay_code_graph_traversal_truncated_total` | counter | 影响分析或 graph expansion 截断次数 |

Health/status 必须能回答:

- 仓库最后成功索引的 commit/tree。
- 当前索引是否 fresh/stale/failed/paused/degraded。
- 哪些文件解析失败、为什么失败、下次重试何时发生。
- 后台队列深度、最老任务年龄、index lag。
- 是否存在 extractor drift 或 grammar/query 版本不一致。

## 13. 测试要求

单元测试:

- source selector 解析 branch/tag/commit/path filters。
- diff/status 解析 add/modify/delete/rename/copy/type-change。
- language detection 和 generated/binary/large file 过滤。
- query capture 到 `CodeSymbol`、`CodeReference` 和 `CodeChunk` 的映射。
- content hash 未变时跳过解析。
- reverse dependent 扩散深度、节点数和时间预算。
- stale/fresh index metadata 按 scope 计算。

集成测试:

- 小型 fixture 仓库 full build 后可查询定义、引用和 import 依赖。
- 修改一个文件后只更新 changed + affected files。
- rename 保留旧路径 tombstone 和 lineage candidate。
- dirty worktree overlay 不污染 clean snapshot，必须通过显式 overlay ref 查询。
- parser failure 不阻塞其他文件入库。
- vector/semantic 不可用时 BM25 + graph 查询继续可用。

性能测试:

- 固定 fixture 规模下记录 full build throughput。
- 记录单文件、十文件、百文件增量更新 p50/p95。
- 并发查询和后台更新同时运行时，查询 p95 不被大规模 embedding 拖垮。
- graph traversal 超出预算时返回 truncated metadata。

## 14. 分阶段落地

建议阶段:

1. **v1 数据 contract**: 增加 code source scope、代码图实体、diff contract、scoped index metadata。当前实现已落地 tree-sitter 输出承接层: `CodeFileRecord`、`CodeSymbolRecord`、`CodeReferenceRecord`、`CodeChunkRecord`、parse status 计数、代码图存储 trait 和 SQLite 表；Git diff contract 与 scoped index metadata 仍在后续阶段。
2. **v1 parser adapter**: 接入少量高价值语言，例如 Rust、TypeScript、Python，支持 definitions/references/imports/chunks。
3. **v1 full build + BM25**: Git tracked files 全量构建，代码 chunk 和 symbol BM25 可查询。
4. **v1 incremental update**: Git diff/status + content hash + 局部 graph/index refresh。
5. **v1 graph queries**: 定义/引用/调用/import/impact 半径。
6. **v2 hybrid retrieval**: semantic/vector、rerank、context pack、跨仓库检索。
7. **v2 background service**: watch、leases、自愈、dead-letter、维护窗口和用户可控静默更新。

每阶段都必须保持文档、测试、CLI/API metadata 和 health/status 同步更新。
