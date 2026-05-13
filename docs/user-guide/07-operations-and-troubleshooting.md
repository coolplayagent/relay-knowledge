# 第 7 章 运维与排障

## 7.1 健康检查

先看项目状态:

```bash
relay-knowledge status --format json
```

再看健康和服务诊断:

```bash
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

重点关注 graph version、index lag、refresh queue diagnostics、`index_refresh.stale_reasons`、runtime directories、HTTP bind、QoS budgets、agent protocol status 和 degraded reason。

## 7.2 索引新鲜度

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

`health`、`service doctor` 和 `index refresh` 的 JSON 响应会返回
`index_refresh.stale_reasons`。优先处理 `reason` 含 failed 或带 `last_error`
的项；只有 lag reason 时，通常先运行 `relay-knowledge index refresh --format json`
或在查询中使用 `--freshness wait-until-fresh`。

## 7.3 常见错误

`missing value for --source` 或 `missing value for --content`: `ingest` 必须同时提供 source scope 和内容。

`invalid --freshness value`: 只接受 `allow-stale`、`wait-until-fresh` 或 `graph-only`。

`invalid --kind value`: `index refresh` 只接受 `bm25`、`semantic` 或 `vector`；`repo query` 只接受 `hybrid`、`symbol`、`definition`、`references`、`callers`、`callees` 或 `imports`。

`source_scope is required by the MCP access policy`: MCP graph tool 请求缺少 scope，或者未配置允许 unspecified scope。

`source_scope is not authorized for this MCP policy`: 请求 scope 不在 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` 中。

路径配置错误: 高级目录覆盖和 `RELAY_KNOWLEDGE_HOME` 必须是绝对路径，且不能包含 `..`。完整变量清单见 [第 8 章](08-advanced-configuration.md)。

`RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL must not be blank` 或
`RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL must not be blank`: 模型名环境变量不能只包含空白字符；删除该变量可回到默认本地 deterministic model。

`version does not support --format streaming-json`: `version` 按 CLI 帮助公开支持 `text` 和 `json`。

`repo impact` 返回 head snapshot 不存在: 先对目标 head 执行 `repo index` 或 `repo update`，再做影响分析。

MCP 返回 HTTP 400: 常见原因是缺少 `Mcp-Session-Id`、缺少 `MCP-Protocol-Version`、初始化 payload 不合法或 session 流程没有先发送 `notifications/initialized`。

MCP 返回 HTTP 404: 常见原因是 session id 未知、过期或被淘汰。重新执行 initialize 流程并保存新的 `Mcp-Session-Id`。

## 7.4 隔离复现

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

## 7.5 PR 验证命令

Rust 质量门禁:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
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

Web Operations 或 `/api/web/operations/execute` 变更时，PR 至少要覆盖 Rust router test、TypeScript build 和 Playwright 关键路径，确认同源执行结果来自后端统一 API，而不是前端模拟。

只修改文档时，至少检查新增链接和 Markdown 文件路径，并在 PR 中说明未运行代码测试的原因。

## 7.6 诊断顺序

遇到结果不符合预期时，按这个顺序缩小范围:

1. `status --format json`: 确认运行时目录、配置和项目状态。
2. `health --format json`: 确认 graph version、index freshness、queue/dead-letter、provider 和 QoS 状态。
3. `graph inspect --format json`: 确认 evidence、entity、structured facts 和 code counts。
4. `index refresh --format json`: 尝试显式刷新并读取 stale reasons。
5. 对应业务命令加 `--format json`: 保留完整 metadata、degraded reason 和 audit correlation。
6. `audit query --limit 50 --format json`: 查看最近 CLI/Web/service/agent 操作是否到达统一 API。

不要只凭 Web 页面摘要判断根因；Web 摘要会挑选最重要字段展示，完整诊断仍在 JSON API 和 CLI 响应中。
