# 第 16 章 SRE 运维手册

[中文](16-sre-operations-runbook.md) | 中文深入专题；英文核心流程见 [English Chapter 9](../../en/01-user-guide/09-resident-service.md)

> 本章面向值守与故障处置；部署、升级、回滚和卸载流程见[第 15 章](15-service-deployment-full-guide.md)，安全策略见[第 17 章](17-security-configuration.md)。

## 16.1 概述：SRE 运维全景图

relay-knowledge 是一个基于 Rust async 运行时构建的知识图谱服务，采用事件驱动架构，核心数据存储在 SQLite 中（支持单库和分区拓扑）。SRE 需要关注的运维面包括：

| 运维领域 | 核心能力 | 关键命令/端点 |
|---------|---------|-------------|
| 服务生命周期 | systemd user service 托管启停，graceful shutdown | `systemctl --user start/stop/restart relay-knowledge.service` |
| 健康诊断 | 只读诊断 + 带 reconcile 的完整检查 | `/api/health`、`/api/v1/control/health` |
| 存储拓扑 | 控制库 + 每仓库独立 shard | `/api/v1/control/storage/topology` |
| 监控指标 | MCP Prometheus 快照 + OpenTelemetry OTLP 导出 | `/mcp/metrics`、`/v1/traces`、`/v1/metrics` |
| 容量管理 | QoS 连接/请求/队列限流 | `service status` 输出 |
| 备份恢复 | 控制库与全部 shard 的停服一致快照 | 停服校验、完整性检查、校验和与可回滚恢复 |

---

## 16.2 服务生命周期管理

### 16.2.1 启动流程

服务启动时按以下顺序初始化（参见 `src/relay_knowledge/interfaces/cli/service/mod.rs`）：

```text
1. RuntimeConfiguration::from_process_environment()  — 读取环境变量
2. runtime.observability.initialize()               — 安装 OpenTelemetry 导出器
3. service.reconcile_startup_indexes()              — 对账启动时索引
4. service.recover_orphaned_code_index_tasks_on_startup() — 恢复孤儿任务租约
5. 启动后台循环：
   - code_repository_watcher
   - file_index_loop        (如果启用)
   - code_index_worker_pool (code_index_max_in_flight 个 worker，每 5s 轮询)
   - code_repository_set_refresh_loop (每 5s 轮询)
6. 根据参数运行 Web + MCP 合并路由、仅 MCP 路由，或等待关闭信号
```

**启动命令示例：**

```bash
# systemd user service 启动（使用部署用户）
systemctl --user start relay-knowledge.service

# 或直接以 long-running 模式启动（需要配置 service 定义文件）
relay-knowledge service run --web --mcp streamable-http
```

### 16.2.2 停止与 Graceful Shutdown

服务通过信号实现优雅关闭（参见 `service_shutdown_signal()`）：

- **Linux/macOS**：同时监听 `SIGTERM` 和 `Ctrl+C`（SIGINT），任一信号触发关闭
- **关闭顺序**：停止 repository watcher，再通知并等待 file-index、code-index 和 repository-set refresh 循环，最后关闭可观测性运行时。

```bash
# 优雅停止 systemd user service
systemctl --user stop relay-knowledge.service
```

非托管前台进程应在原终端用 `Ctrl+C` 停止；需要发送信号时，先审计并记录准确 PID，禁止把
`pgrep` 的多行结果直接展开给 `kill`。

**注意事项**：

- 空闲 worker 可在收到 watch 通知时立即退出；5 秒是无任务时的轮询间隔，不是整个关闭流程的硬上限。
- 已进入一次 code-index 任务的 worker 在本次有界尝试返回后才观察关闭信号；值守时应观察任务 checkpoint 和 lease，不要把 5 秒当作强制退出承诺。
- 遥测导出超时由 `RELAY_OTEL_EXPORT_TIMEOUT_MS` 控制，默认为 5 秒。

### 16.2.3 服务状态查看

```bash
# CLI 方式：带 reconcile 的完整状态（会尝试对账索引）
relay-knowledge service status

# Web API 方式
curl http://localhost:8791/api/service/status
```

输出包含：`service_name`、`mode`（active/disabled）、`background_enabled`、`silent_updates_enabled`、
`service_definition_path`、`storage`、`index_refresh`、`file_index`、`agent_protocols`、
`operator`、`workers`、`code_index_workers`、`proposal_backlog`、`audit_sink`。

---

## 16.3 健康检查与诊断

### 16.3.1 Health Endpoint（`/api/health`）

核心健康检查 API（参见 `src/relay_knowledge/application/service/health/mod.rs`）：

- 底层存储健康快照必须在 **500ms 内**完成（`HEALTH_STORAGE_BUDGET`）
- 超时则返回缓存的健康状态（`degraded_cached_health`），标记 `healthy: false`
- 存储 busy 时同样返回降级缓存结果
- `healthy: true` 的条件：无 `degraded_reason` 且所有已启用索引的版本不低于当前 graph_version

```bash
# 检查服务健康
curl http://localhost:8791/api/health | jq .

# 输出示例
{
  "healthy": true,
  "storage": { ... },
  "graph": { ... },
  "indexes": [ ... ],
  "index_refresh": { ... }
}
```

### 16.3.2 只读健康检查（`/api/v1/control/health`）

`read_only_health` 与 `health` 的区别：
- **不打开冷存储**：如果存储未就绪，返回 storage-free 健康状态
- **不尝试 reconcile**：仅观察现有状态
- 适合监控系统高频轮询，避免触发索引对账

```bash
curl http://localhost:8791/api/v1/control/health | jq .healthy
```

### 16.3.3 `service status`（带 reconcile）vs `read_only_service_status`

两种模式（来自 `ServiceStatusRefreshMode` 枚举）：

| 特性 | `service_status` (Reconcile) | `read_only_service_status` (Observe) |
|-----|------|-----|
| 索引对账 | 执行 `reconcile_index_refreshes` | 只读取现有 `index_refresh_outcome` |
| code-index worker 状态 | 执行 `code_index_worker_status` | 执行 `read_only_code_index_worker_status` |
| 存储状态 | 需要存储就绪 | 存储未就绪时返回 storage-free 状态 |
| 适用场景 | 运维排障、手动诊断 | 监控轮询、自动化告警 |

```bash
# 带 reconcile 的状态（CLI）
relay-knowledge service status

# 只读状态（Web API）
curl http://localhost:8791/api/v1/control/service/status
```

### 16.3.4 Doctor 检查（`relay-knowledge setup doctor`）

`setup doctor` 是不打开存储的配置就绪性检查（来源：`src/relay_knowledge/interfaces/cli/setup/mod.rs`）：

| 检查项 | 验证内容 |
|-------|---------|
| `runtime_paths` | config_dir、data_dir、log_dir 是否就绪 |
| `network_budget` | HTTP bind、body_bytes、QoS connections/in_flight/queue 是否 > 0 |
| `retrieval_backends` | semantic_backend_mode、vector_backend_mode、embedding_dimension 是否配置 |
| `mcp_scope_policy` | 启用 MCP 时是否配置了允许的 scope 或显式允许未指定 scope |
| `service_directory` | platform service definition 目录是否可解析 |
| `worker_budget` | worker 并发预算是否大于 0，并显示 silent-update 状态 |

```bash
relay-knowledge setup doctor

# setup doctor 不检查实时存储、索引和已安装服务状态；按建议继续检查
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

### 16.3.5 存储拓扑快照（`/api/v1/control/storage/topology`）

返回 `StorageTopologyResponse`，其 `storage` 字段为 `StorageTopologyDiagnostics`（参见 `src/relay_knowledge/application/service/storage_diagnostics/mod.rs`）。拓扑快照有 500 毫秒预算；超时时通过 `storage.degraded_reason` 报告，不无界等待。

```bash
curl http://localhost:8791/api/v1/control/storage/topology | jq .
```

以下字段位于响应的 `storage` 对象中：

| 字段 | 说明 |
|-----|------|
| `topology` | `single_sqlite` 或 `partitioned_sqlite` |
| `control_database_path` | 控制库文件路径 |
| `repository_shards_dir` | 分区 shard 目录路径 |
| `shard_catalog_active` | shard catalog 是否有活跃 shard |
| `active_shard_count` | 活跃 shard 数 |
| `staged_shard_count` | 暂存 shard 数 |
| `missing_shard_count` | 缺失 shard 数（需立即处理） |
| `shards[]` | 每个 shard 的详细信息（repository_id、state、path、scope_count、exists） |
| `degraded_reason` | 降级原因（如 missing shard） |

**关键告警指标**：`missing_shard_count > 0` 表示 shard 文件丢失。

---

## 16.4 监控指标

### 16.4.1 OpenTelemetry 配置

遥测配置通过环境变量控制（参见 `src/relay_knowledge/observability/mod.rs`）：

| 环境变量 | 默认值 | 说明 |
|---------|-------|------|
| `RELAY_OTEL_ENDPOINT` | `http://127.0.0.1:4318` | OTLP Collector 地址 |
| `RELAY_OTEL_TRACES` | `false` | 启用 Trace 导出 |
| `RELAY_OTEL_METRICS` | `false` | 启用 Metric 导出 |
| `RELAY_OTEL_EXPORT_TIMEOUT_MS` | `5000` | 导出超时（毫秒） |
| `RELAY_OTEL_SERVICE_ENVIRONMENT` | `local` | 部署环境标签 |

**生产环境典型配置：**

```bash
export RELAY_OTEL_ENDPOINT="http://otel-collector:4318"
export RELAY_OTEL_TRACES="true"
export RELAY_OTEL_METRICS="true"
export RELAY_OTEL_SERVICE_ENVIRONMENT="production"
```

### 16.4.2 Traces

- 使用 OTLP HTTP 协议，导出到 `{endpoint}/v1/traces`
- 通过 `opentelemetry_otlp::SpanExporter` + batch exporter 导出
- `tracing-opentelemetry` layer 将 `tracing` span 桥接到 OpenTelemetry
- 同时保留 `tracing_subscriber::fmt::layer` 用于本地日志输出
- 默认日志级别为 `info`（可通过 `RUST_LOG` 覆盖）

### 16.4.3 OpenTelemetry Metrics

所有指标通过 `SdkMeterProvider` + `PeriodicReader`（每 5s 导出）导出到 `{endpoint}/v1/metrics`。

#### 16.4.3.1 Agent 协议指标

| 指标名 | 类型 | 标签 | 说明 |
|-------|------|------|------|
| `relay_agent_protocol_requests_total` | Counter | `protocol`, `operation`, `status` | 协议请求总数 |
| `relay_agent_protocol_request_duration_ms` | Histogram | `protocol`, `operation` | 请求延迟（毫秒） |
| `relay_agent_context_truncated_total` | Counter | `protocol`, `reason` | 上下文截断次数 |
| `relay_agent_protocol_rejections_total` | Counter | `protocol`, `reason` | 协议拒绝次数 |
| `relay_agent_retrieval_cancelled_total` | Counter | `protocol` | 检索取消次数 |

**PromQL 查询示例**（适用于已将 OTLP metrics 转发到 Prometheus 的 Collector）：

```promql
# 请求速率
rate(relay_agent_protocol_requests_total[5m])

# 拒绝率
rate(relay_agent_protocol_rejections_total[5m]) / rate(relay_agent_protocol_requests_total[5m])

# P99 延迟
histogram_quantile(0.99, rate(relay_agent_protocol_request_duration_ms_bucket[5m]))
```

#### 16.4.3.2 诊断快照指标

`AgentProtocolMetricsSnapshot` 提供内存中的低基数指标（通过 `service status` 暴露）：

| 字段 | 说明 |
|-----|------|
| `requests_total` | 累计请求数 |
| `request_duration_ms_total` | 累计延迟毫秒 |
| `rejections_total` | 累计拒绝数 |
| `cancelled_total` | 累计取消数 |
| `context_truncated_total` | 累计上下文截断数 |

所有计数器使用 `saturating_add` 防止溢出。

### 16.4.4 MCP Prometheus 快照

启用 MCP Streamable HTTP 后，`GET /mcp/metrics` 会直接返回 Prometheus text exposition 格式。该路由使用与 MCP 相同的 origin 校验、QoS 准入和 `max_runtime_ms` 超时，并读取有界健康快照：

```bash
curl -fsS http://127.0.0.1:8791/mcp/metrics
```

直接快照包含图版本、索引刷新队列/死信、QoS 在途/排队数与累计计数、MCP 冷启动采样以及按索引类型标注的 stale 状态。实际指标名以响应中的 `relay_knowledge_*` 为准。

`/mcp/metrics` 是运行时快照，不代替 OTLP 的分布式 trace 和带标签协议指标。需要将 OTLP metrics 暴露给 Prometheus 时，可在 Collector 中配置 exporter：

```yaml
# otel-collector-config.yaml
receivers:
  otlp:
    protocols:
      http:
        endpoint: 0.0.0.0:4318

exporters:
  prometheus:
    endpoint: 0.0.0.0:9464

service:
  pipelines:
    metrics:
      receivers: [otlp]
      exporters: [prometheus]
```

---

## 16.5 告警阈值建议

### 16.5.1 QoS 水位告警

QoS 默认值（参见 `src/relay_knowledge/net/qos/mod.rs`）：

| 参数 | 默认值 | 环境变量 |
|-----|-------|---------|
| `max_connections` | 1024 | `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` |
| `max_in_flight_requests` | 256 | `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` |
| `max_queue_depth` | 512 | `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` |

**告警规则：**

| 告警项 | 阈值 | 严重级别 | 说明 |
|-------|------|---------|------|
| 连接使用率 > 80% | `connections >= max_connections * 0.8` | Warning | 接近连接预算上限 |
| 连接耗尽 | `connections >= max_connections` | Critical | 新连接将被拒绝 (ConnectionBudgetExceeded) |
| 请求并发 > 80% | `in_flight >= max_in_flight * 0.8` | Warning | 接近并发上限 |
| 请求并发耗尽 | `in_flight >= max_in_flight` | Critical | 新请求将被拒绝 (RequestBudgetExceeded) |
| 队列深度 > 80% | `queued >= max_queue_depth * 0.8` | Warning | 排队积压 |
| 队列耗尽 | `queued >= max_queue_depth` | Critical | 新排队请求将被拒绝 (QueueBudgetExceeded) |

`/mcp/metrics` 快照不导出连接数或配置上限，因此比例告警需从 service/control status 取预算与使用量后在监控系统中求比，不应自行假设存在未导出的比率指标。以下规则只使用直接快照确实暴露的指标：

```yaml
# relay-knowledge-qos-alerts.yml
groups:
  - name: relay_knowledge_qos
    rules:
      - alert: RelayKnowledgeQoSRejecting
        expr: increase(relay_knowledge_qos_rejected_total[5m]) > 0
        labels:
          severity: warning
        annotations:
          summary: "relay-knowledge 在过去 5 分钟发生 QoS 拒绝"
      - alert: RelayKnowledgeIndexRefreshDeadLetter
        expr: relay_knowledge_index_refresh_dead_letter_count > 0
        labels:
          severity: critical
        annotations:
          summary: "relay-knowledge 存在索引刷新死信"
```

### 16.5.2 Code-Index Worker Pool 状态告警

`CodeIndexWorkerStatus` 结构（参见 `src/relay_knowledge/api/operations/worker.rs`）：

| 字段 | 说明 |
|-----|------|
| `configured_worker_count` | 配置的 worker 数量 |
| `active_worker_slots` | 可用 worker 槽位 (= 配置数 − 运行中任务数) |
| `queue_depth` | 队列深度 (= queued + retrying) |
| `dead_letter_task_count` | 死信任务数 |

**告警规则：**

| 告警项 | 阈值 | 严重级别 |
|-------|------|---------|
| Worker 槽位耗尽 | `active_worker_slots == 0` | Warning |
| 队列积压 > 100 | `queue_depth > 100` | Warning |
| 死信任务数 > 0 | `dead_letter_task_count > 0` | Critical |
| 运行中租约数与运行中任务数不一致 | `running_lease_count != running_task_count` | Warning |

```bash
# 检查 worker 状态
curl -s http://localhost:8791/api/service/status | jq '.code_index_workers'
```

### 16.5.3 磁盘空间告警

数据目录包含：

| 路径 | 说明 | 增长来源 |
|-----|------|---------|
| `{data_dir}/relay-knowledge.sqlite` | 控制库 | 知识图谱数据、审计日志、worker 状态 |
| `{data_dir}/stores/repositories/*/code.sqlite` | 仓库 shard | 每个注册仓库的代码索引数据 |
| `{log_dir}/agent-audit.jsonl` | 审计日志 | Agent 协议审计事件 |
| `{cache_dir}/model-catalog-cache.json` | 模型缓存 | 定期刷新的模型目录 |

**建议阈值：**

| 告警项 | 阈值 | 严重级别 |
|-------|------|---------|
| 数据分区使用率 > 80% | `df {data_dir} > 80%` | Warning |
| 数据分区使用率 > 90% | `df {data_dir} > 90%` | Critical |
| SQLite WAL 文件 > 100MB | 检查 `*-wal` 文件大小 | Warning |

---

## 16.6 备份与恢复

### 16.6.1 SQLite 备份策略

relay-knowledge 使用两种 SQLite 拓扑：

- **`single_sqlite`**：单个 `relay-knowledge.sqlite` 文件
- **`partitioned_sqlite`**：控制库 + 每仓库独立 shard

控制库记录 shard catalog、任务、租约和发布状态，仓库 shard 保存对应代码事实。逐个制作在线单库快照
只能保证每个文件各自事务一致，不能保证控制库与所有 shard 来自同一发布时刻。因此，
标准备份必须先优雅停止唯一服务实例，确认没有托管或非托管的 `relay-knowledge` 进程，再复制整个数据目录。
不要把单库在线快照组合成分区拓扑备份。

#### 备份内容清单

| 文件/目录 | 路径 | 说明 |
|----------|------|------|
| 控制库 | `{data_dir}/relay-knowledge.sqlite` | 全局状态、知识图谱、worker 状态 |
| 仓库 shard 目录 | `{data_dir}/stores/repositories/` | 每个仓库的代码索引数据 |
| 配置文件 | `{config_dir}/model-profiles.json` | 模型提供商配置 |
| 配置文件 | `{config_dir}/model-fallback.json` | 模型回退策略 |
| 服务定义 | `{service_dir}/relay-knowledge.service` | systemd 服务定义 |

备份产物还应包含：UTC 时间、应用版本、存储拓扑、源目录、文件清单、内部文件校验和以及归档文件校验和。
审计日志可按组织保留策略单独归档，不属于数据库恢复的一致性集合。

### 16.6.2 备份脚本示例

Linux 示例与第 15 章 lifecycle plan 保持一致，固定操作部署用户的 systemd user service。必须以部署用户运行，
并确保其 user manager 可用；不要把 `systemctl --user` 静默改成系统级 manager。

```bash
#!/bin/bash
# relay-knowledge-backup.sh — 停服一致备份脚本
set -euo pipefail

SERVICE_UNIT="relay-knowledge.service"
SYSTEMCTL=(systemctl --user)
BACKUP_ROOT="/backup/relay-knowledge"
TIMESTAMP=$(date -u +%Y%m%d-%H%M%S)
ARCHIVE_NAME="relay-knowledge-${TIMESTAMP}.tar.gz"
FINAL_ARCHIVE="${BACKUP_ROOT}/${ARCHIVE_NAME}"
PARTIAL_ARCHIVE="${FINAL_ARCHIVE}.partial"
DATA_DIR="${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}"
CONFIG_DIR="${RELAY_KNOWLEDGE_CONFIG_DIR:-$HOME/.config/relay-knowledge}"
SERVICE_WAS_ACTIVE=0
STAGING_DIR=""

mkdir -p "$BACKUP_ROOT"
if [ -e "$FINAL_ARCHIVE" ] || [ -e "$PARTIAL_ARCHIVE" ]; then
    echo "ERROR: backup output already exists for timestamp $TIMESTAMP" >&2
    exit 1
fi

require_all_writers_stopped() {
    if "${SYSTEMCTL[@]}" is-active --quiet "$SERVICE_UNIT"; then
        echo "ERROR: $SERVICE_UNIT is still active" >&2
        return 1
    fi
    if ! "${SYSTEMCTL[@]}" show "$SERVICE_UNIT" --property=MainPID --value \
        | grep -Fxq '0'; then
        echo "ERROR: $SERVICE_UNIT still has a MainPID or cannot be inspected" >&2
        return 1
    fi
    if pgrep -x relay-knowledge >/dev/null; then
        echo "ERROR: a relay-knowledge process is still running" >&2
        return 1
    fi
}

cleanup() {
    status=$?
    if [ -n "$STAGING_DIR" ] && [ -d "$STAGING_DIR" ]; then
        if ! rm -r -- "$STAGING_DIR"; then
            status=1
        fi
    fi
    if [ -f "$PARTIAL_ARCHIVE" ]; then
        if ! rm -- "$PARTIAL_ARCHIVE"; then
            status=1
        fi
    fi
    if [ "$SERVICE_WAS_ACTIVE" -eq 1 ]; then
        if ! "${SYSTEMCTL[@]}" start "$SERVICE_UNIT"; then
            echo "ERROR: backup finished but $SERVICE_UNIT could not be restarted" >&2
            status=1
        fi
    fi
    trap - EXIT INT TERM HUP
    exit "$status"
}
trap cleanup EXIT

if "${SYSTEMCTL[@]}" is-active --quiet "$SERVICE_UNIT"; then
    SERVICE_WAS_ACTIVE=1
    "${SYSTEMCTL[@]}" stop "$SERVICE_UNIT"
fi

if ! require_all_writers_stopped; then
    echo "ERROR: not all writers stopped; backup aborted" >&2
    exit 1
fi
if [ ! -f "$DATA_DIR/relay-knowledge.sqlite" ]; then
    echo "ERROR: control database is missing from $DATA_DIR" >&2
    exit 1
fi

STAGING_DIR=$(mktemp -d "${BACKUP_ROOT}/.backup-staging.XXXXXX")
mkdir -p "$STAGING_DIR/payload/data" "$STAGING_DIR/payload/config"
cp -a "$DATA_DIR/." "$STAGING_DIR/payload/data/"
if [ -d "$CONFIG_DIR" ]; then
    cp -a "$CONFIG_DIR/." "$STAGING_DIR/payload/config/"
fi
"${SYSTEMCTL[@]}" cat "$SERVICE_UNIT" > "$STAGING_DIR/payload/service-definition.txt"

relay-knowledge version --format json > "$STAGING_DIR/payload/version.json"
relay-knowledge setup doctor --format json > "$STAGING_DIR/payload/setup-doctor.json"
cat > "$STAGING_DIR/payload/backup-metadata.txt" <<EOF
created_at_utc=${TIMESTAMP}
data_dir=${DATA_DIR}
config_dir=${CONFIG_DIR}
storage_topology=${RELAY_KNOWLEDGE_STORAGE_TOPOLOGY:-single_sqlite}
EOF

# 在复制件上验证；任何一个控制库或 shard 失败都会中止整组备份。
printf 'ok\n' > "$STAGING_DIR/integrity-ok.txt"
while IFS= read -r -d '' database; do
    if ! sqlite3 "$database" 'PRAGMA integrity_check;' \
        > "$STAGING_DIR/integrity-result.txt" \
        || ! cmp -s "$STAGING_DIR/integrity-ok.txt" "$STAGING_DIR/integrity-result.txt"; then
        echo "ERROR: integrity check failed for $database" >&2
        sed -n '1,20p' "$STAGING_DIR/integrity-result.txt" >&2
        exit 1
    fi
done < <(find "$STAGING_DIR/payload/data" -type f -name '*.sqlite' -print0)

(
    cd "$STAGING_DIR/payload"
    find . -type f ! -name SHA256SUMS -print0 \
        | sort -z \
        | xargs -0 sha256sum > SHA256SUMS
)
tar -czf "$PARTIAL_ARCHIVE" -C "$STAGING_DIR" payload
mv "$PARTIAL_ARCHIVE" "$FINAL_ARCHIVE"
(
    cd "$BACKUP_ROOT"
    sha256sum "$ARCHIVE_NAME" > "${ARCHIVE_NAME}.sha256"
)

echo "Backup complete: $FINAL_ARCHIVE"
```

脚本会在退出路径恢复备份前原本处于 active 的服务；如果停止、复制、完整性检查、打包或重启任一步骤失败，
最终退出码均为非零。备份窗口应由调度器串行化，禁止两个备份任务同时操作同一服务和目标目录。归档保留期由
备份系统按已发布的 `.tar.gz` 与同名 `.sha256` 精确配对管理，不在采集脚本中递归删除目录。

### 16.6.3 恢复流程与验证

```bash
#!/bin/bash
# relay-knowledge-restore.sh — 只恢复文件并保持 stopped
set -euo pipefail

if [ "$#" -ne 2 ] || [ "$2" != "--defer-start" ]; then
    echo "Usage: relay-knowledge-restore.sh ABSOLUTE_ARCHIVE --defer-start" >&2
    exit 64
fi
BACKUP_TAR="$1"
SERVICE_UNIT="relay-knowledge.service"
SYSTEMCTL=(systemctl --user)
DATA_DIR="${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}"
case "$BACKUP_TAR" in /*) ;; *) echo "ERROR: archive path must be absolute" >&2; exit 1;; esac
case "$DATA_DIR" in /*) ;; *) echo "ERROR: data path must be absolute" >&2; exit 1;; esac
if [ "$DATA_DIR" = "/" ] || [ ! -d "$DATA_DIR" ] || [ -L "$DATA_DIR" ]; then
    echo "ERROR: data path must be an existing non-symlink directory: $DATA_DIR" >&2
    exit 1
fi
RESTORE_PARENT="${DATA_DIR%/*}"
[ -n "$RESTORE_PARENT" ] || RESTORE_PARENT="/"
TIMESTAMP=$(date -u +%Y%m%d-%H%M%S)
ROLLBACK_DIR="${DATA_DIR}.pre-restore-${TIMESTAMP}"
FAILED_DIR="${DATA_DIR}.failed-restore-${TIMESTAMP}"
RESTORE_DIR=""
ROLLBACK_REQUIRED=0

if [ ! -f "$BACKUP_TAR" ]; then
    echo "ERROR: backup archive does not exist: $BACKUP_TAR" >&2
    exit 1
fi
if [ ! -f "${BACKUP_TAR}.sha256" ]; then
    echo "ERROR: external archive checksum is missing: ${BACKUP_TAR}.sha256" >&2
    exit 1
fi
if [ -e "$ROLLBACK_DIR" ] || [ -e "$FAILED_DIR" ]; then
    echo "ERROR: restore safety directory already exists; choose a new maintenance window" >&2
    exit 1
fi

require_all_writers_stopped() {
    ! "${SYSTEMCTL[@]}" is-active --quiet "$SERVICE_UNIT" \
        && "${SYSTEMCTL[@]}" show "$SERVICE_UNIT" --property=MainPID --value | grep -Fxq '0' \
        && ! pgrep -x relay-knowledge >/dev/null
}

verify_sqlite_tree() {
    root="$1"
    [ -f "$root/relay-knowledge.sqlite" ] || return 1
    while IFS= read -r -d '' database; do
        if ! sqlite3 "$database" 'PRAGMA integrity_check;' > "$RESTORE_DIR/integrity-result.txt" \
            || ! cmp -s "$RESTORE_DIR/integrity-ok.txt" "$RESTORE_DIR/integrity-result.txt"; then
            echo "ERROR: integrity check failed for $database" >&2
            sed -n '1,20p' "$RESTORE_DIR/integrity-result.txt" >&2
            return 1
        fi
    done < <(find "$root" -type f -name '*.sqlite' -print0)
}

cleanup() {
    status=$?
    if [ "$ROLLBACK_REQUIRED" -eq 1 ]; then
        if ! require_all_writers_stopped; then
            echo "CRITICAL: writer detected; restore files require manual recovery" >&2
            status=1
        else
            if [ -e "$DATA_DIR" ] && ! mv -- "$DATA_DIR" "$FAILED_DIR"; then
                echo "CRITICAL: could not preserve failed restored data" >&2
                status=1
            elif ! mv -- "$ROLLBACK_DIR" "$DATA_DIR"; then
                echo "CRITICAL: could not put the original data directory back" >&2
                status=1
            else
                echo "ERROR: original data files restored; service remains stopped" >&2
            fi
        fi
    fi
    if [ -n "$RESTORE_DIR" ] && [ -d "$RESTORE_DIR" ] \
        && ! rm -r -- "$RESTORE_DIR"; then
        status=1
    fi
    trap - EXIT
    exit "$status"
}
trap cleanup EXIT

BACKUP_PARENT="${BACKUP_TAR%/*}"
[ -n "$BACKUP_PARENT" ] || BACKUP_PARENT="/"
BACKUP_NAME="${BACKUP_TAR##*/}"
(
    cd "$BACKUP_PARENT"
    sha256sum -c --strict -- "${BACKUP_NAME}.sha256"
)

if ! tar -tzf "$BACKUP_TAR" | awk '
    $0 !~ /^payload(\/|$)/ || $0 ~ /(^|\/)\.\.(\/|$)/ { invalid=1 }
    END { exit invalid }
'; then
    echo "ERROR: archive contains a path outside payload" >&2
    exit 1
fi
RESTORE_DIR=$(mktemp -d "${RESTORE_PARENT}/.restore-staging.XXXXXX")
printf 'ok\n' > "$RESTORE_DIR/integrity-ok.txt"
tar -xzf "$BACKUP_TAR" -C "$RESTORE_DIR"
PAYLOAD="$RESTORE_DIR/payload"
if [ ! -d "$PAYLOAD/data" ] || [ -L "$PAYLOAD/data" ] \
    || [ ! -f "$PAYLOAD/SHA256SUMS" ] || [ ! -f "$PAYLOAD/version.json" ]; then
    echo "ERROR: archive does not contain a complete relay-knowledge payload" >&2
    exit 1
fi
(
    cd "$PAYLOAD"
    sha256sum -c SHA256SUMS
)
if find "$PAYLOAD/data" -type l -print -quit | grep -q .; then
    echo "ERROR: data payload must not contain symbolic links" >&2
    exit 1
fi
if ! verify_sqlite_tree "$PAYLOAD/data"; then
    echo "ERROR: backup database integrity verification failed" >&2
    exit 1
fi
jq -r '"Backup version: \(.project_name) \(.version)"' "$PAYLOAD/version.json"

"${SYSTEMCTL[@]}" stop "$SERVICE_UNIT"
if ! require_all_writers_stopped; then
    echo "ERROR: not all writers stopped; restore aborted" >&2
    exit 1
fi

mv -- "$DATA_DIR" "$ROLLBACK_DIR"
ROLLBACK_REQUIRED=1
cp -a -- "$PAYLOAD/data" "$DATA_DIR"
verify_sqlite_tree "$DATA_DIR"
require_all_writers_stopped
ROLLBACK_REQUIRED=0
echo "Files restored; $SERVICE_UNIT remains stopped. Rollback copy: $ROLLBACK_DIR"
```

`--defer-start` 是强制参数：脚本只替换完整数据目录并保持 user service stopped，不运行当前 binary，因而不会让
当前版本先迁移旧数据库。需要同时回滚 runtime state 和 binary 时，先运行本脚本，再在 stopped 状态执行
[15.9.2 节的 lifecycle rollback](15-service-deployment-full-guide.md#1592-回滚步骤)；lifecycle 会恢复 checkpointed
binary/definition、刷新平台注册并启动。仅恢复同版本数据时，也要先比对输出的备份版本与归档中的配置、service definition，
确认兼容后再显式启动并执行下列检查。失败时原数据文件会复位且服务仍保持 stopped；成功后保留 `.pre-restore-*` 到观察期结束。

**恢复后验证清单：**

1. `systemctl --user status relay-knowledge.service` — 确认 lifecycle 或人工启动后的服务 running
2. `curl http://localhost:8791/api/health | jq .healthy` — 确认 healthy=true
3. `curl http://localhost:8791/api/v1/control/storage/topology | jq .storage.missing_shard_count` — 确认 missing_shard_count=0
4. `relay-knowledge service status` — 确认 code_index_workers 队列正常

### 16.6.4 一致性边界与禁止操作

- 不在服务运行时复制 `.sqlite`、`-wal` 或 `-shm` 文件，也不逐库拼接所谓“整组快照”。
- 不忽略 systemd 停服错误；停止后必须同时验证 service active 状态、MainPID 和非托管进程。
- 不恢复单个 shard 而保留另一个时间点的控制库。需要从备份恢复时，始终恢复同一归档中的完整数据目录。
- 不在活跃数据库上执行 `VACUUM`、手工截断 WAL、schema 修改或文件替换。
- 不移动、截断或覆盖服务仍在写入的审计日志；日志轮转交给平台日志系统，并在部署前验证轮转策略。
- 不把健康端点首次可连接视为恢复完成；还要验证 `healthy`、拓扑中的 missing shard、worker 队列和关键查询。

---

## 16.7 容量规划

### 16.7.1 存储增长估算

| 数据类型 | 预估大小 | 说明 |
|---------|---------|------|
| 控制库基础大小 | ~10-50 MB | 知识图谱元数据、配置状态 |
| 每个仓库 shard | 50 MB - 5 GB | 取决于仓库规模（文件数 × 符号数） |
| 审计日志 | ~100 MB/月 | 高负载 MCP/ACP 协议下 |
| WAL 文件 | < 100 MB | WAL checkpoint 后回收 |

**估算公式**：
```text
总存储 ≈ 控制库大小 + Σ(每个仓库 shard 大小) + 审计日志大小
```

### 16.7.2 内存需求

| 组件 | 内存估算 | 说明 |
|-----|---------|------|
| 基础进程 | ~50-100 MB | Rust runtime + 加载的库 |
| 每个 HTTP 连接 | ~1-5 MB | 取决于请求体大小 |
| 每个 code-index worker | ~100-500 MB | Git blob 解析和代码分析 |
| SQLite 页缓存 | ~2 MB × shard 数 | 默认页缓存配置 |

**推荐配置**：
- 小型部署（< 10 仓库）：512 MB - 1 GB
- 中型部署（10-50 仓库）：2-4 GB
- 大型部署（50+ 仓库）：8+ GB

### 16.7.3 磁盘 I/O 考量

- SQLite 使用 WAL 模式，读操作无锁
- code-index 写入发生在独立 shard 上，互不阻塞
- 建议使用 SSD 存储，特别是仓库 shard 目录
- 空间回收属于停服维护：先完成一致备份并确认所有写入者退出，再逐库执行完整性检查和 `VACUUM`
- `VACUUM` 需要额外临时空间；磁盘已满时先扩容或迁移数据卷，不能把在线数据库压缩当成应急腾挪手段

---

## 16.8 常见故障处理 SOP

### 16.8.1 服务无法启动

**症状**：`systemctl --user start relay-knowledge.service` 失败，user service 状态显示退出。

**排查步骤：**

```bash
# 1. 查看日志
journalctl --user -u relay-knowledge.service --no-pager -n 50

# 2. 检查配置有效性
relay-knowledge setup doctor

# 3. 检查存储文件
ls -la ${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}/relay-knowledge.sqlite

# 4. 检查数据库完整性
sqlite3 ${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}/relay-knowledge.sqlite \
    "PRAGMA integrity_check;"

# 5. 检查端口占用
ss -tlnp | grep 8791

# 6. 检查环境变量
env | grep RELAY_
```

**常见原因与解决：**

| 原因 | 解决方案 |
|-----|---------|
| 数据目录权限不足 | `chown -R relay-knowledge:relay-knowledge $DATA_DIR` |
| 端口被占用 | 修改 `RELAY_KNOWLEDGE_HTTP_BIND` 或终止占用进程 |
| 数据库损坏 | 从备份恢复（参见 16.6.3 节） |
| QoS 配置为零值 | 检查 `RELAY_KNOWLEDGE_QOS_*` 变量是否 > 0 |
| 存储拓扑配置错误 | 检查 `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY` |
| 网络配置缺失 | `setup doctor` 查看 `network_budget` 检查项 |

### 16.8.2 索引任务卡死（Lease Recovery）

**症状**：code_index_workers 的 `dead_letter_task_count > 0`，或有 running 状态但无进展的任务。

**自动恢复机制**（代码实现）：

1. 服务启动时调用 `recover_orphaned_code_index_tasks_on_startup()`（参见 `src/relay_knowledge/interfaces/cli/service/mod.rs`）
2. 该函数调用 `recover_orphaned_code_index_task_leases()`
3. 检查所有运行中的租约（`running_code_index_task_leases`）
4. 解析每个租约的 `lease_owner` 中的 PID
5. 如果对应进程不存在 → 标记为孤儿，重置任务（最多 `CODE_INDEX_TASK_MAX_ATTEMPTS` 次）
6. 重置的任务以 `lease_orphaned` 错误原因重新入队

**手动干预（如果自动恢复失败）：**

```bash
# 查看卡死的任务
curl -s http://localhost:8791/api/service/status | jq '.code_index_workers'

# 如果 running_task_count > 0 但 active_worker_slots 长时间不变：
# 1. 重启服务（触发 lease recovery）
systemctl --user restart relay-knowledge.service

# 2. 重启后检查
curl -s http://localhost:8791/api/service/status | jq '.code_index_workers.dead_letter_task_count'
```

### 16.8.3 存储空间不足

**症状**：磁盘使用率告警，服务运行缓慢或写入失败。

**应急处理：**

```bash
set -euo pipefail
DATA_DIR="${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}"

# 1. 只读确认容量与大文件；不要删除 SQLite、WAL、SHM 或 dead-letter 数据。
df -h "$DATA_DIR"
du -x -h --max-depth=2 "$DATA_DIR" | sort -h | tail -n 30
find "$DATA_DIR" -type f \( -name '*.sqlite' -o -name '*-wal' -o -name '*-shm' \) \
    -exec ls -lh {} \;

# 2. 停止唯一写入者，并把失败当成硬阻断。
systemctl --user stop relay-knowledge.service
! systemctl --user is-active --quiet relay-knowledge.service
systemctl --user show relay-knowledge.service --property=MainPID --value | grep -Fxq '0'
! pgrep -x relay-knowledge >/dev/null

# 3. 在数据目录之外释放已批准的空间，或先扩展/迁移数据卷。
#    只清理已经轮转并超过组织保留期的日志、已验证且过期的备份和可再生缓存；
#    不改写 agent-audit.jsonl，不删除任何数据库伴生文件。

# 4. 空间恢复后，先按 16.6.2 节创建停服一致备份，再决定是否离线 VACUUM。
while IFS= read -r -d '' database; do
    read -r database_bytes < <(stat -c %s -- "$database")
    database_parent="${database%/*}"
    read -r available_bytes < <(df --output=avail -B1 "$database_parent" | awk 'NR == 2 {print $1}')
    required_bytes=$((database_bytes * 2))
    if [ "$available_bytes" -lt "$required_bytes" ]; then
        echo "ERROR: insufficient scratch space to VACUUM $database" >&2
        exit 1
    fi
    cmp -s <(printf 'ok\n') <(sqlite3 "$database" 'PRAGMA integrity_check;')
    sqlite3 "$database" 'VACUUM;'
    cmp -s <(printf 'ok\n') <(sqlite3 "$database" 'PRAGMA integrity_check;')
done < <(find "$DATA_DIR" -type f -name '*.sqlite' -print0)

# 5. 启动并完成健康、拓扑和 worker 验证。
systemctl --user start relay-knowledge.service
curl -fsS http://127.0.0.1:8791/api/health | jq -e '.healthy == true'
curl -fsS http://127.0.0.1:8791/api/v1/control/storage/topology \
    | jq -e '.storage.missing_shard_count == 0'
relay-knowledge service status
```

如果可用空间不足以先生成一致备份和 `VACUUM` 临时文件，就停在第 3 步扩容或迁移卷，不执行数据库维护。
不要在服务运行时手工 checkpoint WAL，也不要通过截断或替换活跃审计日志换取空间。scope 清理由产品的
保留任务、租约和可观测维护状态负责，不直接删除 shard 或 scope 文件。

**长期方案：**
- 扩展数据分区磁盘容量
- 调整 scope retention 策略限制每个仓库保留的索引版本数
- 部署并演练平台日志轮转；轮转策略必须保留审计要求，且不能依赖移动仍由进程持有的活跃文件

### 16.8.4 高负载下的 QoS 拒绝

**症状**：客户端收到 503/429 响应，或 `relay_agent_protocol_rejections_total` 指标上升。

**拒绝类型分析（参见 `src/relay_knowledge/net/qos/mod.rs`）：**

| 拒绝原因 | 含义 | 排查方向 |
|---------|------|---------|
| `ConnectionBudgetExceeded` | 连接数已达到 `max_connections` | 增加 `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` 或使用连接池 |
| `RequestBudgetExceeded` | 并发请求达到 `max_in_flight_requests` | 增加 `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` 或减少客户端并发 |
| `QueueBudgetExceeded` | 排队请求达到 `max_queue_depth` | 增加 `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` 或扩容 worker |

**处理步骤：**

```bash
# 1. 查看当前 QoS 状态
curl -s http://localhost:8791/api/v1/control/status | jq '.qos'

# 2. 临时调整（需重启服务）
export RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS=2048
export RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS=512
export RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH=1024
systemctl --user restart relay-knowledge.service

# 3. 确认调整生效
curl -s http://localhost:8791/api/v1/control/status | jq '.qos'
```

### 16.8.5 Shard 损坏修复

**症状**：
- `missing_shard_count > 0`（拓扑快照显示）
- `degraded_reason` 包含 "missing shard files"
- 特定仓库查询返回错误

**修复步骤：**

```bash
set -euo pipefail
TOPOLOGY_EVIDENCE=$(mktemp /tmp/relay-knowledge-topology-before-recovery.XXXXXX.json)

# 1. 保存只读拓扑证据。
curl -fsS http://127.0.0.1:8791/api/v1/control/storage/topology \
    | tee "$TOPOLOGY_EVIDENCE" \
    | jq '.storage.shards[] | select(.exists == false or .state != "active")'

# 2. 完整性检查必须在停服且无其他写入进程时执行。
systemctl --user stop relay-knowledge.service
! systemctl --user is-active --quiet relay-knowledge.service
systemctl --user show relay-knowledge.service --property=MainPID --value | grep -Fxq '0'
! pgrep -x relay-knowledge >/dev/null
SHARDS_DIR="${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}/stores/repositories"
while IFS= read -r -d '' database; do
    if ! cmp -s <(printf 'ok\n') <(sqlite3 "$database" 'PRAGMA integrity_check;'); then
        echo "CORRUPT: $database"
        sqlite3 "$database" 'PRAGMA integrity_check;' | sed -n '1,20p'
    fi
done < <(find "$SHARDS_DIR" -type f -name '*.sqlite' -print0)
echo "Topology evidence: $TOPOLOGY_EVIDENCE"
```

不要把某个历史 shard 文件直接复制进当前拓扑：它可能与控制库中的 catalog、scope 发布状态和任务 checkpoint
不匹配。若有同一停服时间点的一致备份，按 16.6.3 节恢复整个数据目录。若没有可用备份但授权源码仍可访问，
重新启动服务后通过受支持的仓库操作删除并重新索引该仓库。完整性检查代码块会有意保持服务停止；在下一步
恢复或重建开始前不要让它处于无人值守状态。重建前先显式提供本次事件的真实值：

```bash
set -euo pipefail
: "${REPOSITORY_ALIAS:?export REPOSITORY_ALIAS with the affected registered alias}"
: "${REPOSITORY_ROOT:?export REPOSITORY_ROOT with its authorized absolute source path}"

systemctl --user start relay-knowledge.service
jq -n --arg alias "$REPOSITORY_ALIAS" \
    '{operation: "code_repository_remove", alias: $alias}' \
    | curl -fsS -X POST http://127.0.0.1:8791/api/web/operations/execute \
    -H 'Content-Type: application/json' \
    --data-binary @-
jq -n --arg alias "$REPOSITORY_ALIAS" --arg root "$REPOSITORY_ROOT" \
    '{operation: "code_repository_register", alias: $alias, root_path: $root}' \
    | curl -fsS -X POST http://127.0.0.1:8791/api/web/operations/execute \
    -H 'Content-Type: application/json' \
    --data-binary @-

curl -fsS http://127.0.0.1:8791/api/v1/control/storage/topology \
    | jq -e '.storage.missing_shard_count == 0'
relay-knowledge service status
```

重新索引完成前，仓库状态可以是 stale 或 degraded；只有 durable task 完成、拓扑无缺失、关键查询命中且
worker 无新增 dead letter 后才能关闭事件。

---

## 附录 A：关键环境变量速查

| 环境变量 | 默认值 | 用途 |
|---------|-------|------|
| `RELAY_KNOWLEDGE_HOME` | (平台默认) | 统一设置所有数据目录的根路径 |
| `RELAY_KNOWLEDGE_DATA_DIR` | `$XDG_DATA_HOME/relay-knowledge` | 数据库文件目录 |
| `RELAY_KNOWLEDGE_LOG_DIR` | `$XDG_STATE_HOME/relay-knowledge` | 日志和审计文件目录 |
| `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY` | `single_sqlite` | 存储拓扑：`single_sqlite` 或 `partitioned_sqlite` |
| `RELAY_KNOWLEDGE_HTTP_BIND` | `127.0.0.1:8791` | HTTP 监听地址 |
| `RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS` | `1024` | QoS 最大连接数 |
| `RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS` | `256` | QoS 最大并发请求数 |
| `RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH` | `512` | QoS 最大排队深度 |
| `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` | (CPU 核心数) | 代码索引并发 worker 数 |
| `RELAY_OTEL_ENDPOINT` | `http://127.0.0.1:4318` | OpenTelemetry Collector 地址 |
| `RELAY_OTEL_TRACES` | `false` | 启用 Trace 导出 |
| `RELAY_OTEL_METRICS` | `false` | 启用 Metric 导出 |
| `RELAY_OTEL_SERVICE_ENVIRONMENT` | `local` | 部署环境标签 |

## 附录 B：核心 API 端点速查

| 端点 | 方法 | 说明 |
|-----|------|------|
| `/api/health` | GET | 服务健康检查（带存储快照） |
| `/api/v1/control/health` | GET | 只读健康检查（不打开冷存储） |
| `/api/service/status` | GET | 服务状态（带 reconcile） |
| `/api/v1/control/service/status` | GET | 只读服务状态 |
| `/api/v1/control/status` | GET | 控制面运行时状态 |
| `/api/v1/control/storage/topology` | GET | 存储拓扑快照 |
| `/api/web/operations/execute` | POST | 执行运维操作 |
| `/api/project/status` | GET | 项目基础状态 |

---

导航：上一章：[第 15 章 完整服务化部署指南](15-service-deployment-full-guide.md) | 下一章：[第 17 章 安全配置完整指南](17-security-configuration.md) | 返回：[用户指南](README.md)
