# relay-knowledge 使用指南

> 文档版本: 1.0
> 编制日期: 2026-05-13
> 范围: 面向本地开发、CLI 操作、Web 诊断、代码仓库索引、MCP 常驻服务、日常排障和高级配置的使用说明。

本使用指南按章节拆分，覆盖从启动到运维检查的常见路径。规格、研究材料和能力参考仍保留在 `docs/specs/`、`docs/research/` 和功能文档中；本目录只描述用户当前可以按步骤执行的工作流。

## 章节目录

- [第 1 章 安装与运行时目录](01-install-and-runtime.md): 构建、运行、平台目录和环境变量覆盖。
- [第 2 章 CLI 基础](02-cli-basics.md): 命令结构、输出格式、版本、状态和 freshness 策略。
- [第 3 章 知识图谱工作流](03-knowledge-graph-workflow.md): evidence 写入、混合检索、图检查和索引刷新。
- [第 4 章 代码仓库工作流](04-code-repository-workflow.md): Git 仓库注册、tree-sitter 索引、代码检索、增量更新和影响分析。
- [第 5 章 Web 工作区](05-web-workspace.md): 静态 Web 构建、诊断面板、stale reasons 和操作预览。
- [第 6 章 Agent 与常驻服务](06-agent-and-service.md): MCP Streamable HTTP、权限、会话和 agent 工具面。
- [第 7 章 运维与排障](07-operations-and-troubleshooting.md): 健康检查、索引新鲜度、常见错误和 PR 验证命令。
- [第 8 章 高级配置参考](08-advanced-configuration.md): 运行时目录、检索后端、网络/QoS、MCP policy 和后续 setup profile 计划。

## 推荐阅读顺序

第一次使用时按第 1 章到第 3 章完成一个零配置知识图谱循环，再根据场景进入后续章节:

```bash
cargo build
target/debug/relay-knowledge status --format json
target/debug/relay-knowledge ingest --source docs --content "Rust async services isolate blocking SQLite work" --entity Rust --format json
target/debug/relay-knowledge query SQLite --source docs --freshness wait-until-fresh --format json
```

如果需要把代码仓库作为检索源，继续阅读第 4 章。如果需要给外部 agent 暴露本地图检索能力，继续阅读第 6 章。只有需要接入外部后端、调整网络预算或部署常驻服务时，才阅读第 8 章。
