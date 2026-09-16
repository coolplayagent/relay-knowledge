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

`expected_degraded_file_count` 使用与完整索引相同的解析器和有界批次，对预览已解析到的快照进行验证。统计包含语法错误、无效 UTF-8、二进制内容、不支持的 grammar 和超大文件，每个有诊断的文件仅计一次；仅缺失外部依赖源码不算降级。预览会读取并解析选中源码，因此比仅列举元数据更耗时，但不写入索引事实、任务或 checkpoint。应用最多允许两个预览 worker 并发，准入等待最多五秒，响应期限为 120 秒。取消或超时后，当前 blocking 批次完成即停止后续批次；worker 退出前仍持有并发许可。不完整的计数不会作为成功预览返回；超时时应缩小注册范围后重试。应与相同 resolved ref 和 scope 的完整索引比较；增量摘要和 worktree overlay 可能覆盖不同文件集合。

preview 适合在收窄注册期 `--path` 后确认不会把无关目录写入代码图谱。clean Git index 以 tracked tree 为权威：只要目录在注册和请求的 path scope 内，Git 跟踪的 `.cloudbuild/`、`.cid/`、`.build_config/`、`build/`、`dist/`、`vendor/` 和 `third_party/` 都可以进入索引。非 Git source directory 默认按白名单扫描根层支持文件和 `src/`、`include/`、`lib/`、`Sources/`、`packages/`、`modules/`、`plugins/`、`extensions/`、`docs/`、`config/` 等 source-like roots；`build/`、`dist/`、`target/`、`node_modules/`、`vendor/`、`third_party/`、cache、virtualenv 和 coverage 目录只有显式 `--path` opt in 才会进入。这个 opt in 是路径特异的：`--path src` 不会扫描兄弟级 `node_modules/` 或 `target/`，只有 `--path build` 或 `build/` 下的路径才允许该宽泛目录进入，`--path .` 则允许整个 root。默认非 Git scan 会跳过不会贡献白名单内容的目录；带过滤条件的非 Git scan 会在读取前跳过无关兄弟目录。若目录含 Git metadata 但 Git 因 unsafe ownership 或 metadata 损坏无法解析，注册会失败，而不是回退为非 Git 索引。默认 `--path src` 注册仍只会扩展到已发现 source root，例如 `external_deps/`、`packages/`、`modules/`、`plugins/`、`extensions/`、`Sources/`、`lib/` 和嵌套 JVM source root；精确请求 path filter 仍只收窄查询。`filesystem:` snapshot id 绑定到 discovery 后实际进入索引的文件，因此未索引文件变化不会让 scoped ref 失效，后台 worker 重放排队 synthetic ref 前也会重新校验，full-index batch 和 incremental delta 接受 live bytes 前会校验计划文件 hash，moving-ref resolution 使用与 indexed scope 相同的 path 和 language filters。显式已存储 `filesystem:` ref 在本地编辑后仍可查询；只有 source fallback 读取要求 live tree 仍匹配。保留的默认 preset 是文件级保护，用于二进制/媒体资产和 `*.jsonl` 数据集转储。`uv.lock` 这类锁文件快照可以贡献 SBOM 依赖事实，但不会展开成源码 chunk 或配置符号。Git worktree overlay 使用 Git status，因此被 `.gitignore` 忽略的 untracked 文件不会进入索引，除非 Git 自身报告它们；未跟踪的宽泛依赖、缓存或构建目录不会递归展开，除非显式 path filter opt in；脏 submodule 工作区不会被读取，需先提交 submodule 并更新父仓 gitlink。

`--path` 是注册期或查询期 scope，不是索引期参数。用
`repo register <path> --path <filter>` 选择源码范围，然后运行不带
`--path` 的 `repo index <alias> --ref HEAD`。非 Git 目录的常规移动文件
系统 selector 是 `HEAD`，会解析为 `filesystem:<hash>` 快照；`worktree`
只用于 Git worktree overlay。

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

当请求的 full scope 尚未 fresh 时，`repo index` 会排入持久化后台任务，并返回包含 `task.state=queued` 和目标 scope metadata 的 JSON，而不是把整个 cold parse 绑在前台请求上。显式提供 `--reuse-historical` 时，Git full 初始化会在构造冷索引计划前，从目标 commit 沿第一父链按近到远检查最多 10 个祖先；最近的非 stale、非 retiring、filter 兼容且 fact version 当前的已发布 scope 会成为增量 base。初始化历史复用的 base→head Git diff 上限为 100 changed path，超限或没有可用 base 时继续走 checkpointed full index；未提供该选项时保持 full index。由 full 请求转换出的增量任务仍使用既有 lease、publication fence、原子 snapshot 和 retention。CLI 会为该任务启动有界单次 `repo index-worker`；非交互式 agent 需要消费 queued 或 retrying 任务时，也可以显式调用 `repo index-worker --task-id <id> --format json`，不用维持一个前台 `service run` 进程。`relay-knowledge service run` 作为 resident master，会在启动时恢复过期 code-index lease，在 stderr 打印启动状态行，并用同一队列上的有界 code-index worker pool 消费任务，默认并发度为 2，可通过 `RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT` 调整，最高按文档上限 clamp 到 8。不同 fingerprint 的任务独立排队、独立 lease、独立 checkpoint；指向同一 target 与请求 scope 的 unfinished full 或 incremental task 会被复用，避免跨模式重复构建。`relay-knowledge service status --format json` 会在 `code_index_workers` 中报告 configured workers、active worker slots、queue depth、queued/running/retrying/dead-letter task counts、running leases 和 last error。跨 batch finalization 期间，`checkpoint.state` 会报告 `finalizing:resolve_references`、`finalizing:rebuild_reference_search`、`finalizing:rebuild_calls` 和 `finalizing:publish_scope` 等具体阶段；只有 checkpoint 到达 `completed` 后，查询才会把该 ref 当作 fresh。

CLI-shaped Web 索引请求接受可选布尔字段 `reuse_historical`；省略或传 `null` 时保持默认 full-index 行为。100-path 复用预算会分别统计 rename/copy 的 old/new path；历史基线若在 task admission 前被 retention 标为 retiring，则安全回退 full task。

远端服务模式下，先在服务端机器注册仓库并启动 `service run --web`，本地 CLI 再用 `--remote http://host:8791` 或 `RELAY_KNOWLEDGE_REMOTE_BASE_URL` 访问远端索引和查询 API。远端 `repo index` 和 `repo update` 只提交 durable task 并返回 task/status/checkpoint，不在本地 CLI 进程执行 `repo index-worker`；任务由远端 resident master 的 code-index worker pool 消费。远端模式支持 `repo list`、`repo index`、`repo update`、`repo scope preview`、`repo status`、`repo query`、`repo context`、`repo framework`、`repo feature-flags`、`repo impact`、`repo report`、`repo software`（包括标准导出）和 `repo view`，不支持把本机路径注册到远端服务。`repo index --reset` 和 `repo index-worker` 必须在服务端机器执行；远端选中的 CLI 会拒绝这些维护命令，而不是回落到本机状态。

```bash
RELAY_KNOWLEDGE_REMOTE_BASE_URL=http://127.0.0.1:8791 \
  relay-knowledge repo index repo --ref HEAD --format json
relay-knowledge --remote http://127.0.0.1:8791 repo list --format json
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

如果 `repo index` 已经完成单次 worker，后续 `repo index-worker` 会返回 `claimed=false` 和 `task=null`，但仍会推进一次有界 retention pass。`maintenance_active=true` 或 `repo status` 仍显示 `maintenance_pending=true` 时应重复调用；可选 `maintenance_error` 非空时应报告并处理它，不能把 `maintenance_active=false` 当作完成。checkpoint 进度、GC 错误和 freshness 仍以 status 为准。

如果旧 service 进程在持有 task lease 时退出，且任务仍然卡住，可以执行 `relay-knowledge repo index repo --reset --format json`，把该仓库未完成 task 重新排队。Reset 不会删除已完成 indexed scope，也不会复活历史 dead-letter task；旧 worker 仍必须匹配当前 lease owner 和 attempt token，因此不能完成已经 reset 的任务。

已经 fresh 的 full index 仍会立即返回完成态 `summary`。freshness 检查会比较嵌入 `scope_id` 的代码事实版本，因此 SBOM 依赖事实或 Web 路由事实这类抽取面变化即使 Git tree hash 不变，也会要求重建。对于包含 submodule 的 Git scope，freshness key 还会记录 scope 内 gitlink 是从可用 submodule 对象展开，还是因不可用而跳过；因此先前被跳过的 submodule 在后续初始化后会让旧 scope 失效。带 path filter 的 Git freshness probe 只检查与请求 scope 相交的 gitlink；无 scope 时才回退到 whole-tree submodule 状态。增量 `repo update` 现在与 full index 共用 durable task、lease、retry 和 publication 路径；只有 full rebuild 暴露 batch checkpoint，受界 incremental snapshot 则原子发布。本地 CLI 执行一次有界 drain，远端或 watcher 触发的调用则可能留在队列中由常驻 worker 消费；新增文件落在 `external_deps/`、`modules/` 等非 `src/` source root 时会沿用同一 source-layout 策略进入增量索引。

## 5.4 符号与关系查询

短类型名调用查询使用已索引的结构化类和直接成员归属：`--query B --kind callers` 汇总指向 B 自身、构造函数和直接成员方法的调用边，`--kind callees` 汇总这些成员发出的调用边。结果保留具体调用方/被调用方法及调用位置。例如 `A.main` 调用 `B.process()` 后，查 B 的 callers 返回 `main calls process`；查 A 的 callers 不会因为 A 出现在调用文本中而误返回这条出边。已匹配的类没有对应方向的边时返回空，不通过全文搜索扩大结果。

该聚合入口适用于下方能力矩阵中具有类型归属结构的语言，限短类型名和 `callers`/`callees`，名称区分大小写。持久化归属区分语言与模块身份，嵌套类型和成员内局部函数不归入外层类型。继承方法和动态分派不作猜测。未解析的出向调用保留原状态。路径过滤约束调用位置，因此调用者可以位于类型定义文件之外；限定名称查询保持原有行为。

类查询最多准入 64 个候选类、1,024 条类/成员记录，沿用最多 200 条调用候选和请求结果上限；类解析及调用读取共享约 410 万条 SQLite 指令预算。类/成员或执行预算耗尽明确报告 `class call query incomplete` 容量错误，不静默截断身份集合；可改用成员方法名缩小展开范围。本次读取行为复用现有索引事实，无 schema、事实版本或安装配置变更，已完成的旧索引可直接查询。

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

Agent 也可以把结构化过滤标签直接写进 `--query`，例如
`--query "kind:function,method lang:rust path:storage name:query search_code"`。
已识别标签包括 `kind:`、`lang:` 或 `language:`、`path:` 和 `name:`；未知
`prefix:value` 会保留为普通检索文本。查询内 language filter 与显式
`--language` 取交集，并下推到 SQL 候选选择；查询内 symbol kind filter
下推到 symbol SQL。查询内 `path:` 和 `name:` 在打分之后、截断之前做后过滤，
用于收窄返回路径、符号名或 SBOM 包 identity，不改变已索引 scope。

结果包含 repository id、alias、`scope_id`、requested ref、resolved commit、tree hash、path、language、byte range、line range、symbol/file id、retrieval layer、index version、freshness、score 和 excerpt。

JSON 响应还包含顶层 `freshness` 对象，用于图谱新鲜度治理。它会报告 `state`（`fresh`、`pending`、`stale` 或 `degraded`）、graph version、实际服务的 source scope、请求 ref 与服务 ref 的 lag、checkpoint cursor 计数、待处理 code-index task 和队列状态、stale/degraded reason，以及是否必须直接读取源码。当 `--freshness allow-stale` 在较新的 ref 已排队或运行索引时返回上一版 completed index，`metadata.stale`、`scope.stale` 和 `freshness.direct_source_read_required` 都会为 true；agent 在编辑或引用变化文件前，必须按 `freshness.direct_source_read_paths` 直接读取源码。`--freshness wait-until-fresh` 会抑制 stale 代码图谱答案，在请求 scope 完成索引前返回错误。

branch、tag 和 `HEAD` 会先解析到 commit/tree；同一 tree hash 的多个 branch 复用同一 scope，但响应仍保留本次请求的 ref 作为审计信息。rebase 或 force-move 后的新 head 必须先重新索引，否则查询会失败而不是返回旧 branch 内容。

Workspace import resolution 是显式启用的索引期能力。API 调用方可以在 `CodeIndexRequest.workspace_detection.enabled` 中启用 pnpm、Go 或 Cargo workspace 格式检测，使 snapshot apply 或 checkpoint finalize 阶段记录 package mapping，并为 unresolved sibling-package imports 派生 `cross_repo_import` edges。用于代码仓库索引的 Web operation payload 同样接受 `workspace_detection` 对象。CLI 索引默认保持关闭，除非调用方显式启用，否则单仓库索引路径保持原行为。

符号命中同时返回 `canonical_symbol_id`，用于跨快照表达逻辑符号身份。引用、调用、import 和 SBOM 命中会返回 `edge_kind`、`edge_resolution_state`、`edge_target_hint`、`edge_confidence_basis_points` 和 `edge_confidence_tier`。当目标无法唯一解析时，结果会标记为 `unresolved` 或 `ambiguous`，不会把猜测写成确定调用。`repo query --kind sbom` 返回索引期从 `Cargo.toml`、`Cargo.lock`、`package.json`、`package-lock.json`、`go.mod`、`go.sum`、`pyproject.toml`、`uv.lock`、`requirements*.txt`、`requirements/` 目录下的依赖文本、`constraints.txt`、Maven effective `pom.xml` dependency 和 BOM import、Gradle dependency block、CMake `CMakeLists.txt`、Conan `conanfile.txt` 或常见 `conanfile.py` 声明，以及 GitHub Actions workflow、GitLab CI、Docker Compose、Helm `Chart.yaml`、Ansible `requirements.yml` 等 allowlist IaC YAML 中提取的依赖清单；YAML、JSON、TOML、INI 和 Java properties 文件也会作为 code language 建索引，用于通过 `--language yaml|json|toml|ini|properties` 检索嵌套配置 key、section 和证据行，但 `package-lock.json` 和 `uv.lock` 这类仅用于依赖建模的锁文件只贡献 SBOM 事实，不会把每个锁定 key 展开成配置符号或源码 chunk；共享的 npm、JVM、CMake、Conan 和 IaC manifest 会保留 TypeScript/JSX、Kotlin/Scala、C/C++、YAML 查询可用的兼容语言 scope；它会处理常见 Python PEP 508 marker、editable Python direct reference、uv dependency groups、Cargo rename 语法、CMake package 声明、Gradle map-style 写法，以及 Maven 仓库内 parent POM/property/dependencyManagement 解析，会去重 `go.sum` 中同一模块版本的普通行与 `/go.mod` 行，跳过本地 Cargo path/workspace 包、本地 npm `file:`/`link:`/`workspace:` spec、本地 npm package-lock v1/v2 workspace 行、本地 Python/Poetry/uv path 依赖、本地 CMake subdirectory 和本地 workflow action，并且把 Maven imported BOM 当作 SBOM 记录；它不会执行包管理器、CI workflow、Maven、CMake、Helm、Docker 或 Kubernetes 工具，不解析传递依赖、访问 registry，也不提供漏洞或许可证分析。当 unresolved external import 的结构化 import-graph excerpt 已包含 source-like statement，且解析出的 specifier 与 edge target 一致时，该结果已是完整的本仓 source evidence；`repo query --kind imports` 和 repository-set import 查询不会为该 surface 冗余增加 `text_fallback` 命中。Relative import、dynamic-import intent、不完整 excerpt，以及完整/不完整混合结果集仍可在当前已索引仓库源码中执行受界内部 source fallback。该 fallback 使用 unresolved target hint，排序位于结构化 import-graph 证据之后；其命中携带 `text_fallback`，只表示本仓源码文本证据，不表示依赖库已经入图。外部依赖源码缺失保持 unresolved edge coverage metadata；只有必需的 fallback 自身失败时才设置 `degraded_reason`。

`definition`、`references` 和 `hybrid` 查询采用 AST/FTS 优先、内部 exact-text source fallback 兜底的顺序。兜底会在当前结构化结果没有覆盖具体身份或引用、hybrid 结果窗口仍有空位，或 fresh scope 报告 parser-degraded 文件时触发；最后一种情况防止健康文件已经产生的结构化引用命中遮蔽降级文件中缺失的引用事实。它搜索已索引 commit 中经过 path/language/scope 过滤并物化的候选文件，而不是直接扫当前脏工作树。对非 Git `filesystem:` commit，兜底会先确认当前 live tree 仍解析到同一个 synthetic snapshot；如果已经变化，则报告降级而不是读取另一个快照的 live 文件。兜底命中的 `retrieval_layers` 至少包含 `lexical` 和 `text_fallback`，definition 兜底还可以包含 `definition`；这些命中没有 resolved edge confidence，因为它们只是源码文本证据。

如果候选路径查询不可用、候选文件数、物化字节或单行长度预算耗尽，查询仍返回已有代码图结果，并在 `degraded_reason` 中说明 source fallback 预算或候选路径原因。缩小 `--path`、`--language` 或先确认目标 ref 已 fresh，通常比扩大 `--limit` 更有效。

### Angular/Vue Framework Graph 查询

`repo framework` 把 component/template 语义作为独立 graph 暴露，而不是混入普通 symbol 命中：

```bash
relay-knowledge repo framework repo --framework angular --kind component --path src/app --format json
relay-knowledge repo framework repo --framework vue --kind prop --query modelValue --limit 20 --format json
```

Angular 索引会读取 decorator 以及 inline/external HTML template。Vue SFC 索引会记录 prop、emit、model、slot、template variable 和 control flow，同时仍把 embedded script 交给普通 TypeScript/JavaScript 抽取。响应分开返回类型化 `nodes`/`edges`，携带 resolution state 与 target hint，并显式标记结果截断。查询只读取已提交的受界 framework table，不会即时解析 template 或读取 worktree。

### 特性开关图查询

存量仓库经常把特性开关分散在环境变量、配置 key、设置对象、SDK client 和条件分支里。`repo feature-flags` 使用索引阶段抽取出的结构化事实列出配置驱动开关及其代码关系:

```bash
relay-knowledge repo feature-flags repo --ref HEAD --format json
relay-knowledge repo feature-flags repo --query checkout --path src --limit 20 --format json
```

响应按 feature flag 分组，包含配置来源、`defines_config`、`reads_config` 或 `guards_code` 关系、source range、置信度、相关符号和 excerpt。索引器识别环境访问、config/settings 读取、支持配置格式里的布尔 config fact，以及 OpenFeature、LaunchDarkly、Unleash 等常见 SDK evaluation 调用中的静态代码/配置证据；provider 控制面的 rollout strategy、segment 和 variant 不在该路径同步。该查询只读取当前 indexed scope 下的 feature-flag 表和 FTS 文档，不在查询时递归 grep 全仓库；新增开关或抽取规则变化后需要重新 `repo index` 或 `repo update`。

### 软件全域本体与兼容投影

`repo software` 暴露同一 repository scope 内的兼容投影、类型化 ontology entity、provenance statement 和冲突诊断：

```bash
relay-knowledge repo software repo --kind files --ref HEAD --format json
relay-knowledge repo software repo --kind topics --ref HEAD --format json
relay-knowledge repo software repo --kind relationships --ref HEAD --format json
relay-knowledge repo software repo --kind systems --ref HEAD --format json
relay-knowledge repo software repo --kind statements --ref HEAD --format json
relay-knowledge repo software repo --kind conflicts --ref HEAD --format json
relay-knowledge repo software export repo --profile cyclonedx-1.7 --ref HEAD --format json
```

`entity_key` 跨 commit 稳定，`occurrence_id` 绑定 snapshot 和 evidence。普通 Markdown/spec heading 只成为 documentation unit/topic；显式 frontmatter、API trait/schema、test symbol、Dockerfile/build file、Compose/Kubernetes/Terraform 或 service definition 才投影到相应受控类型。Dockerfile 和 CI job 不再成为 IaC resource。Statement 保留 source kind、evidence、extractor version、assertion/resolution/fact state、时间和 confidence；无证据或违反 shape 的 statement 返回 `rejected` diagnostic，不会成为 accepted fact。所有切片和 SPDX 3.0.1、CycloneDX 1.7、PROV-O 导出只读取所选 indexed scope 的已提交表，不在查询时扫描包缓存、SDK 目录、未索引外部源码或全仓文档。

### 多仓库 Repository Set 查询

多仓库查询使用显式 `repo-set` 覆盖层。先把每个成员仓库索引成真实单仓 snapshot，再创建集合并把成员指向这些 snapshot:

```bash
relay-knowledge repo-set create workspace --format json
relay-knowledge repo-set add workspace repo --ref HEAD --priority 10 --format json
relay-knowledge repo-set add workspace sdk --ref HEAD --priority 0 --format json
relay-knowledge repo-set refresh workspace --format json
relay-knowledge repo-set remove workspace sdk --format json
```

`repo-set add` 要求目标 ref 和 path/language filter 已经有匹配的单仓索引 scope；如果不存在，会失败而不是回退到旧 scope。同一 repository 再次加入同一个 set 时会替换原成员 snapshot，并废弃上一版 overlay edges。`repo-set remove` 会删除成员指针、废弃 overlay，并让普通 code-scope retention 在没有其它引用时回收该 snapshot。`repo-set refresh` 只重建跨仓 import/module overlay edges，不复制 `code_repository_files`、`code_repository_symbols` 或 `code_repository_chunks` 基础事实。CLI/Web 的默认同步与 async refresh 都先进入同一个有界持久队列；本地默认同步请求只在其精确 task 可被定向 claim 时 drain，否则返回 queued，再由常驻 `service run` worker 消费。Overlay edge 与 member replacement 在同一个 attempt-scoped live-lease 事务内发布，takeover 会回滚旧 attempt。该 overlay 能力仍要求 `single_sqlite`；在跨 shard import/export 聚合实现前，`partitioned_sqlite` 会明确报告 unsupported。

手动 set 最多接纳 64 个 member，每次发布最多替换 64 个 member fact version。一次完整 refresh 在所有 member 间共享 manifest 上限：4,096 个 chunk、16 MiB path/content byte 和 32,768 个 derived item。Overlay refresh 按 `(source_scope, import_id)` 主键 cursor 以每页 512 条扫描不可变 import row，整个 set 最多扫描 262,144 条，再在内存中过滤 unresolved external candidate；resolved 和 local row 同样消耗扫描预算。它最多保留 131,072 个 file/symbol export target 与 8,192 条跨 member candidate edge。没有任何 member export candidate 的外部 import 继续作为 source scope 内的权威 unresolved metadata，不会重复物化为 set edge。Selector request 的 origin/target key 合计最多 512 个。每个 import 最多观测 11 个 export 并保留最多 10 个 candidate ID，所以歧义匹配的 `candidate_count` 是有界而非完整计数。扫描或集合的 cap-plus-one 溢出会返回可重试 `qos_rejected`，不会截断后发布伪 `fresh` overlay。Direct/selector read 最多读取 8,193 条 edge，并排除 origin 或 target scope 已 retiring 的 edge。Refresh/add/member remove 在遗留 overlay 超过 8,192 条 edge 时会在无界删除前拒绝。删除整个 repository 时，受影响 set 超过 64 个，或任一受影响 overlay 超限，也会原子拒绝。当前版未提供有界 legacy-overlay repair 命令，在升级提供 repair tool 前保持数据不变。这些上限只覆盖手动 repository set，不覆盖显式启用的 automatic-workspace cross-edge builder；分阶段 scope GC 会限制过期 workspace state 的删除，但不限制单次 automatic build，因此 workspace detection 仍默认关闭，该 build-path 上限属后续工作。

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

从最近发布的 clean snapshot 更新到 checked-out `HEAD`：

```bash
relay-knowledge repo update repo --format json
```

省略 `--base` 时选择最近一次成功发布的 clean Git commit；如果 active identity 是 worktree overlay，服务会解包其 clean base。省略 `--head` 时选择 `HEAD`。durable task 入队前会把两个 ref 与 target tree 解析并固定为不可变 commit，因此 ref 后续移动不会改变任务输入。完成态 response 包含 `summary.base_resolved_commit_sha`；排队态在 `task.mode` 暴露固定后的 base/head。用 `repo status --format json` 查看 task 与 checkpoint 状态。

需要指定提交对时使用：

```bash
relay-knowledge repo update repo --base <base-commit> --head <head-commit> --format json
```

`repo update` 会把 `base` 到 `head` 的 diff 应用到已持久化的 `base` snapshot。`base` 不需要是当前 active snapshot；只要同一 repository id、path filter 和 language filter 下曾经索引过该 base commit，增量更新就会从对应 persisted scope 克隆并只解析变化文件。对于非 Git scope，delta 解析会拒绝不再匹配计划 filesystem content hash 的 live bytes。Git changed-path set 在应用注册 path filter 前按整个 commit pair 计算，上限为 512；超过时必须 full index，不能把大 delta 变成无界任务。

如果 CLI 报告找不到 matching indexed base scope，先索引目标 base:

```bash
relay-knowledge repo index repo --ref main --format json
relay-knowledge repo update repo --base main --head HEAD --format json
```

增量路径读取 `git diff --name-status --find-renames -z`，只重建新增、修改、复制、重命名或类型变化的文件。删除和重命名源路径会从 cloned base index 移除，rename lineage 会保留为 tombstone。

成功发布后，retention 会保留 active scope 与最近两个成功发布时间窗口的并集（窗口通常已包含 active）、最近一次成功增量的 predecessor、active worktree overlay 的 clean base，再加每个未完成 task 的 target/base 与 repository-set member pin。一个旧 scope 会先原子标成 `retiring` 并退出查询，再由 durable GC job 分阶段删除代码图事实、FTS/search document、software projection、checkpoint、workspace state 和 scope metadata；每个 maintenance transaction 只推进一个 scope-GC phase，该 phase 在受影响的应用表之间合计最多删除 512 个物理行。同 tree commit 复用内容图，并使用每仓 256 条的 commit alias 窗口。完成态 task 历史按每仓库 128 条 succeeded 和 64 条 failed/dead-letter/cancelled 限制，同时保留每个 retained scope 的最新 success。`repo status` 报告 GC phase/progress/error；被淘汰的历史 ref 必须用 full `repo index` 重建。

运行时还会把用户管理 repository set 之外的已索引仓库数默认限制为 10；可通过 `RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES` 修改这个正数上限。成功发布时间最旧的合格仓库通过同一套持久化分阶段 GC 清理，同时保留仓库注册、alias、未完成 task 和清理调度后产生的新发布。Automatic-workspace set 不会让仓库豁免该上限。

## 5.6 Worktree Overlay

需要索引未提交 Git 工作区时使用 `--ref worktree`:

```bash
relay-knowledge repo index repo --ref worktree --format json
relay-knowledge repo query repo --query retry_policy --ref worktree --format json
```

overlay 绑定当前 checked-out `HEAD`，使用合成 snapshot 标识，包含已修改文件、未跟踪文件、staged submodule gitlink 更新，以及 submodule `HEAD` 不同于父仓 gitlink 时的 unstaged submodule worktree commit。如果 submodule 同时有 staged gitlink 和另一个已检出的 submodule `HEAD`，overlay 会索引已检出的 worktree commit。deinit 后只能从缓存 gitdir 读取的 staged submodule commit 也会纳入 overlay。staged submodule 的新增、删除、重命名以及 file/submodule 互换会按展开后的 child path 清理旧索引，而不是只处理 gitlink path。overlay 活跃时，clean commit ref 查询会被拒绝，避免把未提交内容误标成 clean Git snapshot。

Query admission 会把用户输入的 `worktree` selector 固定为当前 active、不可变的 `worktree:<base>:<overlay-hash>` identity。`repo context` 等多步操作的每个内部 graph query 都复用这个已解析 identity，不会再把 synthetic identity 当作 branch 或 commit 送回 Git。发布后再修改 live worktree 不会悄悄改变正在生成的 context pack；需要重新执行 `repo index ... --ref worktree` 发布新 overlay。

对于 CLI 默认关闭 workspace detection 的路径，索引只有在完整 clone/delete/insert surface 装得进 task 冻结的 writer budget 时才使用 direct overlay transaction。若超预算，同一个 durable task 会 staging overlay bytes、按受界 page clone immutable clean base，再把 dirty file 划分为确定性的受界 batch。每个 worker step 最多提交一个 dirty batch，因此过期 lease 可被重新 claim，且不会重放已提交 batch。最终 handoff 还会把 owner cleanup、tombstone、checkpoint control row 与多批 receipt 一并纳入预算。整个操作始终保留 worktree identity 与 scope；不得把它报告成 clean full index，也不得在 finalization 与 publication 完成前返回成功。API/Web 启用 auto-workspace detection 的 worktree request 在需要 durable staging 时目前会 fail closed；系统不会丢弃 workspace metadata，也不会把 overlay 转成 clean snapshot。

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
relay-knowledge repo list --format json
relay-knowledge repo report repo --format json
relay-knowledge repo status repo --format json
```

`repo list` 只列出至少有一个已完成 indexed scope 的仓库，并返回当前 alias、root、indexed commit、state、stale 与文件/符号/reference/chunk 计数。仅注册但尚未完成首次索引的仓库不会出现在清单中。

报告包含 repository id、root、indexed commit、tree hash、文件/符号/reference/chunk 总量、scope、代表性查询、延迟样本和 degradation summary。Markdown 报告适合贴进 PR 或发布说明；JSON 报告适合 CI 比较索引质量。

`repo status --format json` 还会包含 cold index 的 `active_task`、active 或最新 scope 的 `checkpoint` 计数，以及 `retention` 摘要。仓库仍处于 `indexing` 但没有 active task 时，status 回退显示最近 checkpoint。发布后保留 active 与 latest-two-success window 的并集（通常重叠），以及最近 incremental predecessor、active-worktree clean base、未完成 task scope 和 repository-set pin；更旧 scope 被淘汰。

`repo report --format markdown` 还会汇总 edge resolution: resolved、ambiguous 和 unresolved 数量，用于判断当前代码图谱是否主要来自确定 AST 提取，还是存在大量需要人工或后续解析器改进的模糊边。

## 5.9 排障顺序

版本新鲜度与内容完整性独立：查询可以是 `fresh`，同时
`content_integrity.state=partial`；`unknown` 不代表内容完整。不能仅因为兼容字段
`degraded_reason` 存在就反复重建新鲜索引；读模型或查询能力故障仍可使查询降级。
报告最多展示 20 条文件诊断，并提供截断标记和固定快照的诊断命令。应使用报告给出的
命令查看同一快照，再读取全部分页：

```bash
relay-knowledge repo diagnostics repo --ref <pinned-ref> --limit 50 --format json
relay-knowledge repo diagnostics repo --ref <pinned-ref> --limit 50 --cursor <next-cursor> --format json
```

将返回的 `next_cursor` 传给下一页，直到游标为 null，保持 ref 与路径过滤不变。
游标固定已服务的 scope，HEAD 移动不会改变续页快照；快照已被清理时明确报错。
单页最多 200 条，同一文件可能有多条诊断，完整性计数按文件去重。
应先修复相关源码或明确调整授权索引范围，再重建索引以改变内容完整性。

`repo query` 结果为空时，按顺序确认:

1. `repo status <alias>` 是否显示已索引的 clean commit 或 worktree overlay。
2. 查询时的 `--ref` 是否与已索引 snapshot 一致。
3. 请求的 `--path` 和 `--language` 是否只是在注册 scope 内进一步收窄。
4. `--kind` 是否过窄；不确定时先用 `--kind hybrid`。
5. `degraded_reason` 是否报告 source fallback 候选路径或预算问题；exact-text 兜底降级时，结构化命中仍然可用。
6. 文件是否被诊断为 unsupported、binary、oversized、invalid UTF-8 或 parser failed。

`repo impact` 需要 `--head` 对应已索引 snapshot。先运行 `repo index repo --ref <head>` 或 `repo update repo --base <base> --head <head>`，再运行 impact。

### 配置键、读取位置与受控代码

`repo feature-flags` 将 Java 系统属性、环境变量读取、常量键及零参数配置 getter，与 properties、INI、Consul-template（`.ctmpl`）和 Shell 导出的环境变量连接起来。配置符号只在当前返回的仓库快照中解析，不依赖 canonical callers/callees 查询，也不修改 Python/C++ 解析。生产环境开关实时值不在静态注册表范围内。

`defines_config` 表示文件定义，`declares_config_key` 表示 Java 常量键或模板输出键，`reads_config` 表示读取位置。`guards_code` 的 `metadata.read_usage_id` 将条件位置连接到提供值的读取位置。Java 局部绑定在重新赋值后停止传播；延迟执行的类、方法与 lambda 函数体不会覆盖外层绑定。字段、参数及局部 getter 接收者使用词法类型证据。匿名接收者与未知动态值不会被猜测为默认实现；指向不同配置键的符号关系保留未解析状态。

每条使用关系包含来源格式以及可选的默认值、值类型、所属领域、热加载能力，未知值保持缺省。相邻注释如 `# @config domain=business hot-reload=true` 提供显式领域信息。properties 续行及 Unicode 转义保持键值身份；INI 节内键使用 `section.key`。环境变量与系统属性属于不同命名空间。

```powershell
relay-knowledge repo feature-flags demo --query feature_x --domain business --source properties --hot-reload true --format json
relay-knowledge repo feature-flags demo --query feature_y --consistency --format json
```

来源筛选选择符合条件的配置组，并保留其关联 Java 使用关系。一致性分析比较已授权注册范围内、当前返回快照中已观察到的格式，报告 `read_without_definition`、`missing_from_format` 和 `conflicting_defaults`；它不判断生产配置。陈旧或未解析的分析不能证明某键不存在。返回数量限制与完整性分析预算分开处理。注册命令中的 `--path src` 只是范围示例，不要求仓库采用固定目录布局。

远程 CLI 与 Web 仓库端点使用相同的领域请求，其中 `filters` 对象包含 `domain`、`source`、`hot_reload`、`consistency`。MCP 在 `relay_code_feature_flags` 参数中直接暴露这四个字段。

Java getter 值流还支持已确认属于 java.lang 的 Boolean/Integer/Long/Double 解析及装箱转换。无法解析的 getter 返回值流通过 `metadata.flow_incomplete` 标记，不能宣称一致性分析完整。字符串常量只有被范围内的配置读取引用、带显式 `@config domain=...` / `hot-reload=...` 元数据，或遵循声明约定（所属类型名以 `Keys` 结尾、字段名以 `_KEY` 结尾）时才公开为配置声明。其余字符串仅作为内部符号候选，不进入配置查询及通用配置视图。

一致性检查从限定范围的已索引文件清单获取格式覆盖，包含空模板和只有注释的模板，并遵守注册时的仓库路径、语言限制。`conflicting_default_sources` 返回冲突默认值对应的使用记录，可直接通过 `metadata.default_value`、`path`、`line_range`、`excerpt`、`usage_id` 定位每个来源；原有简短 `conflicting_defaults` 诊断继续保留。

Java SDK 开关继续使用现有 SDK 提取器，与配置读取同时提取。静态平台导入不会被无关兄弟类、嵌套类或不适用的重载方法遮蔽。Shell 先赋值后明确导出的变量保留定义与默认值；properties 转义解码不再改变 INI/模板的反斜杠。扩展查询和一致性查询保留所在符号的信息。结果数量限制在符号键解析、分组和排序后应用：候选仍受 10,000 条使用记录预算约束，超出预算或 SQLite 时间/步骤预算时返回明确的分析不完整错误。返回陈旧快照时，即使其持久化状态曾为已完成且新鲜，也不能给出确定性的一致性结论。

一致性查询先应用查询词，再对关联的配置事实执行预算和符号展开；文件格式清单独立遵守注册时的路径及语言范围，不随查询展示筛选收窄。常量引用集合只收集一次，避免每个声明重复扫描全部记录。Java 接收者类型会擦除泛型参数；对已有局部变量的简单赋值可关联后续条件，重新赋值后停止传播；显式静态导入优先于通配符导入。Shell `set -a` / `set -o allexport` 作用于后续赋值，关闭该选项不会撤销已导出变量的属性。通用代码及软件视图不展示原始符号 getter 记录；解析后的配置使用关系仍通过 `feature-flags` 查询。

配置一致性范围说明：注册时的路径和语言限制是证据的授权边界。查询时的路径和语言筛选只投影返回的 `usages`，跨文件绑定及一致性仍使用该已授权快照中的关联证据和格式清单；因此 `conflicting_default_sources` 可以指出显示路径之外、但注册范围之内的定义。仅查看 Java 使用位置不会把已注册的 properties 定义误报为缺失。

### 配置注册表验收矩阵

| 契约 | 必须满足的结果 | 验证 |
| --- | --- | --- |
| Java 读取、常量和 getter（#389/#394） | 真实键、可定位的读取与关联守卫；遵守导入、重载、可见性和非虚分派 | Java 接收者矩阵及快照绑定回归 |
| Properties、INI、ctmpl、Shell、dotenv（#394） | 符合格式的定义、默认值及位置；保留引号内容和续行 | 格式及执行范围矩阵 |
| 元数据及筛选（#394） | 默认值/类型/领域/格式/热加载；CLI、Web、MCP 使用同一请求契约 | 领域、接口和真实索引服务验收 |
| 一致性（#394） | 从授权证据生成可定位的默认值冲突、格式缺失及读取缺失诊断 | 范围、陈旧、歧义和增量快照测试 |
| 未知或条件行为 | 保留证据及不确定性，不从不完整分析推断运行值或确定缺失 | 条件导出/模板和未解析绑定回归 |
| 资源上限 | 文件事实、元数据、扩展及查询超限前返回明确错误 | 边界和超限测试 |

这是有界静态分析，不执行任意 Java、Shell 或模板程序。不能仅为获得无意见审查而删除上述预期行为。检视意见依据该契约及可复现行为判断；描述中的前提不准确，不代表已证实的问题不成立。

本次 Java 平台读取规则清单为 System.getProperty/getenv、直接 System.getenv().get/getOrDefault 和 System.getProperties().getProperty、Boolean.getBoolean、Integer.getInteger、Long.getLong，支持已列明的字面量/常量键及可证明的 getter 转发/转换。等价的全限定名和静态导入形式使用同一规则。“注册表”不隐含承诺识别任意新增 API；但清单内的错误绑定、证据丢失和错误默认值仍属于必须修复的缺陷。


外部类 getter 回退需要已索引继承证据；延迟模板定义不发布根模板默认值。Shell 引号选项遵循引号移除规则，接口静态方法不参与继承。十六进制 Double 默认字符串仍明确不支持：保留原始证据，默认值未知，一致性不完整；本次不扩展为任意 Java 数值语法求值。 Fact version: `config-registry-v49`.

软件本体配置投影排除内部常量、类型/getter 标记及未解析符号行，同时保留其索引证据。Shell set 选项与 export 选项采用相同的静态引号移除规则，覆盖启用、禁用及选项终止符。 Fact version: `config-registry-v50`.

显式但无法求值的 Java 默认值使一致性不完整；已知 String 常量表达式参与重载适用性判断。模板行内不输出值的控制动作保留静态/条件文本。聚合后仍支持边类型查询。无路径/语言投影的纯元数据查询先筛选匹配组，再执行有界符号扩展。集合 containsKey 存在性 API 和命名模板体执行不在限定抽取清单中；缺少定义诊断描述已观察的静态证据，不表示运行时渲染或取值。 Fact version: `config-registry-v51`.

Shell 内置命令名先静态解码，引号、拼接和转义形式使用相同导出分类，普通命令的赋值操作数保留默认值。for/select 循环变量在循环体中遮蔽继承环境值，循环输入展开仍保留读取证据，循环后的可能覆盖保持不确定。Java 属性读取显式 null 默认值经布尔转换得到 false；其他无法求值的默认值仍标记不完整。catch 参数在其语句体内绑定接收者并遮蔽外层字段；无法证明唯一静态类型的 multi-catch 接收者保持未解析。 Fact version: `config-registry-v52`.

配置自由文本查询同时匹配持久化元数据、配置键和使用位置，最终分组匹配与行评分遵守 SQL 元数据搜索契约。显式查询不含任何字母、数字或下划线时，在加载数据前报错；省略查询参数才表示不筛选注册表。Java try-with-resources 声明在 try 体和后续资源初始化中绑定接收者，不在 catch/finally 中生效。Shell 已识别导出内置命令前的赋值，仅在该命令导出同名变量时形成配置定义；普通命令的临时环境赋值不定义父环境配置。 Fact version: `config-registry-v53`.

已证明的 getter 转换同时规范化显式环境回退值与属性回退值，属性特有的可空默认处理保持独立。已知平台通配静态导入只贡献实际提供的受支持成员，final var 配置键须有已证明的 String 初始化值。具名 Java 局部类型使用词法身份，互不相关的方法或代码块不会共享 getter 提供者。异步 Shell 命令不能定义或修改父环境配置及导出状态。nameref 别名跟踪和 command/builtin 分派包装器不在有限 Shell 抽取清单内；直接内置命令名及引号等价形式的识别不执行包装器或间接变量写入。缺少定义诊断描述该清单内已观察的静态证据。 Fact version: `config-registry-v54`.

## 5.10 语言能力矩阵

类型查询使用索引阶段持久化的归属事实，聚合类型及其直接可调用成员；排除继承方法、未知接收者和成员内局部函数。短类型名入口保持不变。调用位置的路径过滤约束实际调用点。类型选择最多接受 64 个类型和 1,024 条类型/成员记录，随后最多读取 200 个调用候选，并保留现有 SQLite 工作预算；超限明确返回查询不完整错误。

配置分析复用索引阶段语法树。下表列出有限的读取 API 清单及提供类型归属的语言结构。现有配置 API 清单中的 `config`/`settings` 读取器继续适用。字面量键、可证明的常量表达式、零参数读取 getter 和局部条件使用可提供证据；依赖运行时的表达式保留未知值。

| 来源 | 类型归属 | 环境变量/属性读取 API 清单 |
| --- | --- | --- |
| Java | 类、构造函数和直接方法 | 现有 `System` 环境变量/属性 API 及已记录的配置读取器 |
| Python | 类和直接方法 | `os.getenv`、`os.environ.get`、`os.environ[key]` |
| JavaScript / JSX | 类和直接方法 | `process.env`、`Deno.env.get`、`Bun.env`、`import.meta.env` |
| TypeScript / TSX | 类、接口和直接方法 | 同 JS API，并适配 TypeScript 语法 |
| C | 不适用；保留普通函数查询 | `getenv` |
| C++ | 类及具有作用域信息的成员实现 | `getenv`、`std::getenv` |
| C# | 类/结构体和直接方法 | `Environment.GetEnvironmentVariable`、`System.Environment.GetEnvironmentVariable` |
| Rust | 类型及固有/trait `impl` 方法 | `std::env::var`、`std::env::var_os`、`env::var`, `env::var_os` |
| Go | 具名类型和 receiver 方法 | `os.Getenv`、`os.LookupEnv` |
| Kotlin | 类和对象 | `System.getenv`、`System.getProperty` |
| Scala | 类、trait 和对象 | `System.getenv`、`System.getProperty`、`sys.env.get`、`sys.env.getOrElse` |
| Ruby | 类/模块和直接方法 | `ENV[key]`、`ENV.fetch` |
| PHP | 类和直接方法 | `getenv`、`$_ENV[key]`、`$_SERVER[key]` |
| Swift | 类型和扩展 | `ProcessInfo.processInfo.environment[key]` |
| Bash（`--source shell`） | 不适用 | 参数展开、现有 export/默认值证据、直接输出 getter |
| Starlark | 不适用 | `ctx.getenv(key, default)` 及已记录的配置读取器；未证明接收者来源时保留不完整状态；`load` 提供显式绑定 |
| Vue | 内嵌 JS/TS 归属 | 脚本复用对应 JS/TS 读取规则 |
| SQL、构建脚本和模板 | 语法没有可调用类型时不适用 | 现有结构化定义、引用和条件证据；不构造类关系 |

跨文件绑定限于同一授权仓库快照，并要求显式导入、类型或语言原生模块证据。环境变量和属性键保持独立命名空间，不能仅凭相同拼写连接不同代码语言的符号。动态键、外部提供方、重赋值、遮蔽、不支持的封装和解析深度耗尽必须作为未解析或不完整证据处理。`analysis_complete=false` 阻止不存在性结论。默认值描述已观察到的静态证据，不预测运行时值。例如，字符串 `"false"` 经 Python `bool` 或 JavaScript `Boolean` 转换为 `true`，经 C# `bool.Parse` 解析为 `false`。

代码事实版本 `config-registry-v56-portable-evidence` 要求通过持久化仓库索引任务重建旧索引。延迟查询索引计划升级为版本 5，保留 v4 的类型归属和语言/文件索引，追加配置身份、配置来源键及 caller/callee 身份索引（序号 19–22）；已有 v1–v4 检查点恢复时继续验证原有前缀。CLI、HTTP 和 MCP 继续复用同一服务合同和配置查询预算。

### 跨文件证据与限制

| 语言组 | 必需证据与保留的限制 |
| --- | --- |
| Python | 明确的模块导入或相对 `from` 导入；词法重新绑定会终止关联。 |
| JS/JSX、TS/TSX、Vue 脚本 | 带源文件扩展名的明确相对导入，以及匹配的具名或默认导出。省略扩展名、包加载器和再导出链保留未解析状态。 |
| C/C++ | 引号形式的仓库相对 include。C++ 归属保留限定作用域；头文件沿用检测到的语法（`.h` 默认使用 C，有 C++ 声明证据时使用 C++；`.hpp` 使用 C++）。 |
| Rust | 用已索引的 `mod` 声明证明模块成员身份，包括静态 `#[path]` 重定向；导入别名保留原始目标名。缺少模块声明、条件模块和宏控制的成员关系保持未解析。 |
| Go | 声明的 package 与所在目录连接 receiver 方法和包级配置提供者。 |
| Kotlin、Scala、C# | 精确的原生 package、namespace 和类型身份；私有提供者不能满足跨文件导入。伴生对象保留独立的直接归属。 |
| Swift | 配置提供者使用已索引的目录模块；跨文件类型扩展要求明确的类型导入，并沿用现有唯一模块目录合同。未知目标保持未解析。 |
| Ruby / PHP | Ruby 使用 `require_relative`；PHP 使用以 `__DIR__` 为锚点的 `require`/`include`。namespace 和名称相同不证明 PHP 文件已加载。 |
| Bash / Starlark | Shell 支持通过 `dirname` 与 `BASH_SOURCE[0]` 定位脚本目录的 `source`；Starlark 使用 `load`。普通相对 Shell source 保留工作目录未知证据；`source`、`eval`、`unset` 可以使先前函数绑定失效。 |

分析器将不返回配置的 getter 声明保存为内部阻断证据，防止无关同名 getter 继承另一个提供者的配置结果。原生参数语法、局部声明和导入共同约束作用域。显式默认值及受支持的转换保留证据；不能求值的默认值保持未知。SQL、构建脚本和模板沿用已有语法的定义及引用覆盖，不表示支持通用程序流求值。

Rust 的 `crate` 导入必须通过常规 Cargo 根和索引中的 `mod` 关系证明导入文件归属。
`src/bin`、`tests`、`examples` 根与 `src/lib.rs` 分开；多根共享文件、内联模块、
条件模块声明和自定义 manifest 根在无法证明归属时保留未解析。
Swift typed import 必须在所有获准检查的 Swift 文件中具有唯一物理模块目录；
语言/文件索引排除其他语言的扫描，模块证据上限为 1,024 个文件。

getter 绑定重新赋值后撤销其导出证明，函数体的原始读取证据仍保留。
C# 显式别名约束目标身份；不支持的其他 native 导入别名保留未解析。
非空布尔/数值转换不会触发空值回退；未解析的跨文件 getter 外层回退保留不完整状态，
不能在证明其返回语义前推测默认值。查询索引计划 v5 保留 v4 的归属和语言/文件索引，追加配置及调用身份查询索引。

Java 零参数配置 getter 可以使用任意方法名，仍遵守现有可见性、继承和遮蔽检查。
本地方法和属性展开复用提供者稳定性检查，包括已知的成员改写。
C# 文件级 namespace 和相对 namespace 别名保留声明作用域；`global::` 明确选择全局命名空间。
尚不能证明的 Scala selector import 会阻止包级名称回退。

C++ 主类模板以声明参数槽区分身份，成员模板使用外层的所属类型参数，具体特化保持独立身份。
复杂模板参数仍受语法和静态身份解析限制。Swift 构造函数、protocol 要求和 subscript
保留各自成员范围。此处 Dockerfile 能力沿用已有 stage/import 图，不求值任意 `RUN` 命令。

C++ 分离实现中的具体命名模板参数需要类型绑定证据；裸参数 `V` 保持未解析，
因为不同命名空间可以定义不同的 `V` 类型。声明参数槽、内建类型和可证明的字面量参数
仍支持有界身份关联。

持久化类型调用回归矩阵同时验证 JavaScript 与 TypeScript 两种 Vue 脚本，
包含 `vue` 语言过滤和调用位置的路径过滤。


类型调用聚合保留真实调用点的字节和行范围；普通函数查询保留既有的上下文行范围。匿名回调具有独立调用归属；未知接收者不能仅凭成员同名证明目标。JS 静态与实例 `this` 分开处理，Java 允许实例限定的静态调用。类头和计算成员名不能建立新声明类的 `this` 绑定。

Ruby 下标读取和 Rust `env::var_os` 提供环境变量证据；纯赋值及 Python/JS 方法选择器不会成为配置键。Rust 按最近词法作用域判断导入来源，区分本地 `std` 模块与 `::std`。Go 包常量可跨文件组合配置键，最多 32 个字符串组成部分、4 层快照解析和 4,096 字节结果。可变值、非字符串提供者、作为常量使用的 getter 和循环保持未解析；不同未知表达式保留独立身份。

无扩展名脚本需要前 256 字节内受支持解释器的 shebang；watcher 保留其修改及删除事件。文件首个注释中的 Flow pragma 使用已有 typed JSX 语法，保留 JS/JSX 标识；不支持的语法继续报告 partial。这是有限语法恢复，不是完整 Flow 类型分析。Vue 只将顶层 SFC 区域计入 16 区域预算，并复用已解析的 HTML 树。

语言选择必须同时满足仓库注册范围与当前请求；共享清单可能满足多种语言的路径规则。不同有效选择使用不同 scope 身份，不能互相复用检查点。shebang 候选在内容检查后被排除时保留 `excluded` 进度，不贡献代码或配置证据，也不会让仓库进入 degraded 状态。源码回退耗尽候选预算时报告不完整，不能把空结果视为不存在。
