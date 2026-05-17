# 本地优先运行时与 CLI

[中文](./02-local-first-runtime-and-cli.md) | [English](../../en/02-capabilities/02-local-first-runtime-and-cli.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

本地优先运行时是所有能力的起点。用户不需要先部署数据库、向量服务或后台系统，就可以构建、运行、写入 evidence、查询和查看健康状态。

## 用户可见行为

- `relay-knowledge status` 返回项目身份、运行时目录、存储状态和基础能力概览。
- `relay-knowledge help --format json` 暴露机器可读命令契约，方便脚本和 LLM 工具先读后调用。
- `relay-knowledge health --format json` 和 `service doctor` 返回统一诊断，而不是各自拼装状态。
- 默认 profile 使用本地 SQLite 和确定性 semantic/vector read model。

## 竞争力特性

本地优先不是简化版。它保留 graph version、index freshness、scope、QoS、worker queue 和 agent audit 的同一套语义，因此从 CLI 切换到 Web 或服务模式时不会出现行为分叉。

## 命令/API 入口

```bash
relay-knowledge status --format json
relay-knowledge help --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

CLI 输出中的 command path、读写影响、必填参数、默认值、允许值、示例和 notes 是公开契约的一部分。

## 降级与诊断

配置错误应在 status、health 或 doctor 阶段解释。外部 provider 未配置不影响本地默认查询；服务未安装不影响 CLI 本地运行。

## 关联架构章节

- [基础运行时层](../03-architecture-specs/03-foundational-runtime.md)
- [统一 API 与交互层架构](../03-architecture-specs/16-unified-api-and-interface-architecture.md)

---

导航: 上一章: [1. 能力版图总览](01-capability-overview.md) | 下一章: [3. 证据与图事实](03-evidence-and-graph-facts.md)
