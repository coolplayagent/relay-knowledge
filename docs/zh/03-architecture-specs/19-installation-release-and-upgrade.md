# 安装、发布与升级

[中文](../../zh/03-architecture-specs/19-installation-release-and-upgrade.md) | [English](../../en/03-architecture-specs/19-installation-release-and-upgrade.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

安装和发布是产品架构的一部分。稳定版本必须可验证、可回滚、可卸载、可诊断；二进制安装路径和运行时状态必须分离；后台服务必须交给平台 service manager 管理。

## 2. 发布渠道

- GitHub Releases 发布跨平台预构建压缩包、checksums 和 release notes。
- crates.io 保持 `cargo install relay-knowledge` 可用。
- Homebrew、Scoop、winget 或发行版包应引用同一 release tag 产物，不重建分叉快照。
- Release tag 使用 `vX.Y.Z`、`X.Y.Z` 或 `vX.Y.Z-rc.1` 这类 prerelease 形式；数字版本必须在推送 tag 前与 `Cargo.toml` 和 `Cargo.lock` 保持一致。手动 dry-run dispatch 复用同一版本契约，但不会发布 crates.io 或 GitHub release 产物；workflow 默认 dry-run tag 必须随每次 release 版本提升同步更新。
- macOS x64 release job 必须使用仍可用的 Intel runner label，例如 `macos-15-intel`，不能继续依赖已退休的 `macos-13` 镜像。Artifact upload/download 和 attestation action 必须保持在兼容 Node 24 的版本，确保 GitHub-hosted runner runtime 迁移后 release workflow 仍可运行。
- Release archive attestation 使用生成的 `checksums.txt` 作为 subject manifest，使 GitHub artifact attestation 覆盖用户本地校验的同一批 archive digest。
- CLI 新版本发现使用可配置双源：GitHub Releases 和 crates.io。检测必须走 `env`、`paths`、`net::http` 边界，继承代理、TLS、timeout 和 runtime cache 策略；普通命令只能提示稳定新版，不能静默替换二进制。
- GitHub Releases 包含从 `skills/relay-knowledge-cli` 构建的 `relay-knowledge-cli-skill-<tag>.tar.gz` skill 产物；其版本跟随 `Cargo.toml`，并会以数字 semver 写入生成后的 `SKILL.md` metadata。skill 产物在 `assets/` 下内置 Linux x64 和 Windows x64 二进制，并要求 agent 在 `PATH` 和内置二进制同时存在时使用 semver 最新版本。配置 `CLAWHUB_TOKEN` 时，release workflow 还可以用 `clawhub publish` 把同一个生成后的 skill 布局发布到 ClawHub。该 skill-over-CLI 产物与 MCP 协议打包分离。

## 3. 安装体验

Installer 或安装脚本支持：版本选择、安装目录选择、dry run、校验和验证、service definition 生成、失败回滚和 uninstall plan。默认不会把数据写入 release 解压目录。

## 4. 运行时状态

配置、数据库、索引、日志、缓存、临时文件和 dead-letter 数据写入 `paths` 管理的平台目录。升级时必须保留 runtime state，并显式执行 schema/index migration。
本地文件定位索引的 SQLite/FTS5 状态也写入同一运行态数据区。安装器和 service
template 不能默认扫描全盘、Linux `/opt`、挂载盘或 Windows 非系统盘；只有用户显式配置
或通过 CLI 传入这些 root 时才建立索引。

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

`relay-knowledge version check` 是只读诊断入口，输出当前版本、最新稳定版本、来源、release URL 和诊断信息。实际升级仍必须由用户、installer 或包管理器显式执行，并继续遵守 preflight、checkpoint、service restart 和 post-upgrade doctor 流程。

## 6. 验收标准

- Release artifact、checksum、版本号和文档能互相对应。
- GitHub Release 将 CLI skill archive 纳入 `checksums.txt`，archive 内含 Linux x64 和 Windows x64 asset 二进制；启用 ClawHub 发布时使用同一个 crate 版本和生成后的 asset 布局。
- CLI 能说明稳定新版本可用，JSON 输出保持机器可读且普通命令不会自动安装新版。
- service install 使用 systemd、launchd 或 Windows Service，而非 unmanaged loop。
- uninstall 清理二进制和服务定义，但保留或按用户确认处理 runtime data。

---

导航: 上一章: [18. 可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md) | 下一章: [20. 多仓库代码图谱薄覆盖层](20-multi-repository-code-graph-overlay.md)
