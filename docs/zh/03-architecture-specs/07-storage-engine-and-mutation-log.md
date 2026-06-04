# 存储引擎与 Mutation Log

[中文](../../zh/03-architecture-specs/07-storage-engine-and-mutation-log.md) | [English](../../en/03-architecture-specs/07-storage-engine-and-mutation-log.md)

> 文档版本: 2.2
> 编制日期: 2026-06-04
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

存储层是图谱状态机，不是数据访问工具。SQLite-first 是默认产品路线，因为它提供本地零依赖、事务、WAL、FTS5、递归 CTE 和低测试成本；但业务层只能依赖存储 trait，不能依赖 SQLite 细节。

## 2. 存储边界

```text
Application Services
        |
        v
Storage Facade: GraphStore, MutationLogStore, IndexStore, CodeGraphStore
        |
        +--> SQLite Adapter: WAL, transactions, FTS5, CTE
        +--> Future Adapters: SurrealDB, Neo4j, NebulaGraph, Memgraph
```

`domain` 不依赖 SQL 或连接池；`retrieval` 不绕过 storage facade；`interfaces` 不复制存储逻辑。

在服务化部署中，主运行时数据库是控制面状态机，负责 task、lease、audit、operator、topology catalog、repository membership 和诊断；代码事实 shard、派生索引和外部后端是数据面实现。控制面和数据面可以物理分离，但调用边界仍必须是 application service 和 storage facade，不能让接口层或 worker 直接访问具体后端。

## 3. 写入事务

图写入固定流程：

```text
validate domain model
  -> begin transaction
  -> upsert evidence/entities/relations/claims/events/code facts
  -> append graph_mutations
  -> bump graph_version
  -> mark affected index cursors stale
  -> commit
  -> publish refresh work
```

图事实、mutation log 和 graph version bump 必须处于同一事务。索引刷新只能在事务提交后发布。

## 4. Mutation Log

Mutation log 是索引恢复和审计的脊柱。每条 mutation 至少包含：graph version、affected scopes、affected entities、affected evidence、source hashes、fact kinds、index families、actor/runtime identity 和 trace id。

索引器只消费 mutation log 和 scoped cursor，不扫描全库猜测需要刷新什么。

## 5. SQLite 运行模型

SQLite 操作可以是同步驱动，但必须通过 blocking worker、专用连接或池边界隔离，不能占用 async executor。写入使用 batch transaction；读取使用 prepared statements 和组合索引；FTS candidate window 必须先应用 scope/path/language filter。

## 6. SQLite 存储拓扑

默认拓扑是 `single_sqlite`：所有图事实、mutation log、worker 状态、审计、文件索引和代码仓库事实共享同一运行时数据库。可选拓扑 `partitioned_sqlite` 是 v1 数据面扩展实现：它使用同一 storage trait contract，把全局控制状态保留在主数据库，并把每个代码仓库的文件、符号、引用、chunk、checkpoint 和 scope 查询路由到独立 SQLite shard。shard 文件必须由 `paths` 生成，位于运行时数据目录下的 `stores/repositories/<safe-id-hash>/code.sqlite`，不能写入源码仓库、当前工作目录或 release 解压目录。

分片拓扑中的 durable task、lease、dead-letter、审计、图事实和 repository set membership 仍由控制库负责，保证最多一个 active writer task per repository 的约束不被绕过。跨仓 overlay refresh 在实现跨 shard import/export 聚合前必须显式返回不可用，不能伪造 fresh 状态或复制基础代码事实到控制库。

包含 active 分片目录记录的运行时数据库不再是有效的 `single_sqlite` 数据库。使用 `single_sqlite` 启动时必须 fail fast，不能只从控制库返回仓库状态而让代码事实留在 shard 文件中不可见。

shard catalog 只能保存可迁移的路由 metadata。读取侧验证 active catalog 记录，但必须从 `RuntimePaths` 和 `repository_id` 重新计算 shard 文件路径，保证备份恢复或 runtime home 移动不会依赖过期绝对路径。

scope 迁移到 shard 时必须同时保留该 scope 的代码事实、checkpoint progress 和派生 software projection 行；否则在派生行刷新完成前必须继续把该 scope 路由到控制库。

## 7. 后端演进

未来图数据库、向量数据库、复制 SQLite 或远程存储 adapter 必须实现同一 contract tests。新后端不能改变 domain fact model、mutation log 语义、freshness contract、degraded reason 或 stable error kind；只能改变持久化、分片、索引和查询原语的实现。

## 8. 验收标准

- 任一存储写入要么完整提交事实、mutation 和版本，要么完整回滚。
- 索引失败不会回滚图写入，但会产生 stale/degraded 诊断。
- SQLite 专用优化不泄漏到 domain、api 或 interface 类型。
- 分片拓扑不能绕过 durable task lease、bounded retry/backoff、checkpoint replay 或 per-repository writer 约束。
- 控制面/数据面分离部署不能让 worker、Web、MCP 或 CLI 绕过 storage facade 直接读写 shard 或外部后端。

---

导航: 上一章: [6. 图事实模型与版本化](06-graph-fact-model-and-versioning.md) | 下一章: [8. 派生索引与新鲜度](08-derived-indexes-and-freshness.md)
