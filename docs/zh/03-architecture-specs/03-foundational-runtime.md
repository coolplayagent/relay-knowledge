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

## 3. 路径边界

`paths` 是唯一构造运行时路径的模块。默认目录必须使用平台约定：配置、数据、日志、缓存、临时文件、dead-letter 和 service definition 分开管理。

安装目录、release 解压目录、当前工作目录和仓库目录不得默认承载运行时状态。用户显式配置路径时，`paths` 负责规范化、权限检查和诊断。

仓库局部 contract（例如 `.knowledge/knowledge-map.yaml`）不属于 runtime state。进程入口可以把 cwd 作为 bootstrap 输入，仓库根发现策略必须收敛在 `paths` 的 repository-root contract 中；application service 只能接收已经解析好的仓库根路径。

## 4. 网络和 QoS 边界

`net` 拥有所有网络能力；`net::http` 拥有 HTTP client/server；`net::qos` 拥有准入控制和资源预算。应用服务请求网络资源时只表达意图、来源、tenant、优先级和预算，不直接创建 socket 或 client。

QoS policy 至少覆盖：

- connection limit、request limit、body limit。
- per-source/per-tenant budget。
- timeout、cancellation、retry backoff。
- overload response 与 dropped work 观测。

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

## 7. 验收标准

- 仓库内只有 `env` 读取环境变量，只有 `paths` 构造运行时路径，只有 `net` 创建网络能力。
- CLI、Web、service 和 tests 能使用相同 typed runtime config。
- 配置错误在启动或 doctor 阶段被解释为稳定错误，不在业务路径中以 panic 暴露。

---

导航: 上一章: [2. 工程硬约束](02-engineering-hard-constraints.md) | 下一章: [4. Source Scope 模型](04-source-scope-model.md)
