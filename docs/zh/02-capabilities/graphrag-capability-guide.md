# GraphRAG 功能文档

[中文](./graphrag-capability-guide.md) | [英文](../../en/02-capabilities/graphrag-capability-guide.md)

> 文档版本: 1.0
> 编制日期: 2026-05-12
> 范围: 当前可用 GraphRAG 能力、CLI/Web/MCP/ACP 使用方式和降级语义。

## 1. 当前能力

`relay-knowledge` 当前提供本地知识图谱、代码知识图谱、混合检索 context pack、诊断状态、MCP Streamable HTTP 接入和本地 ACP session adapter。

当前可用能力:

- evidence ingest: 写入 source-scoped evidence 和 entity label，提交后产生新的 `graph_version`。
- structured fact ingest: API 可写入 evidence source path、span、confidence、status、typed relation、claim 和 event；结构化 facts 必须引用 supporting evidence ids，反序列化后的 span、confidence 和 version range 会重新验证。
- hybrid retrieval: 使用 SQLite FTS5 BM25、local semantic token read model、local hashed-vector ANN read model、可配置 external semantic/vector backend contract、graph evidence fallback、code graph documents、schema path、temporal event、community summary 和 RRF 返回 context pack，并携带实体、source span、结构化 facts、direct graph path evidence、code artifact 和 backend 状态；BM25 会索引 entity/code symbol 的生成式 lexical alias 但不把 alias 当成 canonical label 返回；`rejected`/`superseded` evidence 不会作为检索上下文返回。
- multimodal evidence: evidence 可记录 `text_span`、`image_asset`、`ocr_text`、`caption`、`image_embedding`、`table` 和 `layout_region` 抽取元数据；派生 OCR/caption/image embedding 按 parent evidence 合并为一个 context item；后台或 maintenance worker 通过 `commit_multimodal_extraction` 提交 OCR/caption/table/layout/image embedding 输出，查询热路径不运行抽取。
- code repository indexing: 注册 Git 仓库，索引 clean snapshot，增量更新，查询 symbol/reference/chunk，分析 diff impact。
- index recovery: graph commits 记录 affected scopes、entity ids、evidence ids 和 source hashes；scoped cursors 持久化 kind/scope/modality freshness、source hash、backend cursor，以及 semantic/vector worker 可回传的 model name/dimension；bounded refresh queue、lease/attempt guard、retry/dead-letter、diagnostic reconciler 和 startup reconciler 已接入 ingest、wait-until-fresh query、index refresh、health、service doctor 和 foreground service startup。
- diagnostics: graph inspect、index status、health、service doctor 和 Web readiness；`service status` 与 `service doctor` 当前复用同一统一 API 输出，报告 disabled service mode、后台更新状态、service definition path、agent protocol status、refresh queue diagnostics 和结构化 stale reasons。
- resident agent access: MCP Streamable HTTP 工具暴露 retrieve context、inspect graph、health、service status、index status、授权 code graph query、授权 code impact 和受权限控制的 index refresh；MCP resources 暴露 service/health/index/metrics 只读上下文，policy-gated graph summary 只在允许 unspecified scope 时暴露，MCP prompts 提供 retrieval 和 code-impact 调用模板；本地 ACP session adapter 暴露相同检索 contract，支持 progress updates、cancellation、context artifact、QoS admission、bounded audit events 和可选 JSONL 持久 audit sink。
- evaluation harness: 纯 Rust harness 和 CI fixture gate 覆盖 exact fact、multi-hop、temporal、negative rejection、stale index、ambiguous entity 和 code impact 观测。
- operational productization: ingest 后按 evidence modality 写入 embedding/OCR/vision/extractor worker task；`worker run-once` 通过外部 HTTP worker contract 或 deterministic fallback 生成人工审批 proposal；proposal accept/reject/supersede、持久 audit query、service manager definition plan/write 和 silent-update operator status/pause/resume 都通过统一 application API 暴露给 CLI/Web。

规划中能力:

- 具体外部 embedding/OCR/vision provider、认证、限流、模型并存刷新策略和生产 worker 响应适配；当前实现已有 runtime/read-model/worker contract。
- proposal lifecycle、事实冲突处理和审批流产品化。
- service manager install/upgrade/uninstall、silent update operator、跨进程 worker/watchdog 和 release diagnostics 产品化。
- 更完整的 ACP 远程 adapter。
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

### Backend 配置

Semantic/vector read model 默认使用本地确定性 backend。需要把外部 embedding
worker 的模型元数据接入 read model contract 时，使用 `env` 边界提供的变量:

```bash
RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external \
RELAY_KNOWLEDGE_VECTOR_BACKEND=external \
RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL=text-embed-3-small \
RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL=clip-vit-b32 \
RELAY_KNOWLEDGE_EMBEDDING_DIMENSION=1536 \
relay-knowledge index refresh --kind semantic --kind vector --format json
```

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` 和 `RELAY_KNOWLEDGE_VECTOR_BACKEND`
接受 `local`、`external` 或 `disabled`。`retrieve_context` 的
`backend_statuses` 会报告 configured backend、model、dimension、scope
post-filter 和 indexed graph version；refresh cursor completion 会把
semantic/vector 已索引文档中的 model name/dimension 写入 `index_cursors`。
`disabled` 模式不会运行对应 semantic/vector retriever，也不会调度对应 read
model refresh。模型名只接受 trim 后非空的值。

## 3. Web 工作区

Web workspace 从同源服务读取:

- `/api/project/status`
- `/api/health`
- `/api/web/operations/execute`

当前 Web 页面展示:

- 状态：graph version、health、index lag、mutation count 和图谱计数。
- GraphRAG readiness: evidence graph、BM25 read model、semantic cursor、vector cursor、code graph、runtime budgets、refresh recovery 和 stale reasons。
- Operations: retrieve、ingest、graph、code、index 和 service 操作的命令与 payload 预览，以及同源执行结果。
- Indexes: BM25、semantic、vector 的 index version、indexed graph version、state 和 lag。
- Runtime: HTTP bind、数据目录、状态目录、缓存目录、日志目录和 QoS budgets。

Web operation composer 可以生成、暂存并执行 typed command/request preview。执行时页面把当前 snapshot 发送到 `/api/web/operations/execute`，Rust Web adapter 复用 application service 完成实际 retrieve、ingest、graph inspect、index refresh、code repository workflow 或 service status/run snapshot，并把 result JSON 回显到页面。

`relay-knowledge service run` 会挂载 Web endpoints；如果加上 `--mcp streamable-http`，MCP 和 Web routes 会共用配置的 HTTP listener 与 QoS budget。

## 4. MCP 工作流

启动 MCP Streamable HTTP:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --web --mcp streamable-http
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
MCP 不暴露 index refresh 或 repository indexing；仓库索引需要用户主动运行 `relay-knowledge repo index` 或 `relay-knowledge repo update`。远程 bind 默认被拒绝，非本机监听需要显式设置 `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`。

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
- backend unavailable: semantic/vector 后端配置为 `disabled` 或 cursor metadata 不可用时，BM25 和 graph evidence 仍可工作。
- semantic/vector degraded: configured read model cursor 落后于 graph version 时报告 degraded；外部 embedding、OCR 或视觉 provider 不可用不会阻塞 BM25、graph path 或 temporal retrieval。

`context_pack.items[*].graph_paths` 是从同一 item 的 structured facts 派生的
一跳路径视图。每条 path 保留节点标签、edge fact id、predicate、supporting
evidence ids、confidence、status 和 version range，方便 agent 在引用关系或
事件链时使用路径结构而不是重新解析自然语言片段。

## 6. Context Pack 字段

检索响应的核心字段:

- `metadata.graph_version`: 查询绑定的图提交版本。
- `metadata.indexed_graph_version`: 参与检索的最低索引图版本。
- `retrieval_mode`: `hybrid` 或 `graph_only`。
- `source_scope`：来源过滤条件。
- `freshness`: 调用方请求的 freshness policy。
- `results`: evidence、code symbol 或 code chunk 命中。
- `context_pack.backend_statuses`: semantic/vector 等后端可用性、scope post-filter 和降级原因。
- `context_pack.items`: 可审计 context item，包含 retriever sources、ranking signals、entities、source span、structured graph facts 和 code artifact。
- `fusion`: RRF 算法、k 值和 candidate count。
- `budget_used`: limit、candidate count、returned count 和 context bytes。
- `degraded_reason`: stale、graph-only、backend unavailable 或其它降级原因。
- `truncated`: 结果或 context pack 是否被预算截断。

Health 和 index refresh 响应额外返回 `index_cursors[*]`，包含 kind、source scope、
modality、索引图版本、源哈希、后端游标，以及后端提供时的
模型名称/维度。`index_refresh.stale_reasons[*]` 会按索引族和作用域游标
解释未新鲜或失败的原因，包含类型（kind）、可选的源作用域（source scope）、可选的模态（modality）、原因（reason）、
滞后版本（lag versions）和最后错误（last error）；排障时先看失败/死信原因，再看滞后原因。

## 7. 运维检查

本地开发和 PR 验证建议运行:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
./build.sh
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

浏览器集成测试必须先构建 `web/dist`，再启动静态目录服务并验证 diagnostics、GraphRAG 准备状态、操作组合器、同源执行结果、索引表、运行时面板和移动端布局。
