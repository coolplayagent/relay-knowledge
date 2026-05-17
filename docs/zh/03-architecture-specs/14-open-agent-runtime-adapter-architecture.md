# 开放 Agent Runtime Adapter 架构

[中文](../../zh/03-architecture-specs/14-open-agent-runtime-adapter-architecture.md) | [English](../../en/03-architecture-specs/14-open-agent-runtime-adapter-architecture.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

`relay-knowledge` 支持 agent，但不实现 agent runtime。外部 runtime 负责 planning、tool calls、approval、handoff、长任务状态和最终 LLM 编排；本系统负责知识事实、检索、审计、freshness 和 context pack。

## 2. Adapter 边界

```text
External Agent Runtime / Host
        |
        v
Protocol Adapter: MCP / ACP / future A2A / local SDK
        |
        v
Unified API
        |
        v
Application Services
```

Adapter 只做协议转换、身份注入、权限前置检查、stream/cancel 映射和审计 metadata。它不能访问 storage、index writer、Git、tree-sitter parser 或 embedding client。

## 3. Runtime 独立性

Domain、application、retrieval、storage 和 indexing 类型不得包含 MCP、A2A、OpenAI、LangGraph、CrewAI、AutoGen 或其他 runtime-specific 类型。协议对象在 adapter 层终止，进入系统后变成稳定 API request。

## 4. Agent Action 审计

每次 agent 访问都记录 runtime identity、scope、tool/action、freshness policy、QoS decision、trace id、result count、truncation、degraded state 和 error class。审计事件必须可脱敏持久化。

## 5. 候选事实策略

Agent 或 LLM 输出的实体、关系、摘要、冲突判断和改写默认是 proposal。只有经过 validation/approval/mutation contract 才能进入 accepted graph。

## 6. 验收标准

- 新协议 adapter 不需要修改 domain fact model。
- 禁止 adapter 直接访问数据库或索引表。
- Agent 输出不能绕过 proposal 和 mutation contract。

---

导航: 上一章: [13. 代码检索排序与影响分析](13-code-retrieval-ranking-and-impact-analysis.md) | 下一章: [15. 常驻 Agent 图访问协议](15-resident-agent-graph-access-protocol.md)
