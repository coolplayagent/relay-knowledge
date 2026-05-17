# 统一 API 与交互层架构

[中文](../../zh/03-architecture-specs/16-unified-api-and-interface-architecture.md) | [English](../../en/03-architecture-specs/16-unified-api-and-interface-architecture.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

统一 API 是 CLI、Web、MCP、ACP 和未来 SDK 的共同语义边界。交互层只负责参数解析、展示、streaming 和错误映射；它不能复制核心业务逻辑。

## 2. API Contract

稳定 request/response 至少表达：operation、scope、freshness policy、budget、format、trace context、authorization identity 和 idempotency key。Response 表达 data、metadata、warnings、degraded state、freshness、truncation 和 stable error。

## 3. Interface 分工

| Interface | 职责 |
| --- | --- |
| CLI | 可脚本化命令、JSON 输出、明确读写影响 |
| Web | 操作组合器、诊断、设置管理、可视化结果 |
| MCP/ACP | Agent tool/resource/prompt/session 访问 |
| Local SDK | 嵌入式调用稳定 API，不引入 runtime 类型 |

## 4. 错误模型

错误必须稳定分类：invalid input、unauthorized scope、not found、stale index、timeout、cancelled、overloaded、degraded backend、storage unavailable、internal。接口层可以翻译文案，但不能改变语义。

## 5. Streaming 与取消

长查询、索引、impact analysis 和 agent 请求支持 progress event 和 cancellation。取消不是异常退出；它必须释放预算、停止后续 worker 调度，并写 audit/metric。

## 6. 验收标准

- 同一操作在 CLI JSON、Web API 和 MCP tool 中返回兼容语义。
- 新 UI 不直接调用 storage 或 indexing trait。
- `help --format json` 能描述命令路径、参数、默认值、读写影响和示例。

---

导航: 上一章: [15. 常驻 Agent 图访问协议](15-resident-agent-graph-access-protocol.md) | 下一章: [17. 后台服务、恢复与自愈](17-background-service-recovery-and-self-healing.md)
