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

具名的平台进程输入同样必须经过该边界。进程 bootstrap 期间由 `env::windows_system_root_from_process` 捕获 Windows `SystemRoot`，`paths::windows_tasklist_command` 解析可执行文件，`RuntimeConfiguration::process` 再把结果传给服务恢复；应用工作流在恢复 worker 或调用 service manager 时既不得直接调用 `std::env`，也不得自行拼接平台可执行文件路径。

### 3.1 环境变量边界

`env` 内部按数据流保持单向依赖：`variables` 只拥有受支持的变量名，`error` 和 `overrides` 分别拥有稳定错误模型与 typed override 数据，`value_parser` 负责从已归一化 snapshot 提取并校验 path/string/bool/positive integer，`platform` 负责平台检测、大小写归一化、平台目录输入及 `SystemRoot` 进程读取，`config` 才能捕获完整进程环境并装配公开配置。`mod.rs` 仅维持原有 `env::*` facade，不得重新承载解析规则。对应 UT 必须分别放在 `config_tests`、`platform_tests` 和 `value_parser_tests`，使配置装配、平台规则和标量校验可以独立定位失败。

依赖方向固定为 `error`/`variables` → `value_parser` → `platform`，`overrides` 只组合这些 typed platform 数据，`config` 作为最外层依赖其余模块；不得让 error、override 或 variable catalog 反向依赖配置装配，也不得把 `std::env` 读取扩散到 `env` 目录外。

### 3.2 代码仓库应用工作流

`application::code_repository` 按用例划分内部所有权：`repository` 负责注册、删除、状态和报告，`index_workflow` 负责索引执行、持久任务租约、checkpoint 和 scope preview，`query` 负责版本化 scope 检索、特性开关和新鲜度诊断，`impact` 负责 diff 影响分析。这些模块通过同一个 `RelayKnowledgeService` 暴露稳定 API，并只向内依赖 `domain`、`code` 和 `storage` 合同；不得互相复制工作流或反向依赖 CLI、Web、MCP 等 adapter。

代码库理解视图统一收敛在 `application::code_repository::views` 目录：`service` 只编排 scope、新鲜度和响应，`architecture`、`business_domains`、`dependency_tour`、`process_flow`、`affected_scope` 分别拥有一种派生算法，`builder` 和 `rules` 提供有界构建及确定性分类规则。视图测试与所属目录共置，不再使用含义不清的 `views_*` 平铺文件名。

源码兜底检索统一收敛在 `application::code_repository::source_fallback` 目录：`execution` 是唯一 I/O 编排入口，`plan` 决定是否以及如何执行有界兜底，`identity`、`filters`、`scoring`、`results` 分别负责身份覆盖、请求约束、评分和结果归并，`imports`、`surface`、`worktree` 隔离特定证据边界。目录外不得直接依赖这些内部算法 helper。

代码仓库共享行为必须按明确职责拆分：`index_task` 负责持久任务租约和 worker 恢复，`index_state` 负责已持久化索引状态检查及复用，`scope` 负责 scope 解析和 filter 兼容性，`repository_status` 负责注册状态查询和 checkpoint 选择；`blocking`、`errors`、`clock` 分别隔离 runtime、错误和持久化时间边界。调用方必须直接依赖职责模块，不得重新引入含义宽泛的 `support`、`helper` 或 utility 聚合层。

### 3.3 仓库领域模型职责

仓库领域类型统一按功能收敛在 `domain::code::repository`：`registration` 负责注册、selector、range 和索引请求，`retrieval_request` 负责查询类型、限定词、结果上限和检索层，`indexed_records` 负责持久化文件、符号、引用、关系、诊断及 tombstone 记录，`repository_status` 负责状态、scope preview、汇总和报告，`retrieval_results` 负责查询与特性开关结果，`scope_identity` 是版本化快照 scope 编码的唯一所有者；`validation` 仅作为目录私有校验边界。不得恢复混合职责的 `repository.rs` 或 `repository_helpers.rs`。

### 3.4 模型提供方职责

`model_provider` 必须把 profile 归一化放在 `profile_config`，fallback policy 放在 `fallback`，持久 JSON 写入放在 `persistence`，provider HTTP 与响应诊断放在 `connectivity`，catalog 获取和 catalog 数据解释放在 `catalog`。跨模块协议测试统一放在 `protocol_tests`；不得把生产行为重新合并进通用 helper 模块。

### 3.5 依赖解析器职责

依赖解析必须按所解释的格式划分共享语法：`cargo_source` 分类 Cargo lock source，`npm_lock` 解释 npm 引用和 lock entry，`python_requirements` 解析 Python requirement 语法，`toml_inline_table` 读取 TOML 依赖字段，`gradle_notation` 解析 Gradle 调用和坐标。各生态解析器依赖这些窄模块，不得重新建立跨生态的 `support` 模块。

### 3.6 SQLite 存储边界

SQLite 存储必须把 evidence 与稳定 ID 生成放在 `evidence_identity`，mutation 读取放在 `mutation_log`，提交时有效期归一化放在 `graph_version`，诊断 row count 放在 `table_stats`。存储模块必须导入这些明确边界，不得把无关持久化行为累积到通用 helper 模块。

本地文件持久化统一收敛在 `storage::sqlite::file_index` 目录：`mod.rs` 负责 root lifecycle、文件元数据、path search 和聚合诊断，`content` 负责正文 entry、chunk、FTS、freshness cursor 及正文 search。只有 `file_index::content::search` 对 SQLite store adapter 可见，其余内容索引原语保持目录私有；`tests`、`content_tests`、`retirement_tests` 分别验证元数据、正文和退役行为，不得恢复平铺的 `file_index_*` 兄弟模块。

Graph canvas 持久化统一收敛在 `storage::sqlite::canvas` 目录：`mod.rs` 负责预算校验、knowledge graph 投影和 snapshot builder，`code` 只负责 code file/symbol/reference 与 source-path link 投影，`tests` 覆盖两种投影及 mixed canvas。代码投影 helper 保持 canvas 目录私有，不得恢复含义依赖文件名前缀的 `canvas_code` 顶层兄弟模块。

Code graph fact 持久化统一收敛在 `storage::sqlite::code_graph` 目录：`mod.rs` 负责 schema、受版本约束的 fact replacement/search、行解码和元数据校验，`tests` 验证同一个存储边界。不得把测试拆回 SQLite 根目录，也不得恢复重复前缀的 `code_graph_tests` 文件名。

Durable operations 持久化统一收敛在 `storage::sqlite::operations` 目录：`mod.rs` 负责 worker task、proposal/conflict、audit event、service operator state、相应行解码和稳定 task ID，`tests` 通过存储接口验证这些工作流。SQLite 根模块不得持有 operations 专属测试模块。

Index lifecycle 持久化统一收敛在 `storage::sqlite::indexing` 目录：`mod.rs` 负责 cursor state、refresh orchestration、校验与稳定 refresh-task identity，`cursor_metadata`、`schema`、`task_queue` 隔离各自职责，`refresh_tests`、`queue_tests`、`schema_migration_tests` 与被验证边界放在一起。不得把 index lifecycle 测试或带 `index_refresh_*` 前缀的实现文件放回 SQLite 根目录。

三层 graph retrieval 持久化统一收敛在 `storage::sqlite::retrieval` 目录：`mod.rs` 负责 schema 初始化、document materialization、检索协调和共享 scoring input，具名子模块分别负责 advanced graph path、BM25 与有界 fallback、context assembly、derived document、label trigram、schema migration、alias 和 ranking；定向测试与这些实现同目录。不得恢复父目录级 `retrieval_*` 文件，也不得用 path override 隐藏物理所有权边界。

Maven effective model 构建也必须拆开语法边界：`pom_path` 负责受仓库范围约束的相对 POM 解析，`property_interpolation` 负责有界递归属性展开；不得把两类规则重新合并到通用 Maven support 模块。

代码查询相关性统一收敛在 `storage::sqlite::code_query_relevance`：`tokens` 归一化查询词，`text_scoring`、`symbol_scoring`、`call_scoring` 分别负责各自排名域，`symbol_identity` 负责 scoped identity 匹配，`candidate_plan` 负责有界候选层，`filters` 和 `fts` 负责 SQL/FTS 构造。`mod.rs` 只作为内部相关性接口，不得恢复宽泛的 `code_query_support` 文件。

### 3.7 代码索引基础模块

跨代码索引流程的基础原语必须使用能表达职责的顶层模块：`content_identity` 负责稳定 ID 和内容哈希，`language_metadata` 负责语言检测及语言级元数据，`generated_detection` 负责生成源码分类。不得把无关原语归入 `common` 目录；新增原语必须归属其所描述的行为。

### 3.8 服务生命周期计划

服务生命周期必须按边界划分职责：`application::service::lifecycle_plan` 负责请求校验、install/upgrade/rollback/uninstall 步骤计划和执行编排；只有 `lifecycle_plan::platform_service` 可以选择平台服务定义文件名、渲染 systemd/launchd/Windows Service 定义、声明平台权限并生成 service manager 命令；`lifecycle_plan::execution` 负责阻塞文件和进程执行。平台渲染与命令转义不得重新并入生命周期步骤 planner。

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
