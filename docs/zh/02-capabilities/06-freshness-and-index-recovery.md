# 新鲜度与索引恢复

[中文](./06-freshness-and-index-recovery.md) | [English](../../en/02-capabilities/06-freshness-and-index-recovery.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

新鲜度能力让用户知道检索结果对应哪个图版本和索引版本。系统不会把 stale index 伪装成 fresh，也不会让后台刷新无限制增长。

## 用户可见行为

- `freshness` 支持 `allow-stale`、`wait-until-fresh` 和 `graph-only`。
- Query、health 和 index refresh 响应返回 `index_cursors[*]`。
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

## 文件监听 (fs.watch) 增量索引

系统支持通过文件系统监听实现近实时增量索引更新。当源代码文件发生变更时，watcher 自动检测变更并将增量索引任务推送到持久化任务队列。

### 配置

通过环境变量控制：

| 环境变量 | 默认值 | 说明 |
|---------|--------|------|
| `RELAY_KNOWLEDGE_WATCHER_ENABLED` | `true` | 启用/禁用文件监听 |
| `RELAY_KNOWLEDGE_WATCHER_DEBOUNCE_MS` | `3000` | 事件合并窗口（毫秒）|
| `RELAY_KNOWLEDGE_WATCHER_MAX_WATCH_DIRS` | `1024` | 最大监听目录数 |
| `RELAY_KNOWLEDGE_WATCHER_HASH_CACHE_CAPACITY` | `4096` | 内容哈希缓存容量 |

### 工作原理

1. **事件检测**：使用 `notify` crate 跨平台（Linux inotify、macOS FSEvents、Windows ReadDirectoryChangesW）检测文件创建/修改/删除
2. **事件去抖**：在可配置的时间窗口内合并快速连续的文件变更事件
3. **内容哈希过滤**：通过 FNV-1a 内容哈希跳过无实际内容变化的保存操作
4. **路径过滤**：自动忽略 `.git/`、`target/`、`node_modules/`、`__pycache__/` 等目录和二进制文件
5. **增量任务生成**：变更文件通过 `build_incremental_task_seed` 生成 `CodeIndexTaskSeed`（WorktreeOverlay 模式），进入持久化任务队列

### 状态监控

Watcher 状态通过 `service status` API 暴露，包含以下诊断信息：

- `state`：disabled / active / degraded / failed
- `watched_repository_count`：正在监听的仓库数量
- `total_events_received`：接收到的文件变更事件总数
- `total_events_filtered`：被过滤掉的事件数量
- `total_index_tasks_queued`：生成的增量索引任务数量
- `degraded_reason`：降级原因（如超出监听目录上限）

### 资源保护

- 通过 `max_watch_dirs` 限制防止 inotify/fd 耗尽
- 监听失败时自动降级（Degraded 状态），不影响查询热路径
- 不支持的平台自动禁用（Disabled 状态）

## 关联架构章节

- [派生索引与新鲜度](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [后台服务、恢复与自愈](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)

---

导航: 上一章: [5. 混合检索竞争力](05-hybrid-retrieval-advantage.md) | 下一章: [7. 多模态证据能力](07-multimodal-evidence-capability.md)
