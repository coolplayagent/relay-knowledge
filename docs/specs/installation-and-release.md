# 安装部署与发布规格

> 文档版本: 1.0
> 编制日期: 2026-05-11
> 适用范围: `relay-knowledge` 的打包、发布、安装、升级、卸载和后台服务部署
> 默认路线: GitHub Releases 提供预编译二进制，crates.io 提供中心仓库安装，平台服务管理器托管后台进程

## 1. 设计结论

`relay-knowledge` 的安装部署是产品体验的一部分，不能只依赖源码编译。用户应能用一条清晰命令安装 CLI，并能继续安装后台服务、检查健康状态、升级和卸载。

核心结论:

1. **GitHub Releases 是普通用户默认安装入口**: 每个稳定版本必须发布跨平台预编译二进制、校验和和版本说明，用户不需要安装 Rust toolchain 才能使用。
2. **crates.io 是 Rust 生态中心仓库入口**: 每个可公开发布的版本都应能通过 `cargo install relay-knowledge` 安装，供开发者、CI 和 Rust 用户使用。
3. **安装命令要短且可验证**: README 中暴露的安装路径应包含复制即用命令，并提供 checksum、签名或 provenance 校验说明。
4. **后台服务安装是一等能力**: 安装后必须能通过 `relay-knowledge service install|status|doctor|uninstall` 管理 systemd、Windows Service 或 launchd 托管的后台进程。
5. **升级和卸载不能破坏用户数据**: 二进制、配置、数据库、索引、日志和缓存目录必须有清晰边界；卸载程序默认不删除用户数据，除非用户显式要求 purge。
6. **发布流程必须可重复**: release artifact、crate 包、校验文件和安装脚本都应由 CI 从同一个 tag 生成，避免手工上传造成不可追溯差异。

## 2. 发布渠道

### 2.1 GitHub Releases

GitHub Releases 是默认稳定分发渠道。每个 release tag 必须至少包含:

| Artifact | 要求 |
| --- | --- |
| Linux binary archive | 包含 `relay-knowledge` 可执行文件、license、README 或安装说明 |
| macOS binary archive | 同时规划 Apple Silicon 和 Intel，未支持的平台必须在 release note 中说明 |
| Windows binary archive | 包含 `relay-knowledge.exe` 和 Windows 安装说明 |
| `checksums.txt` | 覆盖所有二进制、安装脚本和服务模板 |
| signature 或 provenance | 用于验证 artifact 来源，至少在稳定版发布前启用 |
| release notes | 说明安装方式、升级注意事项、配置变更和迁移风险 |

README 中面向普通用户的安装示例应优先指向 GitHub Release:

```bash
curl -fsSL https://github.com/coolplayagent/relay-knowledge/releases/latest/download/install.sh | sh
relay-knowledge --version
relay-knowledge service doctor
```

Windows 应提供 PowerShell 入口:

```powershell
irm https://github.com/coolplayagent/relay-knowledge/releases/latest/download/install.ps1 | iex
relay-knowledge.exe --version
relay-knowledge.exe service doctor
```

安装脚本必须默认安装最新稳定版，并支持显式版本、安装目录、是否安装后台服务和 dry-run:

```bash
sh install.sh --version 0.1.0 --prefix ~/.local --install-service --dry-run
```

### 2.2 crates.io

crates.io 是中心仓库发布渠道。发布前必须满足:

- `Cargo.toml` 元数据完整，包括 `description`、`license`、`repository`、`readme`、`keywords` 或 `categories`。
- `cargo package` 能成功生成包，且包内不包含数据库、索引、日志、缓存和临时数据。
- `cargo publish --dry-run` 在 CI 或本地 release checklist 中通过。
- crate 安装后能提供与 GitHub Release 二进制一致的 CLI 行为。

Rust 用户的安装路径:

```bash
cargo install relay-knowledge
relay-knowledge --version
```

`cargo install` 不应成为唯一安装路径，因为普通用户不一定有 Rust toolchain，也不一定需要从源码编译。

### 2.3 包管理器

稳定发布后应逐步提供平台包管理器入口:

| 平台 | 推荐渠道 | 要求 |
| --- | --- | --- |
| macOS / Linux | Homebrew tap | formula 指向 GitHub Release artifact 和 checksum |
| Windows | Scoop bucket | manifest 指向 Windows release archive 和 checksum |
| Windows | winget | manifest 使用稳定版 release，描述服务安装能力 |
| Linux | distro package 或 installer script | 不把 state、cache、log 写入仓库或安装包目录 |

包管理器 manifest 不应重新构建不同源码快照。它们应引用同一个 release tag 产生的 artifact，保证不同安装方式得到相同版本。

## 3. 安装体验

安装完成后，用户至少能执行:

```bash
relay-knowledge --version
relay-knowledge service install
relay-knowledge service status
relay-knowledge service doctor
relay-knowledge service uninstall
```

安装器必须处理:

- 检测目标平台和 CPU 架构，选择正确 artifact。
- 创建或确认安装目录，并把二进制放入 PATH 可访问位置。
- 下载前展示版本、来源 URL 和目标路径；下载后校验 checksum。
- 服务安装前展示服务名、运行账号、数据目录、日志目录和自动启动策略。
- 不在未获授权时启用静默后台更新或扫描用户目录。
- 遇到已有版本时支持 upgrade、downgrade 拒绝或显式 downgrade、rollback。
- 安装失败时清理半成品文件，不破坏已有可用版本。

默认目录应遵守平台约定:

| 类型 | Linux | macOS | Windows |
| --- | --- | --- | --- |
| binary | `~/.local/bin` 或系统 prefix | `/usr/local/bin` 或用户选择目录 | `%LOCALAPPDATA%\relay-knowledge\bin` |
| config | XDG config | Application Support | `%APPDATA%\relay-knowledge` |
| state / database | XDG state | Application Support | `%LOCALAPPDATA%\relay-knowledge\data` |
| cache / indexes | XDG cache | Caches | `%LOCALAPPDATA%\relay-knowledge\cache` |
| logs | XDG state/log | Logs | `%LOCALAPPDATA%\relay-knowledge\logs` |

运行态数据不能写入源码仓库、release archive 解压目录或当前工作目录，除非用户显式把这些目录配置为数据目录。

当前基础运行时层已经通过 `paths` 模块实现这些默认目录，并通过
[`基础运行时层规格`](foundational-runtime.md) 记录 `RELAY_KNOWLEDGE_HOME`、
逐项路径覆盖、HTTP bind 和 QoS 预算等环境变量。安装器和后续
`relay-knowledge service install|doctor` 必须复用同一套路径解析结果，
不能重新实现目录规则。

## 4. 后台服务部署

后台服务部署必须沿用 [后台服务、静默更新与自愈设计](background-service-and-self-healing.md) 的约束:

- Linux 使用 systemd user service；需要系统级部署时再使用 system service。
- Windows 使用 Windows Service。
- macOS 使用 launchd agent；系统级部署再使用 launchd daemon。
- CLI 和 Web 只通过共享 application service 读取状态和发送命令，不复制后台逻辑。
- silent background update 默认关闭，只有用户通过安装参数或配置显式启用后才能运行。

安装器或 `relay-knowledge service install` 至少提供:

```bash
relay-knowledge service install --user
relay-knowledge service install --enable-background
relay-knowledge service install --enable-silent-updates --source <path>
relay-knowledge service uninstall
relay-knowledge service doctor
```

服务模板必须记录:

- 二进制绝对路径。
- 配置、数据、缓存和日志目录。
- 重启策略和 watchdog 或等价健康检查。
- 环境变量白名单。
- 资源预算入口，例如 CPU、并发、磁盘和维护窗口配置。

## 5. 升级、回滚和卸载

升级流程必须先替换二进制，再通过应用内部兼容性检查处理配置和数据版本:

- 升级前记录当前版本、安装路径和服务状态。
- 如果后台服务正在运行，先执行优雅停止或让 service manager 重启到新版本。
- schema、配置或索引格式变更必须有 migration note。
- 破坏性迁移必须显式确认，并提供备份或回滚说明。
- 派生索引可以重建，但图事实、用户配置和授权 source scope 不能静默丢失。

卸载默认只移除二进制、服务注册和 shell 集成，不删除用户数据:

```bash
relay-knowledge service uninstall
relay-knowledge uninstall
relay-knowledge uninstall --purge-data
```

`--purge-data` 必须明确列出将删除的 config、state、cache、index 和 log 路径，并要求用户确认，脚本化环境可用 `--yes`。

## 6. Release CI 要求

发布流水线应由 tag 触发，至少包含:

1. 运行 `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features`。
2. 构建目标平台 release 二进制。
3. 生成 archives、checksums、签名或 provenance。
4. 运行安装脚本 dry-run 和基础 smoke test。
5. 执行 `cargo publish --dry-run`，正式发布时推送 crates.io。
6. 创建 GitHub Release 并上传所有 artifact。
7. 更新包管理器 manifest 或打开自动化 PR。

Release note 必须包含:

- 安装和升级命令。
- 支持的平台和已知限制。
- 配置、数据目录或服务模板变化。
- 是否需要手动重建索引或运行迁移。
- 回滚方式。

## 7. 测试和验收

必须覆盖这些测试场景:

- `release_archive_contains_expected_binary_and_license`
- `install_script_selects_platform_artifact`
- `install_script_verifies_checksum_before_replace`
- `install_preserves_existing_config_on_upgrade`
- `service_install_writes_platform_service_definition`
- `service_doctor_reports_data_cache_and_log_paths`
- `uninstall_removes_service_without_deleting_user_data`
- `purge_data_requires_explicit_confirmation`
- `cargo_package_excludes_runtime_state`
- `published_binary_matches_cli_version`

验收标准:

- 普通用户无需 Rust toolchain 即可从 GitHub Release 安装。
- Rust 用户可以通过 `cargo install relay-knowledge` 安装。
- 用户可以安装、检查、停止和卸载后台服务。
- 所有 artifact 都有 checksum，稳定版发布具备签名或 provenance。
- 升级和卸载路径不会默认删除用户数据。
- README、release note 和 `service doctor` 能说明安装位置、服务状态和运行态目录。
