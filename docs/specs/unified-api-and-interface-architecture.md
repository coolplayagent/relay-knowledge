# 统一 API 层与交互层架构

> 文档版本: 1.0
> 编制日期: 2026-05-11
> 适用范围: CLI、Web、未来 HTTP API / MCP 与核心服务边界
> 默认路线: Rust 核心服务 + 统一 API contract + React/Vite Web 交互层 + 薄 CLI adapter

## 1. 设计结论

`relay-knowledge` 的 CLI 和 Web 都只是交互层，不拥有业务规则、图谱事务、检索策略或索引刷新逻辑。它们必须收口到同一组 application service 和 API contract 上，避免 CLI、Web、HTTP API、MCP 各自实现一套行为。

v1 的架构目标:

1. **统一 API 层**: CLI、Web、未来服务接口共享同一套 request / response / error / stream event 类型。
2. **后端少框架**: domain、application、api 不依赖 Web 框架；HTTP server 只作为最外层 adapter。
3. **Web 先进但克制**: Web v1 使用轻量 TypeScript 静态前端，组件只负责交互和渲染，通过 API client 调用统一 API。
4. **CLI 可机器消费**: CLI 支持 `--format text|json|streaming-json`，其中 `streaming-json` 使用 NDJSON。
5. **可测试分层**: domain 和 application 使用 Rust 单元测试；CLI 用二进制集成测试；Web 用 API client mock、组件测试和关键端到端测试。

## 2. 分层模型

```text
+------------------------+       +------------------------+
| CLI Adapter            |       | Web Adapter            |
| args, exit code, output|       | React views, API client|
+-----------+------------+       +-----------+------------+
            |                                |
            +---------------+----------------+
                            |
                            v
                  +------------------+
                  | Unified API      |
                  | request/response |
                  | error/events     |
                  +--------+---------+
                           |
                           v
                  +------------------+
                  | Application      |
                  | use cases        |
                  +--------+---------+
                           |
           +---------------+----------------+
           |               |                |
           v               v                v
       Domain          Storage          Retrieval/
       model           traits           indexing traits
```

依赖方向固定为:

```text
interfaces -> api -> application -> domain
                         |
                         +-> storage/indexing/event/observability traits
```

禁止事项:

- CLI 或 Web 直接访问数据库、索引后端、队列或 mutation log。
- Web 前端复制 CLI 的业务判断。
- Web 框架、HTTP 框架类型进入 domain、application 或 api 模块。
- 输出格式分支影响核心服务行为。

## 3. 统一 API Contract

API 层负责定义稳定的交互边界，而不是承载业务逻辑。

核心类型:

- `RequestContext`: `interface`、`request_id`、`trace_id`。
- `ApiMetadata`: `trace_id`、`request_id`、`graph_version`、可选 `index_version`、`indexed_graph_version`、`stale`。
- `ApiError`: 稳定 `error_kind` 加可读 `message`。
- `ApiStreamEvent`: 一行一个流式事件，适用于 CLI `streaming-json` 和未来 HTTP streaming。

响应约束:

- 所有成功响应必须包含 `metadata`。
- 检索类响应必须暴露图版本和索引新鲜度。
- 错误必须使用稳定分类，例如 `invalid_argument`、`storage_unavailable`、`timeout`、`internal`。
- 用户输入原文不进入 metrics label；如需排障，进入日志或 trace attribute。

v1 已落地的最小 API:

- async `RelayKnowledgeService::project_status`
- async `RelayKnowledgeService::ingest`
- async `RelayKnowledgeService::retrieve_context`
- async `RelayKnowledgeService::inspect_graph`
- async `RelayKnowledgeService::refresh_indexes`
- async `RelayKnowledgeService::health`
- async `RelayKnowledgeService::service_status`
- async `RelayKnowledgeService::reconcile_startup_indexes`
- async `RelayKnowledgeService::register_code_repository`
- async `RelayKnowledgeService::index_code_repository`
- async `RelayKnowledgeService::query_code_repository`
- async `RelayKnowledgeService::impact_code_repository`
- async `RelayKnowledgeService::code_repository_status`
- CLI `--format text`
- CLI `--format json`
- CLI `--format streaming-json`

当前 CLI 已接入这些命令:

- `status`
- `ingest --source <scope> --content <text> [--entity <label>]`
- `query <text> [--source <scope>] [--limit <n>] [--freshness allow-stale|wait-until-fresh|graph-only]`
- `repo register <path> --alias <name> [--path <filter>] [--language <id>]`
- `repo index <alias> [--ref <ref>]`
- `repo update <alias> --base <ref> --head <ref>`
- `repo query <alias> --query <text> [--kind hybrid|symbol|definition|references|callers|callees|imports] [--limit <n>] [--ref <ref>] [--path <filter>] [--language <id>] [--freshness allow-stale|wait-until-fresh|graph-only]`
- `repo impact <alias> --base <ref> --head <ref> [--limit <n>]`
- `repo status <alias>`
- `graph inspect`
- `index refresh [--kind bm25|semantic|vector]`
- `health`
- `service status|doctor`
- `service run [--web] [--mcp streamable-http]`
- `version`
- `--version`

`ingest`、`query`、`repo *`、`graph inspect`、`index refresh`、`health` 和 `service doctor`
都通过统一 API contract 调用 application service，不直接访问 storage 或 index metadata。
`query` 的可选 `source_scope` 在 application service 边界按 domain 规则验证和归一化；
`query -- <text>` 用于表达以 `-` 开头的查询文本；
`graph-only` freshness 路径不得读取或刷新 index metadata，这样索引元数据损坏时仍可返回图事实查询。
未显式提供 evidence ID 时，application service 只能从已验证的 source scope 和 trim 后
content 生成稳定 ID；哈希输入必须使用无歧义编码，不能用可出现在字段值中的分隔符拼接。

## 4. CLI 输出协议

CLI adapter 由 Tokio runtime 驱动，只做参数解析、调用 async application service、渲染输出和设置退出码。

CLI 参数解释是接口 contract 的一部分。`relay-knowledge help --format json`
必须提供机器可读命令规格，覆盖 command path、operation、读写影响、参数语义、
是否必填、默认值、允许取值、是否可重复、示例和注意事项。skill、脚本和 LLM
工具应优先读取这份规格，而不是解析自然语言 help；新增或改变 CLI/API/config
参数时必须同步更新自描述 metadata 和测试。

格式:

| 格式 | 用途 | 输出形态 |
| --- | --- | --- |
| `text` | 人类直接阅读 | 稳定短文本 |
| `json` | 脚本一次性解析 | 单个 JSON object |
| `streaming-json` | 长任务、ingest、retrieval progress | NDJSON，每行一个 JSON event |

`streaming-json` 事件约定:

```json
{"event":"started","operation":"project.status","metadata":{"trace_id":"...","request_id":"...","graph_version":0,"stale":false}}
{"event":"progress","operation":"project.status","message":"...","metadata":{"trace_id":"...","request_id":"...","graph_version":0,"stale":false}}
{"event":"item","operation":"project.status","project_name":"relay-knowledge","metadata":{"trace_id":"...","request_id":"...","graph_version":0,"stale":false}}
{"event":"completed","operation":"project.status","metadata":{"trace_id":"...","request_id":"...","graph_version":0,"stale":false}}
```

退出码:

- `0`: 成功。
- `2`: 参数或格式错误。
- `1`: 运行时或渲染失败。

## 5. Web 架构

Web v1 默认采用 React + Vite + TypeScript。Web 是交互层，不是第二套业务后端。
当前 Web workspace 工程位于 `web/`，使用 TypeScript 静态前端，通过前端 typed
contract 复用 `ProjectStatusResponse`、`HealthResponse`、index status、
scoped index cursor 和 index refresh diagnostics 字段形状。`index_cursors`
包含 kind、scope、modality、indexed graph version、state、source hash、
backend cursor，以及后端提供时的 model name/dimension。
Web client 必须从同源服务 API 读取 `/api/project/status` 和 `/api/health`，不得在前端
伪造健康状态、图版本、运行时路径或索引元数据。当前 Web 页面从这两个 contract 派生
Status、GraphRAG readiness、Indexes、Runtime 和操作工作台状态；GraphRAG readiness 只
展示 evidence graph、BM25 read model、semantic cursor、vector cursor、code graph、
refresh recovery 和 runtime budgets 的诊断投影。操作工作台可以为检索、摄取、图检查、
代码仓库、索引刷新和服务运行生成 typed request / CLI command preview，并将操作加入
本地 staged queue；执行操作时必须调用同源 `/api/web/operations/execute`，由 Rust Web
HTTP adapter 解析 snapshot、调用 application service、返回 result JSON。`service run --web`
必须挂载 Web endpoints；同时启用 MCP Streamable HTTP 时，MCP routes 和 Web routes 必须共用
同一 `net::http` listener 和 QoS budget。Web execute route 必须使用配置的 HTTP body
budget，非 loopback bind 必须遵守 remote-client access policy，前端不得对长运行操作套用
短诊断请求超时。浏览器测试验证
统一 contract、同源执行结果和本地操作编排状态，不把业务逻辑放入前端。

推荐目录:

```text
web/
  src/
    api/          # typed API client and stream parser
    components/   # reusable UI components
    features/     # graph, retrieval, ingestion feature views
    routes/       # route-level composition
    test/         # fixtures and test helpers
```

实现约束:

- API client 只消费统一 API contract。
- 页面组件不直接拼接存储、索引或检索流程。
- 流式接口复用 `ApiStreamEvent` 语义，前端按 `started/progress/item/completed/failed` 更新状态。
- 状态必须显式表达 loading、success、stale、error、streaming progress。
- Web server 如由 Rust 提供，必须作为 adapter 调用同一组 application service。

测试:

- Vitest 覆盖 API client、stream parser 和纯状态转换。
- React Testing Library 覆盖组件渲染状态。
- Playwright 只覆盖关键用户路径，不替代单元测试。

## 6. 当前落地状态与验收门禁

当前统一 API 后续实施项已经落地到 CLI、Web、HTTP 和 MCP/ACP adapter:

1. `api` 模块已经提供 ingest、hybrid retrieval、graph inspect、index refresh、health、service status、worker、proposal、audit、provider 和 code repository 的 request / response contract；`ApiStreamEvent` 继续作为 CLI `streaming-json` 和未来 HTTP streaming 的统一 NDJSON 事件形状。
2. `RelayKnowledgeService` 是唯一 application service 入口。storage lazy init、index refresh queue、retrieval runtime、worker/proposal/audit/service diagnostics 和 runtime config 都通过 service constructor 接入；测试使用 deterministic environment 或 `with_store` 注入。
3. CLI 子命令已经接入真实 application service，并保持 `--format text|json|streaming-json` 输出协议；`version` 只支持 `text|json`。
4. Web 工程使用轻量 TypeScript 静态前端和手写 typed client。Web Operations 的 `Run` 调用同源 `/api/web/operations/execute`，由 Rust Web adapter 分发到同一 application service。
5. HTTP/Web/MCP 都是外层 adapter；新增 route 或 tool 不改变 application service 的行为语义。Web 与 MCP 同时启用时共用同一 `net::http` listener、QoS budget 和配置的 request body budget。

`/api/web/operations/execute` 当前支持 retrieve context、ingest evidence、graph inspect、index refresh、provider probe、worker status/run-once、proposal list/show/accept/reject/supersede、audit query、code repository register/index/update/query/impact/status，以及 service status/run snapshot。`service.run.streamable_http` 在 Web 中返回服务状态快照，不从浏览器启动常驻 loop。

当前 PR CI 已拆分 Rust format、clippy、unit/integration tests、coverage、build 和
Playwright Chromium browser integration gate。浏览器 gate 先构建 Web workspace，
再安装 Chromium 并运行 `tests/browser`。
