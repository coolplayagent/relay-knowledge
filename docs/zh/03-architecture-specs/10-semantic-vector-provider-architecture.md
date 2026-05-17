# Semantic/Vector Provider 架构

[中文](../../zh/03-architecture-specs/10-semantic-vector-provider-architecture.md) | [English](../../en/03-architecture-specs/10-semantic-vector-provider-architecture.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

Semantic/vector provider 是派生读模型的后端选择，不是知识真源。默认本地确定性模型保证零配置可用；外部 embedding provider 提供质量提升，但必须受 env、QoS、脱敏诊断、缓存、维度校验和降级策略约束。

## 2. Provider 模式

| 模式 | 行为 |
| --- | --- |
| `local` | 使用确定性本地 semantic signature 和 hashed vector，适合测试、默认 UX 和离线使用 |
| `external` | 通过受控 worker 调用外部 embedding endpoint，记录 model、dimension、backend cursor |
| `disabled` | 不调度 semantic/vector 刷新，检索结果声明缺失 family |

Provider 模式只能通过 `env` typed config 进入系统。

## 3. 数据契约

外部 embedding 输出不能直接写 accepted fact；它只能写入 index family metadata 或 derived evidence metadata。每条向量记录必须绑定 scope、evidence/chunk id、model name、dimension、content hash 和 graph version。

## 4. 隐私和安全

- API key、authorization header 和 endpoint secret 只能在保存/执行边界可见。
- Web 和 diagnostics 只返回 configured boolean 或脱敏值。
- 外部请求必须遵守 source authorization 和 redaction policy。
- retry 不得把 secret 写入 log、audit 或 dead-letter payload。

## 5. 降级策略

外部 provider 不可用时，系统按配置降级到 local 或声明 semantic/vector unavailable。Context pack 必须暴露 backend availability、model、dimension、last error 和 stale lag。

## 6. 验收标准

- 默认无外部服务时仍能完成 hybrid retrieval。
- 外部 provider 维度变更会产生明确 stale/rebuild 要求。
- Secret 不出现在日志、Web response、MCP resource 或测试 snapshot 中。

---

导航: 上一章: [9. 混合检索与 Context Packing](09-hybrid-retrieval-and-context-packing.md) | 下一章: [11. 代码知识图谱模型](11-code-knowledge-graph-model.md)
