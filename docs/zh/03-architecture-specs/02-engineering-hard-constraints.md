# 工程硬约束

[中文](../../zh/03-architecture-specs/02-engineering-hard-constraints.md) | [English](../../en/03-architecture-specs/02-engineering-hard-constraints.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

本章是第三卷的硬合同。任何实现、文档、测试、发布或运维变更都必须满足这些约束；它们不是建议，也不能用“后续补齐”绕过。

先进性不是靠复杂组件堆叠，而是靠边界清晰、依赖无环、状态可恢复、资源有界和行为可验证。

## 2. 架构硬约束

- **异步优先**：I/O、图数据库访问、索引刷新、摄取和服务编排必须暴露 async API。
- **热路径不阻塞**：CPU 重、磁盘重或阻塞工作必须放入显式 worker、维护任务或 blocking boundary。
- **有界资源**：事件 pipeline、网络入口、索引刷新和后台任务必须有 queue depth、budget、timeout、cancellation、backpressure 和 overload behavior。
- **事实与读模型分离**：GraphStore 是事实真源；BM25、semantic、vector、summary、community 和 code index 都是派生读模型。
- **无环依赖**：crate、module、trait、service、adapter 和 config object 之间不得形成循环依赖。
- **代码源目录权威清晰**：Git 管理的代码仓库必须以 tracked tree 作为索引目录权威，不能只因 `build/`、`dist/`、`vendor/` 或 `third_party/` 等目录名跳过已跟踪源码；非 Git source directory 默认必须使用源码/配置/文档白名单扫描，避免把构建产物、缓存和依赖副本纳入索引，宽泛目录只能通过显式 path opt in 进入对应目录。非 Git `src` 这类窄 path 不能顺带 opt in 兄弟级宽泛目录，也不能在选择前遍历无关 filtered sibling；未带 path filter 的非 Git 扫描不能遍历不会贡献默认白名单内容的目录；`--path .` 是宽泛目录 whole-root opt in。真实 Git metadata 上的探测失败不能静默回退为 filesystem indexing，source fallback 不能为 stale scoped `filesystem:` commit 读取 live 文件。非 Git synthetic hash 必须来自 source-layout discovery 后的有效 indexed scope，非 Git pre-scope hash 不能读取 file preset 排除的文件，除非显式 path filter opt in 到该文件；非 Git ref resolution、source fallback 校验和 impact path collection 必须包含有效 path 和 language filters，排队 synthetic ref、同步 full-snapshot read、full-index batch 以及 delta 读取 live bytes 前都必须校验，非 Git 文件 byte/hash/metadata materialization 必须拒绝最终路径和祖先目录 symlink 替换，显式已存储 `filesystem:` ref 及其 source fallback 校验、impact collection、impact partition 和 deleted-symbol extraction 必须先于动态 source-kind 或 Git 探测走 filesystem scope 身份解析，repository-set 的更窄 filter 成员和 freshness check 必须复用兼容的更宽非 Git scope，显式非 Git incremental `base_ref` 必须加载该已存储 base scope，增量删除必须覆盖上一版 discovered root，active non-Git task matching 必须用 task 的有效 filters 比较更窄 stale read，非 Git impact path 在 scoped base/head ref 相同时必须返回空 changeset，Git ref normalization 和 fresh full-index check 都不能执行 full tree walk。
- **高性能必须泛化**：优化必须来自数据结构、ranking signal、索引策略、query planning、batching、并发边界或存储布局，不能枚举已知 query、path、symbol 或 fixture。

## 3. 基础模块所有权

| 模块 | 唯一职责 | 禁止事项 |
| --- | --- | --- |
| `env` | 环境变量读取、解析、校验、脱敏诊断 | 其他模块直接读取环境变量 |
| `paths` | 平台路径、运行时目录、数据/日志/缓存目录 | 其他模块拼接运行时路径 |
| `net` | socket、HTTP client/server、listener、网络 loop | 其他模块创建网络能力 |
| `net::http` | 基于成熟 async runtime/library 的 HTTP | blocking socket、thread-per-connection、busy polling |
| `net::qos` | 准入控制、租户/来源限额、优先级、预算、overload metric | 绕过 QoS 消耗无界资源 |

## 4. HTTP 与 QoS

HTTP 必须建立在非阻塞 OS event mechanism 之上，例如 epoll、kqueue 或 IOCP 经由成熟 async runtime 暴露。所有 inbound/outbound 网络工作在消耗资源前都必须经过 QoS policy。

网络入口必须支持：连接预算、请求预算、body 大小限制、timeout、cancellation、graceful shutdown、rate limit、queue depth metric、drop metric 和 overload response。

## 5. 代码质量硬约束

- tracked source、test、documentation、script 或 workflow 文件不得超过 1000 行。locked build 必需的生成式 release lockfile 例外，当前为 `Cargo.lock`，且必须保持机器生成。
- 不添加 shallow function；函数必须负责校验、转换、外部边界、资源生命周期、错误映射、观测或真实编排。
- 不保留 dead code、TODO stub、无调用公共 API、无测试 speculative extension point 或注释掉的实现。
- 项目身份常量集中在 `project` 模块；模块局部运行默认值留在所属模块。
- `unsafe` 默认禁止，除非有明确边界、理由和测试。

## 5.1 文件监听 (fs.watch) 约束

- 文件监听通过 `notify` crate 实现跨平台支持（Linux inotify、macOS FSEvents、Windows ReadDirectoryChangesW）。
- 监听事件必须经过 debounce 窗口合并，防止高频文件变更产生无界任务。
- 内容哈希过滤（`ContentHashCache`）必须过滤无实际内容变化的保存操作。
- `max_watch_dirs` 必须限制最大监听目录数，防止 fd/inotify watch 资源耗尽。
- 监听失败时必须自动降级（`Degraded` 状态），不得影响查询热路径或 async runtime。
- Watcher 配置必须通过 `env` 模块的环境变量覆盖机制加载，不得在其他模块直接读取。
- Watcher 状态和诊断信息必须通过 `service status` API 暴露。
- 增量索引任务（`CodeIndexTaskSeed`）必须进入持久化任务队列，不得跳过 durable task lease、checkpoint 和 bounded retry。

## 6. 文档与测试硬约束

- 任何代码、配置、行为、测试、workflow、benchmark、安装或运维变更都必须同时刷新对应文档。
- Unit test 与 integration test gate 分离。
- Rust 行覆盖率必须保持 90% 以上，覆盖 invariant、错误分支、边界值、async cancellation 和 backpressure。
- Browser integration gate 必须安装 Playwright Chromium，例如 `uv run --extra dev python -m playwright install --with-deps chromium`。
- 文档本身需要检查链接、编号、行数上限和过期状态。

## 7. 验收标准

- 新模块能说明它属于哪个所有权边界，以及为什么不会形成循环依赖。
- 新 background 或 network 行为能说明资源预算、失败模式、取消和观测指标。
- 新检索或性能优化能说明泛化机制，而不是只解释某个样例为什么通过。

---

导航: 上一章: [1. 架构愿景与算法版图](01-architecture-vision-and-algorithm-map.md) | 下一章: [3. 基础运行时层](03-foundational-runtime.md)
