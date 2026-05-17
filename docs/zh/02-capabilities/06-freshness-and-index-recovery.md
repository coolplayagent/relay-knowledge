# 新鲜度与索引恢复

[中文](./06-freshness-and-index-recovery.md) | [English](../../en/02-capabilities/06-freshness-and-index-recovery.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

新鲜度能力让用户知道检索结果对应哪个图版本和索引版本。系统不会把 stale index 伪装成 fresh，也不会让后台刷新无限制增长。

## 用户可见行为

- `freshness` 支持 `allow-stale`、`wait-until-fresh` 和 `graph-only`。
- Health 和 index refresh 响应返回 `index_cursors[*]`。
- `index_refresh.stale_reasons[*]` 按 index family 和 scoped cursor 解释 lag、failure 和 last error。
- Ingest、query、index refresh、health、service doctor 和 service startup 共享 bounded refresh queue。

## 竞争力特性

许多 RAG 系统只告诉用户“有结果”。本系统会说明结果是否新鲜、哪个 backend 落后、哪个 scope stale、是否 dead-letter，以及显式 refresh 是否因为 queue capacity 失败。

## 命令/API 入口

```bash
relay-knowledge index refresh --kind bm25 --format json
relay-knowledge query SQLite --freshness wait-until-fresh --format json
relay-knowledge health --format json
```

## 降级与诊断

常见状态包括 stale index、graph-only、backend unavailable、semantic/vector degraded、failed cursor 和 dead-letter。诊断 reconciler 不会自动复活 dead-letter task，只有显式 retry/refresh 路径可以处理。

## 关联架构章节

- [派生索引与新鲜度](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [后台服务、恢复与自愈](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)

---

导航: 上一章: [5. 混合检索竞争力](05-hybrid-retrieval-advantage.md) | 下一章: [7. 多模态证据能力](07-multimodal-evidence-capability.md)
