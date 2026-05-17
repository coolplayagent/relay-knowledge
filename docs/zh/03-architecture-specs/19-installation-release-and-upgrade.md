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

## 6. 验收标准

- Release artifact、checksum、版本号和文档能互相对应。
- service install 使用 systemd、launchd 或 Windows Service，而非 unmanaged loop。
- uninstall 清理二进制和服务定义，但保留或按用户确认处理 runtime data。

---

导航: 上一章: [18. 可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md)
