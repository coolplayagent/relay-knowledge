# 第 13 章 运维与排障

[中文](../../zh/01-user-guide/13-operations-and-troubleshooting.md) | [English](../../en/01-user-guide/13-operations-and-troubleshooting.md)

排障时先缩小范围，再处理单点错误。不要只凭 Web 页面摘要判断根因；完整诊断仍在 JSON API 和 CLI 响应中。

## 13.1 健康检查

先看项目状态:

```bash
relay-knowledge status --format json
```

再看配置、健康和服务诊断:

```bash
relay-knowledge setup doctor --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

优先看 `setup doctor` 的 `configuration_ready`、`live_health_checked`、`checks` 和 `recommended_actions`。`setup doctor` 的配置通过不代表 live health 通过；随后重点关注 `health`/`service doctor` 中的 graph version、index lag、refresh queue diagnostics、`index_refresh.stale_reasons`、runtime directories、HTTP bind、QoS budgets、agent protocol status、telemetry status 和 degraded reason。

## 13.2 索引新鲜度

查询返回 stale 或 degraded 时，先检查图和索引:

```bash
relay-knowledge graph inspect --format json
relay-knowledge index refresh --kind bm25 --format json
relay-knowledge index refresh --kind semantic --kind vector --format json
```

如果调用方不能接受旧索引，查询时使用:

```bash
relay-knowledge query "topic" --freshness wait-until-fresh --format json
```

如果只需要图事实，不需要派生索引:

```bash
relay-knowledge query "topic" --freshness graph-only --format json
```

`health`、`service doctor` 和 `index refresh` 的 JSON 响应会返回 `index_refresh.stale_reasons`。优先处理 `reason` 含 failed 或带 `last_error` 的项；只有 lag reason 时，通常先运行 `relay-knowledge index refresh --format json` 或在查询中使用 `--freshness wait-until-fresh`。

## 13.3 常见错误

`missing value for --source` 或 `missing value for --content`: `ingest` 必须同时提供 source scope 和内容。

`invalid --freshness value`: 只接受 `allow-stale`、`wait-until-fresh` 或 `graph-only`。

`invalid --kind value`: `index refresh` 只接受 `bm25`、`semantic` 或 `vector`；`repo query` 只接受 `hybrid`、`symbol`、`definition`、`references`、`callers`、`callees`、`imports` 或 `sbom`；`repo software` 只接受 `dependencies`、`sdks`、`files`、`topics`、`relationships`、`build`、`iac`、`design` 或 `all`。

`source_scope is required by the MCP access policy`: MCP graph tool 请求缺少 scope，或者未配置允许 unspecified scope。

`source_scope '<scope>' is not authorized for this MCP policy`: 请求 scope 不在 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` 中，也不是当前运行时已注册的 code repository alias。运行中可先执行 `relay-knowledge repo register <path>`，让 Git root 或 filesystem root 目录名成为默认 alias，或在确实需要自定义 scope 时传入 `--alias <scope>`，然后完成索引；否则按错误提示追加 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=<scope>` 后重启服务。

路径配置错误: 高级目录覆盖和 `RELAY_KNOWLEDGE_HOME` 必须是绝对路径，且不能包含 `..`。完整变量清单见 [第 12 章](12-advanced-configuration.md)。

OTLP Collector 不可用: `service doctor --format json` 的 `runtime.telemetry.last_error` 会记录最近 exporter 初始化或导出错误。该错误只影响 observability，不表示 graph retrieval 不可用。

MCP 返回 HTTP 400: 常见原因是缺少 `Mcp-Session-Id`、缺少 `MCP-Protocol-Version`、初始化 payload 不合法或 session 流程没有先发送 `notifications/initialized`。

MCP 返回 HTTP 404: 常见原因是 session id 未知、过期或被淘汰。重新执行 initialize 流程并保存新的 `Mcp-Session-Id`。

`version does not support --format streaming-json`: `version` 只支持 `text` 和 `json`。

`repo impact` 返回 head snapshot 不存在: 先对目标 head 执行 `repo index` 或 `repo update`，再做影响分析。

`repo status` 显示 `active_task.state=running`，但 `checkpoint.parsed_file_count` 一直是 0: 先查看 `repo status --format json` 中的 `active_task.lease_expires_at_ms` 和 `checkpoint.updated_at_ms`。非交互式 agent session 中不要反复启动 `service run`；它是前台常驻进程。`service run` 启动时会恢复 owner 进程已退出的 `code-index-worker-<pid>` lease，并记录 `lease_orphaned`；仍有存活 worker 持有的 lease 会保留。冷启动索引用到的 Git blob 批量读取有明确边界，`git cat-file` 无响应时会报告 Git 命令错误，使任务进入 retry 或 dead-letter，而不是无限持有 lease。如果任务处于 queued 或 retrying，可执行 `repo index-worker --task-id <active_task.task_id> --format json` 做一次有界 worker attempt。对仍然卡住的未完成任务，执行 `repo index <alias> --reset --format json`，再用 `repo index-worker --task-id <reset_task_id> --format json` 消费重排任务；如果历史任务已经进入 dead-letter，则用 `repo index <alias> --ref <ref>` 有意重新排当前 scope，而不是复活 terminal 历史。不要杀 `relay-knowledge` 进程，也不要绕过任务 lease。

`repo query` 返回 source fallback candidate 或 budget degraded reason: 这表示精确文本兜底层降级，不表示 tree-sitter 代码图或 SQLite FTS 结果不可用。优先收窄 `--path`、`--language`、`--ref` 或注册 scope，再用 `repo status --format json` 确认目标快照 fresh。

## 13.4 隔离复现

排查用户数据或开发机污染时，用独立 runtime home:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-repro \
  relay-knowledge status --format json
```

把同一个变量加到 ingest、query、repo 和 service 命令上，可以完整复现独立数据目录下的行为。

复现 Web 或 MCP 问题时同时固定 bind、scope 和 QoS 变量:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-repro \
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --web --mcp streamable-http
```

另一个终端检查:

```bash
curl http://127.0.0.1:8791/api/health
curl http://127.0.0.1:8791/mcp/metrics
```

## 13.5 PR 验证命令

Rust 质量门禁:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

Web 和浏览器集成测试:

```bash
npm --prefix web ci
npm --prefix web run build
./build.sh
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

只修改文档时，至少检查新增链接和 Markdown 文件路径，并在 PR 中说明未运行代码测试的原因。

## 13.6 诊断顺序

遇到结果不符合预期时，按这个顺序缩小范围:

1. `status --format json`: 确认运行时目录、配置和项目状态。
2. `setup doctor --format json`: 读取配置 checks、`configuration_ready` 和 recommended actions。
3. `health --format json`: 确认 graph version、index freshness、queue/dead-letter、provider 和 QoS 状态。
4. `graph inspect --format json`: 确认 evidence、entity、structured facts 和 code counts。
5. `index refresh --format json`: 尝试显式刷新并读取 stale reasons。
6. 对应业务命令加 `--format json`: 保留完整 metadata、degraded reason 和 audit correlation。
7. `audit query --limit 50 --format json`: 查看最近 CLI/Web/service/agent 操作是否到达统一 API。
