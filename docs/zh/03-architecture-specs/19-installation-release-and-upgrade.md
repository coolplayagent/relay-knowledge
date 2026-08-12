# 安装、发布与升级

[中文](../../zh/03-architecture-specs/19-installation-release-and-upgrade.md) | [English](../../en/03-architecture-specs/19-installation-release-and-upgrade.md)

> 文档版本: 3.4
> 编制日期: 2026-08-12
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

安装和发布是产品架构的一部分。稳定版本必须可验证、可回滚、可卸载、可诊断；二进制安装路径和运行时状态必须分离；后台服务必须交给平台 service manager 管理。

## 2. 发布渠道

- GitHub Releases 发布跨平台预构建压缩包、checksums 和 release notes。
- crates.io 保持 `cargo install relay-knowledge` 可用。
- Homebrew、Scoop、winget 或发行版包应引用同一 release tag 产物，不重建分叉快照。
- Release tag 使用 `vX.Y.Z`、`X.Y.Z` 或 `vX.Y.Z-rc.1` 这类 prerelease 形式；数字版本必须在推送 tag 前与 `Cargo.toml` 和 `Cargo.lock` 保持一致。手动 dry-run dispatch 复用同一版本契约，但不会发布 crates.io 或 GitHub release 产物；workflow 默认 dry-run tag 必须随每次 release 版本提升同步更新。
- v1.1.13 release 准备将 `Cargo.toml`、`Cargo.lock`、CLI skill metadata 和 release workflow dry-run 默认值统一固定到 `1.1.13`；发布仍由 tag 驱动，只有推送 `v1.1.13` 或 `1.1.13` 到 GitHub 后才会开始。
- macOS x64 release job 必须使用仍可用的 Intel runner label，例如 `macos-15-intel`，不能继续依赖已退休的 `macos-13` 镜像。Artifact upload/download 和 attestation action 必须保持在兼容 Node 24 的版本，确保 GitHub-hosted runner runtime 迁移后 release workflow 仍可运行。
- Linux GNU release job 必须在 glibc 2.31 baseline 上构建 `x86_64-unknown-linux-gnu` 和 `aarch64-unknown-linux-gnu` 产物；如果产出的 ELF 需要任何高于 2.31 的 `GLIBC_*` 符号，release 必须失败。CLI skill 内置的 Linux x64 asset 打包后也必须通过同一 ABI 检查。
- OpenTelemetry 依赖构成一个发版兼容族：`opentelemetry`、`opentelemetry_sdk` 与 `opentelemetry-otlp` 使用相同 minor 版本，`tracing-opentelemetry` 使用对应的集成版本。依赖自动化必须整体升级并验证该兼容族；发版候选不能同时包含多条 OpenTelemetry core 或 SDK major/minor 版本线。当前安全基线为 `opentelemetry_sdk` 0.32.1；它按照 GHSA-w9wp-h8wv-79jx / CVE-2026-48504 拒绝超过 8,192 byte 的 W3C Baggage，并最多解析 64 个 list member。
- XML parser 的安全基线为 `quick-xml` 0.41.0；它按 RUSTSEC-2026-0194 与 RUSTSEC-2026-0195 将重复属性检查保持为线性复杂度，并限制每个元素的 namespace declaration 数量。Informational unsoundness warning 如果已有补丁，必须升级 lockfile，不能直接忽略。
- Release archive attestation 使用生成的 `checksums.txt` 作为 subject manifest，使 GitHub artifact attestation 覆盖用户本地校验的同一批 archive digest。
- CLI 新版本发现使用可配置双源：GitHub Releases 和 crates.io。检测必须走 `env`、`paths`、`net::http` 边界，继承代理、TLS、timeout 和 runtime cache 策略；普通命令只能提示稳定新版，不能静默替换二进制。
- GitHub Releases 包含从 `skills/relay-knowledge-cli` 构建的 `relay-knowledge-cli-skill-<tag>.tar.gz` skill 产物；其版本跟随 `Cargo.toml`，并会以数字 semver 写入生成后的 `SKILL.md` metadata。skill 产物包含根目录 `README.md`，并在 `assets/` 下内置 Linux x64 和 Windows x64 二进制，要求 agent 在匹配平台的内置二进制通过 `version --format json` 校验时优先使用它。只有内置二进制不可用、宿主 Linux glibc 低于内置 asset baseline，或用户明确要求系统安装版本时，agent 才回退到 `PATH`。配置 `CLAWHUB_TOKEN` 时，release workflow 还可以用 `clawhub publish` 把同一个生成后的 skill 布局发布到 ClawHub。该 skill-over-CLI 产物与 MCP 协议打包分离。
- Skill 产物包含 `references/knowledge-map-workflows.md`，并通过 policy gate 固化 knowledge-map/code-map 联合 bootstrap 与固定 ref 的 spec 开发默认提示词。升级 skill 只更新 agent 指令；只有获得授权的 agent 显式执行文档中的 CLI 工作流后，才会修改仓库 YAML 或 runtime index state。

## 3. 安装体验

Installer 或安装脚本支持：版本选择、安装目录选择、dry run、校验和验证、service definition 生成、失败回滚和 uninstall plan。默认不会把数据写入 release 解压目录。

服务化部署安装体验必须显式说明拓扑：`embedded_cli` 不安装常驻服务，`resident_single_process` 安装一个平台 service，`resident_partitioned_sqlite` 还要把 shard 目录纳入备份/迁移/卸载确认。`service plan install|upgrade|rollback|uninstall --format json` 必须在 `runtime_state_paths`、`lifecycle_steps`、`rollback_steps`、`permission_requirements` 和 `warnings` 中列出主库、配置/状态/日志/缓存路径、service definition 路径、service 名称、权限要求、失败回滚计划，以及 partitioned 模式下的 shard 目录覆盖要求。`service lifecycle <action> --dry-run` 是默认可审计输出；只有显式传入 `--execute` 才能写 service definition、checkpoint 或安装目录，并调用 systemd、launchd 或 Windows Service 命令。未来 `split_worker_preview` 必须分别生成控制服务和 worker 服务定义并说明每个进程的权限、环境变量、日志和 shutdown 行为。

安装的常驻服务还必须显式说明 commit-loop policy。`RELAY_KNOWLEDGE_WATCHER_ENABLED` 同时控制源码监听和 Git HEAD reconciliation；`RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS` 默认为 `5000`。service definition、lifecycle plan 与 doctor output 必须保留或解释这些值，不能把只对安装 shell 生效的 export 当作安装配置。reconciler 在平台 service manager 下执行受界周期检查并提交 durable code-index task；installer 不得额外安装仓库专用 Git hook 或 unmanaged polling process。

实现必须通过明确所有权保持该合同可审计：生命周期步骤策略留在 `application::service::lifecycle_plan`，服务定义渲染、平台权限以及 systemd/launchd/Windows Service 命令统一放在 `lifecycle_plan::platform_service`。修改任一边界都必须维持所有支持平台一致的 dry-run 计划与执行合同。

精确代码源码兜底由产品内部实现，运行时不能依赖 `rg`。面向 agent 的 setup 说明可以提到使用有界 `rg` 或 `grep -RIn` 做人工检查工具，但安装器不能把递归 grep 作为 service 依赖，也不能把它当成已索引查询行为的替代品。

## 4. 运行时状态

配置、数据库、索引、日志、缓存、临时文件和 dead-letter 数据写入 `paths` 管理的平台目录。升级时必须保留 runtime state，并显式执行 schema/index migration。
commit-loop retention 属于 runtime state。每次发布保留 active 与最近两个成功发布时间窗口的并集（窗口通常已包含 active）、最近一次成功增量的 predecessor、active worktree overlay 的 clean base，再加未完成 task 的 target/base 和 repository-set pin。SQLite 会幂等增加 `retiring` scope state 与 durable GC job：逻辑退役保持原子，后续每个 maintenance transaction 推进一个 scope-GC phase，该 phase 在受影响的应用表之间合计最多删除 512 个物理行，包括旧 facts、code FTS/search row、software projection、checkpoint、workspace state 或 scope metadata；task-audit 与 commit-alias 的独立配额使每个 pass 的主清理合计最多 2,048 个物理行，另加最多一个终态 GC-job bookkeeping 行。同 tree commit 复用内容图，并使用每仓 256 条的 commit alias 窗口。完成态 task history 限制为每仓库 128 条 succeeded 和 64 条 failed/dead-letter/cancelled，同时为每个 retained scope 保留最新 success。升级后会恢复持久 GC job；旧 binary 不理解 retirement state，不能与新 binary 共用 database，也不能从 task row 重建已淘汰 scope。GC 会限制 live generation 并让 SQLite 复用释放页，但不承诺数据库文件立即在 OS 层缩小；回收物理 high-water mark 需要另行执行显式、有界 compaction。
repository-set 迁移会增加持久 refresh-task queue 及其 claim/capacity/audit 索引，overlay selector 迁移还会在 SQLite schema 初始化时幂等增加虚拟 origin-path 列和 origin/target 复合索引。升级后 managed service 会恢复可执行的 refresh task。手动 refresh 在所有 member 间共享 4,096 个 chunk、16 MiB 和 32,768 个 derived item 的 manifest 预算。升级计划必须为一次性索引构建预留时间；回滚应保留增量 queue table、列与索引，不得删除或重建 overlay facts。遗留手动 overlay 超过 8,192 条 edge 时，系统会在无界删除前保持数据不变并拒绝；整仓删除遇到超过 64 个受影响 set 或任一受影响超限 overlay 时也会原子拒绝。当前版本没有有界 repair 命令，operator 需要后续升级提供 repair tool，不能假设 migration 已闭环该清理路径。这些手动 set 上限不适用于显式启用的 automatic-workspace cross-edge builder；scope GC 会限制过期状态删除，但尚未限制该路径的单次 build。

SQLite graph-store schema marker v4 是一次明确的 forward derived-retrieval-state migration。它以包含 scope64 partition token 与 scope-qualified group token 的 indexed、zero-weight `routing_key` 重建 global `graph_bm25` FTS5 table，并新增 route state/document/group/term tables 与持久 global route-term document frequency。Route document 保存 document identity/kind/scope/path、`created_graph_version`、可观测 `label_gram_state`、group token、有界 term-count JSON，以及 `fts_rowid NOT NULL UNIQUE` sidecar。权威 evidence、graph facts、code symbols 与 code chunks 不变，因此所有 v4 retrieval structure 都能从这些 source 重建。

当前 document-write transaction 会一起更新 `routing_key`、route sidecar、route-state document count、每组 collection frequency 与持久 global document frequency。Fresh-open reconciliation 检查 schema、`simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4` route fingerprint、freshness/version state、持久 semantic/vector generation marker，以及 authoritative/active-global/route-document/group/semantic/vector/state population count；它不会在每次 open 时做无界 identity、逐行 tokenizer 或 aggregate 深扫。Canonical identity 与 tokenizer consistency 只在其他 stale/schema/count 条件已经触发重建后校验；仅有 equal-count per-row drift 不会在 open 时触发 rebuild。

重建会取得带 owner/expiry 的 durable lease，并连同 phase/cursor checkpoint 与固定 semantic/vector rebuild plan 发布 `building`，创建 `graph_bm25_rebuild`；旧 attempt 过期后可接管并从持久 checkpoint 续跑。每个 transaction 最多接纳 128 篇文档、4 MiB 估算权威 source bytes、8,192 个 labels 和 8,192 个 links。单篇文档若超过一个或多个工作预算，会独占 transaction 并发出 identity 受界的 warning；该行为保证前进，不代表单文档绝对 byte bound。旧的 flat `graph_bm25` 保持可读，semantic、vector 与 fuzzy lexical fallback 在 `building` 期间暂停，之后以有界 rowid keyset cleanup 删除 stale label/semantic/vector row。当前 evidence/code writer 使用 `IMMEDIATE` transaction，并在 rebuild 活跃时拒绝写入。完整性校验通过后，一个短事务把 active `graph_bm25` 改名为 `graph_bm25_retired`、提升 shadow、把 route state 发布为 `fresh`，并记录 schema marker v4。Retired table 只在提交后删除，因此 crash 或 rollback 不会发布 partial FTS generation。升级计划必须为 active 与 shadow FTS 同时存在、sidecar、WAL 和短暂 retired cleanup 预留时间及临时磁盘余量。

Query hot path 读取持久 version/count/DF，不运行 full-table `COUNT` 或 `SUM`。对每个实际 query term，它会把持久 global DF 与仅限定 business column、最多探测 `df + 1` 行的 `MATCH` 对比；每个 term 都必须不超过 corpus 的 20%，所有探测合计最多预留 65,536 个 postings。Scoped FTS 还会与 scope64 routing token 求交，普通 SQL scope predicate 仍独立承担硬授权。Single-FTS reader 通过 hidden rank column 排序有界 identity window，再经 `fts_rowid` sidecar hydrate；跨越该 window cutoff 的完全同分不承诺确定 membership。Historical unscoped fallback 的 authorized-corpus、label-state 与 `label_lower` probe 使用 version-leading global indexes，scoped index 继续保留。一次完整 graph search 共用一个 deferred read transaction，因此并发 FTS activation 不会拆分其 SQLite snapshot。不能仅因表已存在就报告 routing 为 fresh。

虽然 v4 scorer 把 `routing_key` weight 设为零，FTS5 仍把该列计入 document length 与 corpus average document length，因此 v4 的数值 BM25 baseline 可能不同于 v3。支持的 parity invariant 仅是：同一个 v4 table 上 routed 与 flat 执行的公共文档具有 bitwise-identical score。

既有 v1.1.13 时期的 code index 可能包含已裁掉首尾空白的 Markdown source window；一次性 code-index migration 会在同一原子事务内把含 Markdown 的 scope 标为 stale 并记录 migration marker，但未持久化的字节无法由 database schema migration 恢复。repository graph 物化还会按 indexed file length 校验连续 chunk byte range，并对受影响 scope 返回明确的 lossless/re-index 错误。该 scope 首次使用 `repo graph` 前，operator 必须显式执行完整 `repo index`；incremental `repo update` 会拒绝 stale base，不能完成这项恢复。Markdown window 通过正常的 durable task、single-writer lease、checkpoint、有界重试与 freshness 发布流程重建。安装或替换二进制不能仅因 schema initialization 成功就宣称这项数据刷新已经完成。

本地文件定位索引的 SQLite/FTS5 状态也写入同一运行态数据区。安装器和 service
template 不能默认扫描全盘、Linux `/opt`、挂载盘或 Windows 非系统盘；只有用户显式配置
或通过 CLI 传入这些 root 时才建立索引。

当启用 `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite` 时，主数据库仍保存控制状态，每个代码仓库的 shard 数据库位于运行时数据目录的 `stores/repositories/` 下。备份、迁移、doctor、卸载确认和回滚计划必须把主数据库与 shard 目录视为同一个 runtime state 集合；不能只移动或校验主数据库后宣称升级成功。
shard catalog 路由是可迁移的，恢复时会基于当前 runtime data 目录重新解析；但这只有在 shard 目录随主数据库一起移动时才成立。

未来外部 graph/vector/storage 后端或复制 SQLite 后端也属于 runtime state。安装器、doctor 和升级计划必须记录后端类型、endpoint 或本地目录、认证配置来源、schema/index migration 状态和回滚说明；不能只替换二进制后宣称数据面升级完成。

## 5. 升级与回滚

升级流程：

```text
preflight doctor
  -> operator 停止所有 ad hoc CLI writer
  -> operator 创建事务一致的 runtime-database backup
  -> lifecycle executor 记录 binary/service-definition rollback checkpoint
  -> lifecycle executor 停止 managed service
     -> stop 成功且不存在 ad hoc writer 时建立独占访问
  -> 复制/安装新 binary 并刷新 service definition
  -> 通过 platform service manager 启动新 binary
     -> 首次同步打开 database 时运行 schema/index migration 与 shadow rebuild
     -> 该次打开完成后 service 才可用
  -> post-upgrade doctor
```

停止 ad hoc CLI writer 并创建事务一致的 runtime-database backup 是 operator precondition。Lifecycle 成功停止 managed service 后，结合不存在 ad hoc writer，才建立 migration 所需的 database 独占访问。Lifecycle executor 不会独立探测 exclusive access，也不会创建 runtime-database checkpoint；它的 rollback checkpoint 只覆盖 binary 和 service definition。如果 operator 要求独立的 exclusive-access 检查，必须用外部 maintenance procedure 分阶段执行文档化步骤，不能把一次性 `--execute` 当作该验证。

失败时 lifecycle executor 回滚 binary 和 service definition。Database rollback 使用 operator 创建的 runtime checkpoint；如果没有该 checkpoint，v4 derived-index migration 将按下文说明保持 forward-only。

首次启用 commit reconciliation 的升级应先停止旧 service，备份完整 runtime state set，再以期望的 watcher switch/interval 安装新 service definition；新服务启动时先恢复 lease，再对账 HEAD。升级后必须用 `service status --format json` 检查 watcher state、`total_commit_reconciliations`、`total_commit_tasks_queued`、`total_commit_reconcile_failures`、code-index queue/lease 与 retention。回滚只恢复上一版 binary/service 配置，不会恢复成功发布后已淘汰的 scope；精确恢复历史 scope 需要 runtime database backup，或从源码仓库重新 full index。

只把 binary 回滚到 pre-v4 release 并不会撤销 forward derived-index migration。旧二进制可以忽略 routing sidecar 并使用既有 flat query path，但保留的 v4 `graph_bm25` table 与 v3 index 在数值上并不等价。旧二进制若写入 derived document，不会一致维护 `routing_key` 与 v4 sidecar，此后必须把全部 hierarchical metadata 视为 stale。旧 writer 还会写回旧 schema marker，因此后来启动 v4 时，即使 route state 表面兼容，也会显式将其 invalid，再从权威 document 重建 `routing_key` 与 sidecar，完成后才能重新启用 routing。精确恢复旧 scoring baseline 需要还原 pre-v4 runtime-database checkpoint，而不只是换回旧 executable。v4 的 `IMMEDIATE` 应用 fence 不是跨版本 lock protocol：已经运行的旧 binary 不检查 `building`，可以绕过该 fence 写入。权威 facts 才是 recovery boundary；不能把 route metadata 当作用户数据的唯一副本，upgrade、rebuild 与 rollback 都必须独占 database，不能让新旧 writer 并发写入。

`service lifecycle upgrade --execute` 按现有 dry-run 阶段执行：记录 binary/service-definition rollback checkpoint、停止 managed service、按需复制 binary、写 service definition、刷新平台 service manager、启动 service，并在 post-upgrade doctor 前后保留执行报告。它没有独立的 exclusive-access 验证、runtime-database checkpoint 或 migration/rebuild 阶段。调用前，operator 必须停止 ad hoc CLI writer，并创建必要的事务一致 runtime-database checkpoint；service manager 无法 fence 独立运行的旧进程，lifecycle checkpoint 也不覆盖 runtime data。该命令要求 managed-service stop 步骤成功，但不会另行验证独占性。Platform service manager 启动新 binary 后，binary 会在首次同步打开 database 时执行 schema v4 migration 与必要的 shadow rebuild，并且该次打开完成前 service 不可用。Linux systemd unit 必须引用包含空格的路径，并把字面 `$` 转义为 `$$`。install 写入显式 `--install-dir` 前不得覆盖已有目标二进制或 service definition；Windows install 必须把 service 创建和 registry/environment 写入拆成可单独回滚的步骤。upgrade 必须 checkpoint 已有目标二进制和 service definition，checkpoint backup 必须使用 attempt-scoped 文件并通过原子 checkpoint 发布成为当前回滚依据；没有旧备份时失败回滚和显式 rollback 只能删除本次确实复制或写入的目标文件，definition-only upgrade 不得删除当前运行的 binary。Windows 和 macOS upgrade 必须在启动前刷新 platform service registration，使 SCM `BinaryPathName` 或 launchd loaded job 与更新后的 service definition 一致。uninstall 失败回滚和基于 uninstall checkpoint 的显式 rollback 如果需要恢复已删除的 service definition，必须从 uninstall 前 checkpoint 恢复原 definition，再重新注册 service manager。文件或 service manager 状态变化后任一阶段失败时，必须按 `rollback_steps` 尝试恢复已完成步骤；restore、definition rewrite、unregister 或 service-registration rollback step 失败后不得继续执行依赖的删除、reload/start 步骤；任何此类状态变化前失败时，不得停止、disable、恢复、重启或卸载既有 service。只有选中的 rollback steps 全部成功时，执行报告才能把 rollback 标为完成；外部 service manager 或 doctor 子进程必须有有界执行时间，并在等待退出和超时期间持续读取 stdout/stderr，进程退出或超时后的输出收集也必须有边界。`--execute` 出现失败 step 时，API/CLI 操作必须返回错误并带出失败 step id，不能把失败报告包装成成功响应。`service lifecycle rollback --execute` 使用 checkpoint 备份恢复二进制和 service definition，不恢复 runtime database；没有 lifecycle checkpoint 时必须把缺口暴露在 warnings 或执行错误中，不能静默宣称回滚成功。

`relay-knowledge version check` 是只读诊断入口，输出当前版本、最新稳定版本、来源、release URL 和诊断信息。实际升级仍必须由用户、installer 或包管理器显式执行，并继续遵守 preflight、checkpoint、service restart 和 post-upgrade doctor 流程。

## 6. 发版文档准备

推送 release tag 前，release owner 需要检查用户和运维最先阅读到的文档面：

- 根目录 `README.md` 与 `README.zh-CN.md` 说明当前版本的安装渠道、内置 CLI
  skill 产物和质量门禁。
- `docs/README.md`、`docs/en/README.md` 和 `docs/zh/README.md` 列出当前书籍结构、近期基准/验证记录，以及尚待翻译的中文-only 记录。
- 第 1 章安装说明和本章发布契约在运行时目录、service manager 托管、版本检测、回滚和卸载行为上保持一致。
- `06-verification` 下有带日期的记录，说明文档文件清单、本地链接检查、文件长度检查，以及在未刻意修改产品行为时本次改动是 documentation-only。

文档刷新不能把 release 命令写成会暗示不存在的产物、不支持的包管理器、未受管 service loop
或自动静默升级。

## 7. 验收标准

- Release artifact、checksum、版本号和文档能互相对应。
- Linux GNU release 二进制和 skill Linux x64 内置 asset 不得依赖高于 2.31 的 `GLIBC_*` 符号。
- GitHub Release 将 CLI skill archive 纳入 `checksums.txt`，archive 内含 skill `README.md`、Linux x64 和 Windows x64 asset 二进制；启用 ClawHub 发布时使用同一个 crate 版本和生成后的 asset 布局。
- CLI 能说明稳定新版本可用，JSON 输出保持机器可读且普通命令不会自动安装新版。
- 面向 release 的文档有带日期的 `06-verification` 审计，覆盖导航、清单、链接检查和 documentation-only 改动边界。
- service install 使用 systemd、launchd 或 Windows Service，而非 unmanaged loop。
- `service lifecycle <action> --dry-run` 输出 service 名称、definition 路径、安装目录、运行时路径、权限要求、rollback 计划和 package manifest 校验链路；`--execute` 只在显式请求时运行，并在失败时执行 rollback steps 且返回操作错误。
- uninstall 清理服务注册和服务定义，但保留或按用户确认处理 runtime data。
- 卸载 service 会停止 commit reconciliation；保留 runtime data 也会保留 active/recent scope、protected pin、有界 task history 与后续 full-reindex 能力。显式删除数据时必须覆盖每个 code shard，且没有备份时不能宣称可逆。
- 分片 SQLite 拓扑的 shard 目录参与 backup、migration、doctor 和 uninstall 确认。
- SQLite graph-store upgrade 能识别 schema marker v4，在旧 flat FTS 保持可读时通过可接管续跑的 phase/cursor checkpoint 与有界 document/source-byte/label/link batch，从权威 facts 重建 `graph_bm25_rebuild` 及 rowid/version/label-state sidecar，在 `building` 期间暂停 semantic/vector/fuzzy companion reads，原子激活 route/FTS/marker state，为 rebuild 预留时间/WAL/磁盘，并暴露 v3-to-v4 score-baseline 变化。旧 binary 不遵守应用 fence，因此 upgrade 必须独占访问；binary-only rollback 保留 flat path 但不提供数值 v3 equivalence，精确评分回滚需要恢复 pre-v4 database checkpoint。
- 控制服务和 split worker 的服务定义、运行时目录、日志、环境变量和权限边界在 plan/install/uninstall 中可诊断、可回滚。
- Release workflow 或等价门禁必须运行 service lifecycle dry-run smoke，验证发布二进制生成的 service definition、rollback plan 和 package manifest 检查不会与 release tag 漂移。

---

导航: 上一章: [18. 可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md) | 下一章: [20. 多仓库代码图谱薄覆盖层](20-multi-repository-code-graph-overlay.md)
