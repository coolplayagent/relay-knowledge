# Coding-Agent 端到端评测门禁

Issue #300 在 Rust self-iteration harness 中新增可复现的 coding-agent 工作流门禁。

## 范围

- 命令：`./self-iterate.sh evaluate --use-current-candidate --profile fast --categories agent_workflows`
- CI job：`.github/workflows/benchmark-checks.yml` 中的 `agent-workflow-regression`
- fixture 文件：`tools/self_iteration/cases/agent_workflow_targets.json`
- 生成式仓库：`agent_workflow_fixture`

评测运行时生成包含 Rust、TypeScript、Python、YAML 和 Markdown 文件的仓库，覆盖定义定位、跨语言影响追踪、配置到文档追踪，以及 `wait-until-fresh` 和 `allow-stale` 查询步骤下的 freshness policy 检查。

## 指标

每个 workflow 在任一预算超限时失败：

- 工具调用次数
- 打包进 context 的唯一源码文件数量
- 捕获的命令输出字符数
- context 字符数
- 最少命中证据数量
- text fallback 命中比例
- 总查询延迟

CI job 会把 fast repository 集合限制为 `agent_workflow_fixture`，并检查生成的 JSON report 中是否存在失败 gate、失败 case 或失败 agent workflow metric budget。它不依赖 self-iteration score 的采纳决策，因为采纳决策还会受历史最佳 run 对比影响。该门禁保持本地、确定、成本可控，同时仍能在 context 过大、证据缺失、fallback 过度使用和明显延迟回退时失败。

## 约束

该门禁必须保持产品泛化。不要在产品代码中通过特化 fixture 仓库名、路径、符号、查询文本或 benchmark id 来修复失败。改进应来自检索规划、排序、索引、证据打包或有界 fallback 行为。
