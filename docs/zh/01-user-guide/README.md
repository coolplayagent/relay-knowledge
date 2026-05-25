# relay-knowledge 使用指南

[中文](../../zh/01-user-guide/README.md) | [English](../../en/01-user-guide/README.md)

> 文档版本: 1.3
> 编制日期: 2026-05-17
> 范围: 面向本地开发、CLI 操作、知识图谱检索、代码仓库图谱、Web 工作区、MCP/ACP 接入、常驻服务、后台 worker、可观测性、日常排障和高级配置的可执行用户说明。

本卷只写用户可以直接运行、验证和排障的路径。架构约束、接口边界和前瞻设计仍保留在 `docs/zh/03-architecture-specs/`；本卷负责把能力落到命令、配置、运行状态和诊断步骤上。

## 当前能力边界

`relay-knowledge` 是 local-first 的图谱检索底座，不是通用 agent runtime 或最终答案生成器。当前用户可以直接使用:

- 零配置本地 SQLite 图谱、平台运行时目录和 deterministic semantic/vector read models。
- evidence ingest、三层 GraphRAG 检索、graph-only 查询、index refresh 和 freshness diagnostics。
- 代码仓库图谱，包括 tree-sitter 索引、符号与关系检索、有界内部 exact-text source fallback、增量更新、worktree overlay、影响分析和报告。
- 静态 Web 工作区与同源操作执行，覆盖 retrieval、ingest、repo、index、provider、worker、proposal、audit 和 service snapshot。
- MCP Streamable HTTP、本地 ACP adapter、QoS、scope policy、取消、metrics 和审计。
- service manager 计划、definition 生成、silent-update operator 状态、worker/proposal/audit 运维入口。
- setup doctor 和 setup profile，用于聚合本地配置 readiness 并输出可执行配置画像。

## 章节目录

- 第 0 章 使用指南总览: 当前页面。
- [第 1 章 安装与运行时目录](01-install-and-runtime.md): 构建、本地运行、零配置默认值和平台目录。
- [第 2 章 CLI 基础](02-cli-basics.md): 命令语法、输出格式、freshness 和解析诊断。
- [第 3 章 CLI 命令参考](03-cli-command-reference.md): 命令总览、状态诊断、setup profile 和 provider probe。
- [第 4 章 知识图谱](04-knowledge-graph.md): evidence 写入、context pack 查询、图检查、多模态 evidence 和检索后端入口。
- [第 5 章 代码仓库图谱工作流](05-code-repository-graph-workflow.md): 仓库注册、代码图谱索引、符号与关系查询、source fallback 诊断、增量更新、影响分析和报告。
- [第 6 章 Web 工作区](06-web-workspace.md): 静态资源、同源接口、操作执行、浏览器集成测试和安全边界。
- [第 7 章 MCP Agent 接入](07-mcp-agent-access.md): MCP policy、会话流程、tools/resources/prompts 和访问边界。
- [第 8 章 ACP 本地 Adapter](08-acp-local-adapter.md): 本地 ACP 会话、progress、cancellation 和 context artifact。
- [第 9 章 常驻服务](09-resident-service.md): 前台服务、同端口 Web/API/MCP、service manager 和 silent-update operator。
- [第 10 章 Worker、Proposal 与 Audit](10-workers-proposals-audit.md): 后台 worker、人工审核、proposal 生命周期和审计。
- [第 11 章 可观测性与遥测](11-observability-and-telemetry.md): Prometheus metrics、OTLP traces/metrics 和诊断状态。
- [第 12 章 高级配置参考](12-advanced-configuration.md): 运行时目录、检索后端、网络/QoS、MCP、worker、audit 和 setup 变量。
- [第 13 章 运维与排障](13-operations-and-troubleshooting.md): 健康检查、索引新鲜度、常见错误、隔离复现和 PR 验证。

## 推荐阅读顺序

第一次使用时，先完成一个零配置知识图谱闭环:

```bash
cargo build
target/debug/relay-knowledge status --format json
target/debug/relay-knowledge ingest --source docs --content "Rust async services isolate blocking SQLite work" --entity Rust --format json
target/debug/relay-knowledge query SQLite --source docs --freshness wait-until-fresh --format json
target/debug/relay-knowledge setup doctor --format json
```

需要把代码仓库作为检索源时读第 5 章；需要浏览器操作面时读第 6 章；需要给 agent 暴露本地图检索能力时读第 7 章和第 8 章；需要长期运行、后台任务或遥测时读第 9 章到第 11 章；只有接入外部后端、调整预算或复现环境问题时再查第 12 章和第 13 章。

## 输出与审计约定

指南中的 `relay-knowledge` 表示已构建或已安装的二进制。未安装到系统路径时，用 `target/debug/relay-knowledge` 或 `target/release/relay-knowledge` 替换即可。脚本集成优先使用 `--format json`；人工检查可以使用默认 `text`，报告类命令可以使用 `markdown`。

CLI、Web、MCP、ACP、worker、proposal、audit 和 service 都通过共享 application service 进入核心能力。排障时优先保留 JSON 响应里的 operation、metadata、degraded reason、freshness、audit correlation 和 diagnostics 字段。
