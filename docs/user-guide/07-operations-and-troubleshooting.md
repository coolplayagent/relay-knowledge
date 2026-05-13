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

路径配置错误: `RELAY_KNOWLEDGE_*_DIR` 和 `RELAY_KNOWLEDGE_HOME` 必须是绝对路径，且不能包含 `..`。

## 7.4 隔离复现

排查用户数据或开发机污染时，用独立 runtime home:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-repro \
  relay-knowledge status --format json
```

把同一个变量加到 ingest、query、repo 和 service 命令上，可以完整复现独立数据目录下的行为。

## 7.5 PR 验证命令

Rust 质量门禁:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

Web 和浏览器集成测试:

```bash
npm install --prefix web
npm run build --prefix web
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

只修改文档时，至少检查新增链接和 Markdown 文件路径，并在 PR 中说明未运行代码测试的原因。
