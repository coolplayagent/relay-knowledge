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
- 每条代码检索结果携带 `staleness_hint` 字段，与遗留 `stale` 布尔并存。当前状态包括 `{ "state": "fresh" }`、`{ "state": "pending_index" }` 和 `{ "state": "stale" }`；`pending_index` 表示匹配的刷新任务仍在 queued、running 或 retrying，调用方应先读取直接源文件再信任该命中。代码图落库逐文件修改时间和索引时间之前，不填充逐文件时间戳 payload。

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

驻留服务会为已注册代码仓库检测源码变化和 checked-out Git commit 推进。两条路径都只向持久化 code-index 队列提交任务，不会从 watcher event loop 直接写图谱状态。

### 配置

通过环境变量控制：

| 环境变量 | 默认值 | 说明 |
|---------|--------|------|
| `RELAY_KNOWLEDGE_WATCHER_ENABLED` | `true` | 启用/禁用文件监听 |
| `RELAY_KNOWLEDGE_WATCHER_DEBOUNCE_MS` | `3000` | 事件合并窗口（毫秒）|
| `RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS` | `5000` | checked-out `HEAD` 的有界周期对账间隔（毫秒）|
| `RELAY_KNOWLEDGE_WATCHER_MAX_WATCH_DIRS` | `1024` | 最大监听目录数 |
| `RELAY_KNOWLEDGE_WATCHER_HASH_CACHE_CAPACITY` | `4096` | 内容哈希缓存容量 |

### 工作原理

1. **事件检测**：使用 `notify` crate 跨平台（Linux inotify、macOS FSEvents、Windows ReadDirectoryChangesW）检测文件创建/修改/删除；`.git/HEAD`、refs、packed refs 和 HEAD log 事件只作为低延迟 commit hint
2. **事件去抖**：在可配置的时间窗口内合并快速连续的文件变更事件
3. **内容哈希过滤**：通过 FNV-1a 内容哈希跳过无实际内容变化的保存操作
4. **作用域过滤**：忽略普通 `.git/` 内容、`target/`、`node_modules/`、`__pycache__/` 等目录和二进制文件，仅放行上述窄 Git ref hint；然后按每个仓库作用域自己的 path/language filter 判断是否生成 overlay 任务
5. **首轮索引保护**：只有已经完成全量索引、拥有 `last_indexed_scope_id` 且不是 stale 的仓库才会进入 watcher，避免 worktree overlay 生成不完整的首轮索引或覆盖 stale 重配置状态
6. **工作树任务生成**：变更源码生成 `WorktreeOverlay` 模式的 `CodeIndexTaskSeed`；overlay fingerprint 包含 changed-path set 和 content generation
7. **Commit 对账**：启动时及每个受界 interval 在 async hot path 之外解析当前 `HEAD` 与 tree。该恢复依据覆盖 linked worktree，以及原生 event 漏报或合并。HEAD 推进时，将最近发布 clean base 和已解析 head/tree 固定进 `Incremental` task；稳定的 per-repository/ref/filter fingerprint 会在任务槽未完成时合并重复 hint
8. **持久发布**：full、手动 incremental 和 watcher incremental 共用 queue、attempt-scoped lease、有界 retry/backoff、dead-letter、每仓库单 writer claim 和 publication ordering。每次 claim 都推进 repository-local generation，并在每个 SQLite 发布事务内校验，因此过期后游离的 attempt 不能在接管之后提交。full rebuild 另有 batch checkpoint；受界 incremental/worktree attempt 在单个 snapshot transaction 内发布，以 task state 而不是逐路径 checkpoint 表达进度。启动及后续 tick 会在 crash 后重放 lag；发布完成前继续读取旧 fresh scope
9. **仓库生命周期同步**：服务运行期间注册、刷新或删除仓库时，通过 watcher command channel 执行 watch/update/unwatch；多个仓库作用域可以共享同一个 root 目录，同时仍作为独立目标保留；底层监听失败会进入 degraded 诊断，而不是只更新内存列表

### 状态监控

Watcher 状态通过 `service status` API 暴露，包含以下诊断信息：

- `state`：disabled / active / degraded / failed
- `enabled`：watcher 的实际配置开关，包括没有 live watcher object 的 disabled runtime
- `commit_reconcile_interval_ms`：生效的受管理 HEAD reconciliation interval
- `watched_repository_count`：正在监听的仓库数量
- `total_events_received`：接收到的文件变更事件总数
- `total_events_filtered`：被过滤掉的事件数量
- `total_index_tasks_queued`：生成的增量索引任务数量
- `total_commit_reconciliations`：执行 commit 对账的仓库次数
- `total_commit_tasks_queued`：已持久接受的 commit 更新任务数
- `total_commit_reconcile_failures`：受界 Git 解析或入队失败数
- `total_events_dropped`：有界 debounce channel 满或关闭时丢弃的事件数量
- `degraded_reason`：降级原因（如超出监听目录上限）

### 资源保护

- 通过 `max_watch_dirs` 限制防止 inotify/fd 耗尽
- debounce event channel 和 watcher command channel 都是有界队列
- 内容哈希缓存只会在匹配的 worktree-overlay 任务成功持久化入队后推进，因此临时队列失败仍可由下一次相同文件事件重试
- 任务入队失败会将 watcher 标记为 degraded；已持久化接受的任务仍沿用现有 worker retry/dead-letter 机制
- 监听失败时自动降级（Degraded 状态），不影响查询热路径
- 不支持的平台自动禁用（Disabled 状态）

### 淘汰与恢复

成功发布后先计算 protected set：保留 active scope 与最近两个成功发布时间窗口的并集（窗口通常已包含 active）、最近一次成功增量的 predecessor、active worktree overlay 的 clean base，再加每个未完成 task 的 target/base 和 repository-set member pin。未保护 scope 会先原子标为 `retiring` 并退出查询，再由 durable job 分阶段删除旧代码事实、FTS/search row、software projection、checkpoint、workspace state 与 scope metadata；每个 maintenance transaction 只推进一个 scope-GC phase，该 phase 在受影响的应用表之间合计最多删除 512 个物理行。同 tree commit 复用内容图，并使用每仓 256 条的 commit alias 窗口。完成态 task 历史限制为每仓库 128 条 succeeded 和 64 条 failed/dead-letter/cancelled，同时保留每个 retained scope 的最新 success。状态会暴露 maintenance 进度与错误；已淘汰 ref 必须 full reindex。

这个 publication barrier 刷新当前 source scope 的代码仓库事实、对应 FTS/search document 和由其派生的软件全域模型投影；它不宣称 repository-agnostic Knowledge Graph 或独立 semantic/vector generation 与代码 scope 原子发布。

## 关联架构章节

- [派生索引与新鲜度](../03-architecture-specs/08-derived-indexes-and-freshness.md)
- [后台服务、恢复与自愈](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)

---

导航: 上一章: [5. 混合检索竞争力](05-hybrid-retrieval-advantage.md) | 下一章: [7. 多模态证据能力](07-multimodal-evidence-capability.md)
