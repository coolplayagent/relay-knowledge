# 运维与 Worker 能力

[中文](./14-operations-and-worker-capabilities.md) | [English](../../en/02-capabilities/14-operations-and-worker-capabilities.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

运维能力把后台任务、人工 proposal、审计、静默更新和服务定义纳入同一 application API。它让系统可以在本地开发和常驻服务之间保持一致状态。

## 用户可见行为

```bash
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
relay-knowledge proposal accept <proposal-id> --by reviewer --reason reviewed
relay-knowledge audit query --limit 50 --format json
relay-knowledge service definition write --format json
relay-knowledge service operator pause
```

## 竞争力特性

Worker 可以调用外部 HTTP worker contract，也可以生成 deterministic fallback proposal。Proposal accept/reject/supersede 走同一图变更路径，服务管理器命令只生成平台 service definition，不执行特权安装。

## 命令/API 入口

Silent update operator status、pause、resume 和 service definition path 会出现在 service doctor 和 Web diagnostics 中。Agent audit 可以镜像到由 `paths` 管理的 JSONL sink。

## 降级与诊断

Worker 队列有界，失败进入 retry 或 dead-letter。Audit sink 默认关闭；开启后使用有界异步队列，不能阻塞 agent 请求热路径。

## 关联架构章节

- [后台服务、恢复与自愈](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)
- [安装、发布与升级](../03-architecture-specs/19-installation-release-and-upgrade.md)

---

导航: 上一章: [13. Agent 接入能力](13-agent-access-capabilities.md) | 下一章: [15. 评估与质量门禁](15-evaluation-and-quality-gates.md)
