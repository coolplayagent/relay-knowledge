# relay-knowledge User Guide

[English](../../en/user-guide/README.md) | [中文](../../zh/user-guide/README.md)

This is the English documentation page for `user-guide/README.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> 文档版本: 1.2
> 编制日期: 2026-05-14
> 范围: 面向本地开发、CLI 操作、知识图谱检索、代码仓库索引、Web 操作工作区、MCP/ACP agent 接入、常驻服务、日常排障和高级配置的使用说明。

本使用指南按可执行工作流组织，优先描述用户当前能直接运行和验证的路径。规格、研究材料和能力参考仍保留在 `docs/specs/`、`docs/research/` 和功能文档中；本目录负责把这些能力落到命令、配置、运行状态和排障步骤上。

## 当前能力边界

`relay-knowledge` 是 local-first 的图谱检索底座，不是通用 agent runtime 或最终答案生成器。当前用户可用能力包括:

- 零配置本地 SQLite 图谱、平台运行时目录和 deterministic semantic/vector read models。
- evidence ingest、三层 GraphRAG 检索、graph-only 查询、index refresh 和 freshness diagnostics。
- 代码仓库注册、tree-sitter 索引、增量更新、worktree overlay、代码检索、影响分析和 Markdown/JSON 报告。
- 静态 Web 诊断与同源操作执行，覆盖 retrieval、ingest、repo、index、provider、worker、proposal、audit 和 service snapshot。
- MCP Streamable HTTP、兼容 HTTP+SSE、只读 resources/prompts、本地 ACP adapter、QoS、scope policy、取消和审计。
- service manager 计划和 definition 生成、silent-update operator 状态、worker/proposal/audit 运维入口。
- setup doctor 和 setup profile，用于聚合本地配置 readiness 诊断并输出可执行配置画像。

## 章节目录

- [第 1 章 安装与运行时目录](01-install-and-runtime.md): 构建、运行、平台目录和环境变量覆盖。
- [第 2 章 CLI 基础](02-cli-basics.md): 命令结构、输出格式、版本、状态、provider probe 和 freshness 策略。
- [第 3 章 知识图谱工作流](03-knowledge-graph-workflow.md): evidence 写入、混合检索、图检查和索引刷新。
- [第 4 章 代码仓库工作流](04-code-repository-workflow.md): Git 仓库注册、scope preview、tree-sitter 索引、代码检索、增量更新、影响分析和报告。
- [第 5 章 Web 工作区](05-web-workspace.md): 静态 Web 构建、诊断面板、stale reasons 和同源操作执行。
- [第 6 章 Agent 与常驻服务](06-agent-and-service.md): MCP Streamable HTTP、权限、会话和 agent 工具面。
- [第 7 章 运维与排障](07-operations-and-troubleshooting.md): 健康检查、索引新鲜度、常见错误和 PR 验证命令。
- [第 8 章 高级配置参考](08-advanced-configuration.md): 运行时目录、检索后端、网络/QoS、MCP policy、setup doctor/profile 和高级运维变量。

## 推荐阅读顺序

第一次使用时按第 1 章到第 3 章完成一个零配置知识图谱循环，再根据场景进入后续章节:

```bash
cargo build
target/debug/relay-knowledge status --format json
target/debug/relay-knowledge ingest --source docs --content "Rust async services isolate blocking SQLite work" --entity Rust --format json
target/debug/relay-knowledge query SQLite --source docs --freshness wait-until-fresh --format json
target/debug/relay-knowledge setup doctor --format json
```

如果需要把代码仓库作为检索源，继续阅读第 4 章。如果需要浏览器操作面，继续阅读第 5 章。如果需要给外部 agent 暴露本地图检索能力，继续阅读第 6 章。只有需要接入外部后端、调整网络预算或部署常驻服务时，才阅读第 8 章。

## 输出与审计约定

指南中的命令默认使用 `relay-knowledge` 表示已构建或已安装的二进制。未安装时使用 `target/debug/relay-knowledge` 或 `target/release/relay-knowledge` 替换即可。脚本集成优先使用 `--format json`；人工检查可以使用默认 `text` 或支持报告的 `markdown`。涉及 Web、MCP、ACP、worker、proposal、audit 和 service 的入口都通过共享 application service 执行，避免 CLI、Web 和 agent adapter 行为分叉。
