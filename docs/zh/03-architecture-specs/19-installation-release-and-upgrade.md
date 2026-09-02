# 安装、发布与升级

[中文](../../zh/03-architecture-specs/19-installation-release-and-upgrade.md) | [English](../../en/03-architecture-specs/19-installation-release-and-upgrade.md)

> 文档版本: 3.14
> 编制日期: 2026-08-30
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

安装和发布是产品架构的一部分。稳定版本必须可验证、可回滚、可卸载、可诊断；二进制安装路径和运行时状态必须分离；后台服务必须交给平台 service manager 管理。

## 2. 发布渠道

- GitHub Releases 发布跨平台预构建压缩包、checksums 和 release notes。
- crates.io 保持 `cargo install relay-knowledge` 可用。
- Homebrew、Scoop、winget 或发行版包应引用同一 release tag 产物，不重建分叉快照。
- Release tag 使用 `vX.Y.Z`、`X.Y.Z` 或 `vX.Y.Z-rc.1` 这类 prerelease 形式；数字版本必须在推送 tag 前与 `Cargo.toml` 和 `Cargo.lock` 保持一致。手动 dry-run dispatch 复用同一版本契约，但不会发布 crates.io 或 GitHub release 产物；workflow 默认 dry-run tag 必须随每次 release 版本提升同步更新。
- v1.1.16 maintenance release 准备将 `Cargo.toml`、`Cargo.lock`、CLI skill metadata 和 release workflow dry-run 默认值统一固定到 `1.1.16`；发布仍由 tag 驱动，只有推送 `v1.1.16` 或 `1.1.16` 到 GitHub 后才会开始。该版本刷新 lockfile，将已被 yanked 的传递依赖 `chacha20` 0.10.1 兼容升级到 0.10.2，不改变直接依赖声明面。源码开发版本必须领先于 crates.io stable，禁止未发布行为与不兼容的已发布 binary 共用版本号。
- macOS x64 release job 必须使用仍可用的 Intel runner label，例如 `macos-15-intel`，不能继续依赖已退休的 `macos-13` 镜像。Artifact upload/download 和 attestation action 必须保持在兼容 Node 24 的版本，确保 GitHub-hosted runner runtime 迁移后 release workflow 仍可运行。
- 仓库 Pages 站点必须由管理员一次性启用并设置 `build_type=workflow`。Pages workflow 使用兼容 Node 24 的 `configure-pages`、`upload-pages-artifact` 与 `deploy-pages` release，不得让权限受限的 `GITHUB_TOKEN` 在每次 push 时重复创建或启用站点。上传前必须从 `Cargo.toml` 推导当前 package version，并要求中英文 release page、首页入口、GitHub Release 链接和 crates.io 链接一致；页面仍停留在旧版本或只更新一种语言时必须拒绝部署，不能静默发布过期内容。
- Linux GNU release job 必须在架构匹配的 GitHub-hosted runner 上，分别使用固定 digest 的 manylinux 2.28 x64 与 ARM64 容器原生构建 `x86_64-unknown-linux-gnu` 和 `aarch64-unknown-linux-gnu` 产物。容器必须在构建前断言实际 glibc 为 2.28，并在容器内启动新 binary；如果产出的 ELF 需要任何高于 2.28 的 `GLIBC_*` 符号，release 必须失败。CLI skill 内置的 Linux x64 asset 打包后也必须通过同一 ABI 检查。
- OpenTelemetry 依赖构成一个发版兼容族：`opentelemetry`、`opentelemetry_sdk` 与 `opentelemetry-otlp` 使用相同 minor 版本，`tracing-opentelemetry` 使用对应的集成版本。依赖自动化必须整体升级并验证该兼容族；发版候选不能同时包含多条 OpenTelemetry core 或 SDK major/minor 版本线。当前安全基线为 `opentelemetry_sdk` 0.32.1；它按照 GHSA-w9wp-h8wv-79jx / CVE-2026-48504 拒绝超过 8,192 byte 的 W3C Baggage，并最多解析 64 个 list member。
- XML parser 的安全基线为 `quick-xml` 0.41.0；它按 RUSTSEC-2026-0194 与 RUSTSEC-2026-0195 将重复属性检查保持为线性复杂度，并限制每个元素的 namespace declaration 数量。Informational unsoundness warning 如果已有补丁，必须升级 lockfile，不能直接忽略。
- Release archive attestation 使用生成的 `checksums.txt` 作为 subject manifest，使 GitHub artifact attestation 覆盖用户本地校验的同一批 archive digest。
- CLI 新版本发现使用可配置双源：GitHub Releases 和 crates.io。检测必须走 `env`、`paths`、`net::http` 边界，继承代理、TLS、timeout 和 runtime cache 策略；普通命令只能提示稳定新版，不能静默替换二进制。
- GitHub Releases 包含从 `skills/relay-knowledge-cli` 构建的 `relay-knowledge-cli-skill-<tag>.tar.gz` skill 产物；其版本跟随 `Cargo.toml`，并会以数字 semver 写入生成后的 `SKILL.md` metadata。GitHub Release 产物包含根目录 `README.md`，并在 `assets/` 下内置 Linux x64 和 Windows x64 二进制，要求 agent 在匹配平台的内置二进制通过 `version --format json` 校验时优先使用它。只有内置二进制不可用、宿主 Linux glibc 低于内置 asset baseline，或用户明确要求系统安装版本时，agent 才回退到 `PATH`。配置 `CLAWHUB_TOKEN` 时，workflow 会向 ClawHub 发布相同版本的指令与 references，但不嵌入二进制，因为 ClawHub 要求单文件小于 10 MB；缺少 asset 时会按既有路径解析已校验的 GitHub Release 或 crates.io 安装。该 skill-over-CLI 产物与 MCP 协议打包分离。
- Skill 产物包含 workflow reference，以及 Draft 2020-12 的 `knowledge-map.schema.json`、`codespec-map.schema.json`、`business-glossary.schema.json`。Metadata gate 解析全部三个 schema，保护 Knowledge Map v3 的 map/shard/history/redirect branch、CodeSpec 强类型目录根、Business Glossary v1 bounds、代表性正反例和未知字段兼容策略。GitHub Release archive 必须显式包含全部 schema；基于目录的 ClawHub 发布自动包含相同 reference。Schema 接受不能替代 `map validate`，也不授权直接编辑生成 artifact。

Knowledge Map schema rollout 受 release gate 约束。PR CI 同时使用当前源码 binary 与 crates.io 最新 stable binary 校验仓库 map，并把完整结果发布为 artifact。低于 `1.1.15` 的 stable reader 无法读取可见的 v3 CodeSpec 与 Knowledge map 且源码版本更高时记录为 `staged_pending_reader_release`；源码与 stable 同版本不兼容时以 `incompatible_same_version` 硬失败。stable 达到 `1.1.15` 后，任何不兼容都必须使门禁失败。后续 writer schema 只有在上一 stable reader 已能接受时才可成为默认格式。该门禁只诊断，不会重写仓库 map，也不会静默升级已安装 binary。

Repository Map v3 使用可见的 `codespec/` 与 `knowledge/` 根目录。reader release 必须先于 writer/path migration release。迁移保留 v2 manifest 与不可变资产，最后发布 `knowledge/knowledge-map.yaml`，随后在 `.knowledge/knowledge-map.yaml` 写入 v3 redirect，使旧 reader 明确失败。`map migrate --type knowledge --rollback` 在取得 repository writer lock 后恢复保留的 v2 根，并保留 v3 数据用于向前恢复。仓库 `docs/` 内容不会自动移动，卸载也不得删除任一仓库拥有的 map 目录。

forward migration 在复制任一 legacy artifact 前，会以 no-follow 方式读取 live root，并按 legacy policy 完整验证其 graph；raw redirect、任一引用 artifact 腐坏、tree directory/entry symlink、不安全 destination 或 immutable target 冲突都会在 visible root 发布前 fail closed。v3 rollback 在移动当前可见根之前，必须完整验证保留的 v1/v2 根、topic shard digest/语义以及 history archive/index chain；backup 缺失或任一 artifact 腐坏时操作失败，`knowledge/knowledge-map.yaml` 的字节保持不变。恢复根先通过 legacy 目录内 create-new、no-follow 的 prepared file 完整写入并同步；live destination 必须缺失或为普通文件，symlink/reparse/non-file 会在移动 visible root 前 fail closed，随后同目录 rename 发布失败时必须尽力恢复 visible root。rollback 会把 visible root 移至 `.yaml.v3.previous`，并把普通 `.previous` reader fallback 移至 `.yaml.v3.previous.backup`；两者都为向前恢复保留，但 fallback 不得继续遮蔽已恢复的 `.knowledge` contract。`map init` 与受控 source mutation 会在 legacy/current writer lock 顺序下恢复任一移动后崩溃的未提交 rollback：先精确还原 visible roots 与 redirect，再继续迁移；当精确 legacy root 已替换 prepared file 时，该 rename 是 commit point，恢复会保留两个 rollback-retained visible root。redirect 发布的 `.redirect.prepared`、`.redirect.previous` 和缺失 live path 都是显式 crash-recovery 状态；重启 `map init` 必须幂等收敛为精确 redirect，清理残留但保留已验证的 `knowledge-map.v2.yaml`。topic shard GC 的 live set 同时包含当前根、普通 `.previous`、两个 rollback-retained visible root 以及同一 contract namespace 内的 legacy recovery manifest 引用；任一存在的 recovery manifest 无法解析时 cleanup fail closed，`.knowledge/*` 引用不得跨 namespace 授权 `knowledge/topics/*`。

Knowledge Map v2 是仓库拥有的版本化 contract，不是平台 runtime state。首次由新版执行 `map init` 或任一受控 mutation 时，单文件 `.knowledge/knowledge-map.yaml` v1 会在仓库 writer lock 下迁移为 v2 根 manifest，并创建内容寻址的 `.knowledge/topics/` 与 `.knowledge/history/`；`.knowledge/knowledge-map.yaml.previous` 保留上一代有效 root 作为恢复边界。旧版 v2 writer 会在被忽略的 `.knowledge/knowledge-map.yaml.lock` inode 中写入协议 marker，并只把该 inode 作为 OS advisory lock 目标，因此进程崩溃会自动释放 owner。在发布 canonical 或 prepared lock 之前，每个 target repository 都会先持久建立仓库自有的 `.knowledge/.gitignore` contract，其中包含限定在该目录内的 canonical 与 prepared lock pattern；已有条目会保留，普通 Git repository 与 linked worktree 使用相同的 nested contract，非 Git source directory 也无需发现 Git metadata。重新打开这两类 contract 时都会使用平台 no-follow 语义并且只接受普通文件；符号链接与 Windows reparse point 会被拒绝，而不会被跟随到自有 contract 目录之外。新 lock 会先在唯一且被忽略的 `.knowledge/knowledge-map.yaml.lock.prepared.<pid>.<startup-id>.<nonce>` staging inode 上取得独占 OS lock；随机 startup id 防止进程快速重启、PID 复用且 nonce 归零时撞上年轻残留。staging 完整写入并持久化 marker 后，再通过同目录 hard link 原子发布 canonical 路径。因此 canonical lock 只会处于不存在或 marker 完整两种状态。崩溃可能留下 staging 名称，但不会阻塞另一条唯一 staging 的发布；每次尝试对目录项执行最多 64 项的有界扫描，并且只有名称严格匹配、已超过 60 秒、通过 no-follow 打开且成功取得已释放 OS lock 的候选项才会删除。cleanup 会严格识别当前 `<pid>.<startup-id>.<nonce>` 与上一版 `<pid>.<nonce>` 两种 suffix，使早期 binary 的 crash residue 在升级后仍可回收；新 writer 只创建具备 restart uniqueness 的新格式。活跃、非普通文件、reparse、symlink 与无关名称都会保留。canonical lock 无 marker 时仍会被视为旧 binary 可能持有的 create-new lock，绝不会被新版抢占。首次启动升级 writer 前必须停止 managed 与临时旧 writer；确认已经独占仓库后，operator 才能删除由已崩溃旧进程遗留的无 marker lock。原子 marker 发布之前的 binary 若已留下 canonical 空文件或 partial 文件，它与 legacy lock 无法安全区分，仍必须采用相同的停机确认与 operator 清理流程。生成的 `.knowledge/.gitignore` 应随 Knowledge Map contract 一起提交；升级完成后，lock 排除不再依赖仓库根 `.gitignore`。回滚前必须停止全部当前 writer，并删除带 marker 的 advisory-lock inode 与所有 prepared staging 名称，使旧 create-new 协议能够获得 canonical 路径。只要任一版本 writer 仍可能存活，就不得删除任一种 lock。升级前应与其他仓库源文件一起备份或提交 `.knowledge/`。回滚到只理解 v1 的 binary 还必须恢复迁移前的单文件 map，并移走 v2 分片、归档、previous 文件与 nested ignore contract；旧 binary 不得编辑 v2 contract。正常分片清理保留上一代 root 引用并给更旧 shard 至少 60 秒宽限期。卸载 binary 或 service 不会删除任何仓库拥有的 `.knowledge/` 内容；显式清理仓库文件应使用版本控制或用户备份恢复。

包含 archive chain 但缺少 `history.index` 的早期 v2 根文件在升级后仍可用于 `map show` 和 `map route`；只读 `map validate` 必须返回 `valid=false` 并报告 missing-index 诊断，旧历史分页也以同一诊断拒绝读取，直到运行 `relay-knowledge map init` 后才恢复 valid。该命令在 repository writer lock 下流式读取旧 chain，以有界内存构建 fanout 64、最大高度 10 的内容寻址 B+ tree，先发布 immutable node，最后原子切换 root；崩溃时旧 root 保持有效，重试相同内容不会产生不同 artifact。边界 path rewrite 产生的旧 index node 与 immutable history audit artifact 采用相同 retention contract：mutation hot path 不执行无界 mark/sweep，current 与 `.previous` 恢复状态及旧内容寻址节点均保留。最坏磁盘增长按每 16 条 history 一个 archive、每次 archive append 最多 22 个小 index node 预算；operator 应通过版本控制或仓库 retention policy 统一清理已确认不再需要的旧 `.knowledge/history/` artifact，writer 存活时不得按通配符删除。

## 3. 安装体验

Installer 或安装脚本支持：版本选择、安装目录选择、dry run、校验和验证、service definition 生成、失败回滚和 uninstall plan。默认不会把数据写入 release 解压目录。

服务化部署安装体验必须显式说明拓扑：`embedded_cli` 不安装常驻服务，`resident_single_process` 安装一个平台 service，`resident_partitioned_sqlite` 还要把 shard 目录纳入备份/迁移/卸载确认。`service plan install|upgrade|rollback|uninstall --format json` 必须在 `runtime_state_paths`、`lifecycle_steps`、`rollback_steps`、`permission_requirements` 和 `warnings` 中列出主库、配置/状态/日志/缓存路径、service definition 路径、service 名称、权限要求、失败回滚计划，以及 partitioned 模式下的 shard 目录覆盖要求。`service lifecycle <action> --dry-run` 是默认可审计输出；只有显式传入 `--execute` 才能写 service definition、checkpoint 或安装目录，并调用 systemd、launchd 或 Windows Service 命令。未来 `split_worker_preview` 必须分别生成控制服务和 worker 服务定义并说明每个进程的权限、环境变量、日志和 shutdown 行为。

安装的常驻服务还必须显式说明 commit-loop policy。`RELAY_KNOWLEDGE_WATCHER_ENABLED` 同时控制源码监听和 Git HEAD reconciliation；`RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS` 默认为 `5000`。service definition、lifecycle plan 与 doctor output 必须保留或解释这些值，不能把只对安装 shell 生效的 export 当作安装配置。reconciler 在平台 service manager 下执行受界周期检查并提交 durable code-index task；installer 不得额外安装仓库专用 Git hook 或 unmanaged polling process。

Web Knowledge Map 请求必须显式指定已注册仓库。安装后的服务从托管仓库状态解析该身份，绝不把进程工作目录当作隐式仓库。从早期 cwd 绑定行为升级时，需要先注册 Web workspace 将查询的每个仓库；无需移动仓库文件，rollback 只恢复旧版选择行为。

包含 Web 静态资源的发行包必须把 Software 页面与同 tag 的 schema-version-6 binary 一起发布，不能把新前端与旧 software API 混装。页面只列出已有完成 scope 的托管仓库，不新增运行目录、credential 或后台进程；升级后在 projection 重建完成前会如实显示 stale/degraded，binary-only rollback 可恢复旧前端但不会逆转已经完成的新 projection migration。

实现必须通过明确所有权保持该合同可审计：生命周期步骤策略留在 `application::service::lifecycle_plan`，服务定义渲染、平台权限以及 systemd/launchd/Windows Service 命令统一放在 `lifecycle_plan::platform_service`。CLI 与 resident-service bootstrap 只捕获一次当前 executable path，并通过 typed `ProcessRuntimeConfig` 注入；preflight、copy、upgrade、rollback 与 checkpoint recovery 必须复用该值，不能重新查找进程状态。修改任一边界都必须维持所有支持平台一致的 dry-run 计划与执行合同。

精确代码源码兜底由产品内部实现，运行时不能依赖 `rg`。面向 agent 的 setup 说明可以提到使用有界 `rg` 或 `grep -RIn` 做人工检查工具，但安装器不能把递归 grep 作为 service 依赖，也不能把它当成已索引查询行为的替代品。

## 4. 运行时状态

配置、数据库、索引、日志、缓存、临时文件和 dead-letter 数据写入 `paths` 管理的平台目录。升级时必须保留 runtime state，并显式执行 schema/index migration。早期数据库的 `code_repository_schema_migrations` 可能只有 `name` 列；schema 初始化必须先幂等增加 `applied_at_ms INTEGER NOT NULL DEFAULT 0`，再运行任何会写 capability marker 的 retention、search-owner 或其他迁移，不能要求 operator 重建数据库或手工补列。
Code-search ownership v2 升级不会在同步 database open 期间重写 legacy FTS 数据。Startup 安装 non-replacing writer 与 exact metadata serving gate，以 `search-owner-v2-writer-and-serving-gate` 一次性把既有 scope 及其 active repository 标 stale，并把 source-scope identity 推进到 `search-owner-v2` fact component。该 marker 只证明 writer 与 serving boundary 已安装，不认证旧 FTS row 或 imported FTS row。每个 FTS `MATCH` read 都要求 metadata ownership 的 rowid/scope/kind/record/path 精确匹配；随后由普通 durable full-index task 复用既有 lease、checkpoint 与 publication fence 替换 stale scope。Database import 只有在 attached source 具有该 marker、完整 search/metadata schema shape、每个 indexed metadata row 都按 rowid 与完整 identity JOIN 到一个 FTS row，并且 fact-versioned Git scope 的 identity 与 imported repository、tree、filters 和当前 fact version 匹配时，才能保留 search freshness。Import 与 incremental clone 以 indexed metadata owner 表为枚举权威，只复制这些 JOIN row；绝不通过 FTS 的 `UNINDEXED` scope/kind 列做反向 COUNT。没有 metadata 的 raw FTS row 不复制、不服务，并保留给受界 `search_orphans` GC。Metadata-side orphan、duplicate owner identity 或 affected-count mismatch 必须让 repository metadata、facts、已复制 search row 与 scope publication 一起回滚。缺少该 capability 的 legacy import 可以保留 base facts 以便恢复，但不得复制 search row，并且必须用 full-reindex 原因持久化为 stale；owner exact 但 fact-version identity 过旧的 import 同样必须显式 stale。Manual/custom 非 fact scope 继续遵守既有兼容合同。Upgrade 与 doctor output 不能仅因 database open、marker 创建或 base-fact import 完成就把 search ownership 报告为 fresh。

Framework graph support 是增量 derived-state migration。Schema initialization 会幂等创建 framework-node/framework-edge table 及其受界 lookup index，然后记录 `framework-graph-reindex-v1` 并把 capability 之前的 code scope 及 active repository 标为 stale。Database open 不会从 legacy chunk 同步合成 Angular/Vue fact。在要求 fresh `repo framework`、Web framework-graph 或 `relay_code_framework` 结果前，operator 必须执行普通 durable full `repo index`；既有 lease、checkpoint、retry 与 publication fence 仍是权威边界。只有 source schema 包含这些 table/index 且携带该 migration capability 时，import 才保留 framework freshness；否则 imported base fact 可保留，但 scope 必须显式 stale。Binary-only rollback 可保留这些增量 table；精确回滚则恢复升级前事务一致的 database 与 shard 集合。
Partitioned incremental-base handoff 会在 copy transaction 内重新验证 attached control scope。Stale 或 retiring scope、durable GC job，或无法证明 retirement state 的 legacy schema 都必须在 target metadata 改变前拒绝 handoff；operator 必须执行 full index，不能 clone partial retirement state。
commit-loop retention 属于 runtime state。每次发布保留 active 与最近两个成功发布时间窗口的并集（窗口通常已包含 active）、最近一次成功增量的 predecessor、active worktree overlay 的 clean base，再加未完成 task 的 target/base 和 repository-set pin。SQLite 会幂等增加 `retiring` scope state 与 durable GC job：逻辑退役保持原子，后续每个 maintenance transaction 推进一个 scope-GC phase，该 phase 在受影响的应用表之间合计最多删除 512 个物理行，包括旧 facts、code FTS/search row、software projection、checkpoint、workspace state 或 scope metadata；task-audit 与 commit-alias 的独立配额使每个 pass 的主清理合计最多 2,048 个物理行，另加最多一个终态 GC-job bookkeeping 行。同 tree commit 复用内容图，并使用每仓 256 条的 commit alias 窗口。完成态 task history 限制为每仓库 128 条 succeeded 和 64 条 failed/dead-letter/cancelled，同时为每个 retained scope 保留最新 success。升级后会恢复持久 GC job；旧 binary 不理解 retirement state，不能与新 binary 共用 database，也不能从 task row 重建已淘汰 scope。GC 会限制 live generation 并让 SQLite 复用释放页，但不承诺数据库文件立即在 OS 层缩小；回收物理 high-water mark 需要另行执行显式、有界 compaction。

GC job schema 会增量增加 nullable `search_rowid_cursor` 与独立的 `scope-gc-search-orphans-phase-v1` capability marker。Marker-v6 fast-path validation 同时要求 cursor 列、exact search-owner marker 与该 phase marker，因此即使增量列和旧 global marker 已在一次失败的 initialization 中持久化，下次打开仍会重试；该检查不会重建无关 BM25/code fact。一次性 phase migration 保留处于 `search_documents` 或更早阶段的 job，但会把已越过新增阶段的遗留 job rewind 到 `search_orphans` 并清空 cursor，同时保留 deleted-row 计数、时间戳与错误；后续 phase 都是幂等的。Phase 更新与 capability marker 原子提交；marker 写入后，已经完成 orphan cleanup 的当前 job 绝不会再次 rewind。升级、备份和回滚必须把两个 marker 与 cursor 同数据库及 WAL/SHM 文件一起保留；旧 binary 不识别该 phase checkpoint，不得并发推进同一 job。
repository-set 迁移会增加持久 refresh-task queue 及其 claim/capacity/audit 索引，overlay selector 迁移还会在 SQLite schema 初始化时幂等增加虚拟 origin-path 列和 origin/target 复合索引。升级后 managed service 会恢复可执行的 refresh task。手动 refresh 在所有 member 间共享 4,096 个 chunk、16 MiB 和 32,768 个 derived item 的 manifest 预算。升级计划必须为一次性索引构建预留时间；回滚应保留增量 queue table、列与索引，不得删除或重建 overlay facts。遗留手动 overlay 超过 8,192 条 edge 时，系统会在无界删除前保持数据不变并拒绝；整仓删除遇到超过 64 个受影响 set 或任一受影响超限 overlay 时也会原子拒绝。当前版本没有有界 repair 命令，operator 需要后续升级提供 repair tool，不能假设 migration 已闭环该清理路径。这些手动 set 上限不适用于显式启用的 automatic-workspace cross-edge builder；scope GC 会限制过期状态删除，但尚未限制该路径的单次 build。

SQLite graph-store schema marker v4 是一次明确的 forward derived-retrieval-state migration。它以包含 scope64 partition token 与 scope-qualified group token 的 indexed、zero-weight `routing_key` 重建 global `graph_bm25` FTS5 table，并新增 route state/document/group/term tables 与持久 global route-term document frequency。Route document 保存 document identity/kind/scope/path、`created_graph_version`、可观测 `label_gram_state`、group token、有界 term-count JSON，以及 `fts_rowid NOT NULL UNIQUE` sidecar。权威 evidence、graph facts、code symbols 与 code chunks 不变，因此所有 v4 retrieval structure 都能从这些 source 重建。

当前 document-write transaction 会一起更新 `routing_key`、route sidecar、route-state document count、每组 collection frequency 与持久 global document frequency。Fresh-open reconciliation 检查 schema、`simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4` route fingerprint、freshness/version state、持久 semantic/vector generation marker，以及 authoritative/active-global/route-document/group/semantic/vector/state population count；它不会在每次 open 时做无界 identity、逐行 tokenizer 或 aggregate 深扫。Canonical identity 与 tokenizer consistency 只在其他 stale/schema/count 条件已经触发重建后校验；仅有 equal-count per-row drift 不会在 open 时触发 rebuild。

重建会取得带 owner/expiry 的 durable lease，并连同 phase/cursor checkpoint 与固定 semantic/vector rebuild plan 发布 `building`，创建 `graph_bm25_rebuild`；旧 attempt 过期后可接管并从持久 checkpoint 续跑。每个 transaction 最多接纳 128 篇文档、4 MiB 估算权威 source bytes、8,192 个 labels 和 8,192 个 links。单篇文档若超过一个或多个工作预算，会独占 transaction 并发出 identity 受界的 warning；该行为保证前进，不代表单文档绝对 byte bound。旧的 flat `graph_bm25` 保持可读，semantic、vector 与 fuzzy lexical fallback 在 `building` 期间暂停，之后以有界 rowid keyset cleanup 删除 stale label/semantic/vector row。当前 evidence/code writer 使用 `IMMEDIATE` transaction，并在 rebuild 活跃时拒绝写入。完整性校验通过后，一个短事务把 active `graph_bm25` 改名为 `graph_bm25_retired`、提升 shadow、把 route state 发布为 `fresh`，并记录 schema marker v4。Retired table 只在提交后删除，因此 crash 或 rollback 不会发布 partial FTS generation。升级计划必须为 active 与 shadow FTS 同时存在、sidecar、WAL 和短暂 retired cleanup 预留时间及临时磁盘余量。

Schema marker v6 是一次增量式 code-index publication migration。数据库首次打开时，初始化会幂等创建 `code_repository_publication_receipts` 及 repository/scope lookup index，即使该数据库此前已经处于 marker v5。receipt 记录 durable task、repository、target scope、发布 fence generation 与发布时间，并通过 `ON DELETE CASCADE` 随 task 一起删除。receipt 是收敛与审计证明，不是权威 code/software fact；因此升级继续要求独占 writer，但不需要重建源码索引。旧 binary 可以忽略该增量表，但 binary rollback 不会撤销新版已经发布的 scope；精确 runtime rollback 仍需恢复升级前 database checkpoint。

Schema marker v7 为 `entities` 增加 scoped ontology identity 列，并幂等创建 `business_domains`、`business_terms`、`business_term_aliases`、`business_mappings` 与 `business_knowledge_status`。这些表是从版本控制下 glossary 派生的 projection；升级不改写旧 label-only entity id，它们继续标记为 `untyped`。旧 scope 没有可证明 fresh 的业务投影，必须通过正常 durable `repo index`/`repo update`、同一 lease 和 publication fence 重建后才能重新发布或同树复用。备份必须包含主库、WAL/SHM 与所有 repository shard；binary-only rollback 可忽略新增表，但无法恢复被新版 supersede 或 retention 淘汰的旧投影，精确回滚必须恢复升级前事务一致的整个 runtime state。仓库拥有的 `knowledge/glossary/business-glossary.yaml` 与 map topic shard 不在 runtime database backup 内，应通过 Git 或单独仓库备份恢复。

Software projection schema version 6 是独立的 derived-state compatibility migration，不是 graph-store v4 marker。它保留 version 5 对重复 lockfile derived component 的合并语义，并幂等增加 `ontology_version`、`source_coverage_json`、provenance completeness、freshness、conflict/entity/statement/diagnostic count，以及 `software_entities`、`software_statements`、`software_ontology_diagnostics`。遗留 v1 `publish` checkpoint 映射到新增 ontology phase，不能提前返回 success。

Projection schema version 7 保留上述表，并推进派生分类：常规 JSON/YAML OpenAPI 与 Swagger 文件会物化为 `api_schema` file/API entity。Schema 初始化把所有低于 7 的状态标为 stale、把记录版本推进到 7，并在没有既有错误时记录 `software global projection schema changed; refresh required`。它不会改写权威 code fact，也不能宣称 projection refresh 已完成；受影响 scope 必须经过正常 fenced software-projection v2 的 reset、dependencies、SDK usages、lifecycle、files、topics、relationships、ontology、publish 九阶段重建后才能恢复 fresh。Upgrade 与 doctor 输出必须区分“schema migration 完成”“API-schema projection 已重建”和“software projection 已 fresh”。回滚到只理解 version 6 的 binary 时必须把较新的派生 row 视为不兼容并重建自己的 projection，不能编辑权威 code fact。

SQLite graph-store schema marker v8 证明 software ontology schema 与 scope-GC phase contract 已安装。升级会幂等创建 occurrence/statement/diagnostic 表及索引，并记录 `scope-gc-software-ontology-phase-v1`；已经越过新增 ontology 清理阶段的 legacy GC job 会在一次原子迁移中回退到 `software_entities`，保留累计删除数、时间和错误，再按每事务现有 512 行总预算恢复。二进制回滚可以忽略新增表，但旧 writer 不得与 v8 writer 或正在推进的 v2 software checkpoint/GC job 并发共享数据库；精确回滚必须恢复升级前事务一致的主库、WAL/SHM 和全部 shard。SPDX、CycloneDX 与 PROV-O export 是只读派生接口，不创建独立 runtime state，也不需要额外卸载步骤。

Partitioned control catalog 还会在 `IMMEDIATE` transaction 内执行幂等增量迁移：为 `storage_repository_shard_scopes` 增加 `state TEXT NOT NULL DEFAULT 'active'` 与 nullable `staged_task_id`。因此遗留 route 继续是无 staged owner 的 active route，新 publication attempt 则可保持 task-owned staged 且对 active-only routing 不可见，直到 control transaction 激活；该迁移不改写 shard fact。Binary-only rollback 会保留这些列，但不执行 active-only routing 的旧 writer 不得在仍有 staged route 时运行；精确回滚必须一起恢复升级前 control database 与全部 shard。

Query hot path 读取持久 version/count/DF，不运行 full-table `COUNT` 或 `SUM`。对每个实际 query term，它会把持久 global DF 与仅限定 business column、最多探测 `df + 1` 行的 `MATCH` 对比；每个 term 都必须不超过 corpus 的 20%，所有探测合计最多预留 65,536 个 postings。Scoped FTS 还会与 scope64 routing token 求交，普通 SQL scope predicate 仍独立承担硬授权。Single-FTS reader 通过 hidden rank column 排序有界 identity window，再经 `fts_rowid` sidecar hydrate；跨越该 window cutoff 的完全同分不承诺确定 membership。Historical unscoped fallback 的 authorized-corpus、label-state 与 `label_lower` probe 使用 version-leading global indexes，scoped index 继续保留。一次完整 graph search 共用一个 deferred read transaction，因此并发 FTS activation 不会拆分其 SQLite snapshot。不能仅因表已存在就报告 routing 为 fresh。

虽然 v4 scorer 把 `routing_key` weight 设为零，FTS5 仍把该列计入 document length 与 corpus average document length，因此 v4 的数值 BM25 baseline 可能不同于 v3。支持的 parity invariant 仅是：同一个 v4 table 上 routed 与 flat 执行的公共文档具有 bitwise-identical score。

既有 v1.1.13 时期的 code index 可能包含已裁掉首尾空白的 Markdown source window；一次性 code-index migration 会在同一原子事务内把含 Markdown 的 scope 标为 stale 并记录 migration marker，但未持久化的字节无法由 database schema migration 恢复。repository graph 物化还会按 indexed file length 校验连续 chunk byte range，并对受影响 scope 返回明确的 lossless/re-index 错误。该 scope 首次使用 `repo graph` 前，operator 必须显式执行完整 `repo index`；incremental `repo update` 会拒绝 stale base，不能完成这项恢复。Markdown window 通过正常的 durable task、single-writer lease、checkpoint、有界重试与 freshness 发布流程重建。安装或替换二进制不能仅因 schema initialization 成功就宣称这项数据刷新已经完成。

本地文件定位索引的 SQLite/FTS5 状态也写入同一运行态数据区。安装器和 service
template 不能默认扫描全盘、Linux `/opt`、挂载盘或 Windows 非系统盘；只有用户显式配置
或通过 CLI 传入这些 root 时才建立索引。

当启用 `RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite` 时，主数据库仍保存控制状态，每个代码仓库的 shard 数据库位于运行时数据目录的 `stores/repositories/` 下。备份、迁移、doctor、卸载确认和回滚计划必须把主数据库与 shard 目录视为同一个 runtime state 集合；不能只移动或校验主数据库后宣称升级成功。
shard catalog 路由是可迁移的，恢复时会基于当前 runtime data 目录重新解析；但这只有在 shard 目录随主数据库一起移动时才成立。

安装或升级后的 partitioned code-index mutation 同样必须走 managed durable-task path。Snapshot、checkpoint begin/resume、batch publication、finalization 与 workspace cleanup 会在创建或修改 shard 之前拒绝 unfenced call，并要求调用方先 queue、claim，再携带当前 publication fence。这是 runtime contract，不是 schema rewrite：既有 terminal task history 与 fence 审计 row 继续有效；此前直接调用 partitioned storage 的 service/CLI integration 必须迁移到生产使用的同一 leased-worker workflow。`single_sqlite` 只为 fresh 且通过 budget admission 的 direct full snapshot 与显式 unfenced compatibility call 保留 direct 兼容。所有带 fence 的 clean incremental task 无论大小都走同一 durable clone/finalization 协议；无法进入该协议的 worktree snapshot 会可观察地零写失败。Admission 只使用 `source_scope` 前导 owner index 与 metadata-to-FTS rowid lookup，绝不反向扫描 raw FTS。Base 缺少新的非零 fact proof 时会在 schema/fact/search mutation 前返回类型化 `DurableStagingRequired`，application 随后转入 full staged task。Partitioned control import 仅同步 metadata（一个 repository row 及 canonical alias），且 staged shard route 会在第一次 clone/checkpoint mutation 前持久化，确保 backup/reopen 后可以定位并 resume。没有 owner 的 raw FTS row 保持隔离并交给 retention GC；repository removal 的兼容路径继续保留 legacy raw fallback，直到独立的 global orphan lifecycle 取代它。

未来外部 graph/vector/storage 后端或复制 SQLite 后端也属于 runtime state。安装器、doctor 和升级计划必须记录后端类型、endpoint 或本地目录、认证配置来源、schema/index migration 状态和回滚说明；不能只替换二进制后宣称数据面升级完成。

Query-index finalization plan 当前为 version 3，共 17 个稳定 slot；ordinal 或 repair cursor 继续写入既有 checkpoint `state`，不增加 schema column 或 marker。Version 2 曾向冻结的 v1 plan 追加 unit 16。Version 3 保持每个 name/owner/column ordinal 不变，只退役 unit 1 `code_repository_symbols_lookup` 的创建动作：既不新建也不自动删除。若同名索引存在，每次 open 与 publication path 仍严格校验 exact legacy shape；若缺失，只有 v3 cursor 或 current coarse scan 才视为 complete。规范 v1/v2 subphase token 及 v2 普通/reference-search repair token 继续可读，并在 writer quantum 之间保留原 version；legacy token 若没有物理 legacy unit 1，就不能越过该 ordinal。Current formatter 输出 v3 token。每个 retained 非终态 coarse token 都重新校验 current plan，每事务最多修复一个缺失 required descriptor；显式稳定 resume code 跨 reopen 保存 original phase。Software/partitioned-publication repair 保持在 finalization 并跳过 parsing，partitioned public projection 不能把它报告为 completed。历史终态 `completed` checkpoint 保持不变，自动终态修复必须另行调度 durable leased migration。

每次 database open 只对已经存在的 index 做只读 exact-shape preflight。每个 fresh `Restart` 无论 path count 或后续 byte/row 分批如何，都仅在完整 chunks owner 为空时预建 unit 13/14；owner 已 populated 时两者都延后。所有 resume session 与其他 descriptor 都交给逐 unit durable finalization。Direct snapshot/import writer 在 fact mutation 前预建 required empty-owner index，随后要求所有 required slot；populated owner 缺 required index 时 fail closed，旧 active scope 不变。Upgrade、rollback、backup 与 doctor 必须原样保留 v1/v2/v3 checkpoint text，保留既有 retired unit 1 而不得删除，也不能把 database open 成功当作全部 required query index 已完成的证明。

当前 schema 新增 exact-shape 的 `code_repository_reference_search_progress`、group owner 与 grouped manifest 表。Upgrade/open 只执行结构性工作：空的畸形 derived owner 会在事务内重建，非空不兼容数据 fail closed；绝不扫描 runtime fact 或 raw FTS 推断完成状态。规范 grouped-v2 cleanup/discover/build token 必须有匹配 progress row 与精确冻结 totals。保留的 v1 cleanup/build state 以 projection version 1 迁移，并且只允许在 exact live lease/fence 下与 checkpoint 同事务重置到 v2 cleanup page zero；重置前必须从 durable task budget 重新派生更严格的 row/byte limit。Task 未完成时，安装器与 rollback 流程必须把 database、WAL/SHM recovery file、checkpoint、progress、group 与 manifest 作为整体保留。若要回滚到不识别 grouped v2 的旧 binary，必须先由当前 binary drain，或显式取消并重启 leased task；不得手工删除 owner row 或改写 token。Publication 与 reconcile 会要求 manifest reference count 匹配 scope；缺少 v2 manifest 的遗留 coarse/post-publication state 会 fail closed，并由新的 fact-versioned full task 恢复，而不会被改标为 fresh。

Durable incremental clone 还新增 exact-shape clone progress/affected-path owner，以及两个 checkpoint 列：`committed_fact_row_count INTEGER NOT NULL DEFAULT 0` 与 nullable `incremental_summary_json TEXT`。初始化必须先 ensure 两个 checkpoint 列，再校验或修复 active clone owner；因此 additive DDL 与 marker validation 之间升级中断时，下次打开仍会安全重试。新 full batch 在同一个 checkpoint transaction 中维护实际累计 fact proof，Maven effective-dependency refresh 的精确 delete/insert delta 也必须同步调整它。Legacy 或 attached checkpoint 导入时 proof 为零、receipt 为 null；部分升级 session 的零 proof 会一直保持零，并让后续增量在 target 写入前转入 full staging。Null receipt 表示 generic summary recovery，并不表示 facts 缺失。非空 canonical receipt 受 checkpoint budget 约束且只属于一个 task；该 task 完成前必须与 checkpoint/progress/WAL 一起保留，后续同内容 task 只能在 fenced terminal adoption 中清除它。若要做精确 binary rollback，必须先由当前 binary drain 或取消 active clone/finalizer，并恢复 transaction-consistent runtime database。Operator 不得伪造 proof、跨 task 复制 receipt、删除 clone progress，或手工改写 `indexing`/`finalizing:*` token。

Partitioned upgrade recovery 不能把 receipt 存在本身当成 eligibility 证明。若 inactive staged 遗留 raw `completed` checkpoint 缺少 current descriptor，只允许其 exact leased task 在 raw-token CAS、live fence 与 catalog staged-owner 校验下把它重新打开为 raw partitioned publication；active 历史 `completed` 不得由该路径重新打开。

若 rollback 目标 binary 不识别 query-plan v3，必须先由 current binary 把 checkpoint drain 到该旧版可识别的 coarse/terminal state，或显式取消并重启 leased task；不得把 v3 token 手工改写成 v2，也不得删除 checkpoint 强制回滚。

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

Plan rendering 与 execution 必须使用 bootstrap 捕获的精确 source executable。Preflight、binary copy、upgrade、rollback 与 checkpoint recovery 必须复用该 typed process input；不得稍后调用 `current_exe` 而选择到另一个 binary。

停止 ad hoc CLI writer 并创建事务一致的 runtime-database backup 是 operator precondition。Lifecycle 成功停止 managed service 后，结合不存在 ad hoc writer，才建立 migration 所需的 database 独占访问。Lifecycle executor 不会独立探测 exclusive access，也不会创建 runtime-database checkpoint；它的 rollback checkpoint 只覆盖 binary 和 service definition。如果 operator 要求独立的 exclusive-access 检查，必须用外部 maintenance procedure 分阶段执行文档化步骤，不能把一次性 `--execute` 当作该验证。

失败时 lifecycle executor 回滚 binary 和 service definition。Database rollback 使用 operator 创建的 runtime checkpoint；如果没有该 checkpoint，v4 derived-index migration 将按下文说明保持 forward-only。

首次启用 commit reconciliation 的升级应先停止旧 service，备份完整 runtime state set，再以期望的 watcher switch/interval 安装新 service definition；新服务启动时先恢复 lease，再对账 HEAD。升级后必须用 `service status --format json` 检查 watcher state、`total_commit_reconciliations`、`total_commit_tasks_queued`、`total_commit_reconcile_failures`、code-index queue/lease 与 retention。回滚只恢复上一版 binary/service 配置，不会恢复成功发布后已淘汰的 scope；精确恢复历史 scope 需要 runtime database backup，或从源码仓库重新 full index。

只把 binary 回滚到 pre-v4 release 并不会撤销 forward derived-index migration。旧二进制可以忽略 routing sidecar 并使用既有 flat query path，但保留的 v4 `graph_bm25` table 与 v3 index 在数值上并不等价。旧二进制若写入 derived document，不会一致维护 `routing_key` 与 v4 sidecar，此后必须把全部 hierarchical metadata 视为 stale。旧 writer 还会写回旧 schema marker，因此后来启动 v4 时，即使 route state 表面兼容，也会显式将其 invalid，再从权威 document 重建 `routing_key` 与 sidecar，完成后才能重新启用 routing。精确恢复旧 scoring baseline 需要还原 pre-v4 runtime-database checkpoint，而不只是换回旧 executable。v4 的 `IMMEDIATE` 应用 fence 不是跨版本 lock protocol：已经运行的旧 binary 不检查 `building`，可以绕过该 fence 写入。权威 facts 才是 recovery boundary；不能把 route metadata 当作用户数据的唯一副本，upgrade、rebuild 与 rollback 都必须独占 database，不能让新旧 writer 并发写入。

`service lifecycle upgrade --execute` 按现有 dry-run 阶段执行：记录 binary/service-definition rollback checkpoint、停止 managed service、按需复制 binary、写 service definition、刷新平台 service manager、启动 service，并在 post-upgrade doctor 前后保留执行报告。它没有独立的 exclusive-access 验证、runtime-database checkpoint 或 migration/rebuild 阶段。调用前，operator 必须停止 ad hoc CLI writer，并创建必要的事务一致 runtime-database checkpoint；service manager 无法 fence 独立运行的旧进程，lifecycle checkpoint 也不覆盖 runtime data。该命令要求 managed-service stop 步骤成功，但不会另行验证独占性。Platform service manager 启动新 binary 后，binary 会在首次同步打开 database 时执行当前 migration，包括需要的 graph-store v4 shadow rebuild、software-projection v5 stale invalidation、partitioned catalog publication columns 与增量式 marker-v6 publication-receipt 初始化；该次打开完成前 service 不可用，但 open 完成不等于 stale software projection 已恢复 fresh。Linux systemd unit 必须引用包含空格的路径，并把字面 `$` 转义为 `$$`。install 写入显式 `--install-dir` 前不得覆盖已有目标二进制或 service definition；Windows install 必须把 service 创建和 registry/environment 写入拆成可单独回滚的步骤。upgrade 必须 checkpoint 已有目标二进制和 service definition，checkpoint backup 必须使用 attempt-scoped 文件并通过原子 checkpoint 发布成为当前回滚依据；没有旧备份时失败回滚和显式 rollback 只能删除本次确实复制或写入的目标文件，definition-only upgrade 不得删除当前运行的 binary。Windows 和 macOS upgrade 必须在启动前刷新 platform service registration，使 SCM `BinaryPathName` 或 launchd loaded job 与更新后的 service definition 一致。uninstall 失败回滚和基于 uninstall checkpoint 的显式 rollback 如果需要恢复已删除的 service definition，必须从 uninstall 前 checkpoint 恢复原 definition，再重新注册 service manager。文件或 service manager 状态变化后任一阶段失败时，必须按 `rollback_steps` 尝试恢复已完成步骤；restore、definition rewrite、unregister 或 service-registration rollback step 失败后不得继续执行依赖的删除、reload/start 步骤；任何此类状态变化前失败时，不得停止、disable、恢复、重启或卸载既有 service。只有选中的 rollback steps 全部成功时，执行报告才能把 rollback 标为完成；外部 service manager 或 doctor 子进程必须有有界执行时间，并在等待退出和超时期间持续读取 stdout/stderr，进程退出或超时后的输出收集也必须有边界。`--execute` 出现失败 step 时，API/CLI 操作必须返回错误并带出失败 step id，不能把失败报告包装成成功响应。`service lifecycle rollback --execute` 使用 checkpoint 备份恢复二进制和 service definition，不恢复 runtime database；没有 lifecycle checkpoint 时必须把缺口暴露在 warnings 或执行错误中，不能静默宣称回滚成功。

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
- Linux GNU release 二进制和 skill Linux x64 内置 asset 不得依赖高于 2.28 的 `GLIBC_*` 符号；x64 与 ARM64 产物必须都在经校验的 glibc 2.28 容器内完成启动 smoke test。
- GitHub Release 将 CLI skill archive 纳入 `checksums.txt`，archive 内含 skill `README.md`、`references/knowledge-map.schema.json`、`references/business-glossary.schema.json`、Linux x64 和 Windows x64 asset 二进制；启用 ClawHub 发布时使用同一个 crate 版本、agent 指令和 schema reference，并禁止包含达到或超过单文件 10 MB 限制的文件。
- CLI 能说明稳定新版本可用，JSON 输出保持机器可读且普通命令不会自动安装新版。
- 面向 release 的文档有带日期的 `06-verification` 审计，覆盖导航、清单、链接检查和 documentation-only 改动边界。
- Release workflow 必须运行 `python3 tools/docs/check_docs.py`，拦截文档结构、本地链接与 anchor、卷首导航、章节编号、代码块语言标签和英文版翻译卫生回归。
- service install 使用 systemd、launchd 或 Windows Service，而非 unmanaged loop。
- `service lifecycle <action> --dry-run` 输出 service 名称、definition 路径、安装目录、运行时路径、权限要求、rollback 计划和 package manifest 校验链路；其 preflight/copy/checkpoint 步骤使用 bootstrap 捕获的 executable path；`--execute` 只在显式请求时运行，并在失败时执行 rollback steps 且返回操作错误。
- uninstall 清理服务注册和服务定义，但保留或按用户确认处理 runtime data。
- 卸载 service 会停止 commit reconciliation；保留 runtime data 也会保留 active/recent scope、protected pin、有界 task history 与后续 full-reindex 能力。显式删除数据时必须覆盖每个 code shard，且没有备份时不能宣称可逆。
- 分片 SQLite 拓扑的 shard 目录参与 backup、migration、doctor 和 uninstall 确认。
- SQLite graph-store upgrade 能识别 schema marker v4，在旧 flat FTS 保持可读时通过可接管续跑的 phase/cursor checkpoint 与有界 document/source-byte/label/link batch，从权威 facts 重建 `graph_bm25_rebuild` 及 rowid/version/label-state sidecar，在 `building` 期间暂停 semantic/vector/fuzzy companion reads，原子激活 route/FTS/marker state，为 rebuild 预留时间/WAL/磁盘，并暴露 v3-to-v4 score-baseline 变化。旧 binary 不遵守应用 fence，因此 upgrade 必须独占访问；binary-only rollback 保留 flat path 但不提供数值 v3 equivalence，精确评分回滚需要恢复 pre-v4 database checkpoint。
- SQLite schema marker v6 会为从 v5 升级的数据库幂等创建 publication-receipt table 与 lookup index，不重建权威 code/software facts；receipt 随 task retention/removal 级联清理，并继续受 runtime-database backup 与独占 writer 升级合同保护。
- SQLite schema marker v7 会幂等增加 scoped ontology identity 与业务投影表，保留旧 label-only entity id 为 `untyped`，并要求 legacy scope 经 durable fenced re-index 获得 fresh business projection；精确回滚同时恢复 runtime database/shard 与 Git 管理的 glossary/map artifact。
- Software projection schema v5 会把所有旧 projection status 标为 stale、记录 refresh-required diagnostic、保留 declared 与权威 dependency/SBOM 证据、重建仓库级 locked component 坐标，并在正常 fenced refresh 完成前禁止宣称 fresh。
- Partitioned catalog migration 会在一个 control-database transaction 中幂等增加默认 active 的 route state 与 nullable staged-task owner；backup 与精确回滚必须把 control database 和全部 shard 作为整体。
- 控制服务和 split worker 的服务定义、运行时目录、日志、环境变量和权限边界在 plan/install/uninstall 中可诊断、可回滚。
- 安装后的 Web 服务必须通过显式的托管仓库别名与持久化仓库根目录解析 Knowledge Map 操作；服务行为不得依赖 service manager 设置的进程工作目录。
- Release workflow 或等价门禁必须运行 service lifecycle dry-run smoke，验证发布二进制生成的 service definition、rollback plan 和 package manifest 检查不会与 release tag 漂移。

---

导航: 上一章: [18. 可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md) | 下一章: [20. 多仓库代码图谱薄覆盖层](20-multi-repository-code-graph-overlay.md)
