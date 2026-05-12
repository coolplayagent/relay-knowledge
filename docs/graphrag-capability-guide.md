# GraphRAG 功能文档

> 文档版本: 1.0
> 编制日期: 2026-05-12
> 范围: 当前可用 GraphRAG 能力、CLI/Web/MCP 使用方式和降级语义。

## 1. 当前能力

`relay-knowledge` 当前提供本地知识图谱、代码知识图谱、混合检索 context pack、诊断状态和 MCP Streamable HTTP 接入。

当前可用能力:

- evidence ingest: 写入 source-scoped evidence 和 entity label，提交后产生新的 `graph_version`。
- hybrid retrieval: 使用 SQLite FTS5 BM25、graph evidence fallback、code graph documents 和 RRF 返回 context pack。
- code repository indexing: 注册 Git 仓库，索引 clean snapshot，增量更新，查询 symbol/reference/chunk，分析 diff impact。
- diagnostics: graph inspect、index status、health、service doctor 和 Web readiness。
- resident agent access: MCP Streamable HTTP 工具暴露 retrieve context、inspect graph、health、service status、index status 和受权限控制的 index refresh。

规划中能力:

- typed relation、claim、event、confidence、source span 和 proposal lifecycle。
- semantic/vector 后端和 ANN read model。
- scoped index refresh queue、lease、dead-letter 和 startup reconciler。
- ACP adapter、多模态 evidence、temporal query 和 community summary。

## 2. CLI 工作流

写入普通 evidence:

```bash
relay-knowledge ingest \
  --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust \
  --entity SQLite \
  --format json
```

查询 context pack:

```bash
relay-knowledge query "SQLite graph state" \
  --source docs \
  --freshness wait-until-fresh \
  --limit 8 \
  --format json
```

检查图和索引:

```bash
relay-knowledge graph inspect --format json
relay-knowledge index refresh --kind bm25 --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

代码仓库工作流:

```bash
relay-knowledge repo register /path/to/repo \
  --alias core \
  --path src \
  --language rust \
  --format json

relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo query core --query retry_policy --kind hybrid --format json
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo impact core --base main --head HEAD --format json
relay-knowledge repo status core --format json
```

## 3. Web 工作区

Web workspace 从同源服务读取:

- `/api/project/status`
- `/api/health`

当前 Web 页面展示:

- Status: graph version、health、index lag、mutation count 和图谱计数。
- GraphRAG readiness: evidence graph、BM25 read model、semantic cursor、vector cursor、code graph 和 runtime budgets。
- Operations: retrieve、ingest、graph、code、index 和 service 操作的命令与 payload 预览。
- Indexes: BM25、semantic、vector 的 index version、indexed graph version、state 和 lag。
- Runtime: HTTP bind、数据目录、状态目录、缓存目录、日志目录和 QoS budgets。

Web operation composer 当前用于生成和暂存 typed command/request preview。执行型 Web endpoint 仍待 Rust HTTP API adapter 提供。

## 4. MCP 工作流

启动 MCP Streamable HTTP:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --mcp streamable-http
```

默认地址:

```text
http://127.0.0.1:8791/mcp
```

客户端要求:

- 先调用 `initialize` 并提供受支持的 MCP protocol version。
- 保存服务端返回的 `Mcp-Session-Id`。
- 后续请求携带 session header 和 `MCP-Protocol-Version`。
- 发送 `notifications/initialized` 后再调用工具。

默认 agent policy 要求配置允许 scope。未配置 `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` 时，graph tools 会拒绝 unspecified scope，除非显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true`。

## 5. Freshness 和降级语义

`freshness` 支持三种策略:

- `allow-stale`: 允许返回旧索引结果，metadata 标记 stale 或 degraded。
- `wait-until-fresh`: 尝试刷新 stale index；无法满足时返回错误或 degraded。
- `graph-only`: 绕过派生索引，只返回图事实路径可提供的结果。

常见降级:

- stale index: `indexed_graph_version` 落后于 `graph_version`。
- graph-only: 调用方显式选择只读图事实。
- parser degraded: 文件为 partial、text-only 或 failed，代码查询仍可返回可用文本 chunk。
- budget exceeded: 检索结果或 agent context 超过 limit/context bytes，返回 `truncated=true`。
- backend unavailable: semantic/vector 后端未启用时，BM25 和 graph evidence 仍可工作。

## 6. Context Pack 字段

检索响应的核心字段:

- `metadata.graph_version`: 查询绑定的图提交版本。
- `metadata.indexed_graph_version`: 参与检索的最低索引图版本。
- `retrieval_mode`: `hybrid` 或 `graph_only`。
- `source_scope`: source filter。
- `freshness`: 调用方请求的 freshness policy。
- `results`: evidence、code symbol 或 code chunk 命中。
- `context_pack.items`: 可审计 context item，包含 retriever sources 和 ranking signals。
- `fusion`: RRF 算法、k 值和 candidate count。
- `budget_used`: limit、candidate count、returned count 和 context bytes。
- `degraded_reason`: stale、graph-only、backend unavailable 或其它降级原因。
- `truncated`: 结果或 context pack 是否被预算截断。

## 7. 运维检查

本地开发和 PR 验证建议运行:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
npm run build --prefix web
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

浏览器集成测试必须先构建 `web/dist`，再启动静态目录服务并验证 diagnostics、GraphRAG readiness、operation composer、index table、runtime panel 和移动端布局。
