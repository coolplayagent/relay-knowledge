# 基础运行时层

[中文](../../zh/03-architecture-specs/03-foundational-runtime.md) | [English](../../en/03-architecture-specs/03-foundational-runtime.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

基础运行时层把环境、路径、网络、QoS 和启动边界从业务逻辑中剥离。它的先进性在于：每个外部能力都有唯一入口，运行时配置可诊断、可脱敏、可测试，业务层不需要知道平台目录、环境变量或网络细节。

## 2. 环境变量边界

`env` 是唯一读取环境变量的模块。它负责：

- 读取、解析和校验所有 `RELAY_KNOWLEDGE_*` 变量。
- 把 secret、token、header 和 endpoint 诊断脱敏。
- 区分 absent、empty、invalid、disabled 和 explicitly configured。
- 输出可序列化 runtime config，供 CLI、Web、service doctor 和 tests 使用。

业务模块只能接收 typed config，不能调用 `std::env`。

`application::runtime` 负责组合 typed snapshot，但不得在一个文件内承载所有领域 parser。其 `retrieval` owner 独占 semantic/vector backend mode、rerank policy、remote embedding provider metadata、model name、dimension、batch、concurrency 与 timeout 校验；`file_index` owner 独占 authorized root 解析、platform path normalization、稳定 root identity 生成与 scan/query budget 校验；`worker` owner 独占外部 worker endpoint 校验、通用与 code-index 并发上限，以及 worker kind 到 endpoint 的映射；`agent` owner 独占 MCP endpoint、逗号分隔的 origin 与 scope policy、request runtime budget 和 audit queue 设置校验；`storage` owner 独占 single-database 与 partitioned-SQLite topology 选择解析。UT 直接挂载这些 owner，runtime 根只在 bootstrap 组合时映射其稳定错误。

## 3. 路径边界

`paths` 是唯一构造运行时路径的模块。默认目录必须使用平台约定：配置、数据、日志、缓存、临时文件、dead-letter 和 service definition 分开管理。

安装目录、release 解压目录、当前工作目录和仓库目录不得默认承载运行时状态。用户显式配置路径时，`paths` 负责规范化、权限检查和诊断。

仓库局部 contract（例如 `knowledge/knowledge-map.yaml`）不属于 runtime state。进程入口可以把 cwd 作为 bootstrap 输入，仓库根发现策略必须收敛在 `paths` 的 repository-root contract 中；application service 只能接收已经解析好的仓库根路径。

## 4. 网络和 QoS 边界

`net` 拥有所有网络能力；`net::http` 拥有 HTTP client/server；`net::qos` 拥有准入控制和资源预算。应用服务请求网络资源时只表达意图、来源、tenant、优先级和预算，不直接创建 socket 或 client。

QoS policy 至少覆盖：

- connection limit、request limit、body limit。
- per-source/per-tenant budget。
- timeout、cancellation、retry backoff。
- overload response 与 dropped work 观测。

网络路径清单必须随实现保持同步。当前统一接入点如下：

| 路径 | 方向 | QoS 接入点 | 过载行为与观测 |
| --- | --- | --- | --- |
| Web HTTP API 与静态资源 | inbound | `net::http` listener connection admission 与 Web router request admission | 超过 request budget 返回 `429`；status/MCP metrics 暴露 current usage 与累计 admitted、rejected、timed_out、cancelled、dropped |
| MCP Streamable HTTP 与 `/mcp/metrics` | inbound | `net::http` listener connection admission 与 MCP method admission | JSON-RPC 工具调用返回 QoS error，HTTP endpoint 返回 `429`；audit/metrics 记录 admitted/rejected/timeout/cancel |
| Model catalog、provider probe/discovery、remote embedding、update check、remote CLI、worker HTTP endpoint | outbound | `net::http` outbound request admission | 发送前获取 request permit；超预算映射为稳定 network/QoS 错误并记录 rejected，transport timeout 记录 timed_out |

QoS 诊断计数使用低基数字段。`*_total` 是进程生命周期内累计 counter；`current_*` 是当前连接、in-flight request 与 queued request gauge。不得把原始 URL、IP、request id、用户 id 或 secret 放进 QoS metric label。

## 5. 启动模型

CLI、Web 和 service mode 共享同一组 application services。启动顺序固定：

1. 解析 env。
2. 解析 paths。
3. 初始化 net/QoS policy。
4. 打开 storage 和 index metadata。
5. 运行 startup reconciler。
6. 接受 CLI/API/MCP/Web 请求。

任何入口跳过该顺序都属于架构缺陷。

## 6. 诊断输出

`status`、`health`、`service doctor`、Web diagnostics 和 MCP resources 读取同一份 runtime snapshot。诊断必须说明配置来源、脱敏值、目录、服务状态、QoS budget、index freshness 和 degraded reason。

`net::http` 规范化 Windows 风格 `host:port` 代理，非法环境代理以脱敏 warning 回退直连；outbound client 禁止再次隐式读取环境代理。`env` 捕获 `RELAY_KNOWLEDGE_HEALTH_STORAGE_BUDGET_MS`，`application::runtime::storage` 校验 1–30000ms（默认 500ms）；health owner 统一使用该预算并投影维护建议。SQLite store 的诊断读 worker 在 future 取消后通过 progress handler 中断 SQL，并在释放连接前清除 handler。配置、取消、慢快照、缓存降级和本地 CLI 可用性由直接 owner UT 与 CLI integration 回归保护。

健康响应组装（含 profile 文件读取）与成功存储快照共用同一截止时间。`NetworkRuntime` 保存历次已发布 request timeout 的最小值，刷新不得使健康预算超过旧监听器的 deadline；重启创建新 runtime 后才可提高上限。分片维护诊断按最旧时间汇总，缺失或读取失败不会被其他分片的较新记录覆盖。

`application::service::health::profiles` 独占健康 profile 读取准入与摘要缓存。每个服务实例（所有克隆共享）最多一个进行中的读取，无排队任务；worker 持有令牌直至实际读取完成，不能在请求 timeout 时提前释放。正在读取时只返回带显式 stale/pending 说明的缓存或未检查状态。该边界没有后台轮询。

Windows CI 另外运行代理与本地诊断的二进制集成用例，覆盖 Windows 子进程环境传递及直连降级。

健康存储预算会按当前 HTTP timeout 缩短，为响应保留最多 100ms（短 timeout 使用一半）。分片健康查询的目录、totals 和 SQLite 诊断均使用有界、可取消且不排队的读准入；目录最多接纳 1,024 个活跃分片。降级缓存不读取 profile 文件，使用已缓存 summary 或明确未检查状态。

## 7. 验收标准

- 仓库内只有 `env` 读取环境变量，只有 `paths` 构造运行时路径，只有 `net` 创建网络能力。
- CLI、Web、service 和 tests 能使用相同 typed runtime config。
- 配置错误在启动或 doctor 阶段被解释为稳定错误，不在业务路径中以 panic 暴露。

---

导航: 上一章: [2. 工程硬约束](02-engineering-hard-constraints.md) | 下一章: [4. Source Scope 模型](04-source-scope-model.md)
