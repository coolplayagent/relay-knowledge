# 第 3 章 CLI 命令参考

[中文](../../zh/01-user-guide/03-cli-command-reference.md) | [English](../../en/01-user-guide/03-cli-command-reference.md)

本章提供可执行命令索引。工作流说明分散在后续章节；本章用于快速找到入口和诊断命令。

当请求 `--format json` 或 `--format streaming-json` 时，写入 stderr 的解析诊断和运行期 API 失败都会使用 JSON。运行期 API 失败沿用稳定 API 错误结构，包含 `error_kind`、`message` 和可选 `metadata`；text 和 markdown 格式继续输出便于人工阅读的 stderr 消息。

需要从本地 CLI 访问已部署常驻服务时，使用全局 `--remote <base-url>` 或 `RELAY_KNOWLEDGE_REMOTE_BASE_URL`。远端模式覆盖 `repo list`、`repo index`、`repo update`、`repo scope preview`、`repo status`、`repo query`、`repo graph`、`repo context`、`repo framework`、`repo feature-flags`、`repo impact`、`repo report`、`repo software`（包括 `export`）和 `repo view`，用于访问服务端已经注册的仓库。`repo index --reset` 和 `repo index-worker` 在远端模式选中时会被拒绝，必须在服务端机器执行；仅设置环境变量时，`status`、`health` 等无关本地命令继续使用本机 runtime state。

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

所有 remote embedding 与 provider HTTP 都必须使用共享 QoS runtime。Admission rejection 在网络 I/O 前发生，timeout/cancellation diagnostic 保持可见；不接收 QoS 的 deprecated library constructor 会拒绝 remote provider。

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
relay-knowledge files content <text> [--source <scope>] [--root <root-id>] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
relay-knowledge map init [--type knowledge|codespec|all]
relay-knowledge map show [--type knowledge|codespec|all] [--topic <id>] [--directory <path>]
relay-knowledge map history [--type knowledge|codespec|all] [--from <version>] [--limit <count>]
relay-knowledge map route <topic> --type knowledge
relay-knowledge map source add --type knowledge --id <id> --topic <id> --kind repo|file|doc|config|db|ci|runtime|wiki|monitoring --uri <uri> [--scope <source_scope>] [--description <text>]
relay-knowledge map source update --type knowledge --id <id> [--topic <id>] [--kind repo|file|doc|config|db|ci|runtime|wiki|monitoring] [--uri <uri>] [--scope <source_scope>] [--description <text>]
relay-knowledge map source remove --type knowledge --id <id>
relay-knowledge map directory add --type <knowledge|codespec> --directory <path> --purpose <text> --content-scope <glob> --load-hint <hint> --update-rule <rule> [--key-file <path>] [--relation <kind=target>]
relay-knowledge map directory update --type <knowledge|codespec> --directory <path> [directory fields]
relay-knowledge map directory remove --type <knowledge|codespec> --directory <path>
relay-knowledge map migrate --type knowledge --to-v4
relay-knowledge map validate [--type knowledge|codespec|all]
relay-knowledge map agent-snippet
relay-knowledge repo list
relay-knowledge repo register <path> [--alias <name>] [--path <filter>]
relay-knowledge repo remove <alias>
relay-knowledge repo index <alias> [--ref <ref>] [--dry-run|--reset]
relay-knowledge repo index-worker [--task-id <id>]
relay-knowledge repo scope preview <alias> [--ref <ref>]
relay-knowledge repo update <alias> [--base <ref>] [--head <ref>]
relay-knowledge repo query <alias> --query <text> [--kind hybrid|symbol|definition|references|callers|callees|imports|sbom] [--ref <ref>] [--path <filter>] [--language <id>] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
relay-knowledge repo graph <alias> --focus <path> --path <root> [--ref <ref>] [--depth 1|2] [--node-limit <n>] [--edge-limit <n>]
relay-knowledge repo context <alias> --query <text> [--ref <ref>] [--path <filter>] [--language <id>] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>] [--max-context-bytes <n>] [--no-code] [--exclude-generated]
relay-knowledge repo framework <alias> [--query <text>] [--framework angular|vue] [--kind component|directive|pipe|template|input|output|prop|emit|model|slot|template-variable|control-flow] [--ref <ref>] [--path <filter>] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
relay-knowledge repo feature-flags <alias> [--query <text>] [--ref <ref>] [--path <filter>] [--language <id>] [--limit <n>] [--domain <domain>] [--source <format>] [--hot-reload true|false] [--consistency]
relay-knowledge repo impact <alias> --base <ref> --head <ref>
relay-knowledge repo report <alias> [--format markdown|json]
relay-knowledge repo software <alias> [--ref <ref>] [--kind dependencies|sdks|files|topics|relationships|build|modules|iac|design|systems|apis|resources|tests|deployments|releases|statements|conflicts|all] [--path <prefix>] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>] [--cursor <token>]
relay-knowledge repo software export <alias> --profile spdx-3|cyclonedx-1.7|prov-o [--ref <ref>] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
relay-knowledge repo business <alias> [--ref <ref>] [--domain <id>] [--query <text>] [--kind terms|mappings|all] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
relay-knowledge repo view <alias> [--kind architecture-layers|business-domains|dependency-tour|process-flow|affected-scope] [--ref <ref>] [--path <filter>] [--language <id>] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>] [--changed-path <path>]
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
relay-knowledge service plan install|upgrade|rollback|uninstall [--target-version <version>] [--install-dir <path>]
relay-knowledge service lifecycle install|upgrade|rollback|uninstall [--dry-run|--execute] [--target-version <version>] [--install-dir <path>]
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
- `repo framework --kind`：`component`、`directive`、`pipe`、`template`、
  `input`、`output`、`prop`、`emit`、`model`、`slot`、`template-variable`、
  `control-flow`。
- `repo software --kind`：`dependencies`、`sdks`、`files`、`topics`、
  `relationships`、`build`、`iac`、`design`、`systems`、`apis`、
  `resources`、`tests`、`deployments`、`releases`、`statements`、
  `conflicts`、`all`。
- `repo business --kind`：`terms`、`mappings`、`all`。
- `repo view --kind`：`architecture-layers`、`business-domains`、
  `dependency-tour`、`process-flow`、`affected-scope`。
- `index refresh --kind`：`bm25`、`semantic`、`vector`；省略 `--kind`
  表示请求全部受支持的索引族。
- `worker status|run-once --kind`：`embedding`、`ocr`、`vision`、`extractor`。
- `map source add|update --kind`：`repo`、`file`、`doc`、`config`、`db`、
  `ci`、`runtime`、`wiki`、`monitoring`。

不要跨命令家族复用 kind 取值。影响分析使用 `repo impact`，Angular/Vue template 语义使用
`repo framework`，feature flag 使用 `repo feature-flags`；它们不是 `repo query --kind` 的取值。

`--path` 是 CLI 中 path filter 的参数名。`repo register --path` 保存索引范围，`repo query --path`、`repo framework --path` 和 `repo feature-flags --path` 只在该已索引范围内收窄读取。`repo index` 不接受 `--path`，它使用注册范围和选定的 `--ref`。非 Git 源码目录的常规移动文件系统快照使用 `HEAD`，状态里会记录解析后的 `filesystem:<hash>` commit。`worktree` 是 Git worktree overlay selector，不是非 Git 目录的默认 ref。

冷启动 full `repo index` 会立即返回持久化任务 handle，并由 CLI 进程启动有界后台 worker。对于显式提供 `--reuse-historical` 的 Git 仓库，目标 scope 尚未 fresh 时会沿目标 commit 的第一父链检查最近 10 个祖先；若其中最近的兼容 scope 已发布且仍是当前 fact version，服务会把这次 full 请求固定为真实的 `Incremental { base_ref, head_ref }` 任务。该选择会显示在 `task.mode` 和完成摘要的 `base_resolved_commit_sha` 中；没有兼容基线或 base→head diff 超过历史复用专用的 100 changed-path 上限时，自动回退 checkpointed full index。未提供 `--reuse-historical` 时，`repo index` 保持默认的 checkpointed full-index 行为。非交互式 agent 可以用 `repo index-worker --task-id <id> --format json` 显式单次消费 queued 或 retrying 任务；每次调用还会推进一次有界 scope-retention pass，并返回 `maintenance_active` 与可选 `maintenance_error`。maintenance error 非空表示该 retention pass 失败，它与 code-index task 结果分开报告，此时不能把 `maintenance_active=false` 当作 drain 完成；应查看 `repo status`、处理错误，再重试一次有界 pass。未运行 `service run` 时，应重复本地命令到它和 `repo status` 都不再报告 pending maintenance。`service worker run [--task-id <id>] --format json` 是 split-worker preview 入口，只 claim 一个 durable code-index task，并通过 task id、lease owner 和 attempt count 完成或失败该任务；它不暴露上述本地 retention 字段。`service run` 会消费同一个 code-index 队列，用于已安装服务或前台服务模式。cold repository index 运行中可用 `repo status --format json` 查看 `active_task`、checkpoint 计数和 scope retention。`repo index <alias> --reset --format json` 会清理该仓库未完成 task 的 stale lease，但不会删除已经完成的 indexed scope，也不会复活 terminal dead-letter 历史任务。每个仓库同时只有一个 live index writer；查询、报告、graph 读取、file query 和 health 诊断在 SQLite WAL 允许时走有界只读连接读取已提交快照。

`repo update <alias>` 也通过同一持久队列提交 Incremental task。省略参数时，`--base` 使用最近一次发布的 clean Git commit（worktree-overlay identity 会解包为 clean base），`--head` 使用 `HEAD`；服务会在入队前把两者解析为不可变 commit。没有已发布 clean base 时，先运行 `repo index <alias> --ref HEAD`。本地 CLI 会执行一次有界 drain，远端 `repo update` 则可能返回 `task.state=queued` 交给常驻 worker。完成态 response 的 `summary` 包含 `base_resolved_commit_sha`；排队态可从 `task.mode` 查看固定后的 base/head。单次 Git delta 在应用注册 path filter 前按整个 commit pair 计算，最多 512 个 changed path；超过上限后必须改用 full index。

每次成功发布后运行 scope retention：保留 active scope 与最近两个成功发布时间窗口的并集（窗口通常已包含 active）、最近一次成功增量的 predecessor、active worktree overlay 的 clean base，再加未完成 task 的 target/base scope 和 repository-set pin。它先原子地把一个旧 scope 标为 `retiring`，从查询和增量 base 选择中排除，并记录 durable GC job；后续每个 maintenance transaction 推进一个 scope-GC phase，该 phase 在受影响的应用表之间合计最多删除 512 个物理行，包括事实、FTS/search row、software projection、checkpoint、workspace state 或 scope metadata。同 tree commit 复用内容图，并使用每仓 256 条的 commit alias 窗口。完成态 task 审计行按仓库限制为最近 128 条 success 和 64 条 failure/dead-letter/cancellation，但每个仍保留 scope 的最新 success 行继续保留。`repo status --format json` 暴露 `maintenance_pending`，以及 retiring job 的 phase、累计删除行数与最近错误；`scope_listing_truncated=true` 表示 retained/prunable 数组和显示计数只是有界诊断投影，不是完整列表。已淘汰 ref 必须先 full reindex。

`repo list` 是只读的已索引仓库清单。它只返回至少拥有一个已完成 indexed scope 的仓库；仅执行过 `repo register`、尚未完成 `repo index` 的仓库不会出现在结果中。text 输出逐行显示 alias、state、文件/符号数、stale、indexed commit 和 root；`--format json` 返回 `metadata` 与按 alias/repository id 稳定排序的 `repositories` 状态数组。使用 `--remote` 时读取服务端清单，不会回退到本机 runtime state。

批量代码索引的 snapshot apply 或 checkpointed finalize 成功后，SQLite 存储会自动 best-effort 执行 `PRAGMA optimize` 和 `PRAGMA wal_checkpoint(PASSIVE)`，刷新 planner 统计并折叠 WAL 页。维护失败不会把已成功的索引结果回滚为失败，但 `health --format json` 和 graph inspection 的 `graph.sqlite` 会暴露 `journal_mode`、`wal_size_bytes`、`last_maintenance_at_ms` 和 `last_maintenance_error`。维护时间和错误会持久化到 SQLite，因此服务重启或一次性 worker 退出后仍能看到上一轮维护结果。`partitioned_sqlite` 拓扑下这些字段会通过只读 shard 诊断聚合 control 数据库和 active repository shard 数据库；任一 active shard 无法检查时，`wal_size_bytes` 为未知并保留 shard 错误。大仓 query-plan 或索引性能回归应通过 `tools/self_iteration --categories performance` 覆盖，而不是在普通 CLI 路径里扫描未受控的大 fixture。

`repo remove <alias>` 会从 relay-knowledge 运行时状态中删除该 alias 指向的整个注册仓库，包括该 repository id 的全部 alias、代码索引 scope、code-index task、repository-set 成员关系、repository-set overlay 和软件全域投影行。它不会删除磁盘上的源码仓库。如果仓库仍有 running code-index task lease，删除会被拒绝；删除成功后，同一路径或 alias 可以重新注册。

`query` 会返回兼容展示用的 `results`、面向 agent 的 `context_pack`、按 family 的 `indexes`、scoped `index_cursors` 以及 `index_refresh` queue/lag 诊断。`index_refresh.stale_reasons` 会解释 BM25、semantic、vector 和 scoped cursor 的 lag 或 failure；`index_cursors` 报告 source scope、modality、backend cursor、model metadata、indexed graph version 和可选 last error。`--freshness wait-until-fresh` 会在回答前走有界刷新路径；`--freshness allow-stale` 可以返回 stale read model，但会标记 metadata 和 degraded reason；`--freshness graph-only` 会绕过派生 read model，并让 cursor/queue 诊断保持为空。

`files index` 会把已配置或显式传入的授权本机 root 扫描进两层 read model。低延迟 path/metadata 层服务 `files query`，不依赖内容抽取；有界内容层服务 `files content`，v1 覆盖 Markdown、文本、YAML/JSON、SQL、TOML、CSV、INI、config 和 XML 等在内容字节预算内的文本文件。显式 root 必须是绝对路径，并且必须被 `RELAY_KNOWLEDGE_FILE_INDEX_ROOTS` 授权；省略 `--root` 时扫描配置中的 root。`files query` 和 `files content` 读取已提交的本地索引，不会 shell out 到 Everything、Spotlight、Windows Search、locate、`rg` 或 `grep`。内容命中包含 `content_role="user_source"`、source path、span、fingerprint、content hash、indexed graph version、ranking signals 和 candidate facts；adapter 必须把文件内容当作引用数据处理，不能当作 agent 或 system 指令。JSON freshness 响应还包含 `freshness.state`、`freshness.index_lag`、`freshness.cursors`、`freshness.stale_reason`、`freshness.degraded_reason`、`freshness.bounded_rescan_required`、`freshness.direct_source_read_required`、`freshness.direct_source_read_paths` 和 `freshness.agent_instructions`；当派生内容 read model 落后时，stale file-content cursor count 通过有界 root 诊断报告，而不会 materialize 每个 cursor row。内容字节预算耗尽会报告为 overflow；符合内容索引条件但无法打开或解码的文件会报告为 degraded read failure，并保留 root last error。v1 内容 BM25/fact read model 会在有界扫描中同步刷新，因此成功扫描可以满足 `--freshness wait-until-fresh`；file index 仍为 pending、stale、degraded 或 overflow 时仍会抑制答案，直到有界扫描完成。`--freshness allow-stale` 可以返回带这些诊断的已索引路径或内容；当 `direct_source_read_required=true` 时，agent 在编辑或引用变化文件前必须直接读取返回路径。

`repo query` 的 `definition`、`references` 和 `hybrid` 查询先走已索引 tree-sitter 图和 SQLite FTS 读模型。`--freshness allow-stale` 在目标 ref 正在 full indexing 且尚未 finalize 时，会继续读取上一个已完成 committed scope，并在响应中标记 stale/degraded reason；`wait-until-fresh` 仍会要求目标 scope 新鲜。JSON 响应包含 `freshness.state`、`freshness.index_lag`、`freshness.pending`、`freshness.cursor`、`freshness.direct_source_read_required` 和 `freshness.agent_instructions`，让 agent 能看到 checkpoint 进度，并知道哪些返回路径在编辑或引用前必须直接读取源码。只有这些结构化层存在明确召回缺口时，查询才会在同一 indexed commit 上启动有界内部 exact-text source fallback；命中会在 JSON 中标记 `retrieval_layers=["lexical","text_fallback"]`，definition 兜底还会带 `definition`。候选路径查询、候选文件数、物化字节或单行长度预算耗尽只会降级兜底层，并通过 `degraded_reason` 暴露，不会让结构化代码图结果失效。

`repo context` 是面向 coding agent 的 one-call context pack，复用同一个已提交 read model。它先解析 authored 业务术语和 alias，把 resolved mapping id 或 unresolved `target_hint` 作为有界技术检索 seed，再展开 hybrid、definition、symbol、references、callers、callees 和 imports。JSON 额外暴露 `business_context`；业务与技术候选绑定同一 resolved commit/source scope，并共同受结果数、字节、截断和 provenance 预算约束。该命令不会在查询时读取 glossary YAML，也不会启动 repository indexing。

`repo query --query` 支持内联过滤标签，例如 `kind:function`、`lang:rust` 或 `language:rust`、`path:storage`、`name:query`。未知 `prefix:value` 会保留为普通检索文本。查询内 language filter 与显式 `--language` 取交集；`kind` 和 language 收窄 SQL 候选，`path` 和 `name` 在打分后、截断前过滤命中。`name:` 匹配符号 identity 和 SBOM 包 identity，不匹配任意 excerpt 文本。

`repo feature-flags` 读取索引阶段写入的配置驱动特性开关图事实，默认列出所选 repository scope 内的开关、配置来源和代码使用关系；`--query` 只做名称、配置 key、路径或 excerpt 过滤。JSON 响应包含与 `repo query` 相同的 `freshness` 对象，包括 pending task、checkpoint cursor、index lag、stale/degraded reason，以及返回 feature-flag usage 文件的 direct-source-read paths。抽取器识别环境变量、config/settings key、布尔配置声明，以及 OpenFeature、LaunchDarkly、Unleash 等常见 SDK evaluation 调用。它不会同步 provider 控制面的状态、策略、segment 或 rollout variant。该命令不会在查询时扫描全仓库源码；新增或修正开关抽取逻辑后，需要重新 `repo index` 或 `repo update` 才能看到新事实。

配置注册表支持 `--domain <domain>`（显式注释中的领域值）、`--source java|properties|ini|ctmpl|shell` 和 `--hot-reload true|false`。来源和领域值不区分大小写；未知领域或热加载元数据不匹配显式过滤条件。过滤条件选择配置分组，并保留其关联使用关系。`--query` 使用 Unicode 小写匹配。`--consistency` 增加读取未定义、缺少格式和默认值冲突诊断；`conflicting_default_sources` 给出每个默认值对应的路径、行号和 excerpt。关联绑定未解析或值流不受支持时，相应分组标记 `analysis_complete: false`，不推断缺失，但仍报告已加载默认值证明的冲突；数据过期或降级时不输出确定的一致性结论；无关分组仍可独立分析。使用关系、字节、符号、展开深度或 SQLite 查询超出预算时返回明确的分析不完整错误，需要收窄查询范围。Java 嵌套类型及带显式注释的常量字段参与绑定解析。配置行号范围不包含末尾换行符。 绑定结果和配置证据按引用及剩余深度缓存，避免接口使用关系重复扫描全部实现。Java getter 标记计入每文件 10,000 条事实预算，getter 收集也受限。局部变量守卫不匹配无关的同名方法或字段。Shell 定义要求主 shell 中无条件的赋值和导出；条件分支、延迟执行函数及子 shell 内的导出不能证明主 shell 配置已定义。

配置注解只接受相应格式的注释语法，执行语句和字符串不能提供元数据。Java 无接收者的零参数 getter 调用仅绑定可见的已声明方法，字符串按有界 Java 转义规则解码。Shell 条件赋值后仍保留可能的继承环境变量读取。绑定或流分析不完整时不推断缺失，但保留已加载默认值能够证明的冲突及其来源位置。

只指定元数据过滤条件时，带注解的符号 getter 使用关系会保留至绑定解析完成。Java 解析词法可见的嵌套类型，并在节点与闭包预算内遍历同文件父类型链。Shell 函数导出（`export -f`，含组合选项）不改变变量导出状态；ANSI-C 引号的默认值保持未知，不按普通引号误解码。Properties 支持 CR、LF、CRLF 自然行、续行和准确的来源范围。

Properties 注释不跨自然行续接。模板双引号参数按有界 Go 字节及 Unicode 转义规则解码，注释分隔符不受注释内引号影响。Shell 赋值保留已证明的先前导出属性。Java 通过 AST 保留既有 config/settings/feature_flags/flags/toggles/options 字面量 key 读取形式，支持静态导入的数字转换，并明确报告守卫扫描超限。直接标记过期或降级的作用域不输出确定的一致性结论，包括已加载默认值的冲突。

Java 父类型关系以内部配置层次事实持久化，跨文件解析仅使用当前提供的快照及注册授权范围。层次证据上限为 4 MiB，每个类型闭包最多 64 个类型，展开符号最多 1,000 个。条件重赋值保留可能的守卫，并标记值流不完整；直接布尔读取返回布尔类型证据。相邻 Java 块注释/Javadoc 注解限制在 8 KiB/32 行内扫描。Properties 空白仅指空格、制表符和换页符。Shell 波浪号展开的默认值保持未知，先前赋值扫描超限明确报错。SDK 管理的标志不要求仓库内定义。

注释查找遇到空行即停止，兼容 CR、LF 和 CRLF 边界。Java 内联 switch 选择表达式及 Shell 的 if/elif、循环、case 和短路条件表达式会生成关联到读取的守卫关系。Shell 条件覆盖保留此前确定存在的定义，但默认值未知并标记值流不完整；追加赋值保留定义，不把追加后缀当成完整默认值。Shell 条件扫描有界，预算耗尽时显式报错。

Java 字符串键中的数字加法保持未解析，不将数字错误拼接。被跟踪局部变量的复制会标记守卫值流不完整。Java 包装类型的透明转换携带同包遮蔽条件，在授权快照中核对；存在遮蔽时保留读取、移除 getter 绑定并标记值流不完整。静态或私有 getter 只绑定声明类型，跨文件继承解析也遵守此规则。

显式 super 调用通过直接父类解析。构造表达式保留精确运行时类型，包括外层类型转换，派生类重写不会污染对应 getter 的解析。Java SDK 调用共享邻接注释的 domain/hot-reload 元数据解析。支持的数字字面量在比较默认值前规范化分隔符、类型后缀和进制；不支持的算术仍保持未解析。INI 保留感叹号开头的键。模板读取函数仅在管道命令位置识别，包括嵌套命令、控制和声明管道。

Getter 继承在中间声明处停止，并保留精确派发；私有 getter 不被继承。Java 同包类型声明优先于通配符导入歧义。绑定、继承和一致性证据限定在注册授权范围内，请求的路径/语言筛选只用于返回的使用位置。支持带正负号的数字默认值，包括 Java 最小整数值。INI 和模板分隔符前的反斜杠按字面量处理，仅 Properties 转义分隔符。

静态 getter 调用解析可见的本地类型和显式导入类型接收者，最终查询匹配包含绑定与引用名称。Java key 必须是字符串表达式，数字和布尔字面量仍可作为默认值，未知返回类型的通用读取不再宣称返回字符串。简单 System/Boolean 调用会核对整个授权快照内的同包类型声明，即使输出路径被收窄也会检查；全限定 java.lang 调用和显式导入仍然有效。内部平台遮蔽声明不作为配置分组展示。

文本查询可从匹配的 getter 路径或 excerpt 开始解析，再应用分组元数据过滤。getter 单一返回值分析忽略注释；通配导入本身不能证明接收者归属；缺少索引类型证据时保持未解析，显式导入和可见本地类型优先。Java key 常量声明按实际读取归入属性或环境变量命名空间；同一常量用于两者时分别保留声明证据。Properties 默认值保留末尾空白。Properties、INI、模板和 Shell 每文件最多抽取 10,000 条事实，模板读取动作也计入预算。

查询词用于选择分组，一致性分析前加载所选分组及符号解析所得 key 在已选范围内的全部证据。`--domain`、`--source` 缺值时，即使后面紧跟另一选项也报错。Java `this.KEY` 关联声明字段。裸 key 和空白分隔赋值仅适用于 Properties；INI/模板定义必须包含 `=` 或 `:`。模板 `keyOrDefault` 的带引号回退值及类型计入默认值冲突检查。

Unicode 领域注释与过滤值使用相同的小写规范化。Properties 在 EOF 处理尚未结束的续行。隐式 lambda 参数阻止向外层字段回退，无关嵌套类型不遮蔽 Java 平台 API。Shell 默认值按解析后的引号和转义处理；词法扫描超限明确报告分析不完整。行加载在保留每条记录之前检查累计字节，包括字符串及元数据，避免先分配整个结果再检查 16 MiB 事实预算。操作 payload 的 domain/source 字段接受字符串或 null（视为未提供），拒绝其他值类型。

增强 for 变量在循环体内绑定 getter 接收者，未知推断类型不回退到外层字段。Java key 拼接可跟随同文件内受界的 final 字符串引用，并受深度、节点数量和值大小限制；可变或循环表达式保持未解析。模板控制动作和嵌套管道可抽取 key/env 调用，保留独立调用位置并执行每动作 token 预算。Properties 保留分号前缀及转义空白 key。Unicode 匹配在选择和排序阶段采用相同的小写规范化。

`repo framework` 读取索引阶段写入的独立 Angular/Vue component-template graph。重复传入 `--framework`、`--kind` 或 `--path` 可以取交集过滤；省略时在所选 scope 内受界枚举。Graph 包含 component、template、binding、slot、template variable 和 control flow 等类型化 node，以及 ownership、render、binding、event、read/write、directive 和 slot edge。Vue SFC 的 script symbol/import 仍可通过普通 `repo query` 查询。该命令不在查询期扫描源码，也不启动索引；`wait-until-fresh` 要求 durable indexed snapshot 已包含当前 framework fact。

`repo software` 读取所选 repository scope 的软件全域模型。旧 kind 继续返回兼容投影：`dependencies` 返回 manifest/lockfile package component 和 dependency usage，`sdks` 返回 unresolved/ambiguous/external target，`files`、`topics`、`relationships`、`build`、`iac`、`design` 返回各自旧切片。Dockerfile/Containerfile 现在属于 build definition，CI workflow job 属于 pipeline/build job；只有 Compose、Kubernetes、Helm、Terraform、systemd、launchd 等明确部署证据进入 IaC。普通 README heading 只生成 documentation topic；只有显式 frontmatter、受控 manifest/schema 或结构化代码证据才能晋升为 system/component/API/resource。

类型化 kind `systems`、`apis`、`resources`、`tests`、`deployments` 和 `releases` 返回带稳定 `entity_key` 与 snapshot `occurrence_id` 的 ontology entity。`statements` 返回 subject/predicate/object、source/evidence、assertion mode、resolution、有效期、extractor、confidence 和 fact state；`conflicts` 返回 conflicting、unresolved、superseded/rejected statement 及 shape diagnostics。每个响应 status 都包含 `ontology_version`、`projection_schema_version`、`source_coverage`、`completeness_basis_points`、`freshness` 和 `conflict_count`。

对 `repo software --kind all`，`--limit` 是所有数组合计行数的严格上限，不是每个数组各自拥有一份额度。系统按固定响应顺序 `components`、`dependency_usages`、`sdk_usages`、`files`、`topics`、`relationships`、`build_targets`、`iac_resources`、`design_elements`、`entities`、`statements`、`diagnostics` 在非空数组间逐轮分配；空数组或已耗尽数组的额度会进入后续轮次。上限小于非空数组数时，这个顺序就是确定性的优先级；默认 100 行上限可避免密集 dependency 切片遮蔽后续 lifecycle 与 ontology 切片。

`repo software export` 从同一 snapshot-bound application service 输出原始标准 JSON 文档：`spdx-3` 对应 SPDX 3.0.1 JSON-LD，`cyclonedx-1.7` 对应 CycloneDX 1.7 JSON，`prov-o` 对应 PROV-O JSON-LD。导出不会补造当前 ontology 不掌握的标准字段。查询和导出都不会执行构建工具、扫描包缓存、SDK 目录、云 API、未索引外部源码或查询时全仓文档；source scope 变化后需要重新 `repo index` 或 `repo update` 刷新投影。

`repo business` 读取索引时从 Knowledge Map `business-knowledge` route 授权的 `knowledge/glossary/business-glossary.yaml` 投影。`--kind terms` 返回 canonical term、definition、alias、semantics、冲突和 evidence；`mappings` 返回 `represented_by`/`calculated_from` 技术映射。跨 domain 同名且未给 `--domain` 时返回 `ambiguous`，不会猜测；授权 scope 外或尚未覆盖的目标保留 `resolution_state=unresolved` 和 `target_hint`，不会把仓库标成 degraded。业务定义只能通过版本化 glossary 和代码评审修改。

`repo view` 以 JSON 返回从代码图谱派生的代码库理解视图。`business-domains` 优先合并 glossary 声明的 domain（`evidence.kind=business_glossary`），再补充路径、路由和 feature flag 推断；其余视图从所选 repository scope 中已索引的文件、符号、import、call、route、dependency 和 feature flag 事实派生。`affected-scope` 在 deterministic v1 中需要一个或多个 `--changed-path`，返回变更文件、受影响模块、调用边和附近的测试/配置/文档候选。响应包含 `nodes`、`edges`、`sections`、`evidence`、freshness 诊断和截断预算元数据；section narrative 只是带 evidence id 的短派生说明，不会作为图谱事实持久化，也不是 AI 生成的事实真源。

面向 Agent 的 MCP kind 查询复用同一组 kind family，不引入并行名称。`relay_code_query` 覆盖代码图谱 kind，`relay_business_query` 覆盖 authored 业务术语与技术映射，`relay_software_query` 覆盖全部软件全域模型 kind，并可传 `export_profile=spdx-3|cyclonedx-1.7|prov-o` 返回标准导出 envelope；`relay_code_feature_flags` 覆盖配置驱动 feature flag，`relay_codebase_view` 覆盖 `repo view` kind family。常见 agent 别名会归一到现有 kind：`dependency` 归一为 `dependencies`，`configuration` 归一为 `relationships`，`model` 或 `models` 归一为 `design`。

`map` 命令维护 `codespec/codespec-map.yaml` 与 `knowledge/knowledge-map.yaml`。`map init`、`show`、`history`、`validate` 默认使用 `--type all`；定向 mutation 必须显式指定 `--type knowledge` 或 `--type codespec`，source 与 route 只适用于 Knowledge。Schema v4 保留强类型 `directories` 与 `knowledge/topics/` 内容寻址分片，但每张 map 只在根文件保留最近 16 条历史，不再创建 history 归档目录。`map history` 默认从最早保留版本开始、最多返回 16 条；显式请求 `omitted_through` 及以前的版本会报错，长期审计应使用 Git 或仓库备份。目录治理只能通过 `map directory add|update|remove` 更新，两张 map 各自的五个基线目录不可删除。`map migrate --type knowledge --to-v4` 先验证 legacy artifact，再发布可见与 fallback v4 根、写入 v4 legacy redirect，并以有界、可重试方式清理可识别的旧 history artifact；不再提供数据级 map rollback 命令。文件、digest、关系、历史、路径、保留 source 与 AGENTS 引用均以 `map validate` 为权威。

CLI skill 随附 `references/knowledge-map.schema.json`，它是覆盖 v4 根 manifest、topic shard、recent-history window 与 redirect 的 JSON Schema Draft 2020-12 文档。Editor 或 agent 可在把 YAML 解析成 JSON-compatible value 后，用它发现字段并执行结构检查。Schema 会有意允许未知字段，以保持与当前 Serde reader 一致。Schema 通过不代表 digest 与内容一致、跨 topic source-id 全局唯一、route 完整、recent history 与 omission checkpoint 连续或保留 source 合法；`relay-knowledge map validate` 仍是权威检查。Schema 也不授权 agent 直接编辑 CLI 生成的 root、shard 或 redirect。

Skill 还随附独立的 `references/business-glossary.schema.json`，作为 authored Business Glossary v1 文档的 Draft 2020-12 schema。它覆盖 domain、term、alias、声明式 semantics、技术 mapping、枚举和集合上限，并采用相同的未知字段兼容策略。JSON Schema 的 `maxLength` 只提供按字符计数的结构近似；4 MiB 文件上限、按 UTF-8 byte 计算的字段边界、identity/domain reference 规则和 alias 大小写不敏感唯一性仍以 `relay-knowledge map validate` 为权威。与生成的 Knowledge Map artifact 不同，`knowledge/glossary/business-glossary.yaml` 应在版本控制和正常代码评审下直接维护。

该契约只保存稳定导航和模型入口元数据，不复制文档、代码、配置、CI、运行态系统、外部知识源中的真实知识，也不复制与 snapshot 绑定的架构/构建/部署 projection row。一个 topic 可以包含多个 source，`map source add` 会把不同 source id 追加到该 topic 的 route 顺序中。所有 ref 必须是仓库受控相对路径；绝对路径、父目录穿越和符号链接逃逸会被拒绝。mutation 共用跨平台 OS advisory writer lock，先发布不可变 artifact，最后替换根 manifest；活跃 writer 保持独占，进程异常退出后 owner 自动释放，无需删除持久 `.lock` inode。首次 mutation 还会创建或扩展所选 `knowledge/` 或 `codespec/` 根中的 `.gitignore`；应把这个 nested contract 与 map 一起提交，使普通 Git repository 与 linked worktree 都能排除 canonical/prepared lock inode。LLM agent 必须通过 `map directory` 更新目录治理，通过 `map show` 和 `map route` 定位 Knowledge 知识源，通过 `map source add/update/remove` 维护这些 source，并在变更后运行 `map validate --format json`。AGENTS.md 保留 `CodeSpec map: codespec/codespec-map.yaml` 与 `Knowledge map: knowledge/knowledge-map.yaml` 两个稳定引用。

重复执行字段归一化后内容相同的 `map source update`，不会增加版本、历史或分片，
也不会重写根文件；必要的迁移或保留 route 修复仍执行。即使没有内容更新，
`map init` 也会续跑过期分片回收：保护恢复根引用，等待 60 秒退役宽限期，
每次最多处理 1,024 个未引用普通分片。清理尚未完成时再次运行 init 即可。

## 3.5 读写影响

状态、健康、帮助、setup doctor/profile、provider probe、version check、`repo list`、report、map show/history/route/validate/agent-snippet 和 audit query 是诊断入口，不应修改图谱事实。`health` 是 liveness 快路径，不会排队 index refresh，也不会等待 code-index writer 完成；存储繁忙时它可以返回 stale/degraded `storage_busy`。`version check` 只可能刷新 runtime cache 下的版本检查缓存。`ingest`、`map init`、`map directory add/update/remove`、`map migrate`、`map source add/update/remove`、`repo remove`、`repo index`、`repo update`、`index refresh`、`worker run-once`、proposal 状态变更和 service definition write 会写入运行时状态、派生索引、proposal/audit、仓库导航契约或 service definition。

自动化调用方应优先读取 `help --format json` 中的 operation 和 read/write 说明，再决定是否在 CI、agent 或 Web 操作面中开放命令。

## 3.6 Skill-over-CLI

仓库随附 `skills/relay-knowledge-cli`，这是一个兼容 ClawHub 的 skill，用于让
LLM agent 通过本地 CLI 调用 relay-knowledge，并解析 JSON 输出。它覆盖安装检查、
`version check`、setup/health 诊断、知识图谱 ingest/query，以及代码仓库注册、索引、查询、增量更新、影响分析和报告工作流。Repository bootstrap 会同时初始化/校验
knowledge map 与 code map；spec-grounded commit loop 会固定一个 ref，并组合 map route、
software/architecture model、impact 和 code context 证据。

同一 skill package 还包含用于结构化工具的 Knowledge Map v4 schema。Metadata gate
会在 release 打包前检查 Draft 标识、三类 artifact branch、关键复用定义、开放字段兼容策略，
以及有代表性的正例和反例。

该 skill 不配置 MCP、不调用 MCP 工具，也不管理 ACP session。协议级 agent 接入请使用
MCP/ACP 对应章节。

`repo software --kind dependencies` 返回 Maven 模块、POM 声明依赖、组件和源码使用记录；`--kind modules` 只返回 reactor 图。两者每页所有数组共享 `--limit`（上限 500），有后续数据时返回 `next_cursor`。保持 ref 和过滤条件不变，使用 `--cursor <token>` 继续读取，直到游标省略；合并各页得到完整结果。`repo impact` 返回默认 profile 的下游 POM 证据链。详见[软件全域模型](../03-architecture-specs/21-software-global-domain-modeling.md)。
