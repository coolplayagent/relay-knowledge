# Advanced Architecture and Observability Design

[English](../../en/03-architecture-specs/advanced-architecture-observability.md) | [中文](../../zh/03-architecture-specs/advanced-architecture-observability.md)

This is the English documentation page for `specs/advanced-architecture-observability.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> 文档版本: 1.0
> 编制日期: 2026-05-11
> 适用范围: `relay-knowledge` v1 架构分层、本地运行、日志、OpenTelemetry、Prometheus/Grafana 接入
> 默认路线: 本地优先、异步优先、模块解耦、可观测性可插拔

## 1. 设计结论

`relay-knowledge` 应从一开始按可观测、可替换、可本地运行的系统来设计，而不是先实现一个只适合单机脚本的图谱工具。核心目标是让 CLI、Web、索引器、检索服务和后续 API/MCP 都共享同一套领域服务，并且每条重要工作流都能被日志、指标和 trace 解释清楚。

v1 推荐采用以下原则:

1. **本地优先**: 默认不依赖 Grafana、Prometheus、OpenTelemetry Collector 或外部数据库。用户运行 CLI 时只需要本地日志和健康输出。
2. **Telemetry 可插拔**: 通过配置打开 OTLP、Prometheus 和文件日志，不让 domain 或核心业务依赖具体 telemetry 后端。
3. **事件驱动**: 图写入、索引刷新和检索都围绕带上下文的事件流运行，保留 backpressure、超时、取消和重试空间。
4. **模块解耦**: domain、storage、indexing、retrieval、interfaces 和 observability 之间通过小接口协作，避免跨层访问具体实现。
5. **可解释运行状态**: 每次 ingest、commit、index refresh、retrieval 都能关联 `trace_id`、`graph_version`、`index_version`、耗时、错误类型和 stale 状态。

## 2. 系统分层

```text
CLI / Web / API / MCP
        |
        v
+----------------------+
| Interface Adapters   |
| args, routes, output |
+----------------------+
        |
        v
+------------------------------------------------+
| Application Services                           |
| ingestion, retrieval, orchestration, policies  |
+------------------------------------------------+
        |                    |                 |
        v                    v                 v
+---------------+    +---------------+    +----------------+
| Graph Storage |    | Indexing      |    | Event Runtime  |
| facts, log    |    | BM25/vector   |    | queues, retry  |
+---------------+    +---------------+    +----------------+
        \                    |                 /
         \                   |                /
          v                  v               v
+------------------------------------------------+
| Observability                                  |
| logs, metrics, traces, health, diagnostics     |
+------------------------------------------------+
```

### 2.1 模块职责

| 模块 | 职责 | 禁止事项 |
| --- | --- | --- |
| `domain` | 实体、关系、证据、Claim、图事件、版本、领域错误 | 不依赖数据库、Web、CLI、telemetry 后端 |
| `application` | 用例编排、事务策略、超时、取消、事件发布、降级策略 | 不包含 SQL、HTTP 展示或具体 exporter 逻辑 |
| `storage` | 图写入、图查询、mutation log、索引状态、存储健康 | 不做 LLM 抽取、rerank、UI 展示 |
| `indexing` | 消费图变更事件，刷新 BM25、semantic、vector 索引 | 不直接修改图事实，不绕过 mutation log |
| `retrieval` | 查询理解、混合召回、图扩展、freshness policy、上下文组织 | 不直接访问 SQLite 或向量后端实现 |
| `event_runtime` | 有界队列、backpressure、重试、dead-letter、后台任务生命周期 | 不承载领域判断 |
| `observability` | 日志初始化、span 约定、metrics recorder、health 聚合、diagnostics | 不影响领域模型纯度，不强制外部服务可用 |
| `interfaces` | CLI、Web、未来 API/MCP 的参数解析和展示 | 不复制核心业务逻辑 |

### 2.2 依赖方向

依赖方向必须保持单向:

```text
interfaces -> application -> domain
                          -> storage traits
                          -> indexing traits
                          -> event runtime traits
                          -> observability traits
```

具体适配器在外层组装:

- `sqlite_storage` 实现 `GraphStore`、`MutationLogStore`、`IndexStore`。
- `local_event_runtime` 实现有界队列和后台任务。
- `otel_observability` 实现 OTLP trace/metrics 输出。
- `prometheus_observability` 实现 Prometheus scrape endpoint 或 exporter。
- `cli` 和 `web` 只持有 application service，不直接持有数据库连接。

## 3. 运行模式

### 3.1 Local 模式

Local 是默认模式，适合开发、CI、单机知识库和离线使用。

| 能力 | 默认行为 |
| --- | --- |
| 日志 | `tracing` 输出到 stderr，支持 plain 或 JSON |
| 指标 | 进程内收集，CLI 可通过 diagnostics 命令展示快照 |
| Trace | 生成本地 span，上下文进入日志，不要求外部 collector |
| 存储 | SQLite-first，WAL，文件或内存数据库 |
| 索引 | 本地 BM25、semantic summary、vector backend 可选 |
| 健康检查 | CLI/Web 暴露 storage、index、queue、telemetry 状态 |

Local 模式不能因为 OTLP endpoint、Prometheus 或 Grafana 不存在而启动失败。外部 telemetry 初始化失败时，应记录 warning 并降级为本地日志。

### 3.2 Observed Local 模式

Observed Local 用于本地调试完整可观测链路。

```text
relay-knowledge
   -> stdout/stderr JSON logs
   -> OTLP traces/metrics
   -> OpenTelemetry Collector
   -> Prometheus / trace backend
   -> Grafana dashboards
```

推荐能力:

- 使用 OTLP HTTP/protobuf 上报 trace 和 metrics；当前 Rust 实现默认指向
  `http://127.0.0.1:4318`，traces 使用 `/v1/traces`，metrics 使用 `/v1/metrics`。
  配置 signal-specific endpoint 时，另一个 signal 必须路由到同级 OTLP path。
- OpenTelemetry Collector 负责转发、采样、批处理和协议转换。
- Prometheus 抓取 Collector 或应用暴露的 metrics endpoint。
- Grafana 读取 Prometheus 和 trace backend，用统一 dashboard 观察图写入、索引延迟和检索质量。

### 3.3 Service 模式

Service 模式用于长期运行的 Web/API/MCP 服务。

要求:

- 每个请求生成或继承 `trace_id`。
- 后台索引任务必须有独立 root span，并关联触发它的 `graph_version`。
- shutdown 时按顺序停止接收请求、取消后台任务、flush telemetry、关闭存储连接。
- health endpoint 区分 `ready` 和 `live`。索引 stale 不一定导致进程不 live，但可能导致 ready 降级。

## 4. Observability 设计

### 4.1 日志

Rust 实现建议以 `tracing` 作为统一入口。日志字段必须结构化，不依赖拼接长文本表达状态。

必备字段:

| 字段 | 含义 |
| --- | --- |
| `trace_id` | 跨 CLI/Web 请求、后台任务和索引事件的关联 ID |
| `request_id` | 单次接口请求或 CLI 命令执行 ID |
| `event_id` | 图事件或索引事件 ID |
| `graph_version` | 当前或目标图版本 |
| `index_kind` | `bm25`、`semantic`、`vector`、`summary` |
| `indexed_graph_version` | 索引已处理到的图版本 |
| `latency_ms` | 当前操作耗时 |
| `error_kind` | 稳定错误分类，不直接依赖错误文本 |
| `stale` | 返回结果是否使用落后索引 |
| `truncated` | 返回结果是否被预算截断 |

推荐日志事件:

- `ingest.started`、`ingest.completed`、`ingest.failed`
- `graph.commit.started`、`graph.commit.completed`、`graph.commit.failed`
- `index.refresh.requested`、`index.refresh.completed`、`index.refresh.failed`
- `retrieval.started`、`retrieval.completed`、`retrieval.degraded`
- `storage.busy`、`storage.migration.completed`
- `telemetry.export.failed`

### 4.2 Metrics

Metrics 命名建议保持稳定，避免把高基数字段放进 label。

| 指标 | 类型 | 说明 |
| --- | --- | --- |
| `relay_requests_total` | counter | 按 interface、operation、status 统计请求数 |
| `relay_request_duration_ms` | histogram | CLI/Web/API 请求耗时 |
| `relay_graph_commit_duration_ms` | histogram | 图写入事务耗时 |
| `relay_graph_version` | gauge | 当前最新图版本 |
| `relay_mutation_events_total` | counter | mutation log 追加数量 |
| `relay_event_queue_depth` | gauge | 后台事件队列深度 |
| `relay_event_queue_dropped_total` | counter | 因 backpressure 丢弃或拒绝的事件 |
| `relay_index_lag_versions` | gauge | 各索引落后图版本数 |
| `relay_index_refresh_duration_ms` | histogram | 索引刷新耗时 |
| `relay_retrieval_duration_ms` | histogram | 检索端到端耗时 |
| `relay_retrieval_stale_total` | counter | 使用 stale 索引返回的次数 |
| `relay_storage_errors_total` | counter | 按稳定错误类型统计存储错误 |
| `relay_telemetry_export_errors_total` | counter | telemetry exporter 失败次数 |

Label 规则:

- 允许低基数 label: `interface`、`operation`、`status`、`index_kind`、`error_kind`。
- 禁止高基数 label: `entity_id`、`query_text`、`source_uri`、完整文件路径、用户输入原文。
- 需要排障的高基数信息进入日志和 trace attribute，不进入 metrics label。

### 4.3 Traces

Trace 应覆盖端到端关键路径:

```text
request/cli command
  -> ingestion.extract
  -> graph.commit
  -> mutation_log.append
  -> event.publish
  -> index.refresh
  -> retrieval.hybrid_search
  -> retrieval.graph_expand
  -> response.render
```

Span 约定:

- `graph.commit` 标记 `graph_version`、`mutation_count`、`affected_entity_count`。
- `index.refresh` 标记 `index_kind`、`from_graph_version`、`to_graph_version`、`stale_before_versions`。
- `retrieval.hybrid_search` 标记 `retrieval_mode`、`allow_stale`、`candidate_count`、`truncated`。
- `storage.query` 标记 `query_kind`、`budget_ms`、`result_count`，不记录原始 SQL。
- `event.queue_wait` 标记 `queue_name`、`depth_at_enqueue`、`wait_ms`。

## 5. 配置接口

后续实现建议提供单一 `TelemetryConfig`，由 CLI 参数、环境变量和配置文件共同生成。配置优先级应为 CLI 参数高于环境变量，高于配置文件默认值。

```rust
pub struct TelemetryConfig {
    pub service_name: String,
    pub environment: String,
    pub log_level: String,
    pub log_format: LogFormat,
    pub log_file: Option<PathBuf>,
    pub otlp_endpoint: Option<String>,
    pub metrics_endpoint: Option<SocketAddr>,
    pub traces_enabled: bool,
    pub metrics_enabled: bool,
}

pub enum LogFormat {
    Plain,
    Json,
}
```

建议环境变量:

| 变量 | 含义 |
| --- | --- |
| `RELAY_LOG_LEVEL` | `error`、`warn`、`info`、`debug`、`trace` |
| `RELAY_LOG_FORMAT` | `plain` 或 `json` |
| `RELAY_LOG_FILE` | 可选本地日志文件路径 |
| `RELAY_OTEL_ENDPOINT` | OTLP collector endpoint |
| `RELAY_OTEL_TRACES` | 是否启用 trace export |
| `RELAY_OTEL_METRICS` | 是否启用 metrics export |
| `RELAY_OTEL_EXPORT_TIMEOUT_MS` | OTLP export 超时 |
| `RELAY_OTEL_SERVICE_ENVIRONMENT` | `deployment.environment` resource attribute |
| `RELAY_METRICS_ADDR` | Prometheus metrics endpoint 监听地址 |

当前 Rust 实现已提供 OTLP traces/metrics exporter、shutdown flush 和进程内 diagnostics snapshot。
单个 signal exporter 初始化失败只记录 diagnostics，不能阻断另一个 signal；trace export
不可用时保持本地 tracing fallback。
Prometheus 应优先从 Collector 抓取或转换；应用内独立 Prometheus endpoint 仍可作为后续补充。

## 6. 健康检查和诊断

健康状态应由各模块上报，再由 `HealthReporter` 聚合。

```text
HealthReporter
  -> StorageHealth
  -> IndexHealth
  -> EventRuntimeHealth
  -> TelemetryHealth
  -> InterfaceHealth
```

健康维度:

| 维度 | Ready 降级条件 | Live 失败条件 |
| --- | --- | --- |
| storage | 数据库 busy、迁移未完成、WAL 过大 | 存储不可打开或 schema 不兼容 |
| indexing | 关键索引落后超过阈值 | 索引器任务崩溃且不可重启 |
| event runtime | 队列接近满、重试堆积 | 事件 runtime 停止 |
| telemetry | exporter 连续失败 | 不应单独导致 live 失败 |
| retrieval | stale 降级比例过高 | 核心检索服务不可构造 |

CLI 应提供 diagnostics 输出，至少包含:

- 当前 `graph_version`。
- 各索引 `indexed_graph_version` 和 lag。
- 事件队列深度和最近失败。
- SQLite WAL 状态和最近 checkpoint。
- telemetry exporter 是否启用及最近错误。

## 7. Grafana 能力

Grafana dashboard 应围绕运营问题组织，而不是只展示底层指标。

建议 dashboard 分区:

1. **System Overview**: 请求量、错误率、p95 延迟、当前 graph version、索引最大 lag。
2. **Ingestion and Graph Writes**: 写入批次、commit 延迟、mutation 数量、storage busy。
3. **Indexing Freshness**: 各索引 lag、刷新耗时、失败率、队列深度。
4. **Retrieval Quality and Latency**: 检索耗时、stale 返回次数、truncated 次数、候选数量。
5. **Telemetry Health**: exporter 错误、Collector 连接状态、丢弃事件数。

Alert 建议:

- `relay_index_lag_versions` 超过配置阈值持续 5 分钟。
- `relay_storage_errors_total` 在 5 分钟内快速增长。
- `relay_event_queue_depth` 长时间超过容量 80%。
- `relay_retrieval_stale_total` 比例异常升高。
- telemetry export 错误持续出现，但只作为观测链路告警，不作为业务不可用告警。

Proposal 和 worker 观测要求:

- proposal 记录必须携带 provenance，可按 producer、provider、model、prompt id/version、schema version 和 input source hash 过滤。
- worker 请求应在 audit detail 或 diagnostics 中体现 request timeout、lease、max attempts、max in-flight 和 fallback/degraded reason。
- extractor 结构化事实被降级为 `proposed` 时，应作为正常治理行为记录，而不是错误；若 worker 返回 schema 不匹配，则记录 degraded fallback。

## 8. 实施顺序

建议按以下顺序落地，降低耦合风险:

1. 建立 `domain` 和 `application` 基础边界，保留纯单元测试能力。
2. 引入 `tracing`，先完成结构化本地日志和 span 约定。
3. 实现 `HealthReporter` 和 CLI diagnostics 快照。
4. 接入 metrics recorder，先提供进程内快照，再开放 Prometheus endpoint。
5. 接入 OpenTelemetry trace export，失败时降级为本地日志。
6. 提供 OpenTelemetry Collector、Prometheus、Grafana 的示例配置。
7. 为 ingest、commit、index refresh、retrieval、worker proposal 和 proposal decision 增加 dashboard 和告警规则。

## 9. 验收标准

架构和实现应满足:

- CLI 在无任何外部服务时可正常运行，并输出可读日志。
- 打开 JSON 日志后，每条关键事件包含稳定字段。
- 打开 OTLP 后，Grafana 可按 `trace_id` 关联请求、图写入和索引刷新。
- 索引落后时，health 和 metrics 能明确展示 lag。
- telemetry exporter 失败不会导致图写入或检索失败。
- domain 单元测试不需要初始化日志、数据库或 runtime。
- storage、indexing、retrieval 可通过 trait fake 做集成测试。
