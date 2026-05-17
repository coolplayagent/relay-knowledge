# 可观测性、诊断与 SLO

[中文](../../zh/03-architecture-specs/18-observability-diagnostics-and-slo.md) | [English](../../en/03-architecture-specs/18-observability-diagnostics-and-slo.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

可观测性不是附加日志，而是架构控制面。检索质量、index freshness、QoS overload、worker recovery、agent audit 和外部 provider degradation 都必须可被诊断、度量和追踪。

## 2. Signal 模型

| Signal | 内容 |
| --- | --- |
| Logs | 结构化事件、错误分类、脱敏上下文 |
| Metrics | queue depth、latency、freshness lag、drops、timeouts、hit counts |
| Traces | request span、retriever span、storage span、worker span、adapter span |
| Health | service state、index state、provider state、QoS state、degraded reason |
| Audit | agent/runtime identity、scope、action、decision、result metadata |

## 3. Trace Context

每个用户请求和后台任务都携带 trace id。Context pack、audit event、worker task、mutation log 和 index cursor 应能通过 trace 或 graph version 关联。

## 4. SLO 候选

- 查询 p95 latency 在配置预算内。
- fresh scope 的 stale lag 为 0。
- worker dead-letter rate 低于阈值。
- QoS drop 和 timeout 有明确 overload reason。
- MCP/Web/CLI 相同操作的错误分类一致。

## 5. 诊断界面

CLI health、service doctor、Web diagnostics、MCP resources 和 Prometheus metrics 读取同一诊断聚合层。UI 可以重组展示，不得重新推断业务状态。

## 6. 验收标准

- 任何 degraded response 都能说明 degraded family、原因和恢复入口。
- Collector 不可用时，OTLP export 失败不会中断本地服务。
- 诊断输出不泄漏 secret、私有 endpoint token 或未授权路径。

---

导航: 上一章: [17. 后台服务、恢复与自愈](17-background-service-recovery-and-self-healing.md) | 下一章: [19. 安装、发布与升级](19-installation-release-and-upgrade.md)
