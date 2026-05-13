# GraphRAG 功能文档

> 文档版本: 1.0
> 编制日期: 2026-05-12
> 范围: 当前可用 GraphRAG 能力、CLI/Web/MCP/ACP 使用方式和降级语义。

## 1. 当前能力

`relay-knowledge` 当前提供本地知识图谱、代码知识图谱、混合检索 context pack、诊断状态、MCP Streamable HTTP 接入和本地 ACP session adapter。

当前可用能力:

- evidence ingest: 写入 source-scoped evidence 和 entity label，提交后产生新的 `graph_version`。
- structured fact ingest: API 可写入 evidence source path、span、confidence、status、typed relation、claim 和 event；结构化 facts 必须引用 supporting evidence ids，反序列化后的 span、confidence 和 version range 会重新验证。
- hybrid retrieval: 使用 SQLite FTS5 BM25、local semantic token read model、local hashed-vector ANN read model、graph evidence fallback、code graph documents、schema path、temporal event、community summary 和 RRF 返回 context pack，并携带实体、source span、结构化 facts、direct graph path evidence、code artifact 和 backend 状态；BM25 会索引 entity/code symbol 的生成式 lexical alias 但不把 alias 当成 canonical label 返回；`rejected`/`superseded` evidence 不会作为检索上下文返回。
- multimodal evidence: evidence 可记录 `text_span`、`image_asset`、`ocr_text`、`caption`、`image_embedding`、`table` 和 `layout_region` 抽取元数据；派生 OCR/caption/image embedding 按 parent evidence 合并为一个 context item。
- code repository indexing: 注册 Git 仓库，索引 clean snapshot，增量更新，查询 symbol/reference/chunk，分析 diff impact。
- index recovery: graph commits 记录 affected scopes、entity ids、evidence ids 和 source hashes；scoped cursors 持久化 kind/scope/modality freshness、source hash、backend cursor，以及 semantic/vector worker 可回传的 model name/dimension；bounded refresh queue、lease/attempt guard、retry/dead-letter、diagnostic reconciler 和 startup reconciler 已接入 ingest、wait-until-fresh query、index refresh、health、service doctor 和 foreground service startup。
- diagnostics: graph inspect、index status、health、service doctor 和 Web readiness；`service status` 与 `service doctor` 当前复用同一统一 API 输出，报告 disabled service mode、后台更新状态、service definition path、agent protocol status 和 refresh queue diagnostics。
- resident agent access: MCP Streamable HTTP 工具暴露 retrieve context、inspect graph、health、service status、index status、授权 code graph query、授权 code impact 和受权限控制的 index refresh；本地 ACP session adapter 暴露相同检索 contract，支持 progress updates、cancellation、context artifact、QoS admission 和 bounded audit events。
- evaluation harness: 纯 Rust harness 覆盖 exact fact、multi-hop、temporal、negative rejection、stale index、ambiguous entity 和 code impact 观测。

规划中能力:

- 外部 semantic/vector embedding backend 与模型并存策略；当前实现是确定性的本地 read model。
- proposal lifecycle、事实冲突处理和审批流。
- service manager install/upgrade/uninstall、silent update operator 和持久 audit sink。
- MCP resources/prompts、旧 HTTP+SSE 兼容端点和更完整的 ACP 远程 adapter。
- 真实 OCR/caption/table/layout worker、image embedding backend 和 extractor 产品化。

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
relay-knowledge repo query core \
  --query retry_policy \
  --kind hybrid \
  --ref HEAD \
  --path src \
  --language rust \
  --freshness wait-until-fresh \
  --limit 10 \
  --format json
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
`relay.refresh_indexes` 默认隐藏，只有设置 `RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH=true` 后才会出现在 tool list 中。远程 bind 默认被拒绝，非本机监听需要显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`。

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
- local semantic/vector degraded: 当前 semantic/vector 使用本地确定性 read model；外部 embedding、OCR 或视觉模型不可用不会阻塞 BM25、graph path 或 temporal retrieval。

`context_pack.items[*].graph_paths` 是从同一 item 的 structured facts 派生的
一跳路径视图。每条 path 保留节点标签、edge fact id、predicate、supporting
evidence ids、confidence、status 和 version range，方便 agent 在引用关系或
事件链时使用路径结构而不是重新解析自然语言片段。

## 6. Context Pack 字段

检索响应的核心字段:

- `metadata.graph_version`: 查询绑定的图提交版本。
- `metadata.indexed_graph_version`: 参与检索的最低索引图版本。
- `retrieval_mode`: `hybrid` 或 `graph_only`。
- `source_scope`: source filter。
- `freshness`: 调用方请求的 freshness policy。
- `results`: evidence、code symbol 或 code chunk 命中。
- `context_pack.backend_statuses`: semantic/vector 等后端可用性、scope post-filter 和降级原因。
- `context_pack.items`: 可审计 context item，包含 retriever sources、ranking signals、entities、source span、structured graph facts 和 code artifact。
- `fusion`: RRF 算法、k 值和 candidate count。
- `budget_used`: limit、candidate count、returned count 和 context bytes。
- `degraded_reason`: stale、graph-only、backend unavailable 或其它降级原因。
- `truncated`: 结果或 context pack 是否被预算截断。

Health 和 index refresh 响应额外返回 `index_cursors[*]`，包含 kind、source scope、
modality、indexed graph version、source hash、backend cursor，以及后端提供时的
model name/dimension。

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
