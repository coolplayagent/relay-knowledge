# 文档刷新审计 2026-05-14

[中文](./documentation-refresh-audit-2026-05-14.md) | [英文](../../en/06-verification/documentation-refresh-audit-2026-05-14.md)

本审计记录 2026-05-14 对当前 `relay-knowledge` 实现所做的文档刷新。命令可用性的权威来源是 `relay-knowledge help --format json`；状态和健康检查行为已根据编译后的二进制进行核对。

## 本轮刷新内容

| 范围 | 刷新内容 |
| --- | --- |
| README | 将设置诊断、设置配置文件命令补充到当前能力和 CLI 示例中。 |
| 用户指南 | 将指南版本提升到 1.2，补充 `setup doctor`，记录设置配置文件，并移除仅规划阶段的设置表述。 |
| 高级配置 | 用已实现的 `setup doctor` 和 `setup profile` 行为替换旧的规划中设置章节。 |
| 运维排障 | 将 setup doctor 加入诊断顺序，并记录过期本地数据库重置行为。 |
| Semantic/vector 后端 | 将外部 embedding 设置配置文件记录为推荐起点。 |
| 统一 API 规格 | 将 setup 命令加入当前 CLI 表面，并说明 setup profile 是只读推荐输出。 |
| 安装发布规格 | 更新服务安装说明，使其匹配已实现的 `service plan`、`service definition write` 和 `setup profile service` 流程。 |
| 存储重置 | 本地 SQLite 文件不匹配当前 schema 时，会按最新表定义重建。 |

## 当前文档状态

| 文档组 | 状态 |
| --- | --- |
| 根 README 与文档索引 | 已覆盖当前 CLI、Web、MCP、服务和 setup 能力。 |
| 用户指南 | 已覆盖本地安装、CLI 基础、GraphRAG、代码仓库工作流、Web 工作区、agent/service 运行、排障和高级配置。 |
| 功能文档 | 已覆盖 GraphRAG context pack、semantic/vector provider 和 tree-sitter 仓库检索；审计记录已移入验证卷。 |
| 规格 | 按设计混合存在：硬约束和接口规格是当前契约；产品、存储、后台服务、架构和安装规格仍包含前瞻要求。 |
| 研究材料 | 作为历史和参考资料保留，仍刻意保留路线图与差距分析语气，而不是改写成用户操作手册。 |
| 基准与验证记录 | 是 2026-05-14 的快照记录。除非新的基准运行取代它们，否则应继续作为带日期的证据保留。 |

## 已从规划表述落地

- `relay-knowledge setup doctor`：不触碰存储的只读配置就绪检查，覆盖运行时路径、网络/QoS 预算、检索后端元数据、MCP 作用域策略、服务目录和 worker 预算，并返回 `configuration_ready`、`live_health_checked=false`、`live_health_commands` 和修复用的 `recommended_actions`。
- `relay-knowledge setup profile local|agent-readonly|service|external-embedding`：输出只读环境变量和命令建议，不写文件、不修改 shell profile，也不安装服务。
- SQLite 启动迁移：启动时迁移已知过期表定义，只局部重建可派生的检索表和刷新队列，避免 `health` 和 `service doctor` 运行在过期 schema 上。

## 开放产品化工作

以下条目不是未落地的核心闭环，而是仍需端到端产品化或发布运营承接的工作。

| 能力 | 当前状态 | 开放工作 |
| --- | --- | --- |
| 特权服务安装/卸载 | 已实现 `service plan` 和 `service definition write`。 | 安装器或运维流程仍需执行平台服务管理器命令，并提供回滚和卸载语义。 |
| 包管理器分发 | Release workflow 会产出构件；规格描述了 Homebrew、Scoop、winget 和发行版包要求。 | 发布并维护引用 release 构件的包管理器 manifest。 |
| 外部 embedding/OCR/vision provider | 已具备运行时配置、provider 探测、worker 端点契约、确定性回退提案和设置配置文件。 | 产品化具体 provider adapter、模型共存策略，以及生产部署运维文档。 |
| 更大的评测数据集 | CI 中已有 GraphRAG 行为夹具门禁。 | 增加更大规模真实世界数据集、长期报告和面向 release 的质量阈值。 |
| 远端 ACP 产品化 | 已有本地 ACP adapter。 | 在产品表面成熟后补充远端 host 集成、认证和安装指南。 |

## 验证命令

```bash
relay-knowledge help --format json
relay-knowledge setup doctor --format json
relay-knowledge setup profile external-embedding --format json
cargo test --all-targets --all-features cli
cargo test --all-targets --all-features startup_migrates_obsolete_refresh_queue_schema_without_deleting_graph_data
```
