# 文档发版准备审计 2026-06-05

[中文](../../zh/06-verification/11-documentation-release-readiness-2026-06-05.md) | [English](../../en/06-verification/11-documentation-release-readiness-2026-06-05.md)

> 日期: 2026-06-05
> 范围: documentation-only 发版准备
> 关联契约: [安装、发布与升级](../03-architecture-specs/19-installation-release-and-upgrade.md)

## 1. 目标

本审计记录一次面向发版准备的文档刷新。目标是让 release 入口更容易阅读，让书架导航覆盖当前文档，让语言版本覆盖差异显式可见，并保持产品行为不变。
最后一轮发版准备还确认了命令本地的 `--kind` 家族在根 README、CLI 命令参考和发布给 agent 的 skill 中保持一致。

## 2. 清单

- 本次改动后，`docs/` 树包含 169 个 Markdown 文件，其中包括本中文审计页及其英文对应页。
- 已跟踪 Markdown 文件继续满足仓库 1000 行上限；当前最长的已跟踪 Markdown 页面仍是
  `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`，共 998 行。
- 英文书架现在列出已经存在但此前未进入索引的附录 A.6 和 A.7。
- 中文书架现在列出附录 A.6 到 A.10，以及附录 B.11。
- 英文导航显式标注尚待翻译的中文-only 基准附录 A.8 到 A.10，以及验证附录 B.5 到 B.6。

## 3. 文档改动

- 根目录 `README.md` 和 `README.zh-CN.md` 增加发版准备阅读路径，指向书架、安装指南、发布架构契约和本审计。
- `docs/README.md` 增加发版准备阅读路径和当前语言覆盖策略。
- 中英文书架索引补齐近期基准与验证记录，避免只能通过文件列表发现。
- 第 19 章新增要求：推送 release tag 前需要在 `06-verification` 下保留带日期的文档准备记录。
- `.knowledge/knowledge-map.yaml` 增加 release documentation 路由，供后续 agent 辅助维护文档时使用。
- 根目录 README、中英文 CLI 命令参考和 `relay-knowledge-cli` skill 现在都明确
  `--kind` 取值是命令本地的，并使用同一组 `repo query`、`repo software`、
  `index refresh`、`worker` 和 `map source` 列表。

## 4. 安全边界

本次刷新不修改 Rust 源码、Web 源码、GitHub Actions workflow、构建脚本、包元数据、生成的 release artifact、CLI 命令行为、service 行为、索引、检索、存储或网络行为。

release 文档不能暗示尚未支持的产物、未受管后台循环、自动静默替换二进制，或没有从同一 release tag 产出的包管理器路径。

## 5. 验证

本轮本地验证：

- `git ls-files '*.md' | xargs wc -l | awk '$1 > 1000 && $2 != "total" {print}'`
  未报告任何超过 1000 行的已跟踪 Markdown 文件。
- 使用会忽略 code span 的本地 Markdown 链接检查验证仓库内相对链接，并忽略
  ``rk_pipeline[index](dev)`` 这类源码文本示例，避免把它误判成文档链接。
- `cargo fmt --all -- --check` 确认 documentation-only 改动没有扰动 Rust 格式。
- `cargo clippy --all-targets --all-features -- -D warnings` 通过。
- `cargo test --all-targets --all-features` 通过。

真正发版仍需要执行根 README 和 CI 中列出的常规 release 门禁，包括 package 检查、覆盖率、浏览器集成环境准备，以及准备 release tag 时的 release workflow dry-run 验证。

---

导航: 上一条:
[10. 服务化部署、控制面与数据面分离文档刷新审计](10-service-deployment-control-data-plane-2026-06-04.md)
