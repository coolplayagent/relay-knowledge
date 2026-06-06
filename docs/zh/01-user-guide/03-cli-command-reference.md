# 第 3 章 CLI 命令参考

[中文](../../zh/01-user-guide/03-cli-command-reference.md) | [English](../../en/01-user-guide/03-cli-command-reference.md)

本章提供可执行命令索引。工作流说明分散在后续章节；本章用于快速找到入口和诊断命令。

当请求 `--format json` 或 `--format streaming-json` 时，写入 stderr 的解析诊断和运行期 API 失败都会使用 JSON。运行期 API 失败沿用稳定 API 错误结构，包含 `error_kind`、`message` 和可选 `metadata`；text 和 markdown 格式继续输出便于人工阅读的 stderr 消息。

需要从本地 CLI 访问已部署常驻服务时，使用全局 `--remote <base-url>` 或 `RELAY_KNOWLEDGE_REMOTE_BASE_URL`。远端模式覆盖 `repo index`、`repo scope preview`、`repo status`、`repo query`、`repo feature-flags`、`repo impact`、`repo report` 和 `repo software`，用于访问服务端已经注册的仓库。`repo index --reset` 和 `repo index-worker` 在远端模式选中时会被拒绝，必须在服务端机器执行；仅设置环境变量时，`status`、`health` 等无关本地命令继续使用本机 runtime state。

## 3.1 常用状态命令

项目状态:

```bash
relay-knowledge status --format json
```

健康检查:

```bash
relay-knowledge health --format json
```

服务诊断:

```bash
relay-knowledge service status --format json
relay-knowledge service doctor --format json
```

`service status` 和 `service doctor` 当前复用统一 API 输出，报告 service mode、后台更新状态、service definition path、agent protocol status 和 refresh queue diagnostics。

版本检查:

```bash
relay-knowledge version
relay-knowledge version check --format json
```

`version` 只打印当前二进制版本，不加载 runtime configuration，也不联网。`version check`
通过 `net::http` 按配置查询 GitHub Releases 和 crates.io，结果缓存到 runtime cache
目录；普通交互式 text/markdown CLI 命令只会在发现稳定新版时向 stderr 输出短提示，且会先输出主命令
stdout，不会自动替换二进制。

## 3.2 Provider 诊断

```bash
relay-knowledge provider probe --format json
```

`provider probe` 读取环境边界解析出的 remote embedding provider 配置，并执行一次轻量探测。JSON 响应包含 `ok`、`provider`、`model`、`dimension`、可选 `latency_ms`，失败时还包含 `error_code`、`error_message` 和 `retryable`。HTTP 429、HTTP 402 以及带 quota/backpressure 诊断的 HTTP 400 或 HTTP 403 响应表示 endpoint、认证边界和模型路由已经可达，因此 `ok=true`，同时保留 `error_code=rate_limited` 与 `retryable=true` 作为可观测降级诊断；普通认证、endpoint、model、timeout 和 malformed-response 失败仍返回 `ok=false`。它不会输出 API key 原文，也不会绕过 `env` 模块直接读取环境变量。

OpenAI-compatible embedding base URL 可以配置为 host root、版本化 API root（如 `/v1`、`/v4`）或完整 `/embeddings` endpoint；非版本路径前缀继续按 `<prefix>/v1/embeddings` 解析，query 或 fragment 后缀不参与 endpoint 构造。

endpoint host、batch、timeout、并发和 cursor metadata 属于 `status`、`health` 或 Web Providers 面板的运行时诊断。

## 3.3 Setup 诊断与配置画像

`setup doctor` 是 storage-free 的只读诊断命令:

```bash
relay-knowledge setup doctor --format json
```

它只读取已解析 runtime configuration，不打开或迁移 SQLite，也不刷新索引。`configuration_ready=true` 只表示配置检查通过；`live_health_checked=false` 表示 graph storage、index freshness 和 worker/service live health 仍需通过 `health` 或 `service doctor` 检查。

`setup profile` 不写文件、不安装服务，只输出推荐环境变量、命令和注意事项:

```bash
relay-knowledge setup profile local --format json
relay-knowledge setup profile agent-readonly --format json
relay-knowledge setup profile service --format json
relay-knowledge setup profile external-embedding --format json
```

这些 profile 分别覆盖零配置本地循环、只读 MCP agent 接入、平台 service manager 预览和外部 embedding provider metadata。需要把建议固化到 shell、service manager 或部署工具时，由调用方显式写入自己的配置面。

## 3.4 命令总览

```bash
relay-knowledge status
relay-knowledge help [command...] [--format text|json]
relay-knowledge ingest --source <scope> --content <text> [--entity <label>]
relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness allow-stale|wait-until-fresh|graph-only]
relay-knowledge files index [--root <path>] [--source <scope>]
relay-knowledge files query <text> [--source <scope>] [--root <root-id>] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
relay-knowledge map init
relay-knowledge map show [--topic <id>]
relay-knowledge map route <topic>
relay-knowledge map source add --id <id> --topic <id> --kind repo|file|doc|config|db|ci|runtime|wiki|monitoring --uri <uri> [--scope <source_scope>] [--description <text>]
relay-knowledge map source update --id <id> [--topic <id>] [--kind repo|file|doc|config|db|ci|runtime|wiki|monitoring] [--uri <uri>] [--scope <source_scope>] [--description <text>]
relay-knowledge map source remove --id <id>
relay-knowledge map validate
relay-knowledge map agent-snippet
relay-knowledge repo register <path> [--alias <name>] [--path <filter>]
relay-knowledge repo remove <alias>
relay-knowledge repo index <alias> [--ref <ref>] [--dry-run|--reset]
relay-knowledge repo index-worker [--task-id <id>]
relay-knowledge repo scope preview <alias> [--ref <ref>]
relay-knowledge repo update <alias> --base <ref> --head <ref>
relay-knowledge repo query <alias> --query <text> [--kind hybrid|symbol|definition|references|callers|callees|imports|sbom]
relay-knowledge repo feature-flags <alias> [--query <text>] [--ref <ref>] [--path <filter>] [--language <id>] [--limit <n>]
relay-knowledge repo impact <alias> --base <ref> --head <ref>
relay-knowledge repo report <alias> [--format markdown|json]
relay-knowledge repo software <alias> [--ref <ref>] [--kind dependencies|sdks|files|topics|relationships|build|iac|design|all] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
relay-knowledge repo status <alias>
relay-knowledge graph inspect
relay-knowledge index refresh [--kind bm25|semantic|vector]
relay-knowledge worker status|run-once [--kind embedding|ocr|vision|extractor]
relay-knowledge proposal list [--state proposed|accepted|rejected|superseded] [--limit <n>]
relay-knowledge proposal show <proposal-id>
relay-knowledge proposal accept|reject|supersede <proposal-id> --by <actor> [--reason <text>]
relay-knowledge audit query [--operation <name>] [--limit <n>]
relay-knowledge provider probe
relay-knowledge health
relay-knowledge service status
relay-knowledge service doctor
relay-knowledge service plan install|uninstall
relay-knowledge service definition write
relay-knowledge service operator status|pause|resume
relay-knowledge service worker run [--task-id <id>]
relay-knowledge service run [--web] [--mcp streamable-http]
relay-knowledge setup doctor
relay-knowledge setup profile local|agent-readonly|service|external-embedding
relay-knowledge version
relay-knowledge version check
```

Kind 取值按命令家族隔离：

- `repo query --kind` 和 `repo-set query --kind`：`hybrid`、`symbol`、
  `definition`、`references`、`callers`、`callees`、`imports`、`sbom`。
- `repo software --kind`：`dependencies`、`sdks`、`files`、`topics`、
  `relationships`、`build`、`iac`、`design`、`all`。
- `index refresh --kind`：`bm25`、`semantic`、`vector`；省略 `--kind`
  表示请求全部受支持的索引族。
- `worker status|run-once --kind`：`embedding`、`ocr`、`vision`、`extractor`。
- `map source add|update --kind`：`repo`、`file`、`doc`、`config`、`db`、
  `ci`、`runtime`、`wiki`、`monitoring`。

不要跨命令家族复用 kind 取值。影响分析使用 `repo impact`，feature flag 使用
`repo feature-flags`；它们不是 `repo query --kind` 的取值。

冷启动 full `repo index` 会立即返回持久化任务 handle，并由 CLI 进程启动有界后台 worker。非交互式 agent 可以用 `repo index-worker --task-id <id> --format json` 显式单次消费 queued 或 retrying 任务；`service worker run [--task-id <id>] --format json` 是 split-worker preview 入口，只 claim 一个 durable code-index task，并通过 task id、lease owner 和 attempt count 完成或失败该任务；`service run` 会消费同一个 code-index 队列，用于已安装服务或前台服务模式。cold repository index 运行中可用 `repo status --format json` 查看 `active_task`、checkpoint 计数和 scope retention。`repo index <alias> --reset --format json` 会清理该仓库未完成 task 的 stale lease，但不会删除已经完成的 indexed scope，也不会复活 terminal dead-letter 历史任务。每个仓库同时只有一个 live index writer；查询、报告、graph 读取、file query 和 health 诊断在 SQLite WAL 允许时走有界只读连接读取已提交快照。

`repo remove <alias>` 会从 relay-knowledge 运行时状态中删除该 alias 指向的整个注册仓库，包括该 repository id 的全部 alias、代码索引 scope、code-index task、repository-set 成员关系、repository-set overlay 和软件全域投影行。它不会删除磁盘上的源码仓库。如果仓库仍有 running code-index task lease，删除会被拒绝；删除成功后，同一路径或 alias 可以重新注册。

`query` 会返回兼容展示用的 `results`、面向 agent 的 `context_pack`、按 family 的 `indexes`、scoped `index_cursors` 以及 `index_refresh` queue/lag 诊断。`index_refresh.stale_reasons` 会解释 BM25、semantic、vector 和 scoped cursor 的 lag 或 failure；`index_cursors` 报告 source scope、modality、backend cursor、model metadata、indexed graph version 和可选 last error。`--freshness wait-until-fresh` 会在回答前走有界刷新路径；`--freshness allow-stale` 可以返回 stale read model，但会标记 metadata 和 degraded reason；`--freshness graph-only` 会绕过派生 read model，并让 cursor/queue 诊断保持为空。

`files index` 会把已配置或显式传入的授权本机 root 扫描进有界文件定位索引。显式 root 必须是绝对路径，并且必须被 `RELAY_KNOWLEDGE_FILE_INDEX_ROOTS` 授权；省略 `--root` 时扫描配置中的 root。`files query` 读取已提交的文件索引，不会 shell out 到 Everything、Spotlight、Windows Search、locate、`rg` 或 `grep`。JSON 响应包含 `freshness.state`、`freshness.index_lag`、`freshness.cursors`、`freshness.stale_reason`、`freshness.degraded_reason`、`freshness.bounded_rescan_required`、`freshness.direct_source_read_required`、`freshness.direct_source_read_paths` 和 `freshness.agent_instructions`。`--freshness wait-until-fresh` 会在 file index 仍为 pending、stale、degraded 或 overflow 时抑制答案，直到有界扫描完成。`--freshness allow-stale` 可以返回带这些诊断的已索引路径；当 `direct_source_read_required=true` 时，agent 在编辑或引用变化文件前必须直接读取返回路径。

`repo query` 的 `definition`、`references` 和 `hybrid` 查询先走已索引 tree-sitter 图和 SQLite FTS 读模型。`--freshness allow-stale` 在目标 ref 正在 full indexing 且尚未 finalize 时，会继续读取上一个已完成 committed scope，并在响应中标记 stale/degraded reason；`wait-until-fresh` 仍会要求目标 scope 新鲜。JSON 响应包含 `freshness.state`、`freshness.index_lag`、`freshness.pending`、`freshness.cursor`、`freshness.direct_source_read_required` 和 `freshness.agent_instructions`，让 agent 能看到 checkpoint 进度，并知道哪些返回路径在编辑或引用前必须直接读取源码。只有这些结构化层存在明确召回缺口时，查询才会在同一 indexed commit 上启动有界内部 exact-text source fallback；命中会在 JSON 中标记 `retrieval_layers=["lexical","text_fallback"]`，definition 兜底还会带 `definition`。候选路径查询、候选文件数、物化字节或单行长度预算耗尽只会降级兜底层，并通过 `degraded_reason` 暴露，不会让结构化代码图结果失效。

`repo feature-flags` 读取索引阶段写入的配置驱动特性开关图事实，默认列出所选 repository scope 内的开关、配置来源和代码使用关系；`--query` 只做名称、配置 key、路径或 excerpt 过滤。JSON 响应包含与 `repo query` 相同的 `freshness` 对象，包括 pending task、checkpoint cursor、index lag、stale/degraded reason，以及返回 feature-flag usage 文件的 direct-source-read paths。抽取器识别环境变量、config/settings key、布尔配置声明，以及 OpenFeature、LaunchDarkly、Unleash 等常见 SDK evaluation 调用。它不会同步 provider 控制面的状态、策略、segment 或 rollout variant。该命令不会在查询时扫描全仓库源码；新增或修正开关抽取逻辑后，需要重新 `repo index` 或 `repo update` 才能看到新事实。

`repo software` 读取所选 repository scope 的软件全域模型投影。`--kind dependencies` 返回由 manifest 和 lockfile 生成的包组件，以及把 declared package 与代码/配置 import 证据关联的 `dependency_usages`；`--kind sdks` 返回 unresolved external import/include 目标，作为 SDK 或 API surface 使用候选；`--kind files` 返回代码、配置、文档、构建、部署、测试和模板文件整体节点；`--kind topics` 返回从 Markdown/spec heading 和 `.knowledge/knowledge-map.yaml` 抽取的主题；`--kind relationships` 返回 `documents`、`depends_on`、`uses_sdk` 和 `configures` 等跨域关系。`--kind build` 返回从 Cargo、npm、Python、Go、Maven effective `pom.xml`、Gradle、CMake、Makefile 和 CI workflow 证据中提取的 package、script、target、feature、module、profile、plugin、goal、job 等构建入口。`--kind iac` 返回 Dockerfile、Compose、Kubernetes YAML、Helm chart、Terraform、systemd、launchd 和 CI workflow 中提取的部署/基础设施资源。`--kind design` 返回 README、架构/设计 Markdown 和 package/module manifest 中有证据支撑的软件系统、模块、组件、接口和能力元素。该命令不会执行构建工具、扫描包缓存、SDK 目录、云 API、未索引外部源码或查询时全仓文档；source scope 变化后需要重新 `repo index` 或 `repo update` 刷新投影。

面向 Agent 的 MCP kind 查询复用同一组 kind family，不引入并行名称。`relay_code_query` 覆盖代码图谱 kind，`relay_software_query` 覆盖软件全域模型 kind，`relay_code_feature_flags` 覆盖配置驱动 feature flag。常见 agent 别名会归一到现有 kind：`dependency` 归一为 `dependencies`，`configuration` 归一为 `relationships`，`model` 或 `models` 归一为 `design`。

`map` 命令维护仓库内 `.knowledge/knowledge-map.yaml` 知识导航契约。该 YAML 文件只保存 topic、source、route 和 history 元数据，不复制真实知识内容；真实知识仍以文档、代码、配置、CI、运行态系统或外部知识源为准。一个 topic 可以包含多个 source，`map source add` 会把不同 source id 追加到该 topic 的 route 顺序中。LLM agent 应通过 `map show` 和 `map route` 定位知识源，通过 `map source add/update/remove` 维护契约，并在变更后运行 `map validate --format json`。AGENTS.md 只应保留 `Knowledge map: .knowledge/knowledge-map.yaml` 这样的稳定引用。

## 3.5 读写影响

状态、健康、帮助、setup doctor/profile、provider probe、version check、report、map show/route/validate/agent-snippet 和 audit query 是诊断入口，不应修改图谱事实。`health` 是 liveness 快路径，不会排队 index refresh，也不会等待 code-index writer 完成；存储繁忙时它可以返回 stale/degraded `storage_busy`。`version check` 只可能刷新 runtime cache 下的版本检查缓存。`ingest`、`map init`、`map source add/update/remove`、`repo remove`、`repo index`、`repo update`、`index refresh`、`worker run-once`、proposal 状态变更和 service definition write 会写入运行时状态、派生索引、proposal/audit、知识导航契约或 service definition。

自动化调用方应优先读取 `help --format json` 中的 operation 和 read/write 说明，再决定是否在 CI、agent 或 Web 操作面中开放命令。

## 3.6 Skill-over-CLI

仓库随附 `skills/relay-knowledge-cli`，这是一个兼容 ClawHub 的 skill，用于让
LLM agent 通过本地 CLI 调用 relay-knowledge，并解析 JSON 输出。它覆盖安装检查、
`version check`、setup/health 诊断、知识图谱 ingest/query，以及代码仓库注册、索引、查询、增量更新、影响分析和报告工作流。

该 skill 不配置 MCP、不调用 MCP 工具，也不管理 ACP session。协议级 agent 接入请使用
MCP/ACP 对应章节。
