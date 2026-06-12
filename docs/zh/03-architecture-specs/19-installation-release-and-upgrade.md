# 安装、发布与升级

[中文](../../zh/03-architecture-specs/19-installation-release-and-upgrade.md) | [English](../../en/03-architecture-specs/19-installation-release-and-upgrade.md)

> 文档版本: 2.8
> 编制日期: 2026-06-10
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

安装和发布是产品架构的一部分。稳定版本必须可验证、可回滚、可卸载、可诊断；二进制安装路径和运行时状态必须分离；后台服务必须交给平台 service manager 管理。

## 2. 发布渠道

- GitHub Releases 发布跨平台预构建压缩包、checksums 和 release notes。
- crates.io 保持 `cargo install relay-knowledge` 可用。
- Homebrew、Scoop、winget 或发行版包应引用同一 release tag 产物，不重建分叉快照。
- Release tag 使用 `vX.Y.Z`、`X.Y.Z` 或 `vX.Y.Z-rc.1` 这类 prerelease 形式；数字版本必须在推送 tag 前与 `Cargo.toml` 和 `Cargo.lock` 保持一致。手动 dry-run dispatch 复用同一版本契约，但不会发布 crates.io 或 GitHub release 产物；workflow 默认 dry-run tag 必须随每次 release 版本提升同步更新。
- v1.1.11 release 准备将 `Cargo.toml`、`Cargo.lock`、CLI skill metadata 和 release workflow dry-run 默认值统一固定到 `1.1.11`；发布仍由 tag 驱动，只有推送 `v1.1.11` 或 `1.1.11` 到 GitHub 后才会开始。
- macOS x64 release job 必须使用仍可用的 Intel runner label，例如 `macos-15-intel`，不能继续依赖已退休的 `macos-13` 镜像。Artifact upload/download 和 attestation action 必须保持在兼容 Node 24 的版本，确保 GitHub-hosted runner runtime 迁移后 release workflow 仍可运行。
- Linux GNU release job 必须在 glibc 2.31 baseline 上构建 `x86_64-unknown-linux-gnu` 和 `aarch64-unknown-linux-gnu` 产物；如果产出的 ELF 需要任何高于 2.31 的 `GLIBC_*` 符号，release 必须失败。CLI skill 内置的 Linux x64 asset 打包后也必须通过同一 ABI 检查。
- Release archive attestation 使用生成的 `checksums.txt` 作为 subject manifest，使 GitHub artifact attestation 覆盖用户本地校验的同一批 archive digest。
- CLI 新版本发现使用可配置双源：GitHub Releases 和 crates.io。检测必须走 `env`、`paths`、`net::http` 边界，继承代理、TLS、timeout 和 runtime cache 策略；普通命令只能提示稳定新版，不能静默替换二进制。
- GitHub Releases 包含从 `skills/relay-knowledge-cli` 构建的 `relay-knowledge-cli-skill-<tag>.tar.gz` skill 产物；其版本跟随 `Cargo.toml`，并会以数字 semver 写入生成后的 `SKILL.md` metadata。skill 产物包含根目录 `README.md`，并在 `assets/` 下内置 Linux x64 和 Windows x64 二进制，要求 agent 在匹配平台的内置二进制通过 `version --format json` 校验时优先使用它。只有内置二进制不可用、宿主 Linux glibc 低于内置 asset baseline，或用户明确要求系统安装版本时，agent 才回退到 `PATH`。配置 `CLAWHUB_TOKEN` 时，release workflow 还可以用 `clawhub publish` 把同一个生成后的 skill 布局发布到 ClawHub。该 skill-over-CLI 产物与 MCP 协议打包分离。

## 3. 安装体验

Installer 或安装脚本支持：版本选择、安装目录选择、dry run、校验和验证、service definition 生成、失败回滚和 uninstall plan。默认不会把数据写入 release 解压目录。

服务化部署安装体验必须显式说明拓扑：`embedded_cli` 不安装常驻服务，`resident_single_process` 安装一个平台 service，`resident_partitioned_sqlite` 还要把 shard 目录纳入备份/迁移/卸载确认。`service plan install|upgrade|rollback|uninstall --format json` 必须在 `runtime_state_paths`、`lifecycle_steps`、`rollback_steps`、`permission_requirements` 和 `warnings` 中列出主库、配置/状态/日志/缓存路径、service definition 路径、service 名称、权限要求、失败回滚计划，以及 partitioned 模式下的 shard 目录覆盖要求。`service lifecycle <action> --dry-run` 是默认可审计输出；只有显式传入 `--execute` 才能写 service definition、checkpoint 或安装目录，并调用 systemd、launchd 或 Windows Service 命令。未来 `split_worker_preview` 必须分别生成控制服务和 worker 服务定义并说明每个进程的权限、环境变量、日志和 shutdown 行为。

精确代码源码兜底由产品内部实现，运行时不能依赖 `rg`。面向 agent 的 setup 说明可以提到使用有界 `rg` 或 `grep -RIn` 做人工检查工具，但安装器不能把递归 grep 作为 service 依赖，也不能把它当成已索引查询行为的替代品。

## 4. 运行时状态

配置、数据库、索引、日志、缓存、临时文件和 dead-letter 数据写入 `paths` 管理的平台目录。升级时必须保留 runtime state，并显式执行 schema/index migration。
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
  -> backup or migration checkpoint
  -> install new binary
  -> run schema/index migration
  -> service restart through platform manager
  -> post-upgrade doctor
```

失败时回滚二进制和 service definition；数据 migration 必须有 checkpoint 或 forward-only 说明。

`service lifecycle upgrade --execute` 必须按 dry-run 中的阶段顺序执行：先记录 rollback checkpoint，再停止 service、复制二进制、写 service definition、刷新平台 service manager、启动 service，并在后置 doctor 前后保留执行报告。Linux systemd unit 必须引用包含空格的路径，并把字面 `$` 转义为 `$$`。install 写入显式 `--install-dir` 前不得覆盖已有目标二进制或 service definition；Windows install 必须把 service 创建和 registry/environment 写入拆成可单独回滚的步骤。upgrade 必须 checkpoint 已有目标二进制和 service definition，checkpoint backup 必须使用 attempt-scoped 文件并通过原子 checkpoint 发布成为当前回滚依据；没有旧备份时失败回滚和显式 rollback 只能删除本次确实复制或写入的目标文件，definition-only upgrade 不得删除当前运行的 binary。Windows 和 macOS upgrade 必须在启动前刷新平台 service registration，使 SCM `BinaryPathName` 或 launchd loaded job 与更新后的 service definition 一致。uninstall 失败回滚和基于 uninstall checkpoint 的显式 rollback 如果需要恢复已删除的 service definition，必须从 uninstall 前 checkpoint 恢复原 definition，再重新注册 service manager。文件或 service manager 状态变化后任一阶段失败时，必须按 `rollback_steps` 尝试恢复已完成步骤；restore、definition rewrite、unregister 或 service-registration rollback step 失败后不得继续执行依赖的删除、reload/start 步骤；任何此类状态变化前失败时，不得停止、disable、恢复、重启或卸载既有 service。只有选中的 rollback steps 全部成功时，执行报告才能把 rollback 标为完成；外部 service manager 或 doctor 子进程必须有有界执行时间，并在等待退出和超时期间持续读取 stdout/stderr，进程退出或超时后的输出收集也必须有边界。`--execute` 出现失败 step 时，API/CLI 操作必须返回错误并带出失败 step id，不能把失败报告包装成成功响应。`service lifecycle rollback --execute` 使用 checkpoint 备份恢复二进制和 service definition；没有 checkpoint 时必须把缺口暴露在 warnings 或执行错误中，不能静默宣称回滚成功。

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
- 分片 SQLite 拓扑的 shard 目录参与 backup、migration、doctor 和 uninstall 确认。
- 控制服务和 split worker 的服务定义、运行时目录、日志、环境变量和权限边界在 plan/install/uninstall 中可诊断、可回滚。
- Release workflow 或等价门禁必须运行 service lifecycle dry-run smoke，验证发布二进制生成的 service definition、rollback plan 和 package manifest 检查不会与 release tag 漂移。

---

导航: 上一章: [18. 可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md) | 下一章: [20. 多仓库代码图谱薄覆盖层](20-multi-repository-code-graph-overlay.md)
