# relay-knowledge SRE 运维手册

## 1. 概述：SRE 运维全景图

relay-knowledge 是一个基于 Rust async 运行时构建的知识图谱服务，采用事件驱动架构，核心数据存储在 SQLite 中（支持单库和分区拓扑）。SRE 需要关注的运维面包括：

| 运维领域 | 核心能力 | 关键命令/端点 |
|---------|---------|-------------|
| 服务生命周期 | systemd 托管启停，graceful shutdown | `systemctl start/stop/restart relay-knowledge` |
| 健康诊断 | 只读诊断 + 带 reconcile 的完整检查 | `/api/health`、`/api/v1/control/health` |
| 存储拓扑 | 控制库 + 每仓库独立 shard | `/api/v1/control/storage/topology` |
| 监控指标 | OpenTelemetry OTLP 导出 | `/v1/traces` + `/v1/metrics` |
| 容量管理 | QoS 连接/请求/队列限流 | `service status` 输出 |
| 备份恢复 | SQLite 文件级备份 | 脚本化 `sqlite3 .backup` |

---

## 2. 服务生命周期管理

### 2.1 启动流程

服务启动时按以下顺序初始化（参见 `src/relay_knowledge/interfaces/service_cli.rs`）：

```
1. RuntimeConfiguration::from_process_environment()  — 读取环境变量
2. runtime.observability.initialize()               — 安装 OpenTelemetry 导出器
3. service.reconcile_startup_indexes()              — 对账启动时索引
4. service.recover_orphaned_code_index_tasks_on_startup() — 恢复孤儿任务租约
5. 启动后台循环：
   - file_index_loop        (如果启用)
   - code_index_worker_pool (code_index_max_in_flight 个 worker，每 5s 轮询)
   - code_repository_set_refresh_loop (每 5s 轮询)
6. HTTP 服务监听 (如果启用 web 模式)
```

**启动命令示例：**

```bash
# systemd 服务启动
systemctl start relay-knowledge

# 或直接以 long-running 模式启动（需要配置 service 定义文件）
relay-knowledge service --mcp-transport streamable-http --web
```

### 2.2 停止与 Graceful Shutdown

服务通过信号实现优雅关闭（参见 `service_shutdown_signal()`）：

- **Linux/macOS**：同时监听 `SIGTERM` 和 `Ctrl+C`（SIGINT），任一信号触发关闭
- **关闭顺序**：
  1. 发送 `true` 到 file_index_shutdown channel，等待 file_index_task 完成
  2. 发送 `true` 到 code_index_shutdown channel，等待所有 code_index worker 完成
  3. 发送 `true` 到 repo_set_refresh_shutdown channel，等待完成
  4. 调用 `runtime.observability.shutdown()`（带有 5s 导出超时）flush 遥测数据

```bash
# 优雅停止
systemctl stop relay-knowledge

# 发送 SIGTERM
kill -TERM $(pgrep relay-knowledge)
```

**注意事项**：
- Worker 循环在 poll interval（默认 5s）内响应 shutdown 信号，最长等待约 5s
- 正在执行的 code index 任务会完成当前批次后才退出
- observability shutdown 有 5s 超时保护，防止 hang

### 2.3 服务状态查看

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

## 3. 健康检查与诊断

### 3.1 Health Endpoint（`/api/health`）

核心健康检查 API（参见 `src/relay_knowledge/application/service/health.rs`）：

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

### 3.2 只读健康检查（`/api/v1/control/health`）

`read_only_health` 与 `health` 的区别：
- **不打开冷存储**：如果存储未就绪，返回 storage-free 健康状态
- **不尝试 reconcile**：仅观察现有状态
- 适合监控系统高频轮询，避免触发索引对账

```bash
curl http://localhost:8791/api/v1/control/health | jq .healthy
```

### 3.3 `service status`（带 reconcile）vs `read_only_service_status`

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

### 3.4 Doctor 检查（`relay-knowledge setup doctor`）

`setup doctor` 检查项（来源：`src/relay_knowledge/interfaces/setup_cli.rs`）：

| 检查项 | 验证内容 |
|-------|---------|
| `runtime_paths` | config_dir、data_dir、log_dir 是否就绪 |
| `network_budget` | HTTP bind、body_bytes、QoS connections/in_flight/queue 是否 > 0 |
| `retrieval_backends` | semantic_backend_mode、vector_backend_mode、embedding_dimension 是否配置 |
| `storage_check` | 存储文件是否存在、schema 是否就绪 |
| `telemetry_check` | OTLP endpoint 可达性、traces/metrics exporter 是否初始化成功 |

```bash
relay-knowledge setup doctor
```

### 3.5 存储拓扑快照（`/api/v1/control/storage/topology`）

返回 `StorageTopologyDiagnostics`（参见 `src/relay_knowledge/application/service/storage_diagnostics.rs`）：

```bash
curl http://localhost:8791/api/v1/control/storage/topology | jq .
```

输出字段：

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

## 4. 监控指标（OpenTelemetry）

### 4.1 配置

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

### 4.2 Traces

- 使用 OTLP HTTP 协议，导出到 `{endpoint}/v1/traces`
- 通过 `opentelemetry_otlp::SpanExporter` + batch exporter 导出
- `tracing-opentelemetry` layer 将 `tracing` span 桥接到 OpenTelemetry
- 同时保留 `tracing_subscriber::fmt::layer` 用于本地日志输出
- 默认日志级别为 `info`（可通过 `RUST_LOG` 覆盖）

### 4.3 Metrics（从代码中提取的实际指标）

所有指标通过 `SdkMeterProvider` + `PeriodicReader`（每 5s 导出）导出到 `{endpoint}/v1/metrics`。

#### 4.3.1 Agent 协议指标

| 指标名 | 类型 | 标签 | 说明 |
|-------|------|------|------|
| `relay_agent_protocol_requests_total` | Counter | `protocol`, `operation`, `status` | 协议请求总数 |
| `relay_agent_protocol_request_duration_ms` | Histogram | `protocol`, `operation` | 请求延迟（毫秒） |
| `relay_agent_context_truncated_total` | Counter | `protocol`, `reason` | 上下文截断次数 |
| `relay_agent_protocol_rejections_total` | Counter | `protocol`, `reason` | 协议拒绝次数 |
| `relay_agent_retrieval_cancelled_total` | Counter | `protocol` | 检索取消次数 |

**Prometheus 采集示例**（需在 OTLP Collector 中配置 Prometheus exporter）：

```promql
# 请求速率
rate(relay_agent_protocol_requests_total[5m])

# 拒绝率
rate(relay_agent_protocol_rejections_total[5m]) / rate(relay_agent_protocol_requests_total[5m])

# P99 延迟
histogram_quantile(0.99, rate(relay_agent_protocol_request_duration_ms_bucket[5m]))
```

#### 4.3.2 诊断快照指标

`AgentProtocolMetricsSnapshot` 提供内存中的低基数指标（通过 `service status` 暴露）：

| 字段 | 说明 |
|-----|------|
| `requests_total` | 累计请求数 |
| `request_duration_ms_total` | 累计延迟毫秒 |
| `rejections_total` | 累计拒绝数 |
| `cancelled_total` | 累计取消数 |
| `context_truncated_total` | 累计上下文截断数 |

所有计数器使用 `saturating_add` 防止溢出。

### 4.4 Prometheus Endpoint

relay-knowledge 不直接暴露 Prometheus endpoint。需要在 OTLP Collector 中配置 Prometheus exporter pipeline：

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

## 5. 告警阈值建议

### 5.1 QoS 水位告警

QoS 默认值（参见 `src/relay_knowledge/net/qos.rs`）：

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

Prometheus 告警规则示例：

```yaml
# relay_qos_alerts.yml
groups:
  - name: relay_qos
    rules:
      - alert: RelayQoSConnectionsHigh
        expr: relay_qos_connections_usage_ratio > 0.8
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "relay-knowledge QoS 连接使用率超过 80%"
```

### 5.2 Code-Index Worker Pool 状态告警

CodeIndexWorkerStatus 结构（参见 `src/relay_knowledge/api/operations.rs`）：

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

### 5.3 磁盘空间告警

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

## 6. 备份与恢复

### 6.1 SQLite 备份策略

relay-knowledge 使用两种 SQLite 拓扑：

- **`single_sqlite`**：单个 `relay-knowledge.sqlite` 文件
- **`partitioned_sqlite`**：控制库 + 每仓库独立 shard

#### 备份内容清单

| 文件/目录 | 路径 | 说明 |
|----------|------|------|
| 控制库 | `{data_dir}/relay-knowledge.sqlite` | 全局状态、知识图谱、worker 状态 |
| 仓库 shard 目录 | `{data_dir}/stores/repositories/` | 每个仓库的代码索引数据 |
| 配置文件 | `{config_dir}/model-profiles.json` | 模型提供商配置 |
| 配置文件 | `{config_dir}/model-fallback.json` | 模型回退策略 |
| 服务定义 | `{service_dir}/relay-knowledge.service` | systemd 服务定义 |

### 6.2 备份脚本示例

```bash
#!/bin/bash
# relay-knowledge-backup.sh — 在线备份脚本
set -euo pipefail

BACKUP_ROOT="/backup/relay-knowledge"
TIMESTAMP=$(date -u +%Y%m%d-%H%M%S)
BACKUP_DIR="${BACKUP_ROOT}/${TIMESTAMP}"
DATA_DIR="${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}"
CONFIG_DIR="${RELAY_KNOWLEDGE_CONFIG_DIR:-$HOME/.config/relay-knowledge}"
RETAIN_DAYS=7

mkdir -p "$BACKUP_DIR"

# 1. 备份控制库（使用 SQLite 在线备份确保一致性）
echo "Backing up control database..."
sqlite3 "${DATA_DIR}/relay-knowledge.sqlite" \
    "VACUUM INTO '${BACKUP_DIR}/relay-knowledge.sqlite'"

# 2. 备份仓库 shard（partitioned_sqlite 拓扑）
SHARDS_DIR="${DATA_DIR}/stores/repositories"
if [ -d "$SHARDS_DIR" ]; then
    echo "Backing up repository shards..."
    for shard_dir in "$SHARDS_DIR"/*/; do
        repo_id=$(basename "$shard_dir")
        mkdir -p "${BACKUP_DIR}/stores/repositories/${repo_id}"
        for db_file in "$shard_dir"/*.sqlite; do
            [ -f "$db_file" ] || continue
            fname=$(basename "$db_file")
            sqlite3 "$db_file" \
                "VACUUM INTO '${BACKUP_DIR}/stores/repositories/${repo_id}/${fname}'"
        done
    done
fi

# 3. 备份配置
echo "Backing up configuration..."
cp "${CONFIG_DIR}/model-profiles.json" "$BACKUP_DIR/" 2>/dev/null || true
cp "${CONFIG_DIR}/model-fallback.json" "$BACKUP_DIR/" 2>/dev/null || true

# 4. 打包并清理旧备份
tar -czf "${BACKUP_DIR}.tar.gz" -C "$BACKUP_ROOT" "$TIMESTAMP"
rm -rf "$BACKUP_DIR"

# 清理旧备份
find "$BACKUP_ROOT" -name "*.tar.gz" -mtime "+${RETAIN_DAYS}" -delete

echo "Backup complete: ${BACKUP_DIR}.tar.gz"
```

### 6.3 恢复流程与验证

```bash
#!/bin/bash
# relay-knowledge-restore.sh — 恢复脚本
set -euo pipefail

BACKUP_TAR="$1"
DATA_DIR="${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}"
RESTORE_DIR="/tmp/relay-knowledge-restore-$$"

if [ ! -f "$BACKUP_TAR" ]; then
    echo "Usage: $0 <backup.tar.gz>"
    exit 1
fi

# 1. 停止服务
echo "Stopping relay-knowledge..."
systemctl stop relay-knowledge || true

# 2. 解压备份
mkdir -p "$RESTORE_DIR"
tar -xzf "$BACKUP_TAR" -C "$RESTORE_DIR"
BACKUP_CONTENT=$(ls "$RESTORE_DIR" | head -1)

# 3. 验证 SQLite 完整性
echo "Verifying backup integrity..."
for db in $(find "$RESTORE_DIR" -name "*.sqlite"); do
    if ! sqlite3 "$db" "PRAGMA integrity_check;" | grep -q "ok"; then
        echo "ERROR: Corrupt database: $db"
        exit 1
    fi
done
echo "All databases passed integrity check."

# 4. 恢复文件
echo "Restoring..."
cp "$RESTORE_DIR/$BACKUP_CONTENT/relay-knowledge.sqlite" "$DATA_DIR/"
if [ -d "$RESTORE_DIR/$BACKUP_CONTENT/stores" ]; then
    mkdir -p "$DATA_DIR/stores"
    cp -r "$RESTORE_DIR/$BACKUP_CONTENT/stores/"* "$DATA_DIR/stores/"
fi

# 5. 启动服务并验证
echo "Starting relay-knowledge..."
systemctl start relay-knowledge
sleep 3

echo "Verifying service health..."
HEALTH=$(curl -s http://localhost:8791/api/health | jq -r '.healthy')
if [ "$HEALTH" = "true" ]; then
    echo "Restore successful. Service is healthy."
else
    echo "WARNING: Service started but reports unhealthy."
    curl -s http://localhost:8791/api/health | jq .
fi

rm -rf "$RESTORE_DIR"
```

**恢复后验证清单：**

1. `systemctl status relay-knowledge` — 确认服务 running
2. `curl http://localhost:8791/api/health | jq .healthy` — 确认 healthy=true
3. `curl http://localhost:8791/api/v1/control/storage/topology | jq .storage.missing_shard_count` — 确认 missing_shard_count=0
4. `relay-knowledge service status` — 确认 code_index_workers 队列正常

---

## 7. 容量规划

### 7.1 存储增长估算

| 数据类型 | 预估大小 | 说明 |
|---------|---------|------|
| 控制库基础大小 | ~10-50 MB | 知识图谱元数据、配置状态 |
| 每个仓库 shard | 50 MB - 5 GB | 取决于仓库规模（文件数 × 符号数） |
| 审计日志 | ~100 MB/月 | 高负载 MCP/ACP 协议下 |
| WAL 文件 | < 100 MB | WAL checkpoint 后回收 |

**估算公式**：
```
总存储 ≈ 控制库大小 + Σ(每个仓库 shard 大小) + 审计日志大小
```

### 7.2 内存需求

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

### 7.3 磁盘 I/O 考量

- SQLite 使用 WAL 模式，读操作无锁
- code-index 写入发生在独立 shard 上，互不阻塞
- 建议使用 SSD 存储，特别是仓库 shard 目录
- `VACUUM` 或 `PRAGMA optimize` 可在维护窗口执行以回收空间

---

## 8. 常见故障处理 SOP

### 8.1 服务无法启动

**症状**：`systemctl start relay-knowledge` 失败，`systemctl status` 显示退出。

**排查步骤：**

```bash
# 1. 查看日志
journalctl -u relay-knowledge --no-pager -n 50

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
| 数据库损坏 | 从备份恢复（参见 6.3 节） |
| QoS 配置为零值 | 检查 `RELAY_KNOWLEDGE_QOS_*` 变量是否 > 0 |
| 存储拓扑配置错误 | 检查 `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY` |
| 网络配置缺失 | `setup doctor` 查看 `network_budget` 检查项 |

### 8.2 索引任务卡死（Lease Recovery）

**症状**：code_index_workers 的 `dead_letter_task_count > 0`，或有 running 状态但无进展的任务。

**自动恢复机制**（代码实现）：

1. 服务启动时调用 `recover_orphaned_code_index_tasks_on_startup()`（参见 `service_cli.rs:28`）
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
systemctl restart relay-knowledge

# 2. 重启后检查
curl -s http://localhost:8791/api/service/status | jq '.code_index_workers.dead_letter_task_count'
```

### 8.3 存储空间不足

**症状**：磁盘使用率告警，服务运行缓慢或写入失败。

**应急处理：**

```bash
# 1. 确认空间使用
df -h ${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}
du -sh ${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}/*

# 2. 检查大文件
find ${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge} -name "*.sqlite" -exec ls -lh {} \;

# 3. WAL checkpoint（回收 WAL 空间）
for db in $(find ${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge} -name "*.sqlite"); do
    sqlite3 "$db" "PRAGMA wal_checkpoint(TRUNCATE);"
done

# 4. 清理审计日志（如不需要长期保留）
# 审计日志路径: {log_dir}/agent-audit.jsonl
tail -n 10000 ${RELAY_KNOWLEDGE_LOG_DIR:-$HOME/.local/state/relay-knowledge}/agent-audit.jsonl \
    > /tmp/audit-truncated.jsonl && \
    mv /tmp/audit-truncated.jsonl ${RELAY_KNOWLEDGE_LOG_DIR:-$HOME/.local/state/relay-knowledge}/agent-audit.jsonl

# 5. 清理旧 scope（代码索引历史版本）
# 通过 API 触发 scope 保留策略
```

**长期方案：**
- 扩展数据分区磁盘容量
- 调整 scope retention 策略限制每个仓库保留的索引版本数
- 部署日志轮转（logrotate）管理审计日志大小

### 8.4 高负载下的 QoS 拒绝

**症状**：客户端收到 503/429 响应，或 `relay_agent_protocol_rejections_total` 指标上升。

**拒绝类型分析（参见 `src/relay_knowledge/net/qos.rs`）：**

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
systemctl restart relay-knowledge

# 3. 确认调整生效
curl -s http://localhost:8791/api/v1/control/status | jq '.qos'
```

### 8.5 Shard 损坏修复

**症状**：
- `missing_shard_count > 0`（拓扑快照显示）
- `degraded_reason` 包含 "missing shard files"
- 特定仓库查询返回错误

**修复步骤：**

```bash
# 1. 确认损坏的 shard
curl -s http://localhost:8791/api/v1/control/storage/topology | \
    jq '.storage.shards[] | select(.exists == false)'

# 2. 检查 shard 文件
SHARDS_DIR="${RELAY_KNOWLEDGE_DATA_DIR:-$HOME/.local/share/relay-knowledge}/stores/repositories"
for repo_dir in "$SHARDS_DIR"/*/; do
    repo_id=$(basename "$repo_dir")
    db="$repo_dir/code.sqlite"
    if [ -f "$db" ]; then
        integrity=$(sqlite3 "$db" "PRAGMA integrity_check;")
        if [ "$integrity" != "ok" ]; then
            echo "CORRUPT: $repo_id — $integrity"
        fi
    else
        echo "MISSING: $repo_id — shard file not found at $db"
    fi
done

# 3a. 如果 shard 文件存在但损坏 — 从备份恢复
systemctl stop relay-knowledge
cp /backup/relay-knowledge/latest/stores/repositories/<repo_id>/code.sqlite \
   "${SHARDS_DIR}/<repo_id>/code.sqlite"
systemctl start relay-knowledge

# 3b. 如果 shard 完全丢失且无备份 — 重新注册并索引
# 注销仓库
curl -X POST http://localhost:8791/api/web/operations/execute \
    -H "Content-Type: application/json" \
    -d '{"operation": "code_repository_remove", "alias": "<repo-alias>"}'

# 重新注册并索引
curl -X POST http://localhost:8791/api/web/operations/execute \
    -H "Content-Type: application/json" \
    -d '{"operation": "code_repository_register", "root_path": "/path/to/repo", "alias": "<repo-alias>"}'

# 4. 验证修复
curl -s http://localhost:8791/api/v1/control/storage/topology | \
    jq '.storage.missing_shard_count'
# 期望输出: 0
```

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
