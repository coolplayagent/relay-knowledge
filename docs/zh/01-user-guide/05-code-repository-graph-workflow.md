# 第 5 章 代码仓库图谱工作流

[中文](../../zh/01-user-guide/05-code-repository-graph-workflow.md) | [English](../../en/01-user-guide/05-code-repository-graph-workflow.md)

代码仓库图谱把 Git tree 或 filesystem synthetic snapshot、文件、符号、引用、调用、import 关系和依赖清单纳入同一检索面。它不是简单的文件搜索；查询和影响分析都依赖已索引的代码图谱快照。精确文本 `grep` 只作为已索引快照上的有界兜底层，用来补齐 AST/FTS 明确漏召的源码行。

## 5.1 注册仓库

把 Git 仓库或非 Git source directory 注册为代码检索源:

```bash
relay-knowledge repo register /path/to/repo \
  --path src \
  --format json
```

省略 `--alias` 时，后续命令使用的短名默认是解析后的 Git root 或 filesystem root 目录名。对于 `/path/to/repo`，后续命令默认使用 `repo`，除非注册时显式传入 `--alias` 覆盖。`--path` 可以重复；注册会拒绝 `--language`，确保混合语言仓库保留完整语言面；后续 `repo query --language` 可以收窄结果，但不会缩小已索引快照。

注册只记录仓库根路径、alias 和允许 scope，不立即解析文件。路径可以指向本机可读 Git worktree 或普通 source directory；索引时再解析目标 ref、worktree overlay 或 filesystem synthetic snapshot。再次注册同一个 root 时会为同一个 repository id 增加 alias，不会让旧 alias 失效；如果 alias 已经属于另一个 repository id，注册会失败。

如果需要从头重建某个仓库的运行时状态，可以删除已注册仓库:

```bash
relay-knowledge repo remove repo --format json
```

删除会移除该 repository id 的注册记录、全部 alias、已索引 scope、code-index task、repository-set 成员和 overlay，以及软件全域投影行。它不会删除磁盘上的源码仓库。如果该仓库仍有 code-index task 正在运行，删除会被拒绝；删除成功后，同一路径或 alias 可以重新注册。

## 5.2 Scope 预览

索引前预览当前 scope 会覆盖哪些文件:

```bash
relay-knowledge repo scope preview repo --ref HEAD --format json
```

`repo index --dry-run` 使用同一个 preview path:

```bash
relay-knowledge repo index repo --ref HEAD --dry-run --format json
```

preview 适合在收窄注册期 `--path` 后确认不会把无关目录写入代码图谱。clean Git index 以 tracked tree 为权威：只要目录在注册和请求的 path scope 内，Git 跟踪的 `.cloudbuild/`、`.cid/`、`.build_config/`、`build/`、`dist/`、`vendor/` 和 `third_party/` 都可以进入索引。非 Git source directory 默认按白名单扫描根层支持文件和 `src/`、`include/`、`lib/`、`Sources/`、`packages/`、`modules/`、`plugins/`、`extensions/`、`docs/`、`config/` 等 source-like roots；`build/`、`dist/`、`target/`、`node_modules/`、`vendor/`、`third_party/`、cache、virtualenv 和 coverage 目录只有显式 `--path` opt in 才会进入。这个 opt in 是路径特异的：`--path src` 不会扫描兄弟级 `node_modules/` 或 `target/`，只有 `--path build` 或 `build/` 下的路径才允许该宽泛目录进入，`--path .` 则允许整个 root。默认非 Git scan 会跳过不会贡献白名单内容的目录；带过滤条件的非 Git scan 会在读取前跳过无关兄弟目录。若目录含 Git metadata 但 Git 因 unsafe ownership 或 metadata 损坏无法解析，注册会失败，而不是回退为非 Git 索引。默认 `--path src` 注册仍只会扩展到已发现 source root，例如 `external_deps/`、`packages/`、`modules/`、`plugins/`、`extensions/`、`Sources/`、`lib/` 和嵌套 JVM source root；精确请求 path filter 仍只收窄查询。`filesystem:` snapshot id 绑定到 discovery 后实际进入索引的文件，因此未索引文件变化不会让 scoped ref 失效，后台 worker 重放排队 synthetic ref 前也会重新校验，full-index batch 和 incremental delta 接受 live bytes 前会校验计划文件 hash，moving-ref resolution 使用与 indexed scope 相同的 path 和 language filters。显式已存储 `filesystem:` ref 在本地编辑后仍可查询；只有 source fallback 读取要求 live tree 仍匹配。保留的默认 preset 是文件级保护，用于二进制/媒体资产和 `*.jsonl` 数据集转储。`uv.lock` 这类锁文件快照可以贡献 SBOM 依赖事实，但不会展开成源码 chunk 或配置符号。Git worktree overlay 使用 Git status，因此被 `.gitignore` 忽略的 untracked 文件不会进入索引，除非 Git 自身报告它们；未跟踪的宽泛依赖、缓存或构建目录不会递归展开，除非显式 path filter opt in；脏 submodule 工作区不会被读取，需先提交 submodule 并更新父仓 gitlink。

对 `--ref worktree`，已提交的 submodule 更新会在父仓 gitlink 已 staged 时进入 overlay；如果父仓 gitlink 尚未 staged，但 submodule worktree 的 `HEAD` 已移动，也会进入 overlay。当两种状态同时存在时，overlay 会采用已检出的 submodule worktree `HEAD`，让 worktree snapshot 与磁盘上的文件一致。deinit 后只要 `.git/modules` 中仍有 staged submodule commit 对象也会读取；submodule 内未提交的脏内容仍会被忽略。

## 5.3 建立代码图谱索引

索引当前 `HEAD`:

```bash
relay-knowledge repo index repo --ref HEAD --format json
```

索引不可变提交更适合复现实验:

```bash
relay-knowledge repo index repo --ref <commit-sha> --format json
```

全量索引通过 Git 从 clean tree 读取普通 blob，或从非 Git source directory 读取 filesystem synthetic snapshot；随后先做受预算约束的 source-layout discovery，再由 tree-sitter 解析 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift、Bash、SQL，以及常见项目配置、构建和模板文件。SQL 文件会贡献 table、view/materialized view、function/procedure、trigger、type 等 schema object 符号，以及 SQL 对象引用和函数/过程调用边。配置面覆盖 Markdown、XML、Bazel/Starlark、Make、CMake、Dockerfile/Containerfile、Java properties、TOML、INI、YAML、JSON、Go module、Ninja、Jinja2 和 Go template；层级配置会写入 `server.port`、`containers[].name`、`bin[].name` 这类稳定路径。同一 source scope 内的本地文件、模板和构建目标引用会在 finalize 阶段解析；外部或有歧义的引用保留为 unresolved metadata。位于请求 path scope 内的 Gitlink submodule 在提交 blob 可从已检出 worktree 或缓存的 `.git/modules` gitdir 读取时，会按 `vendor/module/src/lib.rs` 这类父仓路径展开进父仓 snapshot，并支持自定义 submodule name 和嵌套 submodule。未初始化或不可访问的 submodule 会跳过，直到执行 `git submodule update --init --recursive` 或可用缓存 gitdir 让其提交 blob 可读。增量更新会为索引和影响分析展开受预算约束的 submodule gitlink 变化；可读的 submodule commit bump 会使用子模块内部 diff，避免把未变化 child file 重新解析或作为 impact seed；嵌套 gitlink bump 会展开为嵌套 child file，而不是把 gitlink path 当作待解析文件；删除 gitlink 会展开 base submodule tree 以移除陈旧 child path。增量索引、worktree overlay 和影响分析都会先应用 path scope，再展开 gitlink 和执行展开预算检查，因此 out-of-scope 的 submodule bump 会保留为普通 changed path，不会触发大型 submodule 扫描；若 gitlink 更新在请求 scope 内超过增量文件预算，应运行 full index，让工作进入 checkpointed batch。需要独立仓库身份时，submodule 仍可单独注册。Unsupported、invalid UTF-8、binary、oversized 或 parser 失败文件会降级为 text-only 或 failed diagnostics，不会让整个批次失败。

当请求的 full scope 尚未 fresh 时，`repo index` 会排入持久化后台任务，并返回包含 `task.state=queued` 和目标 scope metadata 的 JSON，而不是把整个 cold parse 绑在前台请求上。CLI 会为该任务启动有界单次 `repo index-worker`；非交互式 agent 需要消费 queued 或 retrying 任务时，也可以显式调用 `repo index-worker --task-id <id> --format json`，不用维持一个前台 `service run` 进程。`relay-knowledge service run` 作为 resident master，会在启动时恢复过期 code-index lease，在 stderr 打印启动状态行，并用同一队列上的有界 code-index worker pool 消费任务，默认并发度为 2，可通过 `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` 调整，最高按文档上限 clamp 到 8。不同 fingerprint 的任务独立排队、独立 lease、独立 checkpoint；完全相同的 full-index fingerprint 会复用当前任务，避免重复 full rebuild。`relay-knowledge service status --format json` 会在 `code_index_workers` 中报告 configured workers、active worker slots、queue depth、queued/running/retrying/dead-letter task counts、running leases 和 last error。跨 batch finalization 期间，`checkpoint.state` 会报告 `finalizing:resolve_references`、`finalizing:rebuild_reference_search`、`finalizing:rebuild_calls` 和 `finalizing:publish_scope` 等具体阶段；只有 checkpoint 到达 `completed` 后，查询才会把该 ref 当作 fresh。

远端服务模式下，先在服务端机器注册仓库并启动 `service run --web`，本地 CLI 再用 `--remote http://host:8791` 或 `RELAY_KNOWLEDGE_REMOTE_BASE_URL` 访问远端索引和查询 API。远端 `repo index` 只提交 durable task 并返回 task/status/checkpoint，不在本地 CLI 进程执行 `repo index-worker`；任务由远端 resident master 的 code-index worker pool 消费。远端模式支持 `repo index`、`repo scope preview`、`repo status`、`repo query`、`repo feature-flags`、`repo impact`、`repo report` 和 `repo software`，不支持把本机路径注册到远端服务。`repo index --reset` 和 `repo index-worker` 必须在服务端机器执行；远端选中的 CLI 会拒绝这些维护命令，而不是回落到本机状态。

```bash
RELAY_KNOWLEDGE_REMOTE_BASE_URL=http://127.0.0.1:8791 \
  relay-knowledge repo index repo --ref HEAD --format json
relay-knowledge --remote http://127.0.0.1:8791 repo query repo --query retry_policy --kind definition --freshness wait-until-fresh --format json
relay-knowledge --remote http://127.0.0.1:8791 repo software repo --kind relationships --ref HEAD --format json
```

面向 agent 的初始化应让每条命令都能有限返回:

```bash
relay-knowledge repo register /path/to/repo --format json
relay-knowledge repo index repo --ref HEAD --format json
relay-knowledge repo status repo --format json
relay-knowledge repo index-worker --task-id <task-id-from-repo-index> --format json
relay-knowledge repo status repo --format json
```

如果 `repo index` 已经完成单次 worker，后续 `repo index-worker` 会返回 `claimed=false` 和 `task=null` 的 JSON；此时以 `repo status` 的 checkpoint 进度和 freshness 为准。

如果旧 service 进程在持有 task lease 时退出，且任务仍然卡住，可以执行 `relay-knowledge repo index repo --reset --format json`，把该仓库未完成 task 重新排队。Reset 不会删除已完成 indexed scope，也不会复活历史 dead-letter task；旧 worker 仍必须匹配当前 lease owner 和 attempt token，因此不能完成已经 reset 的任务。

已经 fresh 的 full index 仍会立即返回完成态 `summary`。freshness 检查会比较嵌入 `scope_id` 的代码事实版本，因此 SBOM 依赖事实或 Web 路由事实这类抽取面变化即使 Git tree hash 不变，也会要求重建。对于包含 submodule 的 Git scope，freshness key 还会记录 scope 内 gitlink 是从可用 submodule 对象展开，还是因不可用而跳过；因此先前被跳过的 submodule 在后续初始化后会让旧 scope 失效。带 path filter 的 Git freshness probe 只检查与请求 scope 相交的 gitlink；无 scope 时才回退到 whole-tree submodule 状态。增量 `repo update` 保持同步执行，因为它绑定显式 base-to-head diff，工作量受 changed path 集合约束；新增文件落在 `external_deps/`、`modules/` 等非 `src/` source root 时会沿用同一 source-layout 策略进入增量索引。

## 5.4 符号与关系查询

混合查询:

```bash
relay-knowledge repo query repo \
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
relay-knowledge repo query repo --query RetryPolicy --kind symbol --format json
relay-knowledge repo query repo --query retry_policy --kind definition --format json
relay-knowledge repo query repo --query retry_policy --kind references --format json
relay-knowledge repo query repo --query retry_policy --kind callers --format json
relay-knowledge repo query repo --query retry_policy --kind callees --format json
relay-knowledge repo query repo --query crate::retry_policy --kind imports --format json
relay-knowledge repo query repo --query serde --kind sbom --format json
```

结果包含 repository id、alias、`scope_id`、requested ref、resolved commit、tree hash、path、language、byte range、line range、symbol/file id、retrieval layer、index version、freshness、score 和 excerpt。

JSON 响应还包含顶层 `freshness` 对象，用于图谱新鲜度治理。它会报告 `state`（`fresh`、`pending`、`stale` 或 `degraded`）、graph version、实际服务的 source scope、请求 ref 与服务 ref 的 lag、checkpoint cursor 计数、待处理 code-index task 和队列状态、stale/degraded reason，以及是否必须直接读取源码。当 `--freshness allow-stale` 在较新的 ref 已排队或运行索引时返回上一版 completed index，`metadata.stale`、`scope.stale` 和 `freshness.direct_source_read_required` 都会为 true；agent 在编辑或引用变化文件前，必须按 `freshness.direct_source_read_paths` 直接读取源码。`--freshness wait-until-fresh` 会抑制 stale 代码图谱答案，在请求 scope 完成索引前返回错误。

branch、tag 和 `HEAD` 会先解析到 commit/tree；同一 tree hash 的多个 branch 复用同一 scope，但响应仍保留本次请求的 ref 作为审计信息。rebase 或 force-move 后的新 head 必须先重新索引，否则查询会失败而不是返回旧 branch 内容。

Workspace import resolution 是显式启用的索引期能力。API 调用方可以在 `CodeIndexRequest.workspace_detection.enabled` 中启用 pnpm、Go 或 Cargo workspace 格式检测，使 snapshot apply 或 checkpoint finalize 阶段记录 package mapping，并为 unresolved sibling-package imports 派生 `cross_repo_import` edges。用于代码仓库索引的 Web operation payload 同样接受 `workspace_detection` 对象。CLI 索引默认保持关闭，除非调用方显式启用，否则单仓库索引路径保持原行为。

符号命中同时返回 `canonical_symbol_id`，用于跨快照表达逻辑符号身份。引用、调用、import 和 SBOM 命中会返回 `edge_kind`、`edge_resolution_state`、`edge_target_hint`、`edge_confidence_basis_points` 和 `edge_confidence_tier`。当目标无法唯一解析时，结果会标记为 `unresolved` 或 `ambiguous`，不会把猜测写成确定调用。`repo query --kind sbom` 返回索引期从 `Cargo.toml`、`Cargo.lock`、`package.json`、`package-lock.json`、`go.mod`、`go.sum`、`pyproject.toml`、`uv.lock`、`requirements*.txt`、`requirements/` 目录下的依赖文本、`constraints.txt`、Maven effective `pom.xml` dependency 和 BOM import、Gradle dependency block、CMake `CMakeLists.txt`、Conan `conanfile.txt` 或常见 `conanfile.py` 声明，以及 GitHub Actions workflow、GitLab CI、Docker Compose、Helm `Chart.yaml`、Ansible `requirements.yml` 等 allowlist IaC YAML 中提取的依赖清单；YAML、JSON、TOML、INI 和 Java properties 文件也会作为 code language 建索引，用于通过 `--language yaml|json|toml|ini|properties` 检索嵌套配置 key、section 和证据行，但 `package-lock.json` 和 `uv.lock` 这类仅用于依赖建模的锁文件只贡献 SBOM 事实，不会把每个锁定 key 展开成配置符号或源码 chunk；共享的 npm、JVM、CMake、Conan 和 IaC manifest 会保留 TypeScript/JSX、Kotlin/Scala、C/C++、YAML 查询可用的兼容语言 scope；它会处理常见 Python PEP 508 marker、editable Python direct reference、uv dependency groups、Cargo rename 语法、CMake package 声明、Gradle map-style 写法，以及 Maven 仓库内 parent POM/property/dependencyManagement 解析，会去重 `go.sum` 中同一模块版本的普通行与 `/go.mod` 行，跳过本地 Cargo path/workspace 包、本地 npm `file:`/`link:`/`workspace:` spec、本地 npm package-lock v1/v2 workspace 行、本地 Python/Poetry/uv path 依赖、本地 CMake subdirectory 和本地 workflow action，并且把 Maven imported BOM 当作 SBOM 记录；它不会执行包管理器、CI workflow、Maven、CMake、Helm、Docker 或 Kubernetes 工具，不解析传递依赖、访问 registry，也不提供漏洞或许可证分析。如果 import 指向没有作为代码图谱 target 建索引的 unresolved 外部依赖，`repo query --kind imports` 和 repository-set import 查询可以用受限的内部 source fallback 在当前已索引仓库源码中搜索。fallback 搜索词来自 unresolved target hint，并排在结构化 import-graph 证据之后，因此小 `limit` 下 agent 仍能看到 `edge_resolution_state` 和 `edge_target_hint`。此类 fallback 命中会携带 `text_fallback`，因此 agent 应把结果当成本仓源码文本证据，而不是依赖库图谱证据。外部依赖源码缺失保持 unresolved edge coverage metadata，除非兜底自身失败，否则不设置 `degraded_reason`。

`definition`、`references` 和 `hybrid` 查询采用 AST/FTS 优先、内部 exact-text source fallback 兜底的顺序。兜底只在当前结构化结果没有覆盖具体身份、引用或 hybrid 结果窗口仍有空位时触发；它搜索已索引 commit 中经过 path/language/scope 过滤并物化的候选文件，而不是直接扫当前脏工作树。对非 Git `filesystem:` commit，兜底会先确认当前 live tree 仍解析到同一个 synthetic snapshot；如果已经变化，则报告降级而不是读取另一个快照的 live 文件。兜底命中的 `retrieval_layers` 至少包含 `lexical` 和 `text_fallback`，definition 兜底还可以包含 `definition`；这些命中没有 resolved edge confidence，因为它们只是源码文本证据。

如果候选路径查询不可用、候选文件数、物化字节或单行长度预算耗尽，查询仍返回已有代码图结果，并在 `degraded_reason` 中说明 source fallback 预算或候选路径原因。缩小 `--path`、`--language` 或先确认目标 ref 已 fresh，通常比扩大 `--limit` 更有效。

### 特性开关图查询

存量仓库经常把特性开关分散在环境变量、配置 key、设置对象、SDK client 和条件分支里。`repo feature-flags` 使用索引阶段抽取出的结构化事实列出配置驱动开关及其代码关系:

```bash
relay-knowledge repo feature-flags repo --ref HEAD --format json
relay-knowledge repo feature-flags repo --query checkout --path src --limit 20 --format json
```

响应按 feature flag 分组，包含配置来源、`defines_config`、`reads_config` 或 `guards_code` 关系、source range、置信度、相关符号和 excerpt。索引器识别环境访问、config/settings 读取、支持配置格式里的布尔 config fact，以及 OpenFeature、LaunchDarkly、Unleash 等常见 SDK evaluation 调用中的静态代码/配置证据；provider 控制面的 rollout strategy、segment 和 variant 不在该路径同步。该查询只读取当前 indexed scope 下的 feature-flag 表和 FTS 文档，不在查询时递归 grep 全仓库；新增开关或抽取规则变化后需要重新 `repo index` 或 `repo update`。

### 软件全域投影

`repo software` 暴露 repository scope 内的软件图投影，包括依赖、未解析 SDK/API 使用、文件整体节点、文档主题和跨域关系：

```bash
relay-knowledge repo software repo --kind files --ref HEAD --format json
relay-knowledge repo software repo --kind topics --ref HEAD --format json
relay-knowledge repo software repo --kind relationships --ref HEAD --format json
```

该投影会把 Markdown/spec heading 和 `.knowledge/knowledge-map.yaml` topic 与文档文件连接，把依赖 manifest 与 package component 连接，把 unresolved import 与 SDK/API usage 候选连接，并把配置/feature-flag facts 与代码或配置文件连接。它只读取所选 indexed scope 的已提交 projection 表，不在查询时扫描包缓存、SDK 目录、未索引外部源码或全仓文档。

### 多仓库 Repository Set 查询

多仓库查询使用显式 `repo-set` 覆盖层。先把每个成员仓库索引成真实单仓 snapshot，再创建集合并把成员指向这些 snapshot:

```bash
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace repo --ref HEAD --priority 10 --format json
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
relay-knowledge repo update repo --base main --head HEAD --format json
```

`repo update` 会把 `base` 到 `head` 的 diff 应用到已持久化的 `base` snapshot。`base` 不需要是当前 active snapshot；只要同一 repository id、path filter 和 language filter 下曾经索引过该 base commit，增量更新就会从对应 persisted scope 克隆并只解析变化文件。对于非 Git scope，delta 解析会拒绝不再匹配计划 filesystem content hash 的 live bytes。

如果 CLI 报告找不到 matching indexed base scope，先索引目标 base:

```bash
relay-knowledge repo index repo --ref main --format json
relay-knowledge repo update repo --base main --head HEAD --format json
```

增量路径读取 `git diff --name-status --find-renames -z`，只重建新增、修改、复制、重命名或类型变化的文件。删除和重命名源路径会从 cloned base index 移除，rename lineage 会保留为 tombstone。

## 5.6 Worktree Overlay

需要索引未提交工作区时使用 `--ref worktree`:

```bash
relay-knowledge repo index repo --ref worktree --format json
relay-knowledge repo query repo --query retry_policy --ref worktree --format json
```

overlay 绑定当前 checked-out `HEAD`，使用合成 snapshot 标识，包含已修改文件、未跟踪文件、staged submodule gitlink 更新，以及 submodule `HEAD` 不同于父仓 gitlink 时的 unstaged submodule worktree commit。如果 submodule 同时有 staged gitlink 和另一个已检出的 submodule `HEAD`，overlay 会索引已检出的 worktree commit。deinit 后只能从缓存 gitdir 读取的 staged submodule commit 也会纳入 overlay。staged submodule 的新增、删除、重命名以及 file/submodule 互换会按展开后的 child path 清理旧索引，而不是只处理 gitlink path。overlay 活跃时，clean commit ref 查询会被拒绝，避免把未提交内容误标成 clean Git snapshot。

## 5.7 影响分析

分析 diff 影响:

```bash
relay-knowledge repo impact repo \
  --base main \
  --head HEAD \
  --limit 100 \
  --format json
```

影响分析会验证 `head_ref` 对应已索引 snapshot，按注册 scope 过滤 changed paths，再用模块、符号、caller、import 和已删除符号名推导受影响位置。非 Git impact 请求使用同一个 indexed filesystem scope filters，因此显式索引的 `build/` 或 `vendor/` 路径不会被默认非 Git scan policy 丢掉。

## 5.8 报告与状态

生成可读报告:

```bash
relay-knowledge repo report repo --format markdown
```

脚本使用 JSON:

```bash
relay-knowledge repo report repo --format json
relay-knowledge repo status repo --format json
```

报告包含 repository id、root、indexed commit、tree hash、文件/符号/reference/chunk 总量、scope、代表性查询、延迟样本和 degradation summary。Markdown 报告适合贴进 PR 或发布说明；JSON 报告适合 CI 比较索引质量。

`repo status --format json` 还会包含 cold index 的 `active_task`、active 或最新 scope 的 `checkpoint` 计数，以及 `retention` 摘要。如果仓库仍处于 `indexing` 但没有 active task，status 会回退显示该仓库最近的 checkpoint，让运维能看到最后一个持久化阶段，而不是空进度。后台 full index 成功后，retention 会保留 active scope、最近两个完成 scope 和未完成任务 scope；更旧的 scope 会被淘汰，避免大型仓库持续累积无界 SQLite 行。

`repo report --format markdown` 还会汇总 edge resolution: resolved、ambiguous 和 unresolved 数量，用于判断当前代码图谱是否主要来自确定 AST 提取，还是存在大量需要人工或后续解析器改进的模糊边。

## 5.9 排障顺序

`repo query` 结果为空时，按顺序确认:

1. `repo status <alias>` 是否显示已索引的 clean commit 或 worktree overlay。
2. 查询时的 `--ref` 是否与已索引 snapshot 一致。
3. 请求的 `--path` 和 `--language` 是否只是在注册 scope 内进一步收窄。
4. `--kind` 是否过窄；不确定时先用 `--kind hybrid`。
5. `degraded_reason` 是否报告 source fallback 候选路径或预算问题；exact-text 兜底降级时，结构化命中仍然可用。
6. 文件是否被诊断为 unsupported、binary、oversized、invalid UTF-8 或 parser failed。

`repo impact` 需要 `--head` 对应已索引 snapshot。先运行 `repo index repo --ref <head>` 或 `repo update repo --base <base> --head <head>`，再运行 impact。
