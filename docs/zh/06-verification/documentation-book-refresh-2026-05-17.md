# 文档书架刷新审计 2026-05-17

[中文](./documentation-book-refresh-2026-05-17.md) | [英文](../../en/06-verification/documentation-book-refresh-2026-05-17.md)

本审计记录 2026-05-17 对文档书架、目录职责和已落地能力关闭状态的刷新。此次变更只调整文档，不改变 Rust、Web 或工具行为。

## 本轮刷新内容

| 范围 | 刷新内容 |
| --- | --- |
| 文档书架 | 明确 `01-user-guide`、`02-capabilities`、`03-architecture-specs`、`04-research`、`05-benchmarks`、`06-verification` 的目录职责。 |
| 目录调整 | 将 `documentation-refresh-audit-*` 从能力卷移动到验证卷，避免把审计证据当成用户能力。 |
| 索引 | 根 README、`docs/README.md`、中英文书架索引均指向新的验证卷路径，并补充中文 Linux 验证和自迭代优化记录入口。 |
| 路线规格 | GraphRAG 路线规格改为 Phase 1-4 已关闭、Phase 5 开放产品化工作，关闭 `setup doctor/profile`、MCP resources/prompts、service definition preview、audit sink 和 metrics exporter 等已落地项。 |
| 研究参考 | 实现参考文档改为“关闭状态 + 开放产品化路线”，不再把 local semantic/vector、MCP resources/prompts、Web operations、proposal lifecycle 基础闭环写成缺失项。 |
| 架构规格 | storage、background service、resident agent、capability reference 的实施顺序改为关闭状态或开放产品化方向。 |

## 开放产品化工作

仍开放的事项集中在具体外部 provider、特权服务安装/回滚/包管理器分发、watchdog/maintenance、valid-time 与 conflict 产品语义、query router、A2A gateway、远端 ACP host integration，以及更大真实数据集和 release-facing 质量阈值。

## 验证命令

```bash
rg -n '02-capabilities/documentation-refresh' docs README.md README.zh-CN.md --glob '!docs/*/06-verification/documentation-book-refresh-2026-05-17.md'
rg -n '剩余实现工作|Remaining Implementation|Done:' docs README.md README.zh-CN.md --glob '!docs/*/06-verification/documentation-book-refresh-2026-05-17.md'
find docs -type f -maxdepth 4 -exec wc -l {} + | sort -nr | head -30
git diff --check
```
