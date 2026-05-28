# 多仓库代码图谱薄覆盖层

[中文](../../zh/03-architecture-specs/20-multi-repository-code-graph-overlay.md) | [English](../../en/03-architecture-specs/20-multi-repository-code-graph-overlay.md)

> 文档版本: 1.0
> 编制日期: 2026-05-19
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

多仓库能力必须建成薄覆盖层，而不是把多个仓库物化成一个新的大 `source_scope`。单仓库 `repository_snapshot` 继续是代码事实、索引和查询精度的最小边界；多仓库层只负责把用户指定的仓库集合展开为多个真实单仓 snapshot scope，协调查询、合并排序，并存储少量跨仓库派生边。

这个设计的目标是同时满足四个约束：

- 不复制 `CodeFile`、`CodeSymbol`、`CodeChunk`、reference、call 或 import 基础事实。
- 不改变现有单仓查询语义，单仓查询仍然先按 `source_scope` 下推。
- 多仓库结果能说明每个命中来自哪个 repository、commit、tree hash 和 scope。
- 跨仓库关系作为可解释、可降级的 overlay edge，而不是伪装成单仓 resolved edge。

## 实现状态

当前实现提供三阶段的初始产品化路径：

- `repo-set create/add/remove/query/status/refresh` 和共享 API/Web/MCP 入口使用显式 repository set selector。
- SQLite 持久化 `code_repository_sets`、`code_repository_set_members`、`code_repository_cross_edges`、overlay status 和 overlay refresh task；基础代码事实表不为 repository set 复制行。
- 多仓查询在应用层 fan-out 到每个成员持久化的 `source_scope`，按成员 priority、freshness 和 overlay confidence 合并排序；请求级 path/language filter 只收窄成员 scope，不会通过当前仓库注册默认值重新解析或扩大 scope。去重键包含 repository、scope、path、line range 和 excerpt。
- `repo-set refresh` 构建 import/module 层面的跨仓 overlay edges，支持 resolved、ambiguous 和 unresolved 状态，并暴露 evidence JSON。本地、相对或已在成员仓库内解析的 import 只留在成员仓库内，不会通过跨仓 symbol-name 或 basename fallback 解析。
- scope retention 会保留仍被 repository set member 引用的单仓 snapshot；后台 overlay refresh task 使用持久租约、重试、dead-letter 状态和常驻 `service run` overlay refresh worker。

## 2. 当前基线

现有实现已经具备多仓库能力的底座：

- `CodeRepository` 有稳定 `repository_id`、alias、root path 和注册时的 path scope；language filter 是请求期收窄控制，不是注册默认值。
- `repository_snapshot` scope 由 `repository_id`、tree hash、path filter 和 language filter 生成。
- SQLite 代码事实表按 `source_scope` 分区，并保留 `repository_id`。
- `repo register/index/query/impact/status/report` 当前都以一个 repository selector 为入口。
- 查询 SQL 的候选窗口先限定 `source_scope`，因此单仓检索不会被其他仓库污染。

缺口在于：系统还没有一等 `RepositorySet` / workspace selector，没有跨多个真实 scope 的查询协调层，也没有单独保存跨仓库解析边。

## 3. 核心模型

新增模型应分为三层。

### 3.1 Repository

`CodeRepository` 继续表示一个本地 Git worktree 的稳定身份和授权边界。

```text
CodeRepository {
  repository_id
  alias
  root_path
  allowed_path_filters
  allowed_language_filters
}
```

同一 Git root 可以有多个 alias；同一 alias 不得指向不同 repository id。这个约束不因多仓库集合而改变。

### 3.2 Repository Snapshot

`RepositorySnapshot` 是真实代码事实的唯一分区。

```text
RepositorySnapshot {
  source_scope
  repository_id
  resolved_commit_sha
  tree_hash
  path_filters
  language_filters
  freshness_state
}
```

文件、符号、chunk、单仓 reference/call/import、diagnostic 和 tombstone 都只写入这个真实 snapshot scope。多仓库集合不得复制这些行。

### 3.3 Repository Set

`RepositorySet` 是用户指定的多仓库查询和授权边界。它本身不持有代码事实，只持有成员指针。

```text
RepositorySet {
  set_id
  alias
  description
  default_ref_policy
  created_at_ms
  updated_at_ms
}

RepositorySetMember {
  set_id
  repository_id
  repository_alias
  ref_selector
  resolved_commit_sha
  source_scope
  path_filters
  language_filters
  priority
}
```

`source_scope` 必须指向已经存在的单仓 `repository_snapshot`。如果成员仓库的 ref 解析到尚未索引的 snapshot，多仓查询应报告缺失或 stale，而不是自动扩大到旧 snapshot。同一 repository 再次加入同一个 set 时会替换原成员指针，避免 `HEAD` 等移动 ref 同时 fan-out 到旧 snapshot 和新 snapshot。

面向用户的 repository-set 操作只按 alias 解析。`set_id` 是暴露给存储、响应和 overlay 行的稳定标识，但不得与 alias 共用查询路径；即使某个 alias 恰好等于另一个 set 的 `set_id`，也不能读取或修改另一个 set。

## 4. 图模型

多仓库图不是把多个仓库合成一个节点空间，而是增加一个 workspace overlay。

```text
RepositorySet
  contains -> CodeRepository
  resolves_to -> RepositorySnapshot

RepositorySnapshot
  contains -> CodeFile
  defines -> CodeSymbolSnapshot
  contains -> CodeChunk

CodeSymbolSnapshot
  belongs_to -> CanonicalSymbol
  references -> CodeSymbolSnapshot
  calls -> CodeSymbolSnapshot
  imports -> ModuleReference

CrossRepositoryEdge
  from -> CodeSymbolSnapshot | CodeFile | ModuleReference
  to -> CodeSymbolSnapshot | CodeFile | ExternalPackage | UnresolvedTarget
```

`CanonicalSymbol` 仍然优先表达同一 repository 内跨 snapshot 的稳定身份。跨 repository 的同名符号不得自动合并为同一个 canonical identity，除非跨仓库 import、package/module metadata 或显式配置提供足够证据。

## 5. 存储设计

多仓库新增表应很薄：

```text
code_repository_sets
  set_id TEXT PRIMARY KEY
  alias TEXT NOT NULL UNIQUE
  description TEXT
  default_ref_policy_json TEXT NOT NULL
  created_at_ms INTEGER NOT NULL
  updated_at_ms INTEGER NOT NULL

code_repository_set_members
  set_id TEXT NOT NULL
  repository_id TEXT NOT NULL
  repository_alias TEXT NOT NULL
  ref_selector TEXT NOT NULL
  resolved_commit_sha TEXT NOT NULL
  source_scope TEXT NOT NULL
  path_filters_json TEXT NOT NULL
  language_filters_json TEXT NOT NULL
  priority INTEGER NOT NULL
  PRIMARY KEY (set_id, repository_id, source_scope)
```

存储主键保留历史 scope 身份，但写入路径把 `(set_id, repository_id)` 视为当前活动成员指针；插入替换成员前会删除同仓库旧成员行。

跨仓库派生边单独存储：

```text
code_repository_cross_edges
  edge_id TEXT PRIMARY KEY
  set_id TEXT NOT NULL
  from_source_scope TEXT NOT NULL
  from_repository_id TEXT NOT NULL
  from_record_kind TEXT NOT NULL
  from_record_id TEXT NOT NULL
  to_source_scope TEXT
  to_repository_id TEXT
  to_record_kind TEXT NOT NULL
  to_record_id TEXT
  edge_kind TEXT NOT NULL
  resolution_state TEXT NOT NULL
  confidence_basis_points INTEGER NOT NULL
  confidence_tier TEXT NOT NULL
  evidence_json TEXT NOT NULL
  created_at_ms INTEGER NOT NULL
```

禁止新增物化的虚拟事实表，例如 `code_repository_files` 中不得为 `RepositorySet` 复制成员仓库文件行。允许新增小型统计表或 query cache，但缓存必须以成员 `source_scope` 和其 index version 为失效键。

## 6. 查询语义

单仓查询路径保持不变：

```text
CodeRepositorySelector
  -> resolve repository alias
  -> resolve indexed source_scope
  -> search source_scope = ?
  -> rank and return
```

多仓查询新增 selector：

```text
CodeRepositorySetSelector {
  set_alias
  members
  ref_policy
  path_filters
  language_filters
}
```

多仓查询流程：

```text
set alias
  -> authorize set and members
  -> expand to member source scopes
  -> run bounded single-scope candidate queries
  -> merge candidates with repository metadata
  -> add cross_repository_edge evidence when available
  -> rerank and truncate
```

实现上可以先用应用层 fan-out 到现有 `search_code` 能力，再做 merge/rerank。只有在候选窗口和 observability 足够清晰后，才考虑 SQL `source_scope IN (...)` 优化路径。无论采用哪种执行方式，都必须保留每个候选的原始 `source_scope`。

Repository-set 查询必须直接搜索成员保存的 `source_scope`。查询时传入的 path 和 language filter 作为额外收窄条件应用在这个 scope 上；它们不得和成员 selector 合并成 OR 语义，也不得让查询改按仓库当前注册默认值解析。

## 7. 排序与精度约束

单仓查询不得引入多仓库排序信号。多仓库 query 可以增加以下信号：

- repository member priority。
- exact symbol 或 path match 是否来自用户显式指定的成员。
- cross repository edge confidence。
- matching repository alias、package/module name 或 dependency evidence。
- member snapshot freshness。

多仓库重排不得按路径或符号名全局去重。去重 key 至少包含 `repository_id`、`source_scope`、path、line range 和 excerpt。两个仓库中的 `src/lib.rs`、`init` 或 `main` 是不同事实实例。

当跨仓库边为 `ambiguous`、`unresolved` 或 `inferred` 时，结果必须暴露 resolution state 和 confidence，不得把它提升为单仓 `resolved` 调用或引用。多仓响应只能把 overlay edge evidence 附加到目标命中，或 path 与 line range 都匹配的 import-origin 命中；同一文件里的普通 symbol 或 chunk 不得获得 import edge bonus。

## 8. 跨仓库解析

跨仓库解析是单仓索引后的 finalization 阶段。

```text
repository set members
  -> collect exported modules and public symbols per source_scope
  -> collect import/call/reference target hints
  -> match dependency and module evidence
  -> write cross_repository_edges
  -> mark repository set overlay freshness
```

第一版只需要支持 import/module 层面的明确证据。没有 package metadata 或唯一 module match 时，边应保持 `ambiguous` 或 `unresolved`。调用和 reference 的跨仓库解析可以依赖后续语言级增强，不应阻塞多仓库基础查询。

## 9. 新鲜度、保留与失效

`RepositorySet` 的 freshness 由成员 snapshot 和 overlay edge 两部分组成：

- 如果任一成员 `source_scope` 缺失，set 为 incomplete。
- 如果任一成员 snapshot stale，set 为 stale。
- 对 `HEAD` 或分支名这类移动成员 ref，status 会通过仓库 worktree 重新解析；如果当前 ref 指向的 commit 已不同于成员保存的 snapshot，成员和 set 都应保持 stale，直到刷新成员指针。
- 如果成员 snapshot fresh 但 cross edge overlay 落后，set 可返回基础多仓查询结果，但必须标记 overlay stale。

单仓 scope retention 不能删除仍被 repository set member 引用的 scope。删除 repository set 时，可以解除这些引用；之后单仓 retention 才能按原规则清理旧 scope。

## 10. API 与 CLI 入口

新增 API 应保持单仓 API 兼容，并添加显式多仓入口：

```text
repo-set create <alias>
repo-set add <set> <repo-alias> --ref <ref> [--path <filter>] [--language <id>]
repo-set remove <set> <repo-alias>
repo-set query <set> --query <text> --kind <kind> --limit <n>
repo-set status <set>
repo-set refresh <set>
```

接口响应必须包含：

- set alias 和 set id。
- 每个 member 的 repository id、alias、requested ref、resolved commit、source scope 和 freshness。
- 每个 result 的 repository metadata 和 source scope。
- overlay freshness、cross edge evidence、truncation 和 degraded reason。

MCP 和 Web 入口应使用相同 selector，不允许用普通 `source_scope` 字符串暗中表示多仓集合。MCP 只有在 repository set alias 被显式允许且没有与已注册 repository alias 冲突，或当前每个成员 repository scope 已被静态/运行时策略允许时，才可以在运行时提升该 set alias。Repository-set 授权需要在每次 MCP 调用时重新校验，不能写入 repository alias 运行时缓存；MCP 审计也要为 repository-set query response 记录 set alias。

## 11. 实施阶段

第一阶段只做薄集合和多仓查询协调：

- 新增 repository set 表、domain 类型、storage contract 和 CLI/API 注册入口。
- `repo-set query` 展开成员并 fan-out 到现有单仓查询。
- merge/rerank 保留 repository metadata，不做跨仓库边解析。

第二阶段增加跨仓库 import overlay：

- 为 repository set 构建导出 module/symbol 只读索引。
- 写入 `code_repository_cross_edges`。
- 查询结果可附带跨仓库 edge evidence。

第三阶段优化性能和恢复：

- 增加 overlay freshness cursor、refresh queue、status diagnostics。
- 增加 bounded parallelism、per-set budgets、candidate window metrics。
- 根据实际瓶颈决定是否加入 SQL `IN` 查询或小型 materialized candidate cache。

## 12. 验收标准

- 创建多仓库 set 不会增加 `code_repository_files`、`code_repository_symbols` 或 `code_repository_chunks` 行数。
- 单仓 `repo query` 的候选窗口仍只包含一个 `source_scope`，已有单仓准确性测试不变。
- 多仓 `repo-set query` 能返回多个仓库的命中，并在每条命中上标明 repository alias、repository id、commit 和 source scope。
- 两个仓库包含同名路径或同名符号时，结果不会互相覆盖或错误去重。
- 删除或重索引一个成员仓库后，repository set status 能报告 missing、stale 或 overlay stale。
- 跨仓库边只出现在 overlay 表和多仓响应中，不回写单仓基础边表。

---

导航: 上一章: [19. 安装、发布与升级](19-installation-release-and-upgrade.md) | 下一章: [21. 软件全域建模架构](21-software-global-domain-modeling.md)
