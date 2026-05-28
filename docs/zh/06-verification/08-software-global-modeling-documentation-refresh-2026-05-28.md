# 软件全域建模文档刷新审计 2026-05-28

[中文](../../zh/06-verification/08-software-global-modeling-documentation-refresh-2026-05-28.md) | [English](../../en/06-verification/08-software-global-modeling-documentation-refresh-2026-05-28.md)

> 文档版本: 1.0
> 编制日期: 2026-05-28
> 范围: 软件全域建模研究与架构归档验证。

## 1. 刷新内容

本次刷新为文档研究归档，不改变 Rust、CLI、Web、存储 schema 或测试 harness 行为。

| 区域 | 更新 |
| --- | --- |
| 研究资料 | 新增第 10 章，归档软件全域建模、依赖与 SDK、代码生成方向、动态图谱和 SBOM 研究结论。 |
| 架构规格 | 新增第 21 章，定义全域实体、关系、变化传播、生成上下文和验收标准。 |
| 目录导航 | 更新中英文总目录和研究卷目录，加入新增章节。 |
| 验证记录 | 新增本审计页，说明验证范围和无代码行为变更。 |

## 2. 验证范围

- 新增文档保持中英文镜像和互链。
- 新章节编号延续现有 `03-architecture-specs` 与 `04-research` 序列。
- 文档内容遵守现有硬约束：版本化图事实、派生索引新鲜度、后台持久任务、未解析外部依赖 metadata、禁止查询热路径无界扫描。
- 本次没有安装、发布、服务模板、迁移或运行时目录变更，因此不需要更新安装发布规格。

## 3. 建议检查命令

```sh
rg -n "software-global-domain-modeling|全域建模|SoftwareSystem|uses_sdk|constrains_generation" docs
find docs -type f -name '*.md' -exec wc -l {} + | sort -nr | head
```

Rust 行为未变化，不要求运行 `cargo test`。如 CI 统一执行 Rust 门禁，应以现有代码状态为准。
