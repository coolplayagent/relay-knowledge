# 第 3 章 CLI 命令参考

[中文](../../zh/01-user-guide/03-cli-command-reference.md) | [English](../../en/01-user-guide/03-cli-command-reference.md)

本章提供可执行命令索引。工作流说明分散在后续章节；本章用于快速找到入口和诊断命令。

当请求 `--format json` 或 `--format streaming-json` 时，写入 stderr 的解析诊断和运行期 API 失败都会使用 JSON。运行期 API 失败沿用稳定 API 错误结构，包含 `error_kind`、`message` 和可选 `metadata`；text 和 markdown 格式继续输出便于人工阅读的 stderr 消息。

需要从本地 CLI 访问已部署常驻服务时，使用全局 `--remote <base-url>` 或 `RELAY_KNOWLEDGE_REMOTE_BASE_URL`。远端模式覆盖 `repo list`、`repo index`、`repo update`、`repo scope preview`、`repo status`、`repo query`、`repo graph`、`repo context`、`repo framework`、`repo feature-flags`、`repo impact`、`repo report`、`repo software` 和 `repo view`，用于访问服务端已经注册的仓库。`repo index --reset` 和 `repo index-worker` 在远端模式选中时会被拒绝，必须在服务端机器执行；仅设置环境变量时，`status`、`health` 等无关本地命令继续使用本机 runtime state。

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
relay-knowledge map migrate --type knowledge <--to-v3|--rollback>
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
relay-knowledge repo feature-flags <alias> [--query <text>] [--ref <ref>] [--path <filter>] [--language <id>] [--limit <n>]
relay-knowledge repo impact <alias> --base <ref> --head <ref>
relay-knowledge repo report <alias> [--format markdown|json]
relay-knowledge repo software <alias> [--ref <ref>] [--kind dependencies|sdks|files|topics|relationships|build|iac|design|all] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
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
  `relationships`、`build`、`iac`、`design`、`all`。
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

`repo framework` 读取索引阶段写入的独立 Angular/Vue component-template graph。重复传入 `--framework`、`--kind` 或 `--path` 可以取交集过滤；省略时在所选 scope 内受界枚举。Graph 包含 component、template、binding、slot、template variable 和 control flow 等类型化 node，以及 ownership、render、binding、event、read/write、directive 和 slot edge。Vue SFC 的 script symbol/import 仍可通过普通 `repo query` 查询。该命令不在查询期扫描源码，也不启动索引；`wait-until-fresh` 要求 durable indexed snapshot 已包含当前 framework fact。

`repo software` 读取所选 repository scope 的软件全域模型投影。`--kind dependencies` 返回由 manifest 和 lockfile 生成的包组件，以及把 declared package 与代码/配置 import 证据关联的 `dependency_usages`。同一仓库级 package/version 坐标在多个 lockfile 中重复时，派生视图只返回一个带确定性代表证据的 locked component；declared component 与原始 `repo query --kind sbom` 证据仍按证据位置独立保留。`--kind sdks` 返回 unresolved external import/include 目标，作为 SDK 或 API surface 使用候选；`--kind files` 返回代码、配置、文档、构建、部署、测试和模板文件整体节点；`--kind topics` 返回从 Markdown/spec heading 和 `knowledge/knowledge-map.yaml` 抽取的主题；`--kind relationships` 返回 `documents`、`depends_on`、`uses_sdk` 和 `configures` 等跨域关系。`--kind build` 返回从 Cargo、npm、Python、Go、Maven effective `pom.xml`、Gradle、CMake、Makefile 和 CI workflow 证据中提取的 package、script、target、feature、module、profile、plugin、goal、job 等构建入口。`--kind iac` 返回 Dockerfile、Compose、Kubernetes YAML、Helm chart、Terraform、systemd、launchd 和 CI workflow 中提取的部署/基础设施资源。`--kind design` 返回 README、架构/设计 Markdown 和 package/module manifest 中有证据支撑的软件系统、模块、组件、接口和能力元素。该命令不会执行构建工具、扫描包缓存、SDK 目录、云 API、未索引外部源码或查询时全仓文档；source scope 变化后需要重新 `repo index` 或 `repo update` 刷新投影。

`repo business` 读取索引时从 Knowledge Map `business-knowledge` route 授权的 `knowledge/glossary/business-glossary.yaml` 投影。`--kind terms` 返回 canonical term、definition、alias、semantics、冲突和 evidence；`mappings` 返回 `represented_by`/`calculated_from` 技术映射。跨 domain 同名且未给 `--domain` 时返回 `ambiguous`，不会猜测；授权 scope 外或尚未覆盖的目标保留 `resolution_state=unresolved` 和 `target_hint`，不会把仓库标成 degraded。业务定义只能通过版本化 glossary 和代码评审修改。

`repo view` 以 JSON 返回从代码图谱派生的代码库理解视图。`business-domains` 优先合并 glossary 声明的 domain（`evidence.kind=business_glossary`），再补充路径、路由和 feature flag 推断；其余视图从所选 repository scope 中已索引的文件、符号、import、call、route、dependency 和 feature flag 事实派生。`affected-scope` 在 deterministic v1 中需要一个或多个 `--changed-path`，返回变更文件、受影响模块、调用边和附近的测试/配置/文档候选。响应包含 `nodes`、`edges`、`sections`、`evidence`、freshness 诊断和截断预算元数据；section narrative 只是带 evidence id 的短派生说明，不会作为图谱事实持久化，也不是 AI 生成的事实真源。

面向 Agent 的 MCP kind 查询复用同一组 kind family，不引入并行名称。`relay_code_query` 覆盖代码图谱 kind，`relay_business_query` 覆盖 authored 业务术语与技术映射，`relay_software_query` 覆盖软件全域模型 kind，`relay_code_feature_flags` 覆盖配置驱动 feature flag，`relay_codebase_view` 覆盖 `repo view` kind family。常见 agent 别名会归一到现有 kind：`dependency` 归一为 `dependencies`，`configuration` 归一为 `relationships`，`model` 或 `models` 归一为 `design`。

`map` 命令维护 `codespec/codespec-map.yaml` 与 `knowledge/knowledge-map.yaml`。`map init`、`show`、`history`、`validate` 默认使用 `--type all`；定向 mutation 必须显式指定 `--type knowledge` 或 `--type codespec`，source 与 route 只适用于 Knowledge。Schema v3 增加强类型 `directories`，同时保留 `knowledge/topics/` 内容寻址分片、有界 recent history、`knowledge/history/` 校验归档和有界深度 index。目录治理只能通过 `map directory add|update|remove` 更新，两张 map 各自的五个基线目录不可删除。`map migrate --type knowledge --to-v3` 保留 v1/v2 内容、最后发布可见根文件并在旧路径写入 v3 redirect；`--rollback` 恢复保留的 v2 根文件。文件、digest、关系、历史、路径、保留 source 与 AGENTS 引用均以 `map validate` 为权威。

CLI skill 随附 `references/knowledge-map.schema.json`，它是覆盖 v2 根 manifest、topic shard、history archive 和 history index node 的 JSON Schema Draft 2020-12 文档。Editor 或 agent 可在把 YAML 解析成 JSON-compatible value 后，用它发现字段并执行结构检查。Schema 会有意允许未知字段，以保持与当前 Serde reader 一致。Schema 通过不代表 digest 与内容一致、跨 topic source-id 全局唯一、route 完整、history 连续、index range/height 关系正确或保留 source 合法；`relay-knowledge map validate` 仍是权威检查。Schema 也不授权 agent 直接编辑 CLI 生成的 shard、archive 或 index node。

Skill 还随附独立的 `references/business-glossary.schema.json`，作为 authored Business Glossary v1 文档的 Draft 2020-12 schema。它覆盖 domain、term、alias、声明式 semantics、技术 mapping、枚举和集合上限，并采用相同的未知字段兼容策略。JSON Schema 的 `maxLength` 只提供按字符计数的结构近似；4 MiB 文件上限、按 UTF-8 byte 计算的字段边界、identity/domain reference 规则和 alias 大小写不敏感唯一性仍以 `relay-knowledge map validate` 为权威。与生成的 Knowledge Map artifact 不同，`knowledge/glossary/business-glossary.yaml` 应在版本控制和正常代码评审下直接维护。

该契约只保存稳定导航和模型入口元数据，不复制文档、代码、配置、CI、运行态系统、外部知识源中的真实知识，也不复制与 snapshot 绑定的架构/构建/部署 projection row。一个 topic 可以包含多个 source，`map source add` 会把不同 source id 追加到该 topic 的 route 顺序中。所有 ref 必须是仓库受控相对路径；绝对路径、父目录穿越和符号链接逃逸会被拒绝。mutation 共用跨平台 OS advisory writer lock，先发布不可变 artifact，最后替换根 manifest；活跃 writer 保持独占，进程异常退出后 owner 自动释放，无需删除持久 `.lock` inode。首次 mutation 还会创建或扩展所选 `knowledge/` 或 `codespec/` 根中的 `.gitignore`；应把这个 nested contract 与 map 一起提交，使普通 Git repository 与 linked worktree 都能排除 canonical/prepared lock inode。LLM agent 必须通过 `map directory` 更新目录治理，通过 `map show` 和 `map route` 定位 Knowledge 知识源，通过 `map source add/update/remove` 维护这些 source，并在变更后运行 `map validate --format json`。AGENTS.md 保留 `CodeSpec map: codespec/codespec-map.yaml` 与 `Knowledge map: knowledge/knowledge-map.yaml` 两个稳定引用。

## 3.5 读写影响

状态、健康、帮助、setup doctor/profile、provider probe、version check、`repo list`、report、map show/history/route/validate/agent-snippet 和 audit query 是诊断入口，不应修改图谱事实。`health` 是 liveness 快路径，不会排队 index refresh，也不会等待 code-index writer 完成；存储繁忙时它可以返回 stale/degraded `storage_busy`。`version check` 只可能刷新 runtime cache 下的版本检查缓存。`ingest`、`map init`、`map directory add/update/remove`、`map migrate`、`map source add/update/remove`、`repo remove`、`repo index`、`repo update`、`index refresh`、`worker run-once`、proposal 状态变更和 service definition write 会写入运行时状态、派生索引、proposal/audit、仓库导航契约或 service definition。

自动化调用方应优先读取 `help --format json` 中的 operation 和 read/write 说明，再决定是否在 CI、agent 或 Web 操作面中开放命令。

## 3.6 Skill-over-CLI

仓库随附 `skills/relay-knowledge-cli`，这是一个兼容 ClawHub 的 skill，用于让
LLM agent 通过本地 CLI 调用 relay-knowledge，并解析 JSON 输出。它覆盖安装检查、
`version check`、setup/health 诊断、知识图谱 ingest/query，以及代码仓库注册、索引、查询、增量更新、影响分析和报告工作流。Repository bootstrap 会同时初始化/校验
knowledge map 与 code map；spec-grounded commit loop 会固定一个 ref，并组合 map route、
software/architecture model、impact 和 code context 证据。

同一 skill package 还包含用于结构化工具的 Knowledge Map v2 schema。Metadata gate
会在 release 打包前检查 Draft 标识、四类 artifact branch、关键复用定义、开放字段兼容策略，
以及有代表性的正例和反例。

该 skill 不配置 MCP、不调用 MCP 工具，也不管理 ACP session。协议级 agent 接入请使用
MCP/ACP 对应章节。
