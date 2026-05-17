# 后台服务、恢复与自愈

[中文](../../zh/03-architecture-specs/17-background-service-recovery-and-self-healing.md) | [English](../../en/03-architecture-specs/17-background-service-recovery-and-self-healing.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

后台服务不是 unmanaged CLI loop。长运行刷新、索引、维护、诊断和静默更新必须托管在平台 service manager 之下，并以有界资源、持久租约、启动调和和 dead-letter 保证可恢复。

## 2. 运行模式

| 平台 | 管理器 |
| --- | --- |
| Linux | systemd |
| macOS | launchd |
| Windows | Windows Service |

CLI 可以生成服务定义和执行 doctor，但不应伪装成后台常驻管理器。

## 3. 工作队列

所有后台任务都有 kind、scope、priority、budget、attempt、lease owner、lease expiry、target graph version、payload hash 和 last error。队列容量是硬上限；入队失败返回 overload/retryable error。

## 4. Reconciler

启动调和器负责：

- 重放 mutation log 中未完成的 index refresh。
- 回收过期 lease。
- 保留 dead-letter 隔离。
- 报告 index lag、queue depth、stale scope 和 failed cursor。
- 修正运行中 task 完成时 graph version 已前进的 cursor 状态。

## 5. 静默更新

静默更新必须用户可配置、可暂停、可观测、可回滚。它只能在授权 scope 内刷新图数据和派生索引，并暴露 fresh、stale、paused、degraded、failed 状态。
常驻本地文件索引遵守同一规则：扫描器只处理已配置的绝对路径 root，
在扫描前拒绝相对路径配置，持久化 cursor 和诊断，执行扫描/查询 timeout 预算，
报告被截断 root、扫描错误、新鲜度和 lag，不能阻塞查询路径，也不能静默扩大到未授权磁盘。

文件系统 watcher 和 scan worker 必须按平台能力降级：Windows 可使用 USN cursor，macOS 可使用 FSEvents cursor，Linux 可使用 inotify/fanotify 或定期 bounded rescan。事件 overflow、journal reset、权限变化、root missing 和 cursor invalidation 都进入可恢复诊断状态，而不是触发无界全盘扫描。

Overload 处理遵循 SRE 和 adaptive concurrency 原则：当队列、IO、CPU 或 provider budget 饱和时，系统优先拒绝新后台 work、延迟低优先级内容索引、保留查询热路径预算，并返回 retryable/paused/degraded 状态。

## 6. 验收标准

- 崩溃重启后不会丢失必要刷新工作。
- dead-letter task 不被诊断路径自动复活。
- 后台 CPU/IO-heavy work 不阻塞查询热路径。
- watcher lag、scan backlog、cursor invalidation 和 overload decision 可在 health/service doctor 中解释。

---

导航: 上一章: [16. 统一 API 与交互层架构](16-unified-api-and-interface-architecture.md) | 下一章: [18. 可观测性、诊断与 SLO](18-observability-diagnostics-and-slo.md)
