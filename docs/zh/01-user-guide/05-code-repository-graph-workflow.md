# 第 5 章 代码仓库图谱工作流

[中文](../../zh/01-user-guide/05-code-repository-graph-workflow.md) | [English](../../en/01-user-guide/05-code-repository-graph-workflow.md)

代码仓库图谱把 Git tree、文件、符号、引用、调用和 import 关系纳入同一检索面。它不是简单的文件搜索；查询和影响分析都依赖已索引的代码图谱快照。精确文本 `grep` 只作为已索引快照上的有界兜底层，用来补齐 AST/FTS 明确漏召的源码行。

## 5.1 注册仓库

把 Git 仓库注册为代码检索源:

```bash
relay-knowledge repo register /path/to/repo \
  --alias core \
  --path src \
  --language rust \
  --format json
```

`--alias` 是后续命令使用的短名。`--path` 和 `--language` 可以重复；注册时定义的范围会限制索引、查询和影响分析，后续请求只能收窄范围，不能扩大范围。

注册只记录仓库根路径、alias 和允许 scope，不立即解析文件。路径必须指向本机可读 Git worktree；索引时再解析目标 ref 或 worktree overlay。再次注册同一个 Git root 时会为同一个 repository id 增加 alias，不会让旧 alias 失效；如果 alias 已经属于另一个 repository id，注册会失败。

## 5.2 Scope 预览

索引前预览当前 scope 会覆盖哪些文件:

```bash
relay-knowledge repo scope preview core --ref HEAD --format json
```

`repo index --dry-run` 使用同一个 preview path:

```bash
relay-knowledge repo index core --ref HEAD --dry-run --format json
```

preview 适合在收窄 `--path` 或 `--language` 后确认不会把无关目录写入代码图谱。默认 source preset 会排除 dependency/cache/vendor/build/out/target 目录、二进制/媒体资产、`*.jsonl` 数据集转储和 `uv.lock` 这类锁文件快照；`dist` 下被 Git 跟踪的源码语言 runtime 子树（例如 `dist/js/core` 或 `dist/js/app`）会进入索引，minified 文件、CSS/assets 和其他 distribution 子树仍默认排除。确实需要检索其他默认排除文件时，用精确 `--path` 注册或请求对应文件即可显式纳入。

## 5.3 建立代码图谱索引

索引当前 `HEAD`:

```bash
relay-knowledge repo index core --ref HEAD --format json
```

索引不可变提交更适合复现实验:

```bash
relay-knowledge repo index core --ref <commit-sha> --format json
```

全量索引通过 Git 从 clean tree 读取普通 blob，tree-sitter 解析 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift 和 Bash。Gitlink submodule 会在父仓 snapshot 中跳过；需要覆盖其内容时，应把 submodule 作为独立仓库注册。Unsupported、invalid UTF-8、binary、oversized 或 parser 失败文件会降级为 text-only 或 failed diagnostics，不会让整个批次失败。

当请求的 full scope 尚未 fresh 时，`repo index` 会排入持久化后台任务，并返回包含 `task.state=queued` 和目标 scope metadata 的 JSON，而不是把整个 cold parse 绑在前台请求上。CLI 会为该任务启动有界单次 `repo index-worker`；`relay-knowledge service run` 也会用同一队列上的单个仓库索引 worker 消费任务。同一仓库已有 queued/running task 时，重复索引请求会复用当前任务，不会并行启动多个 full rebuild。

已经 fresh 的 full index 仍会立即返回完成态 `summary`。增量 `repo update` 保持同步执行，因为它绑定显式 base-to-head diff，工作量受 changed path 集合约束。

## 5.4 符号与关系查询

混合查询:

```bash
relay-knowledge repo query core \
  --query retry_policy \
  --kind hybrid \
  --ref HEAD \
  --path src \
  --language rust \
  --freshness wait-until-fresh \
  --limit 10 \
  --format json
```

按窄类型查询:

```bash
relay-knowledge repo query core --query RetryPolicy --kind symbol --format json
relay-knowledge repo query core --query retry_policy --kind definition --format json
relay-knowledge repo query core --query retry_policy --kind references --format json
relay-knowledge repo query core --query retry_policy --kind callers --format json
relay-knowledge repo query core --query retry_policy --kind callees --format json
relay-knowledge repo query core --query crate::retry_policy --kind imports --format json
```

结果包含 repository id、alias、`scope_id`、requested ref、resolved commit、tree hash、path、language、byte range、line range、symbol/file id、retrieval layer、index version、freshness、score 和 excerpt。

branch、tag 和 `HEAD` 会先解析到 commit/tree；同一 tree hash 的多个 branch 复用同一 scope，但响应仍保留本次请求的 ref 作为审计信息。rebase 或 force-move 后的新 head 必须先重新索引，否则查询会失败而不是返回旧 branch 内容。

符号命中同时返回 `canonical_symbol_id`，用于跨快照表达逻辑符号身份。引用、调用和 import 命中会返回 `edge_kind`、`edge_resolution_state`、`edge_target_hint`、`edge_confidence_basis_points` 和 `edge_confidence_tier`。当目标无法唯一解析时，结果会标记为 `unresolved` 或 `ambiguous`，不会把猜测写成确定调用。如果 import 指向没有作为代码图谱 target 建索引的 unresolved 外部依赖，`repo query --kind imports` 和 repository-set import 查询可以用受限的内部 grep fallback 在当前已索引仓库源码中搜索。grep 搜索词来自 unresolved target hint，并排在结构化 import-graph 证据之后，因此小 `limit` 下 agent 仍能看到 `edge_resolution_state` 和 `edge_target_hint`。此类 fallback 命中会携带 `text_fallback`，响应诊断会说明外部依赖 import 未被索引，因此 agent 应把结果当成本仓源码文本证据，而不是依赖库图谱证据。

`definition`、`references` 和 `hybrid` 查询采用 AST/FTS 优先、`ripgrep` 兜底的顺序。兜底只在当前结构化结果没有覆盖具体身份、引用或 hybrid 结果窗口仍有空位时触发；它搜索已索引 commit 中经过 path/language/scope 过滤的候选文件，而不是直接扫当前脏工作树。兜底命中的 `retrieval_layers` 至少包含 `lexical` 和 `text_fallback`，definition 兜底还可以包含 `definition`；这些命中没有 resolved edge confidence，因为它们只是源码文本证据。

如果 `rg` 不存在、超时、候选文件数或物化字节预算耗尽，查询仍返回已有代码图结果，并在 `degraded_reason` 中说明 `ripgrep unavailable`、`ripgrep timeout` 或相应预算原因。缩小 `--path`、`--language` 或先确认目标 ref 已 fresh，通常比扩大 `--limit` 更有效。

### 特性开关图查询

存量仓库经常把特性开关分散在环境变量、配置 key、设置对象和条件分支里。`repo feature-flags` 使用索引阶段抽取出的结构化事实列出配置驱动开关及其代码关系:

```bash
relay-knowledge repo feature-flags core --ref HEAD --format json
relay-knowledge repo feature-flags core --query checkout --path src --limit 20 --format json
```

响应按 feature flag 分组，包含配置来源、`defines_config`、`reads_config` 或 `guards_code` 关系、source range、置信度、相关符号和 excerpt。该查询只读取当前 indexed scope 下的 feature-flag 表和 FTS 文档，不在查询时递归 grep 全仓库；新增开关或抽取规则变化后需要重新 `repo index` 或 `repo update`。

### 多仓库 Repository Set 查询

多仓库查询使用显式 `repo-set` 覆盖层。先把每个成员仓库索引成真实单仓 snapshot，再创建集合并把成员指向这些 snapshot:

```bash
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace core --ref HEAD --priority 10 --format json
relay-knowledge repo-set add workspace sdk --ref HEAD --priority 0 --format json
relay-knowledge repo-set refresh workspace --format json
relay-knowledge repo-set remove workspace sdk --format json
```

`repo-set add` 要求目标 ref 和 path/language filter 已经有匹配的单仓索引 scope；如果不存在，会失败而不是回退到旧 scope。同一 repository 再次加入同一个 set 时会替换原成员 snapshot，并废弃上一版 overlay edges。`repo-set remove` 会删除成员指针、废弃 overlay，并让普通 code-scope retention 在没有其它引用时回收该 snapshot。`repo-set refresh` 只重建跨仓 import/module overlay edges，不复制 `code_repository_files`、`code_repository_symbols` 或 `code_repository_chunks` 基础事实。CLI、Web 或 MCP 排入的异步 repository-set refresh 会由常驻 `service run` 中的 repository-set overlay refresh worker 消费。

查询集合时会 fan-out 到成员的真实 `source_scope`，然后合并排序:

```bash
relay-knowledge repo-set query workspace \
  --query retry_policy \
  --kind definition \
  --freshness allow-stale \
  --limit 20 \
  --format json
```

每条结果都包含 member repository alias、repository id、resolved commit、tree hash 和原始 `source_scope`。查询里的 `--path` 和 `--language` 只会收窄成员保存的 scope，不会扩大 scope，也不会切到仓库最新注册默认值。同名路径或同名符号不会跨仓去重；去重键包含 repository、scope、path、line range 和 excerpt。`--freshness wait-until-fresh` 会要求所有成员 snapshot fresh、`HEAD` 等移动 ref 仍解析到成员保存的 commit，且 overlay 不落后；否则返回明确错误。MCP 使用独立的 `relay_code_repository_set_query` 工具，每次调用都会重新校验当前成员，会在审计条目中记录 set alias，并要求 set alias 或每个成员 scope 已被策略允许。

## 5.5 增量更新

索引两个 ref 之间的变化:

```bash
relay-knowledge repo update core --base main --head HEAD --format json
```

`repo update` 会把 `base` 到 `head` 的 diff 应用到已持久化的 `base` snapshot。`base` 不需要是当前 active snapshot；只要同一 repository id、path filter 和 language filter 下曾经索引过该 base commit，增量更新就会从对应 persisted scope 克隆并只解析变化文件。

如果 CLI 报告找不到 matching indexed base scope，先索引目标 base:

```bash
relay-knowledge repo index core --ref main --format json
relay-knowledge repo update core --base main --head HEAD --format json
```

增量路径读取 `git diff --name-status --find-renames -z`，只重建新增、修改、复制、重命名或类型变化的文件。删除和重命名源路径会从 cloned base index 移除，rename lineage 会保留为 tombstone。

## 5.6 Worktree Overlay

需要索引未提交工作区时使用 `--ref worktree`:

```bash
relay-knowledge repo index core --ref worktree --format json
relay-knowledge repo query core --query retry_policy --ref worktree --format json
```

overlay 绑定当前 checked-out `HEAD`，使用合成 snapshot 标识，包含已修改和未跟踪文件。overlay 活跃时，clean commit ref 查询会被拒绝，避免把未提交内容误标成 clean Git snapshot。

## 5.7 影响分析

分析 diff 影响:

```bash
relay-knowledge repo impact core \
  --base main \
  --head HEAD \
  --limit 100 \
  --format json
```

影响分析会验证 `head_ref` 对应已索引 snapshot，按注册 scope 过滤 changed paths，再用模块、符号、caller、import 和已删除符号名推导受影响位置。

## 5.8 报告与状态

生成可读报告:

```bash
relay-knowledge repo report core --format markdown
```

脚本使用 JSON:

```bash
relay-knowledge repo report core --format json
relay-knowledge repo status core --format json
```

报告包含 repository id、root、indexed commit、tree hash、文件/符号/reference/chunk 总量、scope、代表性查询、延迟样本和 degradation summary。Markdown 报告适合贴进 PR 或发布说明；JSON 报告适合 CI 比较索引质量。

`repo status --format json` 还会包含 cold index 的 `active_task`、active 或最新 scope 的 `checkpoint` 计数，以及 `retention` 摘要。后台 full index 成功后，retention 会保留 active scope、最近两个完成 scope 和未完成任务 scope；更旧的 scope 会被淘汰，避免大型仓库持续累积无界 SQLite 行。

`repo report --format markdown` 还会汇总 edge resolution: resolved、ambiguous 和 unresolved 数量，用于判断当前代码图谱是否主要来自确定 AST 提取，还是存在大量需要人工或后续解析器改进的模糊边。

## 5.9 排障顺序

`repo query` 结果为空时，按顺序确认:

1. `repo status <alias>` 是否显示已索引的 clean commit 或 worktree overlay。
2. 查询时的 `--ref` 是否与已索引 snapshot 一致。
3. 请求的 `--path` 和 `--language` 是否只是在注册 scope 内进一步收窄。
4. `--kind` 是否过窄；不确定时先用 `--kind hybrid`。
5. `degraded_reason` 是否报告 `ripgrep unavailable`、`ripgrep timeout` 或 grep fallback 预算；exact-text 兜底降级时，结构化命中仍然可用。
6. 文件是否被诊断为 unsupported、binary、oversized、invalid UTF-8 或 parser failed。

`repo impact` 需要 `--head` 对应已索引 snapshot。先运行 `repo index core --ref <head>` 或 `repo update core --base <base> --head <head>`，再运行 impact。
