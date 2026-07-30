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

HTTP 基础边界必须完整收敛在 `net/http/`：`mod.rs` 维护配置、client/server runtime、timeout、cancellation 和 graceful shutdown，`qos_admission.rs` 与 `qos_client.rs` 分别隔离 inbound/outbound QoS，`mod_tests.rs` 验证该 facade。`net/` 父目录不得重新出现 `http.rs` 或 `http_tests.rs`。

### 3.1 环境变量边界

`env` 内部按数据流保持单向依赖：`variables` 只拥有受支持的变量名，`error` 和 `overrides` 分别拥有稳定错误模型与 typed override 数据，`value_parser` 负责从已归一化 snapshot 提取并校验 path/string/bool/positive integer，`platform` 负责平台检测、大小写归一化、平台目录输入及 `SystemRoot` 进程读取，`config` 才能捕获完整进程环境并装配公开配置。`mod.rs` 仅维持原有 `env::*` facade，不得重新承载解析规则。对应 UT 必须分别放在同级 `config_tests`、`platform_tests` 和 `value_parser_tests` 文件，并由匹配的实现 owner 而非 facade 显式挂载，使配置装配、平台规则和标量校验可以独立定位失败。

依赖方向固定为 `error`/`variables` → `value_parser` → `platform`，`overrides` 只组合这些 typed platform 数据，`config` 作为最外层依赖其余模块；不得让 error、override 或 variable catalog 反向依赖配置装配，也不得把 `std::env` 读取扩散到 `env` 目录外。

### 3.2 代码仓库应用工作流

`application::code_repository` 按用例划分内部所有权：`repository` 目录负责注册、删除、状态、报告、staleness 标注和 worktree overlay 校验，`indexing` 负责索引执行、持久任务租约、checkpoint、scope preview 和 worker 管理，`query` 负责版本化 scope 检索、特性开关和新鲜度诊断，`impact` 负责 diff 影响分析。这些模块通过同一个 `RelayKnowledgeService` 暴露稳定 API，并只向内依赖 `domain`、`code` 和 `storage` 合同；不得互相复制工作流或反向依赖 CLI、Web、MCP 等 adapter。

Web adapter 统一收敛在 `interfaces::web`：`mod.rs` 负责 router composition 和共享 response/error 边界，`code_api`、`code_index_request`、`code_view_request`、`files`、`model_config` 负责各自命名的 HTTP contract，定向测试保持同目录。`code_api_integration_tests` 与 `files_integration_tests` 会运行装配后的 router 和共享 application service，因此由 facade 显式挂载；实现局部 UT 仍与精确 owner 一一配对。不得恢复根目录级 `web_*` 兄弟文件，也不得从该 adapter 打开 socket。

MCP adapter 在物理上统一收敛到 `interfaces::agent::mcp`：`mod.rs` 负责 server composition、streamable-HTTP dispatch、QoS admission、cancellation 与 tool coordination；`json_rpc` 负责 protocol initialization validation、session/error envelope、response encoding 和 typed request-ID identity；`tool_contract` 负责 freshness parsing、argument/domain error mapping、MCP request context 和稳定 tool result envelope；其余具名子模块分别拥有 audit、HTTP contract、resources、prompts、state、authorization、registry 与 code-tool 行为。`json_rpc_tests` 和 `tool_contract_tests` 与各自 owner 一一配对，不得把 protocol 与 tool-mapping primitive 累积回根 facade 测试。`code_tools` 子树由 `mod.rs` 维护 tool dispatch 与 argument mapping，`agent_budget` 维护有界 agent output policy，`codebase_view` 维护派生 codebase-view 执行；facade 与预算测试分别以 `mod_tests` 和 `agent_budget_tests` 与 owner 共置。根 adapter 测试以 `mod_tests`、`protocol_tests`、`tool_tests`、`software_tool_tests`、`feature_flag_tool_tests` 和 `runtime_guardrail_tests` 与 facade 共置，可复用的测试存储和 HTTP transport fixture 明确命名为 `test_support` 与 `transport_harness`。不得在这些所有权边界外恢复根目录级 `mcp_*` 文件、同级 `code_tools.rs` 或 issue 编号测试模块名。

CLI adapter 统一收敛在 `interfaces::cli`：`mod.rs` 负责全局 option 解析、dispatch 和稳定公开 CLI surface，`spec` 负责 machine-readable command contract，`render` 负责输出序列化，`repo`、`repo_set`、`setup` 负责各自命令族，解析、命名、remote、service、map、version 定向测试放在 `tests`。需要 white-box 访问或兼容性时，命令模块可保留既有逻辑名称，但禁止恢复根目录级 `*_cli` 前缀桶。

代码库理解视图统一收敛在 `application::code_repository::views` 目录：`service` 只编排 scope、新鲜度和响应，`architecture`、`business_domains`、`dependency_tour`、`process_flow`、`affected_scope` 分别拥有一种派生算法，`builder` 和 `rules` 提供有界构建及确定性分类规则。聚焦 UT 与实现 owner 一一配对；`affected_scope_integration_tests` 和 `dependency_tour_integration_tests` 同时覆盖 service dispatch、builder、rules 与派生算法，因此由 facade 显式拥有。视图测试与所属目录共置，不再使用含义不清的 `views_*` 平铺文件名。

源码兜底检索统一收敛在 `application::code_repository::source_fallback` 目录：`execution` 是唯一 I/O 编排入口，`plan` 决定是否以及如何执行有界兜底，`identity`、`filters`、`scoring`、`results` 分别负责身份覆盖、请求约束、评分和结果归并，`imports`、`surface`、`worktree` 隔离特定证据边界。`surface_integration_tests` 同时验证 `plan`、`results` 与 `surface` 的组合，因此由 facade 显式拥有；聚焦 UT 仍与精确实现 owner 一一配对。目录外不得直接依赖这些内部算法 helper。

`indexing` 目录是严格的工作流边界：`mod.rs` 编排全量与增量执行，`state` 负责已持久化索引状态检查及复用，`task` 负责持久租约与 worker 恢复，`queue` 负责有界 overlay 任务提交，`fast_path` 负责经过校验的新鲜索引复用，`tasks` 负责任务管理。目录只向父模块暴露仓库注册所需的租约恢复操作，内部索引 helper 不得泄漏到查询或 adapter。`repository` 目录同样由 `mod.rs` 承载服务实现，`status` 负责注册状态和 checkpoint 选择，`staleness` 负责结果新鲜度标注，`worktree` 负责 overlay 校验，白盒 fixture 与所属行为保持共置。共享的 `scope` 继续负责 scope 解析和 filter 兼容性，`blocking`、`errors`、`clock`、`worktree_ref` 分别隔离 runtime、错误、持久化时间和 overlay 身份边界。不得恢复根级 `repository_*`、`worktree_freshness`、`index_*`、`fast_index`、`queue` 或 `tasks` 文件桶。

### 3.3 仓库领域模型职责

仓库领域类型统一按功能收敛在 `domain::code::repository`：`registration` 负责注册、selector、range 和索引请求，`retrieval_request` 负责查询类型、限定词、结果上限和检索层，`indexed_records` 负责持久化文件、符号、引用、关系、诊断及 tombstone 记录，`repository_status` 负责状态、scope preview、汇总和报告，`retrieval_results` 负责查询与特性开关结果，`scope_identity` 是版本化快照 scope 编码的唯一所有者；`validation` 仅作为目录私有校验边界。不得恢复混合职责的 `repository.rs` 或 `repository_helpers.rs`。

### 3.4 模型提供方职责

`model_provider` 必须把 profile 归一化放在 `profile_config`，fallback policy 放在 `fallback`，持久 JSON 写入放在 `persistence`，provider HTTP 与响应诊断放在 `connectivity`，catalog 获取和 catalog 数据解释放在 `catalog`。跨模块协议测试统一放在 `protocol_tests`；不得把生产行为重新合并进通用 helper 模块。

### 3.5 依赖解析器职责

依赖解析必须按所解释的格式划分共享语法：`cargo_source` 分类 Cargo lock source，`npm_lock` 解释 npm 引用和 lock entry，`python_requirements` 解析 Python requirement 语法，`toml_inline_table` 读取 TOML 依赖字段，`gradle_notation` 解析 Gradle 调用和坐标。各生态解析器依赖这些窄模块，不得重新建立跨生态的 `support` 模块。

依赖解析器必须完整收敛在 `code/parser/dependencies/`：`mod.rs` 维护 manifest 分类、跨生态分派和稳定 fact 装配，生态解析器与共享格式原语使用职责命名文件，`mod_tests.rs` 验证该 facade。父级 parser 目录不得重新出现同名 `dependencies.rs`，也不得用父级相对 `#[path]` 隐藏物理所有权。

C/C++ parse recovery 必须完整收敛在 `code/parser/recovery/`：`mod.rs` 负责有界 recovery 判定与 declaration-shape 校验，`scan.rs` 负责 literal-aware code scanning，`line_classification.rs` 负责 recoverable line 分类，`type_body.rs` 负责 decorated-type body 校验。聚焦的语言与 recovery 单元保留配对 `mod_tests` 或实现具名测试；C/C++ `parser_integration_tests` 与 `gcc_recovery_integration_tests` 同时覆盖 language adapter、syntax parsing 和 recovery 的完整解析入口，因此由 parser facade 显式拥有。parser 父目录不得重新出现 `recovery.rs`；language adapter 可以使用这个窄 recovery contract，但不得复制其中的规则。

路由检测的语法辅助必须归解释该语法的 framework 或 lexical layer 所有。Express 子域完整收敛在 `detect/express/`：`mod.rs` 负责 detection 编排，`arguments` 负责引号路径、middleware 尾部选择、callback array 与具名 handler 校验，`bindings` 负责 ESM/CommonJS namespace、Router factory alias、application/router 赋值识别和 identifier 校验，`syntax` 负责 Express method/path、嵌套 call/array、receiver、顶层参数和 URL 合并原语，`statements` 负责有界多行 registration/mount 聚合与 literal-aware call closure，`mounts` 负责 `.use(...)` 发现、静态/动态前缀分类、router array 展开与 mount 记录，`registrations` 负责直接与链式 method 识别以及 route-info 和 handler 映射，`materialize` 负责 mount prefix 传播、动态前缀过滤与结果去重。八个 owner 都必须直接挂载聚焦同级测试，`detect/` 父目录不得恢复 `express.rs`、`express_arguments.rs` 或 `express_materialize.rs` 平铺文件。Spring 路由检测必须完整收敛在 `detect/spring/`：`mod.rs` 负责编排与 Spring scope 状态，`java` 负责 comment/text-block 过滤、literal-aware brace depth 与 Java type/method declaration 识别，`attributes` 负责 positional/named path value、array、拼接拒绝、字符串解码与 request-method attribute。三个 owner 都直接挂载同级测试，父目录不得恢复平铺 `spring.rs`。`detect::python_strings` 负责 Python 静态字符串前缀和 escape 处理；`detect::javascript` 负责 JavaScript 注释、字符串与正则词法状态。禁止恢复通用 `detect::shared` 模块，因为 JavaScript callback 与 Python string 不共享同一个语义合同。

### 3.6 SQLite 存储边界

SQLite 存储必须把 evidence 与稳定 ID 生成放在 `evidence_identity`，mutation 读取放在 `mutation_log`，提交时有效期归一化放在 `graph_version`，诊断 row count 放在 `table_stats`。存储模块必须导入这些明确边界，不得把无关持久化行为累积到通用 helper 模块。

SQLite adapter 根模块必须完整收敛在 `storage/sqlite/`：`mod.rs` 负责 `SqliteGraphStore`、有界 blocking-worker 入口、schema 编排、graph-fact 校验与根测试声明，具体持久化职责由命名明确的子模块维护。`storage/` 父目录不得重新出现 `sqlite.rs`；根测试模块必须与 `sqlite/mod.rs` 共置，不得使用带 `sqlite/` 前缀的路径重定向。

根 graph-store 行为由同级 `graph_storage_tests.rs` 验证；retrieval schema migration 与 BM25 fallback integration 场景收敛在职责明确的 `graph_retrieval_tests` 目录，并由 graph-storage 测试 owner 声明。不得恢复含糊的 `graph_tests.rs` 加 `graph_tests/` 组合，也不得把这些 graph-store 场景混入 code-graph fact 测试。

SQLite connection lifecycle 必须在逻辑与物理上统一收敛到 `storage::sqlite::connection_runtime`：`maintenance` 负责 connection pragma、WAL checkpoint 与 maintenance diagnostics，`read_pool` 负责有界读连接 lane 与锁等待 deadline，`retry` 负责有界 transient-SQLite retry policy。SQLite 持久化模块必须通过 `connection_runtime` 使用这些能力，不得将它们重新平铺到 SQLite 根模块。

Partitioned SQLite adapter 必须完整收敛在 `storage/partitioned/`：`mod.rs` 维护公开 store 与 trait 实现，catalog、control delegate、diagnostics、retention、routing、status 和 totals 使用职责命名文件，`mod_tests.rs` 验证跨 shard contract。`storage/` 父目录不得重新出现 `partitioned.rs`、`partitioned_tests.rs` 或指向该子域的相对 `#[path]`。

Software projection 持久化必须在物理与逻辑上完整收敛到 `storage::sqlite::software`：SQLite 根模块声明该域，code-store adapter 以兄弟模块导入它，不得再通过相对路径持有该域。`mod.rs` 负责 schema 与 projection 编排，`graph.rs` 负责从图派生的 file、topic 和 relationship 物化与查询，dependency usage、lifecycle 和 query scope 保持各自职责模块。SQLite 根级 `scope_filters.rs` 统一维护 code retrieval 与 software projection 共享的 indexed-scope 覆盖判定，两个域不得导入对方的私有 helper；对应 path、language 与 indexed-scope 不变量由同级 `scope_filters_tests.rs` 维护。`mod_tests.rs` 验证根 projection 生命周期，`projection_tests.rs` 验证带过滤条件的 projection 读取。`storage/sqlite/` 父目录不得重新出现 `software.rs`、`software_graph.rs` 或 software 根测试文件。

Maven effective-model 解析必须完整收敛在 `storage/sqlite/maven/model/`：`mod.rs` 负责 document resolution 与 inheritance 编排，`parse.rs` 负责 POM 解码，`effective.rs` 负责 effective dependency、plugin、profile 和 property 构造。Maven 父目录不得重新出现 `model.rs` 或指向模型域的相对 `#[path]`。

Code-query 核心 white-box 测试必须收敛在 `storage/sqlite/code_query/tests/unit/`：`mod.rs` 负责通用 query planning、fallback、ranking 与 outage 不变量，`case_intent_tests.rs` 负责 case-intent fixture 族。`tests` 父模块必须以 `unit` 声明该测试组，不得恢复笼统的 `test_modules::tests` 身份、同级 `unit.rs` 或指向 unit 测试组的相对路径重定向。

本地文件持久化统一收敛在 `storage::sqlite::file_index` 目录：`mod.rs` 负责 root lifecycle、文件元数据、path search 和聚合诊断，`content` 负责正文 entry、chunk、FTS、freshness cursor 及正文 search。只有 `file_index::content::search` 对 SQLite store adapter 可见，其余内容索引原语保持目录私有；`tests`、`content_tests`、`retirement_tests` 分别验证元数据、正文和退役行为，不得恢复平铺的 `file_index_*` 兄弟模块。

Graph canvas 持久化统一收敛在 `storage::sqlite::canvas` 目录：`mod.rs` 负责预算校验、knowledge graph 投影和 snapshot builder，`code` 只负责 code file/symbol/reference 与 source-path link 投影，`tests` 覆盖两种投影及 mixed canvas。代码投影 helper 保持 canvas 目录私有，不得恢复含义依赖文件名前缀的 `canvas_code` 顶层兄弟模块。

Code graph fact 持久化统一收敛在 `storage::sqlite::code_graph` 目录：`mod.rs` 负责 schema、受版本约束的 fact replacement/search、行解码和元数据校验，`tests` 验证同一个存储边界。不得把测试拆回 SQLite 根目录，也不得恢复重复前缀的 `code_graph_tests` 文件名。

Durable operations 持久化统一收敛在 `storage::sqlite::operations` 目录：`mod.rs` 负责 worker task、proposal/conflict、audit event、service operator state、相应行解码和稳定 task ID，`tests` 通过存储接口验证这些工作流。SQLite 根模块不得持有 operations 专属测试模块。

Index lifecycle 持久化统一收敛在 `storage::sqlite::indexing` 目录：`mod.rs` 负责 cursor state、refresh orchestration、校验与稳定 refresh-task identity，`cursor_metadata`、`schema`、`task_queue` 隔离各自职责，`refresh_tests`、`queue_tests`、`schema_migration_tests` 与被验证边界放在一起。不得把 index lifecycle 测试或带 `index_refresh_*` 前缀的实现文件放回 SQLite 根目录。

三层 graph retrieval 持久化统一收敛在 `storage::sqlite::retrieval` 目录：`mod.rs` 负责 schema 初始化、document materialization、检索协调和共享 scoring input，具名子模块分别负责 advanced graph path、BM25 与有界 fallback、context assembly、derived document、label trigram、schema migration、alias 和 ranking；定向测试与这些实现同目录。不得恢复父目录级 `retrieval_*` 文件，也不得用 path override 隐藏物理所有权边界。

Maven 持久化在物理结构上统一收敛到 `storage::sqlite::maven`：`mod.rs` 协调 build/dependency 投影，`model` 负责 raw/effective POM model，`xml` 负责有界 XML 提取，`pom_path` 负责受仓库范围约束的相对 POM 解析，`property_interpolation` 负责有界递归属性展开；定向测试与 review regression 测试保持同目录。不得把这些规则重新合并到通用 Maven support 模块，也不得用父目录相对 path override 隐藏边界。

Checkpointed code batch 持久化统一收敛在 `storage::sqlite::code_batch`：`mod.rs` 负责 session 启动、有界 batch apply、checkpoint 和 finalize 协调，`dependencies`、`progress` 与 `finalize` 子树负责更窄的写入阶段；session finalize、TypeScript finalize 与 search materialization 回归测试保持同目录。`storage::sqlite::code` 可以调用该边界，但不得持有 batch 专属测试模块。

Code snapshot 持久化统一收敛在 `storage::sqlite::code_snapshot`：`mod.rs` 负责 snapshot 校验、事务 apply、scope replacement、状态发布和旧数据库导入协调，`candidate_paths`、`fingerprints`、`snapshot_import`、`import_compat` 负责各自命名的读取或兼容边界；candidate path、progress accounting 与 import 回归测试保持同目录。不得再通过 SQLite 根目录中重复的 `code_snapshot_*` 文件表达所有权。

Codebase view 持久化统一收敛在 `storage::sqlite::code_views`：`mod.rs` 协调 snapshot assembly，`affected`、`call_focus`、`dependencies`、`truncation` 负责各自有界派生，`tests` 验证组合投影。必须把这些文件保持在一起，不得再把依赖前缀关联的兄弟文件散落在 SQLite 根目录。

Durable code index task 在物理结构上统一收敛到 `storage::sqlite::code_tasks`：`mod.rs` 负责 queue、attempt-scoped lease、有界 retry、completion/failure、reset、checkpoint 和 scope retention，`worktree` 保护活跃 overlay base scope，queue/lease/reset/retention/status 定向测试与该边界放在一起。为保持 white-box 访问，测试的逻辑模块可继续作为 code facade 的兄弟，但文件不得回到 SQLite 根目录。

Repository set 持久化统一收敛在 `storage::sqlite::code_set`：`mod.rs` 负责 set membership、overlay refresh、跨仓 edge matching 和状态，`manifest` 负责有界 module-key 派生，`refresh_tasks` 负责持久 refresh-task lease 与 retry，set/workspace/manifest/refresh-task 测试保持同目录。不得以 facade 级测试可见性为由，再把 `code_set_*` 文件散落到 SQLite 根目录。

Monorepo workspace 持久化统一收敛在 `storage::sqlite::code_workspace`：`mod.rs` 负责自动 workspace set、package mapping、跨成员 import resolution 和 workspace-format normalization，`tests` 覆盖 lifecycle/mapping 不变量，`lookup_tests` 覆盖语言级 import normalization。不得恢复根目录级 `code_workspace_*` 兄弟文件。

Code index schema 所有权统一收敛在 `storage::sqlite::code_schema`：`mod.rs` 负责当前 table/index 和初始化顺序，`migrations` 负责有界兼容转换，`route_schema` 负责 route 专属 DDL，`tests` 验证 schema 与 migration 不变量。不得再通过 `code_schema_*` 前缀把这些文件拆散到 SQLite 根目录。

Code query 持久化统一收敛在 `storage::sqlite::code_query`：`mod.rs` 协调有界检索层，`calls`、`imports`、`symbols`、`hybrid` 负责 edge 或 plan 专属行为，`scoring` 负责聚焦的 ranking signal，`accuracy` 负责端到端排名 fixture。共享 query 回归保留在 `tests`，并按 `calls`、`ranking`、`generated` 和 `hybrid` 分组；跨域的 unit、score、identity、excerpt、field-filter、line-context 和 SBOM case 保留为具名根子项。跨越聚焦子域的 row decoding、excerpt、identifier、line range、route、reference 和 SBOM retrieval 保留为具名生产根子模块；任何 query 或 test 目录都不得变成新的平铺前缀桶。

代码查询相关性原语统一收敛在 `storage::sqlite::code_query::relevance`：`tokens` 归一化查询词，`text_scoring`、`symbol_scoring`、`call_scoring` 分别负责各自排名域，`symbol_identity` 负责 scoped identity 匹配，`candidate_plan` 负责有界候选层，`filters` 和 `fts` 负责 SQL/FTS 构造。`mod.rs` 只作为内部相关性接口，不得恢复宽泛的 `code_query_support` 文件或根目录级 `code_query_*` 兄弟文件。

SQLite code-store facade 及其直接拥有的持久化行为必须在物理上收敛到 `storage::sqlite::code`：`mod.rs` 协调 store trait 并引用同级持久化领域，`feature_flags`、`generated`、`impact`、`routes`、`search` 和 `symbols` 分别维护与名称一致的 code-store 行为。Scope cleanup、removal、status 和 report 职责统一收敛到 `code::lifecycle`，每个配对 UT 文件必须与实现共置。Facade 回归、元数据/状态用例、共享夹具和测试支持必须使用描述性名称共置于该目录。不得再用 SQLite 根目录下一组扁平 `code_*` 文件模拟领域归属，也不得把 lifecycle 文件移回 facade 根目录。

SQLite connection 执行职责在物理上统一收敛到 `storage::sqlite::connection_runtime`：`maintenance` 负责 writer pragma、WAL checkpoint 和 maintenance diagnostics，`read_pool` 负责有界读连接选择与 deadline，`retry` 负责有界 transient lock retry 分类；配对 UT 与 owner 共置。根 `sqlite.rs` 继续作为 store facade 并显式引用这些模块；不得把 runtime 文件恢复到拥挤的 SQLite 根目录。

### 3.7 代码索引基础模块

跨代码索引流程的基础原语必须使用能表达职责的顶层模块：`content_identity` 负责稳定 ID 和内容哈希，`language_metadata` 负责语言检测及语言级元数据，`generated_detection` 负责生成源码分类。不得把无关原语归入 `common` 目录；新增原语必须归属其所描述的行为。

### 3.8 服务生命周期计划

服务生命周期必须按边界划分职责：`application::service::lifecycle_plan` 负责请求校验、install/upgrade/rollback/uninstall 步骤计划和执行编排；只有 `lifecycle_plan::platform_service` 可以选择平台服务定义文件名、渲染 systemd/launchd/Windows Service 定义、声明平台权限并生成 service manager 命令；`lifecycle_plan::execution` 负责阻塞文件和进程执行。平台渲染与命令转义不得重新并入生命周期步骤 planner。

生命周期计划必须完整收敛在 `application/service/lifecycle_plan/`：`mod.rs` 是 planner 与 execution coordinator，`execution.rs` 和 `platform_service.rs` 是具名子边界，`mod_tests.rs`、`review_tests.rs` 和 `review_followup_tests.rs` 与其共置。父级 `application/service/` 不得重新出现同名 `lifecycle_plan.rs` 或 `lifecycle_plan_*_tests.rs` 平铺文件。

#### 3.8.1 领域模型职责

`domain` 必须是五个真实 Rust 子领域之上的稳定公开 facade，不能继续用生产 `#[path]` 别名把物理子目录伪装成扁平模块。`core` 拥有校验错误、source scope、graph version、entity identity 与 index state；`graph` 拥有多模态证据、mutation 与 retrieval contract；`code` 拥有 repository record/request、index task、repository set、staleness、view 与 workspace contract；`knowledge` 拥有 knowledge-map contract；`operations` 拥有 worker/service lifecycle 与 software-global projection contract。依赖必须保持无环：graph、code 与 knowledge 建立在 core 之上，code 可以消费 graph retrieval policy，operations 组合 core、graph 与 code contract。根模块保留公开 `domain::*` facade，但不得恢复掩盖物理归属的路径别名。

每个带状态或校验行为的 domain 实现必须直接挂载具名同级 `*_tests.rs`。Repository registration、scope identity、retrieval request、repository status 与 repository-index summary 的测试分别留在精确 owner 旁；禁止恢复跨 owner 的 `domain/code/repository_tests.rs` 聚合桶。仅包含序列化 record 的纯数据文件可以不制造无意义测试，但不得以此为由把其他 owner 的断言移入 facade 测试。

### 3.9 自迭代评估器职责

`tools/self_iteration::evaluator` 必须按评估阶段和证据类型分组：`runtime` 负责一次评估运行的顶层协调、并发限制、合同、报告与结果汇总，`quality` 分别拥有门禁定义和执行，`workloads` 按 repository、repository-set、agent、CLI、file 和 semantic-vector 工作负载划分，`fixtures` 只拥有生成式仓库 fixture 及其写入生命周期，`judge` 负责研究判断的配置、prompt、backend 和结果合同。evaluator 根仅声明模块并暴露 `evaluate_candidate` 与 `EvaluationRun`。runtime 必须分离 `contracts`、`concurrency`、`reporting`、`finish` 与 `orchestration`；workloads 只能依赖底层 runtime 合同、并发和报告服务，由 orchestration 向上组合 workloads。workload 专属 JSON case 失败映射留在 `workloads::case_scoring`，避免 reporting 反向依赖 workloads。每个有行为的 runtime owner 直接挂载同级测试，覆盖 repository work-plan、有界 parallel-map、finish 序列化与报告不变量。`quality` 子树必须在领域根拥有门禁合同、分离策略与执行，并由两个 owner 直接挂载聚焦 UT；生产门禁装配不得使用 `include!`，quality-policy UT 不得混入 workload 选择断言。`judge` 子树必须在领域根拥有共享 evaluation input，保持 evaluation 到 settings、prompt、backend 与 outcome owner 的单向组合，并为每个 owner 直接挂载同级测试文件。shell 命令解析归 judge settings 而不是 outcome 验证，生产 judge 装配不得使用 `include!` 或跨 owner 的 test-support 片段。`workloads` 子树必须使用真正的 Rust 模块分别拥有 agent、CLI、file、repository、repository-set、selection 与 semantic-vector 行为。共享 case 失败与 payload 约束归聚焦的 `case_scoring` 模块，而不是 file 或 repository workload；每个有行为的源码文件必须直接挂载同目录 owner 测试文件，生产与测试装配均不得使用 `include!`。`fixtures` 子树必须使用真正的 Rust 模块分别拥有语言/源码族、agent-workflow 源码、仓库装配和共享文件 writer 边界；repository 与 writer UT 必须由对应 owner 直接挂载。生产 fixture 装配不得使用 `include!`，fixture 源码常量不得留在 workload 执行模块。评估器 UT 必须与被验证边界同目录并使用可定位的 `*_tests.rs` 名称；不得恢复 evaluator 根测试装配、`evaluator_tail`、跨职责 `evaluator_tests` 或在 `tools/self_iteration/src` 根目录平铺同一领域的 `evaluator_*` 文件。

### 3.10 自迭代评分职责

`tools/self_iteration::scoring` 必须使用真正的 Rust 模块。`mod.rs` 拥有 observation、公开 score 和私有阶段合同；`ranked` 负责排名证据匹配，`evaluation` 负责总分装配，`decision` 只负责拒绝策略，`capability` 负责能力上限、性能与稳定性分量，`change_detection` 负责全部跨运行变化提取，`case_fields` 负责类型化 JSON 用例字段读取，`score_math` 负责有界均值与截断原语。每个行为 owner 必须直接挂载同级 `*_tests.rs` contract，`mod_tests` 只验证 observation 合同；生产代码和测试不得使用 `include!` 把阶段或测试作用域合并进隐式命名空间。不得恢复根目录级 `scoring_ranked`、`scoring_tests`、引入笼统的 `common` 桶、把变化提取移回 `decision`，或把不同评分阶段重新合并进单个评分文件。

### 3.11 自迭代配置职责

`tools/self_iteration::config` 必须使用真正的 Rust 模块：`mode` 负责模式与策略，`jobs` 负责类型化并行输入，`categories` 负责类别集合，`model` 负责公开配置合同，`parse` 协调 CLI 解析，`category_exclusions` 应用排除策略，`job_plan` 解析资源预算，`value_parser` 校验标量参数。`mod.rs` 只维护常量和稳定 facade。每个行为 owner 必须直接挂载同级 `*_tests.rs` contract，`mod_tests` 只检查 facade 级文档合同；生产代码和测试不得使用 `include!` 把这些边界合并进隐式命名空间。不得恢复同时包含模型、解析器、预算和内联测试的根目录 `config.rs`。

### 3.12 自迭代历史与记忆职责

`tools/self_iteration::history` 必须使用真正的 Rust 模块：`runs` 负责运行记录加载及 workload/profile 选择，`persistence` 负责 report/run 写入和记录构造，`export` 负责 CSV/SVG 渲染，`run_state` 解释采用与评估状态。`mod.rs` 只拥有 `HistoryPaths` 和稳定 facade，`synthesis` 负责生成有界历史摘要。`memory` 子树也使用真正的模块：`api` 协调公开记忆查询与写入，`records` 构造类型化记忆条目，`store` 负责原子 JSONL 与 Markdown 边界，`summaries` 负责有界 prompt/report 渲染，`metadata` 提取规范化运行证据。每个行为 owner 必须直接挂载同级 `*_tests.rs` contract；生产代码不得使用 `include!` 把这些边界合并进隐式命名空间。调用方必须通过 `history` facade、`history::synthesis` 或 `history::memory` 表达依赖。不得恢复根目录级 `history_synthesis.rs`、`memory.rs`、跨边界测试桶，或带有内联大测试模块的单体 `history.rs`。

### 3.13 自迭代无人值守工作流职责

无人值守运行必须属于 `tools/self_iteration::workflow::unattended` 子领域，不能作为与 `workflow` 互相依赖的顶层同级模块。它必须使用真正的 Rust 模块分别负责长运行生命周期、持久状态、循环选择、候选尝试、评估持久化、派生配置、元数据、类别轮换、宏触发、深度检查和结果策略。`mod.rs` 只维护共享阶段合同、策略常量和子领域 facade；每个实现必须显式导入依赖，生产代码不得使用 `include!` 把工作流合并进隐式命名空间。状态、类别轮换和触发策略 UT 必须与对应实现同级共置并由实现挂载。不得恢复顶层 `unattended` 模块、根目录 `unattended.rs` 或 workflow/unattended 依赖环。

### 3.14 自迭代 Codex 生成职责

`tools/self_iteration::codex` 必须把进程执行、命令构建、普通提示词构建、无人值守提示词构建、历史派生提示上下文和命令结果映射分别放在真正的 Rust 模块 `execution`、`command`、`prompt`、`unattended_prompt`、`history_context` 和 `result_mapping`。每个行为 owner 必须直接挂载同级 `*_tests.rs` contract，`mod_tests` 只验证 `CodexResult`。`mod.rs` 只维护结果合同和 facade；禁止生产代码使用 `include!` 装配，也不得恢复同时组合外部进程策略、提示词策略、历史格式化与内联测试的根目录 `codex.rs`。

### 3.15 自迭代工作流职责

`tools/self_iteration::main` 只能作为二进制组合入口。`tools/self_iteration::workflow` 必须使用真正的 Rust 模块，分别以模式分派、循环控制、手工评估、生成迭代、候选评估、文档门禁、评分持久化、报告元数据、已采用优化文档、终端输出、节奏控制和运行标识命名。`mod.rs` 只声明这些模块并暴露 crate 级 workflow facade；每个实现必须显式导入依赖，生产代码不得使用 `include!` 把整个 workflow 合并进隐式命名空间。运行标识和文档门禁 UT 必须与对应实现同级共置并由实现挂载。跨工作流调用方通过 crate facade 使用能力；不得把编排、持久化、文档逻辑或内联测试恢复到 `main.rs`。

### 3.16 自迭代进程边界职责

`tools/self_iteration::command` 必须由 `mod.rs` 维护外部进程合同，真正的 `execution` 模块管理子进程生命周期与超时，`pipes` 管理管道读写 worker，`logging` 记录进度事件，`output` 选择有界输出，`failure` 构造失败结果。每个行为 owner 必须直接挂载同级 `*_tests.rs` UT contract，`mod_tests` 只验证公开 command/result 合同。禁止生产代码使用 `include!` 装配。不得恢复同时组合进程编排、worker 管道、可观测性、格式化与内联测试的根目录 `command.rs`。

### 3.17 自迭代用例配置职责

`tools/self_iteration::cases` 必须把递归用例文件加载、确定性对象/数组合并、类型化 JSON 字段读取和按仓库分组分别放在真正的 Rust 模块 `loading`、`merge`、`fields` 和 `grouping`。每个行为 owner 必须直接挂载同级 `*_tests.rs` UT contract，`mod.rs` 只声明模块并维持公开 facade。禁止生产代码使用 `include!` 装配，因为这种方式会抹去模块所有权，并让多个同级文件共享一个隐式命名空间。`tools/self_iteration/cases.json` 只维护有界 workload manifest 和全局 suite，repository query target 必须放入具名 JSON include 文件，其中 project-alias、relay-teams、Linux、LevelDB、Spring Framework 和 Kubernetes 各自独立。不得恢复同时组合配置 I/O、合并策略、访问辅助、分组和内联测试的根目录 `cases.rs`，也不得把 manifest 再扩成单体 query-case 文件。

### 3.18 自迭代研究计划职责

`tools/self_iteration::research_plan::mod` 必须维护输入合同并把 `render` 声明为真正的 Rust 模块，`render` 负责确定性计划渲染并显式挂载同级 `render_tests` UT 合同。这些文件必须共置于研究计划领域目录；禁止生产代码使用 `include!` 装配，facade 不得代管渲染测试，也不得把渲染和内联测试恢复到根目录 `research_plan.rs`。

### 3.19 自迭代候选 Git 职责

`tools/self_iteration::candidate_git` 必须由真正的 Rust 模块 `mod`、`command`、`dynamic_command`、`worktree`、`patch` 和 `lifecycle` 分别维护补丁快照合同、有界 Git 命令执行、工作树检查、补丁捕获/路径提取以及候选拒绝/提交生命周期。每个行为 owner 必须直接挂载同级 `*_tests.rs` contract；具名 `git_repository_fixture` 仅作为隔离仓库的 test-only 基础设施。禁止生产代码使用 `include!` 装配。循环休眠属于 `workflow::pacing`，不得放入 Git 边界。调用点必须使用明确的 `candidate_git` 名称；不得恢复含糊的根目录 `git_ops.rs`，也不得把工作流节奏混入仓库变更。

### 3.20 生产代码与单元测试文件职责

生产 Rust 文件不得内嵌 `#[cfg(test)] mod` 实现。每个单元测试模块必须放入命名明确的同级 `*_tests.rs` 文件，并由所属生产文件通过显式 test-only `#[path]` 挂载；模块声明仍由生产文件唯一维护，以保持 white-box 可见性和测试身份稳定。`api` contract 已对 `agent`、`code_repository`、`error` 和 `stream` 实施一一配对，application 层的 repository、indexing、repository-set、view、knowledge、service 和 update 单元也遵循同一规则。代码摄取与索引单元在 language metadata、generated detection、identity、index plan/snapshot、parser workspace/language 和 source discovery 中同样执行该规则。domain core、graph、code、repository、workspace、knowledge-map、runtime 和 software contract 也必须把 UT 放入显式同级文件，即使测试声明位于后续生产类型之前。bootstrap、evaluation、顶层 indexing、network/QoS、observability、paths、retrieval 和 watcher 基础模块同样一一配对，且不得削弱其所有权边界。存储 contract 测试必须逐一执行 `CodeRepositoryStore` 的全部可选默认能力，使不支持的租约、检查点、有界候选检索、repository set、view 与 software projection 显式报错，而不是静默成功。partitioned storage 的 `mod_tests` contract 必须覆盖空控制库委派、已索引分片路由、任务租约、repository-set 控制状态和 staged 检查点收尾。interface 测试必须留在所属 CLI、Web、ACP、MCP、audit 或 policy adapter 目录；MCP HTTP/JSON-RPC 夹具边界必须命名为 `transport_harness`。SQLite code-store、code-query、scoring、import/call planning、view、retrieval、maintenance、retry、pool 和 schema 测试必须与精确的持久化 owner 共置；`code_tasks` 直接拥有 lifecycle、retention、status、lease 和 reset 测试套件，`record_mapping` 则隔离 task/checkpoint 行解码与 SQL projection 构造。repository-set membership、overlay 和 workspace 测试套件由 `code_set` 自己拥有，持久化 refresh-task 测试由 `refresh_tasks` 拥有，外层 code-store facade 不得代为挂载。candidate-path filtering、FTS planning、generated exclusion、legacy import 和 fallback 测试由 `candidate_paths` 直接挂载，不得再由更宽的 snapshot facade 代管。partitioned SQLite 集成数据构造集中于 `partitioned_sqlite_fixtures`。不得把这些测试重新合并进生产文件，也不得建立共享的笼统测试桶。

`code_workspace` owner 直接挂载 facade `mod_tests` 与聚焦的 `lookup_tests`；外层 code-store facade 不得代管 workspace normalization 测试。

SQLite import-query 的 target、generated filtering、ranking 与 foundational ranking 测试套件由 `code_query::imports` 挂载；ambiguous-callee 单元与 generated-filter 测试套件由 `ambiguous_callees` 挂载，外层 code-store facade 不得代管这两个子域的测试。

Symbol query 直接拥有 `mod_tests` 与 generated-filter 测试套件。Typed function-value parsing 与 ranking 收敛到聚焦的 `symbols::typed_function_value` 模块，避免 symbol SQL retrieval、surface interpretation 和测试继续堆积在一个接近长度上限的文件中。

Application repository、derived-view、runtime 与 shared-service facade 测试统一使用明确的 `mod_tests` 名称；不得为这些模块 owner 恢复含糊的 `tests.rs`。

代码 feature-flag extraction、route parsing 与 source-search facade 测试同样使用 `mod_tests`；行为聚焦的子测试套件继续保留描述性名称。

CLI render、repository、repository-set 与 setup adapter 的 `mod.rs` owner 都与 `mod_tests.rs` 一一配对；Web router facade 采用相同约定，跨 router 测试套件则保留明确的 integration 名称。

Model-provider facade 以 `mod.rs` 与 `mod_tests.rs` 配对，覆盖 profile、catalog、probe、discovery 与 fallback 行为。

SQLite canvas、code-graph、code-schema、code-view、file-index、Maven、operations 与 retrieval owner 都以 `mod_tests.rs` 配对 facade。聚焦的 schema、ranking、migration 与 persistence 测试套件保留描述性文件名；这些存储子域禁止使用通用 `tests.rs`。

SQLite code-query 的 hybrid chunk 证据准入归 `hybrid::chunk_gate` 所有，并与 `chunk_gate_tests` 配对；direct-result 准入、FTS 查询构造、candidate 预算和 chunk 结果合并测试分别留在自己的生产 owner 旁。code-query facade 只负责编排检索层，不得重新承担 hybrid 证据密度或语言域策略。

根 SQLite schema 兼容职责统一归入纯文件型 `sqlite/schema` 分组：`initialization` 负责核心 graph DDL 与 fact-evidence 回填，`columns` 负责旧列修复，`marker` 负责 schema 指纹，`migration` 负责安全的 obsolete-schema 准备。每个有状态 schema owner 都把聚焦测试留在该目录；SQLite store facade 不得再内嵌 DDL 或 migration 循环。

SQLite software dependency-usage 的持久化、语言匹配辅助与 UT 合同统一位于纯文件型 `software/dependency_usage` 分组。上层 software projection facade 不得重新收纳 dependency-usage 实现或测试文件。

在 dependency usage 内部，`schema` 独占建表与一次性 projection 失效判定，并覆盖“表已存在时不失效”的合同。匹配和持久化代码不得执行 schema DDL，也不得决定历史 projection 是否转为 stale。

Dependency-usage 的 `persistence` 独占按 scope 删除、幂等写行、有界过滤读取、import-evidence 行映射与 graph-version 重建。其配对测试覆盖 round-trip、路径/语言过滤和 scope 删除；工作流 owner 不得内嵌 SQL projection 或行解码逻辑。

Dependency-usage 的 `matching` 独占不可变 component-key 索引、manifest owner 收窄、Cargo alias 证据与有界跨语言 import key 归一化，并拥有匹配测试套件。`mod` 工作流只编排证据、匹配、置信度交集、去重和确定性排序；当没有组件可匹配时必须在读取 import 表前短路。

ACP adapter、prompt-context builder 及其配对测试统一位于纯文件型 `interfaces/agent/acp` 分组，同时保持公开 `interfaces::agent::acp` 路径不变。上层 agent 目录不得再把 ACP session 或 context 实现文件平铺在 MCP、audit 与 policy 领域旁。

ACP 初始化、会话、提示词、进度更新、结果与错误 wire contract 统一归 `acp::protocol` 所有，并由 `protocol_tests` 验证 JSON 字段名、省略规则与状态转换。adapter facade 只重导出这些公开类型并编排会话请求，不得重新内嵌序列化 DTO。

ACP session identity、活动请求 cancellation channel 和自动清理 lease 统一归 `acp::session_registry` 所有。该 owner 必须规范化不可信客户端元数据，并由配对测试覆盖 session lookup、取消通知、显式 release 与 drop 清理；adapter facade 不得直接维护共享 map 或 mutex。

ACP prompt 的 scope authorization、freshness parsing、资源 limit/context-byte 校验及 domain request 构造统一归 `acp::prompt_mapping` 所有。`prompt_context` 只执行已验证的 graph 或 codegraph 请求并汇总结果；依赖方向必须保持 `prompt_context -> prompt_mapping`，禁止反向依赖形成环。

Worktree-overlay 索引实现统一位于 `code/index/worktree_overlay` 目录，物理文件名按 `dirs`、`git_overlay`、`overlay_plan`、`overlay_scope` 和 `untracked` 职责命名。不得在 `code/index` 根目录恢复 `worktree_overlay_*` 前缀文件，也不得改变 overlay 的有界变更、Gitlink 展开或 scope 过滤合同。

Worktree-overlay 的 hash marker、删除集合、待解析文件集合和同内容跳过判定归 `worktree_overlay::recording` 所有，并由同级 UT 固定二进制 framing 与删除后重建语义。主 overlay 编排和 Gitlink 处理只能复用该记录边界，不得各自实现不同的 overlay hash 输入协议。

Worktree-overlay 的 Gitlink 输出聚合、子路径删除回放和 scope-aware recorder 归 `worktree_overlay::gitlink_recording` 所有。该 owner 必须使用共享 `recording` 协议，并由配对 UT 证明 retained、out-of-scope 与待删除子路径不会混淆；Gitlink 状态机不得复制 recorder 或直接发明新的 marker。

顶层 `code` facade 由 `mod.rs` 与同级 `mod_tests.rs` 一一配对；源码发现、布局、submodule、filesystem 和 worktree-overlay 场景测试继续收敛在 `code/tests/source`，可复用 fixture 由 `code/tests/fixtures.rs` 维护。不得在场景测试目录旁恢复同名 `tests.rs`，也不得把 facade 不变量混入源码场景测试。

其余同级测试挂载必须全部显式：runtime、service、repository/source-fallback/view 工作流、code feature/search 边界，以及 SQLite Maven、view、schema、batch、graph、workspace、operation、indexing、retrieval、snapshot 和根 adapter 都必须通过 test-only `#[path]` 声明具体测试文件。禁止依赖隐式 `#[cfg(test)] mod name;` 文件解析，避免 rename 或同名目录掩盖物理 owner。

自迭代的 config、scoring 和 history facade 必须把 facade 合同装配留在同级 `mod_tests.rs`。evaluator 根不含行为，因此不设置 facade 测试桶；runtime、quality、judge、fixture 与 workload 测试直接由精确 owner 挂载，其中 repository-set workload 自行挂载同级 `repository_set_tests.rs` provenance UT 合同。不得把测试体写回生产文件或恢复跨 owner 的 evaluator 测试装配。

自迭代 Codex adapter 的 `command_tests`、`execution_tests`、`history_context_tests`、`prompt_tests`、`unattended_prompt_tests` 与 `result_mapping_tests` 必须由各自精确的生产 owner 通过显式 test-only `#[path]` 挂载，测试文件本身直接包含测试项。禁止生产期 `include!` 展开、facade 代管行为测试或在配对文件中再嵌套同名 test module。

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
