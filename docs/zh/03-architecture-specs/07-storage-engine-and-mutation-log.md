# 存储引擎与 Mutation Log

[中文](../../zh/03-architecture-specs/07-storage-engine-and-mutation-log.md) | [English](../../en/03-architecture-specs/07-storage-engine-and-mutation-log.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
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

## 6. 后端演进

未来图数据库 adapter 必须实现同一 contract tests。新后端不能改变 domain fact model、mutation log 语义或 freshness contract；只能改变持久化和查询原语的实现。

## 7. 验收标准

- 任一存储写入要么完整提交事实、mutation 和版本，要么完整回滚。
- 索引失败不会回滚图写入，但会产生 stale/degraded 诊断。
- SQLite 专用优化不泄漏到 domain、api 或 interface 类型。

---

导航: 上一章: [6. 图事实模型与版本化](06-graph-fact-model-and-versioning.md) | 下一章: [8. 派生索引与新鲜度](08-derived-indexes-and-freshness.md)
