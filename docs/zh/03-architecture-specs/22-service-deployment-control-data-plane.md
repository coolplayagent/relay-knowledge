# 服务化部署、控制面与数据面分离

[中文](../../zh/03-architecture-specs/22-service-deployment-control-data-plane.md) | [English](../../en/03-architecture-specs/22-service-deployment-control-data-plane.md)

> 文档版本: 1.0
> 编制日期: 2026-06-04
> 适用范围: 第三卷架构与算法白皮书；GitHub issue #250

## 1. 设计结论

`relay-knowledge` 的服务化部署路线是 SQLite-first 的控制面/数据面分离，而不是立即引入外部图数据库、消息队列或 Kubernetes operator。v1 继续保持本地零依赖和单二进制体验，同时把现有 `service run`、HTTP `/api/*`、MCP、QoS、持久 worker 队列、operator 状态和 `partitioned_sqlite` 明确为可演进的服务化底座。

控制面拥有配置、权限、API、任务租约、审计、运行状态、拓扑 catalog、升级/回滚、诊断，以及监督有界 worker pool 的 resident master。数据面拥有图事实、代码事实、派生索引、查询执行和仓库 shard。所有接口必须通过 application service 和 storage trait 进入，不能让 Web、MCP、CLI 或 worker 直接访问 SQLite shard、外部后端或索引文件。

## 2. 竞品技术结论

现有竞争力技术说明了方向，但不改变 v1 默认架构：

| 技术类别 | 代表实现 | 可借鉴能力 | relay-knowledge 决策 |
| --- | --- | --- | --- |
| 图数据库集群 | Neo4j、NebulaGraph、Memgraph | 数据库/服务器管理分离、查询服务与存储服务分离、读扩展和故障恢复 | 只借鉴控制面/数据面边界；后端 adapter 必须实现现有 storage contract |
| 多模型数据库 | SurrealDB | 单二进制、嵌入式到分布式、文档/图/向量/全文统一 | 保持单二进制体验；不把多模型查询语言泄漏到 domain/API |
| 向量数据库 | Qdrant、Milvus、Weaviate、LanceDB、pgvector | 向量 shard、payload filter、BM25+vector hybrid、索引/查询节点拆分 | 作为 semantic/vector read model adapter 候选；不能替代 graph version 和 freshness contract |
| 事件与流平台 | NATS JetStream、Kafka/Redpanda | 持久消息、重放、consumer、Raft metadata/control plane | v1 继续使用 SQLite durable task；未来 event transport 不得替代 mutation log |
| 工作流运行时 | Temporal | service/worker 分离、event history、失败后继续执行 | 只借鉴 durable execution 语义；任务仍用 attempt-scoped lease、checkpoint 和 dead-letter |
| 嵌入式/边缘存储 | SQLite、libSQL/Turso、RocksDB 类引擎 | 本地优先、可复制、低安装成本 | v1 默认 SQLite；远程/复制后端必须保留备份、迁移、doctor 和卸载语义 |

参考资料：

- Neo4j Operations Manual: <https://neo4j.com/docs/operations-manual/current/clustering/introduction/>
- NebulaGraph architecture: <https://docs.nebula-graph.io/3.8.0/1.introduction/3.nebula-graph-architecture/1.architecture-overview/>
- SurrealDB documentation: <https://surrealdb.com/docs/surrealdb>
- Qdrant overview: <https://qdrant.tech/documentation/overview/>
- Milvus overview: <https://milvus.io/docs/overview.md>
- NATS JetStream: <https://docs.nats.io/nats-concepts/jetstream>
- Kafka KRaft overview: <https://docs.confluent.io/platform/current/kafka-metadata/kraft.html>
- Temporal platform documentation: <https://docs.temporal.io/temporal>

## 3. 部署拓扑

v1 支持并文档化四种拓扑：

| 拓扑 | 控制面 | 数据面 | 用途 |
| --- | --- | --- | --- |
| `embedded_cli` | CLI 进程内 application service | `single_sqlite` | 临时命令、测试、开发机一次性操作 |
| `resident_single_process` | `service run` HTTP/Web/MCP/operator/master-worker pools | `single_sqlite` | 默认常驻服务，最小运维成本 |
| `resident_partitioned_sqlite` | 主 SQLite 控制库 | 每仓库 SQLite shard | 大仓库和多仓库本地扩展 |
| `split_worker_preview` | 常驻控制服务 | 独立 worker 进程 claim task 后工作 | 未来进程级扩展，禁止无 lease 写入 |

长期后台运行必须由 systemd、Windows Service 或 launchd 托管。`run.sh --daemon` 只适合开发验证，不是正式安装模型。任何 split worker 或未来远程 worker 都必须先通过控制面 claim durable task，获得 attempt-scoped lease 后才能触发数据面写入。

## 4. 控制面职责

控制面 API 必须覆盖：

- runtime/config/status/health/doctor，不执行长任务、不阻塞查询热路径。
- service manager plan、definition write、operator pause/resume/status。
- worker task queue、lease、retry、dead-letter、checkpoint、progress 和 reset。
- code-index master-worker 诊断，包括 configured workers、active worker slots、queue depth、running leases、retry/dead-letter state。
- storage topology、shard catalog、backup/migration/rollback/uninstall diagnostics。
- repository register/index/status/report/set overlay refresh。
- audit、authorization identity、request id、trace id、QoS admission 和 overload decision。

新增控制面接口必须先定义共享 `api` request/response 类型和 application service 方法，再映射到 CLI、Web、MCP 或 HTTP route。接口层不得复制业务逻辑、直接读取 storage catalog、直接续租 worker task，或绕过 QoS。当前只读控制面 HTTP preview 暴露 `/api/v1/control/status`、`/api/v1/control/health`、`/api/v1/control/service/status` 和 `/api/v1/control/storage/topology`。这些 route 必须在 cold runtime 上保持安全：health 和 service-status 诊断不得打开或迁移 storage；topology diagnostics 只能使用有界只读 catalog probe，并且要暴露 single-SQLite 配置下残留的 active partitioned catalog；backlog counter 必须使用 storage count API，不能 materialize 无界列表。

## 5. 数据面职责

数据面只负责执行被控制面授权和预算约束的读写：

- 图事实、mutation log、graph version 和 index cursor 的事务一致性。
- 代码仓库文件、符号、引用、chunk、software projection、checkpoint 和 scope 查询。
- BM25、semantic、vector、代码检索和 canvas read model 的 bounded 查询。
- 外部 graph/vector/storage adapter 的持久化和查询原语。

数据面不得拥有 service lifecycle、用户授权、scope policy、operator pause/resume、任务调度、dead-letter 恢复或升级决策。外部后端不可把缺少依赖、授权不足、索引落后或存储繁忙伪装成 fresh 成功。

## 6. 存储扩展契约

新后端必须实现同一组 contract tests：

- 写入要么同时提交事实、mutation log、graph version、affected cursor，要么完整回滚。
- 读取必须显式携带 graph version、source scope、limit、freshness policy 和 budget。
- 后端可改变物理分片、索引和查询计划，但不能改变 domain fact model、mutation log、freshness、degraded reason 或 error kind。
- `partitioned_sqlite` 的主库和 shard 目录是一个 runtime state 集合；备份、迁移、doctor、卸载和回滚不能只处理主库。
- 任一仓库最多一个 active writer task；跨进程或跨后端部署也必须由 durable lease 保护。

## 7. API 扩展契约

控制面 HTTP route 使用 `/api/*`，同源 Web 操作继续使用 `/api/web/operations/execute`。外部控制面 API 使用 `/api/v1/control/*` 或等价命名；当前 preview 只开放只读 status、health、service status 和 storage topology diagnostics，并保持 CLI JSON、Web、MCP tool 的语义兼容。

API response 必须包含 metadata、warnings/degraded state、freshness/truncation、stable error kind 和 trace context。长任务只返回 task handle、checkpoint 和可查询 status；不能同步执行无界索引、无界扫描、外部 provider 大批量调用或 shard 迁移。

## 8. 验收标准

- `single_sqlite` 拒绝打开已有 active shard catalog 的 runtime database。
- `partitioned_sqlite` 的 doctor/status、backup、migration、uninstall plan 同时覆盖控制库和 shard 目录，并通过 storage diagnostics 暴露 active/staged/missing shard 计数。
- split worker preview 通过 `service worker run [--task-id <id>]` claim durable code-index task；未 claim、lease 过期或 attempt 不匹配时无法 complete/fail/write。
- `health`、`service status` 和 Web diagnostics 在数据面繁忙时仍返回 bounded degraded 状态。
- 新 graph/vector/event/workflow adapter 只作为实现细节进入 storage、retrieval、net 或 worker boundary，不改变 domain/API 语义。

---

导航: 上一章: [21. 软件全域建模架构](21-software-global-domain-modeling.md)
