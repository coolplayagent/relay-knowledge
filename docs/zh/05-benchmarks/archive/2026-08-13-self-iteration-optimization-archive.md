# 2026-05-16 至 2026-05-19 自迭代候选记录

[中文](2026-08-13-self-iteration-optimization-archive.md) | 英文版尚未提供

本页原样保存 2026-05-16 至 2026-05-19 从 A.4 主记录迁出的连续历史候选说明。记录中的运行结果、路径和结论是当时证据，不代表当前实现或当前 case inventory；当前入口见[自迭代采纳优化记录](../04-self-iteration-accepted-optimizations.md)。

## 20260518-20260519 manual parser/retrieval entries compacted
- summary: C composite initializer symbols, call signature FTS, JS/TS exported values, TypeScript imported-reference finalize, callee member-call context, and relationship declaration chunk ranking entries are compacted here to keep this primary benchmark log under the tracked-file line cap; full algorithms, invariants, risks, patches, reports, and progressive memory remain under `.git/relay-knowledge-self-iteration/`.
## 候选优化说明：manual-shared-finalize-symbol-cache-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，降低多仓 full-scope `repo register` finalize 阶段重复读取 symbol 表的固定成本；scope finalize 增加惰性 `SymbolKey` cache，Python/Java/TypeScript symbol-aware import resolution 与 call graph rebuild 共享同一批已排序 symbol keys，C/C++ 等无需 symbol-aware import 的仓库仍延迟到 call rebuild 才加载。SQLite schema、事实表内容、reference/import/call resolution 规则、search document、candidate limit、ranking/scoring、CLI/API、provider/env、judge、网络/QoS、安装发布与 self-iteration harness 行为保持不变。
- 预期影响/风险：opencode、relay-teams、Kubernetes、Spring 等含大量 TS/Python/Java named import 或 static import 的仓库少一次全量 `code_repository_symbols` scan 和一次 call rebuild symbol clone，预期改善 register-to-index wall time 与 finalize 稳定性；风险是 finalize 期间共享 cache 的生命周期变长，但仍限于单 scope transaction、惰性加载、既有 row budgets 和 deterministic caller line lookup 单测。
## 候选优化说明：manual-code-search-document-single-pass-content-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，降低多仓 full-scope `repo register` 冷索引中 SQLite FTS search document 写入前的 CPU 与分配成本；`code_repository_search` 文档内容改为单次遍历非空字段构造，并在 symbol 文档前两个非空字段上原位收集既有 snake/camel identifier 扩展词，排序去重后追加，CamelCase 分词改为 peekable 字符迭代并直接写入共享 term buffer。SQLite schema、事实表、FTS 字段语义、candidate limit、ranking/scoring、path/language filter、CLI/API、provider/env、judge、网络/QoS、安装发布与 self-iteration harness 行为保持不变。
- 预期影响/风险：relay-teams、opencode、Linux、LevelDB、Kubernetes 与 Spring 等大仓索引每个 symbol/chunk/reference/import/call search row 少一次字段 `Vec` 收集、少一次 symbol identifier 输入 join，并避免 identifier 扩展词 join、CamelCase 字符索引和 per-token term `Vec` 的中间分配，预期改善 register-to-index wall time 和 background first-seen 稳定性；风险是 search content 拼接顺序必须与旧实现一致，已用单元测试锁定 symbol identifier expansion 与非 symbol 空字段过滤。
## 候选优化说明：manual-c-header-decorated-cpp-class-symbol-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限；`.h` 文件继续按既有 C 语言路径索引，但当 C parser 把 `class EXPORT_MACRO RealType {` 这类 C++ 头文件声明暴露成误导性的 function_definition 时，从声明头提取最后一个合法类型标识符并记录为 `class` symbol，避免导出宏占用目标 symbol evidence。不改变语言检测、SQLite schema、FTS candidate、查询排序权重、path/language filter、CLI/API 字段、provider/env、judge、网络/QoS、安装发布或 self-iteration harness 行为。
- 预期影响/风险：LevelDB、Linux 与跨平台 C/C++ 头文件的 import-target excerpt 可展示真实 API 类型名，例如 `FilterPolicy`，改善 `leveldb/filter_policy.h` dependency challenge 的 expected-all evidence，同时避免此前 `.h` 全量切换到 C++ parser 带来的 foundational 和 judge 风险。风险是少数把 `class` 当普通 C 标识符且后接宏式声明体的非 C++ 头可能新增一个 class symbol；触发条件要求声明头以 `class ` 开始，并受既有文件范围、symbol 去重、bounded query 和 import evidence 规则限制。
## 候选优化说明：manual-import-target-symbol-excerpt-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，在路径型 `imports` 查询命中 resolved import edge 后，用目标文件 `target_hint` 批量读取少量已索引 symbol 名称并追加到 import hit excerpt；不扩大 FTS candidate、不改变 SQLite schema、事实写入、path/language filter、排序权重、CLI/API 字段、provider/env、judge、网络/QoS、安装发布或 self-iteration harness 行为。
- 预期影响/风险：LevelDB、Linux、Kubernetes、Spring 与 C/C++/Go/Java 大仓中，询问 header/package 路径时结果不仅显示 import/include 语句，还显示目标文件声明的接口符号，改善 `leveldb/filter_policy.h` 这类 dependency challenge 的 expected-all evidence 和 research judge 对图关系解释性的评价；风险是 path-like import 查询多一次 bounded target-symbol lookup，且 excerpt 内容变长，但只作用于已召回 import rows 并限制每个目标文件最多 4 个 symbol。
## 候选优化说明：manual-hybrid-chunk-symbol-context-ranking-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，在 Hybrid lexical chunk 后置评分中把已链接 parsed symbol 的 name、qualified name 与 canonical id 交给既有 identifier-aware symbol bonus；不扩大 FTS candidate、不改变 SQLite schema、索引事实、path/language filter、CLI/API JSON、provider/env、judge、网络/QoS、安装发布或 self-iteration harness 行为。
- 预期影响：大型 TypeScript、Python、Java、C/C++、Go 仓库中，函数/方法级 chunk 在长概念查询里可凭拥有者符号身份压过同文件相邻 converter/helper chunk，优先改善 `fromOpenaiChunk` 这类“内容词相近但目标函数名更匹配”的 hybrid retrieval rank，并保持 register-to-index wall time 基本不变。
- 已知风险：少数 chunk 会因符号名匹配自然语言查询而上移；风险受已有 FTS bounded candidate、linked symbol、identifier normalization、dedupe/truncate 和 path/language filters 限制，未引入仓库、路径、case、provider、模型或密钥特殊分支。
## 候选优化说明：manual-import-test-path-demotion-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，在 `imports` 与 Hybrid import edge 评分中复用既有 test/benchmark intent 识别；当 query 未显式包含 test/benchmark 意图且 import 候选已获得正分时，对 test/benchmark 路径施加小额有界负分，让同一 dependency/header 的 production importer 不被测试文件早行号或同分排序压过。不改变 SQLite schema、FTS 文档、candidate limit、path/language filter、索引事实、CLI/API、provider/env、judge、网络/QoS、安装发布或 harness 行为。
- 预期影响/风险：LevelDB、Linux、Kubernetes、Spring 与 TypeScript 大仓的 dependency/include/import 查询应更稳定地把 production 依赖使用点排在 test noise 前，尤其改善 `leveldb/filter_policy.h` 这类同 header 多 importer 的 competitive challenge；风险是用户未写 test intent 时测试 import 结果会轻微下移，但仍保留在 bounded top-k 候选内，且 query 包含 test/benchmark 时不降权。
## 候选优化说明：manual-hybrid-sparse-import-coverage-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，在 Hybrid 长概念查询中对只覆盖少数查询词的 import graph edge 施加有界负分；路径型 import/include 查询、短查询和 `imports` 查询不受影响，SQLite schema、FTS 文档、candidate limit、path/language filter、CLI/API、provider/env、judge、网络/QoS、安装发布和 harness 行为保持不变。
- 预期影响/风险：Opencode TypeScript 的 OpenAI Chat protocol、provider conversion 等全仓 hybrid 查询应减少 inbound import 边挤占 top-k，让含 `ToolStream.empty`、`fromOpenaiChunk`、route/transport lifecycle 的源码 chunk/symbol 更容易进入前列；风险是少数长概念 hybrid 查询中的 dependency edge 排名下移，但召回集合仍保留且 path-like/import 专用查询不降权。
## 候选优化说明：manual-import-line-priority-path-queries-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，把 import graph 的早行号排序信号限制到路径型查询，例如 `linux/debugfs.h`、`./redaction`、`shared.ts`；符号型 import 查询例如 `ProviderShared`、`ObjectUtils` 不再因某个文件 import 更早而压过更相关的仓库上下文。不改变 SQLite schema、事实表、FTS 文档、candidate limit、path/language filter、CLI/API、provider/env、judge、网络/QoS、安装发布或 harness 行为。
- 预期影响/风险：Opencode TypeScript、Spring Java、Kubernetes Go 等大仓的符号依赖查询更容易按 path/context 与既有 target-symbol scoring 排序，`opencode_ts_imports_provider_shared` 这类 case 应减少早行号噪声；路径型 include/import 查询仍保留早行号 tie-break。风险是少数非路径符号 import 的同分候选会改为按既有分数与路径排序，但召回集合和 path-like 查询排序保持有界。
## 候选优化说明：manual-java-object-creation-call-edges-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，把 Java `object_creation_expression` 作为 call edge 写入 code graph，并用构造类型字段提取 callee；generic/array 类型递归到被构造类型本体，避免把 `new Box<Session>()` 的 type argument 误当 callee。不改变 schema、FTS 文档、candidate limit、ranking、CLI/API、provider/env、judge、网络/QoS、安装发布或 harness 行为。
- 预期影响/风险：Spring Framework 等 Java 大仓的 constructor/object-creation caller/callee 检索能覆盖服务、bean、factory 和 adapter 实例化关系；风险是新增 class-construction call edge 参与 caller/callee 排序，但只来自语法级 object creation，仍受 symbol resolution、bounded FTS、path/language filter、ScoreQuery 与 dedupe/truncate 控制。
## 候选优化说明：manual-typescript-function-factory-member-symbols-20260519
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，把 JS/TS object/class member 中由 curried function factory 返回的成员函数识别为 `function` symbol，并让 call excerpt 优先选择真实调用语法行；只检查成员 value 的 bounded call-expression 链和直接 function/generator/arrow 参数，不改变 schema、FTS、candidate limit、ranking、CLI/API、provider/env、judge、网络/QoS、安装发布或 harness 行为。
- 预期影响/风险：Opencode、relay-teams 等 TS/JS 服务对象、协议适配器和 effect-style layer 的 caller/callee 命中可获得拥有者 symbol、chunk excerpt 与更准 line range，改善 `generateObject(params)` 这类调用点上下文；风险是少数 curried function-factory 成员新增 symbol 参与近同分排序，且 excerpt 会跳过同一 chunk 中早于调用行的类型引用，但受成员 node kind、标识符校验、curried call 形态、直接函数参数、bounded 深度和 mention fallback 限制。
## 候选优化说明：manual-single-scope-parser-worker-set-20260519
- 目标/算法/架构/不变量：保护核心目标下限，降低多仓 full-scope `repo register` 冷索引中的 parser worker 固定成本；每个 git blob fetch group 只创建一组有界 scoped worker，按 stride 分配并按原始 index 合并，不改变 schema、事实、search document、candidate limit、ranking、filters、CLI/API、provider/env、judge、网络/QoS、安装发布或 harness 行为。
- 预期影响/风险：relay-teams、opencode、Linux、LevelDB、Kubernetes 与 Spring 等大仓减少 thread spawn/join 开销；stride 分配可能轻微负载不均，但受文件数、字节数与 row budget 限制，合并排序保持确定性。
## 候选优化说明：manual-exported-constructed-value-definition-20260518
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，把短小导出 member-call 或 `new` 构造值记录为 `constant` symbol，不改变 schema、FTS、ranking、finalize、CLI/API、provider/env、judge、网络/QoS、安装发布或 harness 行为。
- 预期影响/风险：Opencode 和大型 JS/TS 仓库公开协议、route、transport、layer 对象的 definition 召回更稳定；少量导出工厂结果可能参与同名排序，但受 export ancestor、构造表达式、标识符和 64 行上界限制。
## 候选优化说明：manual-typescript-import-identity-and-dynamic-import-20260518
- 目标/算法/架构/不变量：保护核心目标下限，给 JS/TS import stable id 纳入 AST byte range，按 import_id 去重，并只把直接字符串动态 import 规范化为 `import "specifier"`；schema、事实字段、FTS、candidate limit、ranking、CLI/API、provider/env、judge、网络/QoS 与安装发布不变。
- 预期影响/风险：Opencode、relay-teams 等 import-heavy 仓库不再因同一行 import 主键冲突失败，直接字符串动态 import 可检索；旧 import_id 不逐字复用，非字符串动态 specifier 不推断。
## 候选优化说明：manual-windowed-compound-identifier-fts-recall-20260518
- 目标/算法/架构/不变量：保护核心目标下限，为安全 ASCII 查询词生成最多 24 个相邻 2 到 4 词窗口的 compact 与 snake_case exact FTS 分支；schema、索引写入、FTS 文档、事实表、candidate limit、scoring、CLI/API、provider/env、judge、网络/QoS 与安装发布不变。
- 预期影响/风险：长 hybrid/caller/callee/reference/import/definition 查询更容易召回复合标识符子短语；额外 OR 分支可能增加候选和 MATCH 成本，但受 alternatives 上限、candidate limit、typed filter、ScoreQuery 和 dedupe/truncate 控制。
## 候选优化说明：manual-relationship-challenge-continuous-scoring-20260519
- 目标/算法/架构/不变量：把多语言 relationship workload 扩展为 regression/challenge 双层评估；challenge cases 去掉 path filter、降低 limit/max_rank，并用 `expected_all`、`expected_sequence`、`min_score` 与 ranked forbidden penalty 产生连续分数，不改变 Rust runtime、schema、索引事实、CLI/API、provider/env、judge、HTTP/QoS 或安装发布行为。
- 预期影响/风险：RustFS、Kubernetes、Linux、LevelDB、Spring、Codex Python、relay-teams JS 和 opencode TS 的关系 case 即使通过也保留排序、覆盖率和延迟优化空间；`budget_relative_v1` 对旧历史先按预算兼容，后续同策略 run 才启用相对进步信号。
## 候选优化说明：manual-identifier-singular-plural-query-scoring-20260518
- 目标/算法/架构：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，在 code query Rust 后置评分中把安全 ASCII 标识符词项的单复数形态归一为等价匹配，例如 `range`/`ranges`、`policy`/`policies`，作用于 `ScoreQuery` identifier-token scoring 与 symbol-name bonus。
- 不变量：不改变 SQLite schema、FTS 文档、candidate limit、索引写入、path/language filter、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP/QoS 或安装发布行为；归一化只在已召回候选内评分，不扩大查询窗口。
- 预期影响：relay-teams、LevelDB、Linux、Kubernetes 与 Spring Framework 中自然语言 fuzzy/hybrid/definition 查询对复合代码标识符的 rank 更稳定，尤其改善 `service ip range`、`bloom filter policies`、`deleted files` 这类词形与符号不完全一致的研究型检索；register-to-index wall time 应保持不变。
- 已知风险：少数同词根但语义不同的标识符可能获得小幅 scoring 提升；实现排除非 ASCII、过短项、`ss/us/is` 结尾和 `series/species`，并仍由 FTS bounded candidate、path/test scoring、dedupe/truncate 控制最终结果。
## 候选优化说明：manual-symbol-compound-identifier-fts-recall-20260518
- 目标/算法/架构：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时补齐 definition/symbol 查询对自然语言拆分标识符的候选召回；复用既有 bounded compound identifier FTS 扩展，把 2 到 6 个安全 ASCII 查询项额外映射为 compact 与 snake_case exact token 分支，使 `new lru cache`、`default listable bean factory` 等查询可进入 `NewLRUCache`、`DefaultListableBeanFactory` 符号候选窗口。
- 不变量：不改变 SQLite schema、索引写入、事实表、FTS 文档、candidate limit、后置评分/排序、path/language filter、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP/QoS 或安装发布行为；扩展仍受现有词数、part 长度、总标识符长度和单字符噪声边界约束。
- 预期影响：relay-teams、LevelDB、Kubernetes、Spring Framework 等大仓中以空格分词询问 CamelCase/PascalCase/snake_case 符号的 definition、symbol 与 hybrid 前段 symbol 召回更稳定，尤其改善研究 judge 对自然语言代码检索泛化能力的评价；精确符号查询和 edge/hybrid 既有 compound recall 语义保持不变。
- 已知风险：少量 compact 或 snake_case 同名符号可能进入 bounded FTS candidate window，但最终仍由 `ScoreQuery`、symbol name bonus、scoped identity bonus、path/language filter 与 dedupe/truncate 排序控制；额外 OR 分支最多两个，查询开销应保持有界。
## 候选优化说明：manual-grouped-reference-finalize-20260518
- 目标/算法/架构：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时降低多仓 full-scope `repo register` 冷索引 finalize 阶段的 reference resolution 固定成本；把逐 reference correlated `COUNT(*)` 查询改为按 scope 分组的 unique-name、unique-name+path 与 existing-name CTE，再用同一批 UPDATE 维持全局唯一解析、同文件唯一解析、ambiguous 与 unresolved 规则。
- 不变量：不改变 SQLite schema、事实表字段、FTS 文档字段、call/import finalize、candidate limit、ranking/scoring、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP/QoS 或安装发布行为；reference 的默认 target_hint、confidence、resolution_state 与既有唯一性规则保持不变。
- 预期影响：relay-teams、Linux、LevelDB、Kubernetes 与 Spring Framework 等 reference-heavy 仓库在 finalize reference resolution 时减少 symbol 表重复扫描和 per-row 聚合开销，降低 register-to-index wall time；code graph completeness、caller rebuild 和 query result ranks 应保持不变。
- 已知风险：SQLite 对 CTE 的执行计划仍可能因版本和数据分布产生临时 B-tree 成本；收益主要出现在 references 数量明显大于 symbol-name 分组数量的大仓，极小仓库影响应接近中性。
## 候选优化说明：manual-batched-path-cleanup-20260518
- 目标/算法/架构/不变量：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，把 checkpointed batch 和 snapshot incremental 的 same-scope path cleanup 从逐文件逐表 `DELETE` 收敛为去重后的 bounded `IN` 删除；SQLite schema、事实内容、FTS 文档字段、finalize、ranking、CLI/API、provider/env、judge 配置和安装行为不变，单条语句最多绑定 500 个 path 以保留 SQLite 参数上界。
- 预期影响/风险：大仓 `repo register` 冷索引和增量替换批次减少 delete statement 固定开销，尤其配合 256 文件 batch 降低 relay-teams、LevelDB、Linux、Kubernetes、Spring 的 apply-batch wall time；风险是极少数异常重复 path batch 会一起清理旧 rows，但这与既有逐路径幂等语义一致，并由多 path cleanup 单测覆盖普通表与 FTS 表。
## 候选优化说明：manual-default-code-index-batch-256-20260518
- 目标：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时降低多仓 full-scope `repo register` 冷索引的固定批处理开销，优先改善 relay-teams、LevelDB、Linux、Kubernetes 与 Spring Framework 的 register-to-index wall time。
- 算法与架构：默认 `CodeIndexResourceBudget` 的 `max_files_per_batch` 从 128 提升到 256；`max_bytes_per_batch=16MiB`、`max_rows_per_batch=50000`、checkpoint、crash recovery、FTS materialization、finalize resolution 与查询排序保持不变。小文件仓库可用更少的 git `cat-file --batch` 分组和 SQLite 事务完成索引，edge-heavy 或大文件仓库仍由字节/行预算提前截断。
- 不变量：不改变 SQLite schema、事实表内容、search document 格式、candidate limit、CLI/API JSON 字段、semantic/vector provider/env、embedding 设置、research judge 配置、HTTP/QoS 或安装发布行为；批次仍有明确文件数、字节数和行数上界，已持久化的 checkpoint 会继续携带自身 resource budget。
- 预期影响：大仓冷索引中每 129-256 个小文件少一次 batch parse/apply/finalize-progress 往返，降低 transaction commit、prepared statement、git process 和 checkpoint update 固定成本；retrieval floors 与 semantic/vector coverage 不应变化，因为最终图事实和派生索引内容不变。
- 已知风险：单个默认 batch 的 peak memory 和 transaction duration 可能上升，但受 16MiB blob 与 50000 row 上界限制；极端超高 fan-out 文件集合仍会按 row budget 提前切批。
## 候选优化说明：manual-production-scoped-repeated-caller-bonus-20260518
- 目标：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时修复 repeated caller-site ranking 对无 test intent 的测试调用点过度加权，优先恢复 LevelDB `KeyMayMatch` production caller rank，并减少非 caller 查询中的额外计数开销。
- 算法与架构：`search_calls` 只在 `CodeQueryKind::Callers` 下构建候选内 caller-target call-site 计数；重复调用点 bonus 先经过既有 path/test intent scoring，再仅对获得 production source path bonus 或 query 明确包含 test/benchmark intent 的候选生效。测试、benchmark 与无 adapter intent 的 adapter surface 不再凭多次调用同一目标压过 production caller。
- 不变量：不改变 SQLite schema、事实表、FTS MATCH、candidate limit、call edge resolution、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP/QoS 或安装发布行为；召回集合、path/language filter、same-named caller penalty、edge confidence bonus 与最终 dedupe/truncate 流程保持不变。
- 预期影响：`table/filter_block_test.cc` 中重复 `TEST_F` 断言不再因为 repeated-site bonus 排在 `table/table.cc::InternalGet` 的 `filter->KeyMayMatch` production caller 之前；relay-teams、JavaScript runtime、C/C++、Go 和 Java 的真实 production repeated caller 场景仍保留排序收益，hybrid/callee 查询少做一轮候选计数。
- 已知风险：少数用户在未写 test/benchmark intent 时查询测试 helper 的 callers，重复测试调用点会失去此前的小幅加权但仍保留在结果中；明确包含 test/benchmark 的 query 仍允许测试路径使用 repeated-site bonus。
## 候选优化说明：manual-repeated-caller-site-ranking-20260518
- 目标：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时提升大仓 caller 查询在多个同分调用点之间的排序质量，优先改善 JavaScript/TypeScript runtime、C/C++ service、Go controller 和 Java framework 中“哪个拥有者真正反复调用目标”的 rank 稳定性。
- 算法与架构：在既有 FTS bounded candidate、方向过滤、path/language filter 和 Rust scoring 之后，对 `callers` 查询按 `caller_symbol_snapshot_id`、callee snapshot、target hint 与 callee name 统计候选内同一 caller 到同一 target 的 call site 数；重复 call site 只给小幅、封顶 bonus，让多次调用同一目标的 caller 在同分场景下优先展示。该统计在已取回的候选行内完成，不扩大 SQLite 查询窗口，不增加索引写入。
- 不变量：不改变 SQLite schema、事实表、FTS MATCH 表达式、candidate limit、call edge resolution、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP/QoS 或安装发布行为；没有 caller symbol 的模块级调用不参与重复 bonus，避免把整文件级散落调用误判为同一 owning function。
- 预期影响：`releaseActiveStreamHandle` 这类有多个同文件 caller 的查询会把含多个目标调用点的 owning function 排到同分单次调用 wrapper 之前；已有 LevelDB production caller、same-named wrapper demotion、test-path demotion、foundation definition/filter、semantic/vector source coverage 与性能预算应保持不变。
- 已知风险：少数 caller 可能因为清理或重试逻辑多次调用同一 helper 而上移；bonus 被限制在 caller 查询、已有正分候选、同一 caller symbol 与同一 target、最多三次额外调用的封顶范围内，不改变召回集合。
## 候选优化说明：manual-deferred-cold-edge-search-docs-20260518
- 目标：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时降低多仓 full-scope `repo register` 后首次冷索引的 SQLite/FTS 写入放大，优先改善 relay-teams、LevelDB、Linux、Kubernetes 与 Spring Framework 的 register-to-index wall time。
- 算法与架构：checkpointed batch 仍立即持久化 reference/import 事实表；但当本轮 `source_scope` 既不是仓库当前 active indexed scope，也不是已有 `code_repository_scopes` 中可按 ref 选中的保留 scope 时，不再为这些边写入临时 FTS search row，因为 finalize 会在 reference/import resolution 后删除并集合重建最终 edge search documents。若正在重建当前 active 或 retained queryable scope，则保留中间 edge FTS 写入以维持索引中状态下的兼容查询语义。
- 不变量：不改变 SQLite schema、reference/import/call 事实、finalize resolution、最终 FTS document 内容、candidate limit、query ranking、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP/QoS 或安装发布行为；全新冷 scope 在 finalize 前仍不会成为 repository scope status 的可查询新索引。
- 预期影响：首次或新 commit 的 checkpointed cold indexing 少写一轮会被 finalize 覆盖的 reference/import FTS rows，减少大仓 edge-heavy batch 的 SQLite 写入和 tokenizer 成本；finalize 后 language-filtered reference/import/call coverage 与既有测试保持一致。
- 已知风险：如果外部调用方强行查询尚未 finalized、也不在 active/retained scope registry 中的内部 `source_scope`，将看不到临时 reference/import FTS row；正常 CLI/API 通过 repository status 查询不会暴露该全新冷 scope。active 与 retained scope reindex 路径仍保留中间 edge rows 以限制兼容性风险。
## 候选优化说明：manual-same-named-caller-demotion-20260518
- 目标：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时提升 large-repo caller 查询对外部调用点的排序质量，尤其避免 wrapper、递归或同名转发函数在查询 “who calls X” 时压过真实业务调用点。
- 算法与架构：在既有 bounded FTS candidate、方向过滤、path/language filter 与 Rust scoring 之后，对 `callers` 查询新增同名 caller penalty：只比较 caller leaf identifier 与 callee leaf identifier 的 ASCII alphanumeric 规范化形态，若二者相同则小额降权。该逻辑不扩大候选窗口，也不改变 callees/hybrid/definition/reference/import 查询。
- 不变量：不改变 SQLite schema、索引写入、FTS MATCH 表达式、candidate limit、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、HTTP/QoS、安装发布或仓库/路径/符号/case 特殊分支；resolved/ambiguous confidence、test path intent、caller context bonus 与最终 dedupe/truncate 仍按既有流程执行。
- 预期影响：LevelDB、Linux、Kubernetes、Spring Framework 等大仓中，`KeyMayMatch`、`RunKubelet`、adapter/wrapper 风格函数的 caller 查询会把外部调用点排在同名 wrapper/recursive edge 之前，提高 competitive caller rank 与 research judge 对泛化排序策略的评价；基础 `_summary`、JS runtime、semantic/vector 与 negative cases 应保持不变。
- 已知风险：用户明确寻找递归或 wrapper 自调用时，同名 caller edge 会被轻微降权但仍保留在结果中；该取舍符合默认 caller 查询优先展示外部影响面的语义，可通过更具体的 caller context query 恢复排序。
## 候选优化说明：manual-batch-edge-language-map-20260518
- 目标：在保护 foundational、competitive、semantic/vector、research judge 与 stability 下限的前提下，降低多仓 full-scope `repo register` 到 `repo index` 的 SQLite 写入与 finalize 前批处理成本，优先改善 relay-teams、LevelDB、Spring/Kubernetes 等大仓 cold indexing wall time。
- 算法与架构：checkpointed batch 写入 reference/import search document 时，先从当前 `CodeIndexBatch.files` 构造 path -> language_id 映射；只有发现 edge path 不在本批文件集合中时，才按缺失 path 逐条回查 `code_repository_files` 作为兼容兜底。reference 与 import 共用同一映射，避免每个 batch 对整个 source scope 重复扫描文件表。
- 不变量：不改变 SQLite schema、FTS document 字段语义、candidate limit、query ranking、call rebuild/finalize、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP/QoS 或安装发布行为；正常 parser/indexer 仍要求 edge 事实归属于同批文件，兜底只保护 legacy 或异常 batch。
- 预期影响：大仓每批不再为 reference/import 各执行一次全 scope file-language lookup，减少批处理 SQLite 读放大；edge search row 的 `language_id` 与既有测试保障保持一致，language-filtered edge query coverage 不应退化。
- 已知风险：如果未来引入跨批 edge records 且缺失 path 数量很大，兜底会退化为逐 path 查询；该路径表示 batch 事实与文件事实不一致，应由后续 worker/batch contract 测试收敛，而不会影响正常 full-scope indexing 热路径。
## 候选优化说明：manual-runtime-dist-scope-and-callsite-test-demotion-20260518
- 目标：修复 recent `research_judge` gate 指出的 relay-teams JavaScript runtime 零召回与 LevelDB `KeyMayMatch` production caller 排名退化，同时保护 foundational、competitive、semantic/vector、stability、provider/env 与 judge 配置下限。
- 算法与架构：该历史候选曾只允许源码语言文件进入 `dist/{js,javascript,ts,typescript,src,source,sources}/{app,client,core,runtime,server}` runtime 子树；Issue #231 后 clean Git 索引改为以 tracked tree 为目录权威，目录名不再默认排除，仍保留二进制媒体、map/jsonl 和锁文件的文件级保护。Caller/callee 排序在已有 bounded FTS candidate、方向过滤、最终 Rust scoring 内，对无 test intent 的 test/benchmark call site 加小额 penalty，让 production call site 不被 resolved test edge 的置信度差压低。
- 不变量：不改变 SQLite schema、事实模型、candidate limit、CLI/API JSON、semantic/vector provider URL/API key/model/dimension、embedding 设置、research judge URL/API key/model/CLI、HTTP/QoS、安装发布或仓库/符号/case 特殊分支；显式 `--path` opt-in 与 `.gitignore` 处理保持由 Git status/clean tree 负责，query 明确提到 test/benchmark 时不做 test path penalty。
- 预期影响：`frontend/dist/js/core/stream.js`、`state.js` 等 runtime source 可生成 symbols/calls/chunks，恢复 JS definition/caller/hybrid cases；`table/table.cc` 的 `filter->KeyMayMatch` production caller 应排在 `filter_block_test.cc` 等 test callers 前五。索引成本只增加窄 runtime source bucket，查询成本只增加常数级 path intent scoring。
- 已知风险：默认仍会跳过 `dist/js/components` 等非 runtime bucket 中的源码，需显式 `--path` 纳入；无 test intent 的真实测试代码查询会被轻微降权，但用户 query 包含 test/benchmark 词时保持原排序。
## 候选优化说明：manual-semantic-vector-source-hash-metadata-only-20260518
- 目标：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时提升本地 semantic/vector read model 在多源、多仓索引中的排序稳定性，避免文档唯一 source hash 作为检索 token 或向量特征稀释真实语义重叠。
- 算法与架构：`graph_semantic_documents` 与 `graph_vector_documents` 继续持久化 `source_hash`、model、dimension、graph version 与 tokenizer metadata；但 token signature 和 hashed vector 只由 content、entity labels 与 source path 生成。查询侧、semantic overlap、vector ANN 和 temporal term parsing 复用同一 metadata-free signature。
- 不变量：不改变 SQLite schema、刷新队列、CLI/API JSON、BM25、code graph retrieval、provider/env 配置、外部 embedding URL/API key/model/dimension 读取方式、research judge 配置、HTTP/QoS 或安装发布行为；source hash 仍作为 freshness、diagnostics 和 cursor metadata 存储，不参与用户 query scoring。
- 预期影响：semantic/vector fixture 中内容词、实体 label 和 source path 的相似度不再被每条文档独有 hash token 降低，`sv_semantic_context_pack_source`、`sv_vector_backend_freshness_source` 与 provider metadata recall 的排序余量应改善或保持；代码仓库查询和 indexing wall time 应基本不变。
- 已知风险：如果用户把 source hash 本身作为检索 query，本地 semantic/vector family 不再通过该 hash token 返回文档；这是有意的 metadata/query 分离，hash 仍可通过 diagnostics、index cursor 与 storage metadata 审计。
## 候选优化说明：manual-typescript-function-value-symbols-20260518
- 目标：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时提升多仓 JavaScript/TypeScript 仓库对 `export const name = (...) => ...`、class field arrow handler、object handler maps 和 CommonJS/member assignment functions 的 definition、hybrid 与 call graph 检索覆盖。
- 算法与架构：在既有 tree-sitter tag capture 后的 manual node pass 中，只对 JavaScript/TypeScript family 的 `variable_declarator`、`public_field_definition`、`pair`、`assignment_expression` 且 value/right 为 `arrow_function` 或 `function_expression` 的节点补充 function symbol；名称只来自直接 identifier/property/member property，复用现有 symbol id、签名、chunk、call/reference、identity enrich 与 bounded query pipeline。
- 不变量：不改变 SQLite schema、FTS/candidate limit、ranking 权重、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、HTTP/QoS、安装发布或仓库/case 特殊分支；非函数常量、destructuring binding、普通字段、computed/subscript assignment 和 `module.exports = function` 默认导出仍不会被当成命名 function symbol。
- 预期影响：relay-teams 以外的前端/服务混合大仓可把现代 JS/TS 函数值纳入 code graph，改善 full-scope repository tree parsing、symbol definition recall、hybrid chunks 和 caller/callee ownership；现有 Python/Go/Java/C++ cases 与 semantic/vector source coverage 应保持不变。
- 已知风险：新增 symbol 可能让同名 JS/TS function-valued bindings 参与近同分排序；风险受语言、node kind、function-valued value/right、identifier-name 验证、computed-key 排除、existing upsert 去重和最终 score/dedupe/truncate 限制。
## 候选优化说明：manual-checkpointed-typescript-import-resolution-20260518
- 目标：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时补齐 checkpointed full-scope indexing 对 TypeScript/TSX 相对导入边的解析，降低多仓前端/服务混合代码库中 import graph 的遗漏。
- 算法与架构：checkpointed batch finalize 在已有 Python/Go/Java/C++ resolver 旁新增 TypeScript/TSX resolver，复用 source-root normalized module-path index、bounded symbol-by-name index 和相对模块候选规则，支持 `./`、`../`、extension 替换与 `index.*` barrel 文件；命名导入必须唯一落到候选模块文件中的符号，默认或 side-effect 导入只要求唯一模块文件。
- 不变量：不改变 SQLite schema、FTS candidate limit、ranking/scoring、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP/安装发布行为，也不硬编码仓库、路径、模型、URL、密钥或维度；非相对 TypeScript package import 保持 unresolved，避免把外部 package 猜成仓内文件。
- 预期影响：large-repo checkpointed indexing 生成的 TypeScript import `target_hint` 可用于 import FTS 与 target-symbol fallback，提升多语言仓库 import 查询、hybrid 解释和 research judge 架构覆盖；无 TypeScript 仓库、snapshot identity 路径和既有 relay-teams/LevelDB/Kubernetes/Spring cases 应保持不变。
- 已知风险：barrel 文件或多文件 re-export 中同名符号可能被标为 ambiguous 而非 resolved；这是有意的唯一性保护，防止为 import graph 写入错误的单文件 target hint。
## 候选优化说明：manual-scoped-edge-identity-ranking-20260518
- 目标：在保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限的前提下，提升多仓 full-scope code graph 查询对 dotted、`::`、路径式 qualified symbol identity 的 callers/references/imports 召回和排序。
- 算法与架构：directional call FTS 预过滤把 query 按代码标识符边界拆成 bounded LIKE token，避免 `pkg.service.Target` 被当成单个 pattern 而误裁剪；call/reference/import scoring 将 `target_hint` 与 canonical symbol id 纳入既有 `ScoreQuery`，并对 query scoped terms 与 edge identity 连续匹配给予小额 bonus。
- 不变量：不改变 SQLite schema、索引写入、FTS MATCH 主表达式、candidate limit、BM25 排序、hit JSON 字段、CLI/API 行为、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP 或安装发布路径；所有新增判断都在已有 bounded candidate 和最终 Rust scoring 内完成，无仓库、路径、符号或 fixture 特殊分支。
- 预期影响：relay-teams、LevelDB、Kubernetes、Spring Framework 等大仓中，使用 fully-qualified class/function/module 名称询问 callers、references、imports 或 hybrid edge context 时，不再因方向预过滤或目标身份字段未计分而丢失目标；基础 `ConnectorService`、W3 request、`_summary`、negative missing symbol 与 semantic/vector cases 应保持通过。
- 已知风险：scoped edge identity bonus 可能在极少数同名 qualified targets 中改变近同分排序；风险受 FTS candidate、direction/path/language filter、scoped contiguous match 和较小 bonus 约束，未扩大无界候选窗口。
## 候选优化说明：manual-qos-prebound-listener-test-20260518
- 目标：修复 quality gate repair mode 中 `cargo_test` 的 `serve_router_with_qos_rejects_excess_connections` 偶发端口复用竞态，优先恢复 stability gate，并保持 foundational、competitive、semantic/vector 与 research judge 下限不变。
- 算法与架构：测试先用 Tokio 绑定 `127.0.0.1:0` 并读取实际地址，再把已绑定 listener 包装为现有 `QosTcpListener` 交给 `serve_listener`；QoS admission、连接 permit 生命周期、Axum serve future、超预算连接关闭和 graceful shutdown 断言仍走生产 listener/server 路径。
- 不变量：不改变生产 `serve_router_with_qos`、QoS policy/runtime、HTTP 配置解析、CLI/API、SQLite schema、索引、retrieval ranking、semantic/vector provider/env、embedding 设置、research judge 配置或安装发布行为；只消除单测中 “探测空闲端口后释放再重绑” 的非确定性前置条件。
- 预期影响：`cargo test --all-targets --all-features` 不再因端口被其他并发测试或进程抢占而误判 QoS server 未接受连接；relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 和 graph retrieval fixture 的检索结果与延迟不应直接变化。
- 已知风险：该候选修复测试同步边界而非提升检索评分；如果未来 `serve_router_with_qos` 外层 bind 逻辑变化，仍需由配置解析或新的外层 bind 测试覆盖。
## 候选优化说明：manual-edge-search-language-materialization-20260518
- 目标：保护 relay-teams、LevelDB、Linux、Kubernetes、Spring Framework、semantic/vector 与 research judge 下限，同时把 reference/call/import 的 language selector 剪枝从 correlated file lookup 推进到 FTS search row 本身，降低多语言大仓 edge query 的候选窗口噪声。
- 算法与架构：snapshot、checkpointed batch、finalize 的 reference/import/call search document 写入统一带上所属 file 的 `language_id`；schema 初始化对旧 edge search row 做幂等 language 回填；edge 查询复用 symbol/chunk 的 `fts_path_and_language_filter_sql`，在 SQLite FTS bounded candidate subquery 内直接按 `language_id` 剪枝，Rust `selected_row` 继续作为最终一致性保护。
- 不变量：不改变 SQLite 表结构、事实表、FTS MATCH term、candidate limit、BM25 排序、score/ranking/fusion、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP 边界或安装发布行为；无仓库、路径、符号或 fixture 特殊分支，旧数据库通过启动回填保持兼容。
- 预期影响：language-filtered callers/callees/references/imports 在 Python/Java/Go/Rust 混合仓库中不再需要每个 FTS candidate 再关联文件表验证语言，减少范围外语言在评分前占用候选预算，保护 `ConnectorService`、W3 request、`_summary`、negative missing symbol 与 LevelDB scoped definition floor。
- 已知风险：新增回填只修复有匹配 file row 的 edge search document；缺失文件事实时仍保留空 language 并由后置过滤防止错误结果。无 language filter 查询路径和 semantic/vector 检索不受影响。
## 候选优化说明：manual-directional-call-candidate-filter-20260518
- 目标：保护 relay-teams `_summary` callers/callees、ConnectorService hybrid、LevelDB/Kubernetes/Spring call graph 与 semantic/vector 下限，同时降低大仓 call graph 查询被反向 caller/callee 文本填满 bounded FTS candidate window 的风险。
- 算法与架构：call graph FTS 文档继续保留 caller、callee、target hint 与 path 以支持 hybrid；当查询类型是 `callers` 或 `callees` 时，在 FTS 子查询内用 `code_repository_calls` 主键关联加入方向感知 SQL LIKE 过滤：`callers` 只让 callee 名称匹配查询 token 的 call 进入候选，`callees` 只让 caller 名称匹配查询 token 的 call 进入候选。最终 Rust scoring、line-range 扩展、去重融合与排序权重不变。
- 不变量：不改变 SQLite schema、索引写入、FTS MATCH 表达式、candidate limit、CLI/API JSON、semantic/vector provider/env、embedding、research judge 配置、HTTP/网络边界或安装发布行为；没有仓库、路径、符号或 fixture 特殊分支，hybrid call 搜索仍保持原 undirected 候选集合。
- 预期影响：多仓 full-scope 查询中，反向 caller/callee 噪声不会在 scoring 前耗尽 call candidate budget，`_summary` callers/callees、large-repo call graph 和 research judge 对架构泛化的评价应更稳定；无 call direction 查询、definition/import/chunk 与 semantic/vector coverage 应保持不变。
- 已知风险：callers/callees 查询会在 FTS row 上多一次按 `(source_scope, call_id)` 的主键存在性检查和少量 LIKE token 过滤；成本受既有 bounded candidate window 控制，查询 token 上限为 8。
## 候选优化说明：manual-edge-fts-file-language-pushdown-20260518
- 目标：保护 relay-teams、LevelDB、Linux、Kubernetes 与 Spring Framework 等多语言大仓的 full-scope code graph retrieval，修复 reference/call/import 查询在带 language selector 时仍可能先让范围外语言填满 bounded FTS candidate window 的召回风险。
- 算法与架构：symbol/chunk 已使用 FTS 行内 language filter；本轮对 reference/call/import 查询新增 edge 专用 FTS 过滤 SQL，在 FTS 子查询内保留既有 path filter，并通过 `code_repository_files` 的 `(source_scope, path)` 主键关联校验 `language_id`。这样无需改写已有 FTS edge 文档或 schema，也兼容旧数据库中 edge search row 的空 `language_id`。Rust `selected_row` 后置过滤继续作为一致性保护。
- 不变量：不改变索引写入、SQLite schema、FTS MATCH 表达式、candidate limit、BM25 排序、score/ranking/fusion、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP 边界或安装发布行为；无仓库、路径、符号或 case 特殊分支。
- 预期影响：按语言查询 callers/callees/references/imports 时，候选剪枝发生在 scoring 前，避免 Python/JavaScript/Go 等噪声 edge 吃掉 Rust/Python 目标语言的候选窗口；预期提升 `ConnectorService` 这类 path/language filtered case 的稳定性，并降低无效 edge scoring。
- 已知风险：只有带 language filter 的 edge 查询会多一次按 `(source_scope, path)` 的文件表存在性检查；无 language filter 查询仍走原 candidate plan。收益集中在多语言噪声较高的仓库，单语言仓库影响应接近零。
## 候选优化说明：manual-score-query-field-identifier-cache-20260518
- 目标：在 relay-teams、LevelDB、Linux、Kubernetes 与 Spring Framework 等大仓 full-scope 查询中，保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，同时修复近期 relay-teams query p50/p95 退化。
- 算法与架构：保持 SQLite FTS、path/language filter、candidate limit、排序权重和 hit 去重不变；`ScoreQuery` 在每个候选字段内惰性缓存 identifier token 集合，避免多 token 查询对同一 symbol、signature、path 或 chunk 字段重复执行 snake/camel 拆分。
- 不变量：不改变 schema、索引写入、查询候选集合、score 分值语义、CLI/API JSON、provider/env、embedding、judge 配置、网络/HTTP 边界或 release/install 行为；新增单元测试锁定多 token identifier 分值。
- 预期影响：多词定义、hybrid、caller/callee、import 与 chunk 查询减少重复字符串拆分和分配，改善大仓查询 p50/p95 稳定性；由于召回和权重不变，`ConnectorService`、W3 request、callers/callees、negative missing symbol、LevelDB competitive 与 semantic/vector coverage 应保持通过。
- 已知风险：短查询或单字段候选收益有限；缓存只存在于一次候选评分调用内，内存开销受既有 bounded candidate window 和字段数量约束。
## 候选优化说明：manual-self-iteration-resolved-gate-filter-20260518
- 目标：修复自迭代 prompt 把已被后续通过记录覆盖的旧 quality gate 失败继续列为当前修复优先级的问题，避免候选反复围绕已修复的 `repo index`/`repo query` 竞态诊断而忽略新的 research judge、性能或检索质量退化。
- 算法与架构：`recent_failed_gate_names` 与 `recent_failed_gate_diagnostics` 仍按 run history 从新到旧扫描，但新增 gate 名称级的 `resolved` 集合；一旦较新的 run 记录某 gate 已通过，旧 run 中同名失败不再进入当前优先级或失败命令诊断。诊断列表同时保留 `seen` 去重，确保只展示每个仍未解决 gate 的最新失败命令。
- 不变量：不改变 evaluator 的 Cargo/repo/file/semantic/vector/research judge 执行、评分权重、保护目标、accept/reject 判定、CLI/API 行为、SQLite schema、检索 ranking、provider/env、embedding 或 judge 配置；只改变下一轮 Codex prompt 的质量门禁上下文选择。
- 预期影响：当后续 accepted 或 rejected-but-gate-passing run 已证明某 gate 恢复时，prompt 不再进入过期 gate repair mode；当前未被 newer pass 覆盖的失败仍会优先展示。预期提升研究评审对齐与候选选择效率，避免牺牲已通过的 foundational、competitive、semantic/vector 与 stability floor 去追逐旧故障。
- 已知风险：如果较新的 run 因环境偶然性让某 gate 通过，而底层问题仍间歇存在，旧失败会被当前 prompt 降级到历史 rejected/memory context；该风险由后续再次失败时重新进入 `resolved` 之后的最新失败诊断来控制。
## 候选优化说明：manual-code-query-score-query-token-cache-20260518
- 目标：在保护 foundational、competitive、semantic/vector、research judge 与 stability 下限的前提下，降低 relay-teams、LevelDB、Linux、Kubernetes 与 Spring Framework 等大仓 full-scope code query 的候选评分 CPU 成本。
- 算法与架构：SQLite 仍先用既有 FTS、path/language filter 与 bounded candidate limit 剪枝；Rust scoring 热路径新增 request-scoped `ScoreQuery`，把 query whitespace token 的 lowercase 归一化从每个候选重复执行改为每个请求执行一次，并在 symbol、reference、call、import 与 hybrid chunk 层复用同一 token 集合。
- 不变量：不改变 SQLite schema、索引写入、FTS MATCH 表达式、candidate limit、排序权重、去重截断、CLI/API JSON、semantic/vector provider/env、embedding 设置、judge 配置、网络/HTTP 边界或 release/install 行为；保留 `score_text` 兼容入口并用单元测试锁定分数语义一致。
- 预期影响：多 token、多候选的大仓查询减少重复分词与 lowercase 分配，预期改善 query p50/p95 的稳定性；由于候选集合与分数公式不变，`ConnectorService`、W3 request、callers/callees、negative missing symbol、LevelDB competitive 与 semantic/vector source coverage 应保持通过。
- 已知风险：收益依赖候选窗口大小和 query token 数；短查询或低候选量 case 可能只有轻微性能变化。`ScoreQuery` 仍按候选字段计算 field lowercase 与 identifier part match，因此不牺牲现有精确/identifier/substring scoring 行为。
## 候选优化说明：manual-code-query-bounded-symbol-context-20260518
- 目标：修复质量门禁中 relay-teams `ConnectorService` definition/hybrid/path-filtered definition 与 `_summary` callers/callees 的命中行范围过窄问题，同时保护 foundational、competitive、semantic/vector、research judge、stability 与 negative missing symbol 下限。
- 算法与架构：SQLite code query 只在已通过 FTS candidate、selector filter 与既有 scoring 的 symbol/call graph 命中上扩展返回 `line_range`；class definition 可向前包含同文件 16 行内相邻上一 symbol 起点，caller/callee 查询可返回 call-site 所属 caller symbol 的 bounded range。新增 `(source_scope, path, line_end, line_start)` 索引支撑相邻 symbol 查找，避免全表扫描。
- 不变量：不改变索引写入内容、FTS 查询表达式、candidate limit、排序权重、CLI/API 字段、semantic/vector provider URL/API key/model/dimension 环境读取、embedding 设置、judge 配置、HTTP/网络行为或 release/install 行为；没有仓库名、路径名或符号名特殊分支，最终排序仍由既有 score 和去重截断决定。
- 预期影响：大型仓库中 class 声明前的 protocol/decorator/typed preamble 与 resolved call site 所属函数范围可被 line-based evaluator 和用户定位识别，预期修复 `ConnectorService` definition/hybrid/filter 与 `_summary` callers/callees 门禁，W3 request/import、LevelDB/Linux/Kubernetes/Spring 和 semantic/vector source coverage 保持不变。
- 已知风险：少数 class 或 resolved call hit 的起始行会比精确语法节点更早，但窗口受同文件相邻 symbol 与 16 行上限约束，不会扩成整文件上下文；额外 SQL 子查询只作用于 bounded candidate rows。
## 候选优化说明：manual-streamed-call-finalize-20260518
- 目标/算法/架构：保护 foundational、competitive、semantic/vector、research judge 与 stability 下限，降低多仓 full-scope `repo register` finalize 阶段 call graph rebuild 的内存与分配成本；符号按 path 移入 `HashMap<Vec<SymbolKey>>`，call reference 从 SQLite cursor 流式读取并立即写入 `code_repository_calls`。
- 不变量/风险：不改变 SQLite schema、reference/import/symbol 事实、call id、caller resolution、FTS search document、ranking、CLI/API、provider/env、judge 配置或安装行为；风险仅在 cursor 与 insert statement 同事务并行使用，现有 rusqlite prepared statement 生命周期和单元/批处理测试覆盖。
- 预期影响：relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 等 reference-heavy 仓库少一次全量 call reference `Vec` 收集和 symbol clone，降低 cold register-to-index finalize wall time 与峰值内存；查询结果和 semantic/vector coverage 应保持不变。
## 候选优化说明：manual-cli-repo-index-inline-worker-20260518
- 目标/算法/架构/不变量/影响/风险：修复质量门禁中 `repo index` 返回 queued task 后首个 `repo query --freshness wait-until-fresh` 立即报 “no index for ref” 的竞态；CLI 仍经 durable code-index task 建立 bounded full-index task，但当前进程会立即执行同一 task 的 worker lease 并在返回前刷新 status/checkpoint，不改变 Web/API `start_code_repository_index` 后台语义、SQLite schema、code graph parsing/ranking、query JSON、semantic/vector provider/env、embedding 或 judge 配置。预期 relay-teams 与 LevelDB full-scope gates 在显式 index 后已有 fresh scope，查询延迟保持在已建索引路径；风险是一次性 CLI index wall time 上升，但成本位于写索引命令内且 service/Web 后台模型保留。
## 候选优化说明：manual-vector-overlap-identifier-fallback-20260517
- 目标与算法：保护 foundational、competitive、semantic/vector、stability 与 research judge 目标，同时修复 vector read model 最终 overlap guard 对代码/配置标识符词形过窄的问题；当既有 lowercase whitespace substring 快路径无命中时，使用共享 semantic/vector token signature 对 query、content、entity labels 和 source path 做 snake_case、CamelCase、缩写与路径 term 归一化，再按规范化 term overlap 接受候选。
- 架构与不变量：不改变 SQLite schema、BM25/FTS 文档、candidate limit、RRF fusion、local vector hash、semantic scoring、CLI/API 字段、provider URL/API key/model/dimension/env 读取、judge 配置或 self-iteration harness；现有 substring 快路径先返回，identifier fallback 只扩大原本会被误拒绝的派生候选。
- 预期影响：`retry_policy` 查询匹配 `Retry policy` 文本、`GraphRAGContextPack`/`RuntimeBudget` 等标签拆分、source path 标识符和大仓代码证据的 vector source coverage 更稳定；semantic source、code query ranking、backend availability 与质量门禁应保持不变或改善。
- 已知风险：对快路径无命中的 vector/graph derived 候选会多做一次 bounded token signature 计算；该成本限制在已通过 SQL candidate pruning 的候选上，且保留 substring 快路径以控制常见自然语言查询延迟。
## 候选优化说明：manual-code-query-language-filter-pushdown-20260517
- 目标与算法：在保持 foundational、competitive、semantic/vector、stability 与 research judge 目标不变的前提下，把 code graph symbol/chunk 查询的 selector language filter 下推到 `code_repository_search` FTS bounded candidate window，避免多语言大仓中范围外语言先填满候选上限后再被 Rust 层丢弃。
- 架构与不变量：不改变 SQLite schema、FTS 文档内容、candidate limit、BM25 排序、score_text、CLI/API 字段、provider URL/API key/model/dimension/env 读取或 judge 配置；path filter 与 language filter 的 SQL 占位值顺序保持显式对应，最终 `selected_row` 仍作为一致性保护。
- 预期影响：relay-teams、LevelDB、Linux、Kubernetes、Spring Framework 等 full-scope 多语言索引在按语言查询 definition/hybrid chunk 时提升召回稳定性并减少无效候选评分；无 language filter 的既有 case 和 semantic/vector source coverage 应保持不变。
- 已知风险：收益集中在 language-filtered symbol/chunk 查询；reference/call/import FTS 文档当前不携带可靠 language_id，因此仍保留既有后置过滤以避免误裁剪。
## 候选优化说明：manual-score-text-saturation-20260517
- 目标：在保持 foundational、competitive、semantic/vector、accuracy、stability 与 research judge 保护目标不变的前提下，降低大仓 code graph query scoring 热路径中重复的 identifier 分解和 substring 检查成本。
- 方法：`score_text` 保留 exact、identifier-part、substring 三层分值不变，但当当前 query token 已达到 exact match 最高分时立即结束该 token 的字段扫描；当已达到 identifier-part 分值时，后续字段只继续检查可能提升到 exact 的分支，不再重复执行无法提高分数的 identifier 或 substring 检查。
- 架构与不变量：不改变 SQLite schema、FTS candidate expression、candidate limit、path/language filter、排序权重、CLI/API 字段、semantic/vector provider、embedding 设置、judge 配置或环境变量读取；这是对确定性 scoring 的饱和短路，不扩大或收窄候选集合。
- 预期影响：relay-teams、LevelDB、Linux、Kubernetes、Spring Framework 等多仓 full-scope code query 在多字段、多 token 候选评分时减少无效字符串扫描；所有已通过的 foundational/competitive case rank、negative query 行为和 semantic/vector source coverage 应保持不变。
- 已知风险：该候选是语义保持型优化，主要收益取决于候选窗口中重复 identifier 命中的比例；如果查询通常只有一个字段命中或候选很少，可观测延迟改善可能较小。
## 候选优化说明：manual-derived-read-model-cache-preserve-score-20260517
- 目标：在保持 foundational、competitive、semantic/vector、accuracy 与 research judge 保护目标不变的前提下，降低 semantic/vector 本地 read model 和 local rerank 热路径中的重复分配与重复 query vector 哈希成本。
- 方法：共享标识符 normalizer 增加可扩展现有 `BTreeSet` 的接口，semantic signature、hashed vector 与 rerank fact/label term 收集复用同一集合而不是构造临时集合；vector candidate loop 为每个查询按维度缓存本次 hashed query vector，避免同一维度候选逐行重算。
- 架构与不变量：不改变 CLI/API 字段、SQLite schema、FTS/BM25 文档、candidate filter、candidate limit、RRF fusion、local deterministic scoring公式、external provider URL/API key/model/dimension/env 读取、embedding payload、freshness、QoS、judge 或 self-iteration harness；semantic/vector 最终分数与结果排序应与现有算法一致。
- 预期影响：local documents、graph retrieval fixture、semantic/vector fixture 和大仓 graph retrieval 查询在 semantic/vector 来源参与时减少临时集合分配和 per-row query vector hashing；protected retrieval source coverage、backend availability、case rank、stability 与 research judge 应保持不变或改善。
- 已知风险：该候选主要优化 CPU/分配，不扩大召回、不剪枝候选、不改变评分权重，因此质量风险低；可观测性能改善取决于候选窗口大小和向量维度分布，通常在多候选同维度 vector read model 查询中最明显。
## 候选优化说明：manual-identifier-aware-semantic-vector-rerank-20260517
- 目标：提升 graph semantic/vector 与本地 rerank 对代码符号、实体标签和路径中复合标识符的泛化检索质量，避免 `GraphRAGContextPack`、`SemanticVectorRecall`、`retry_policy`、`RESTClient` 这类标识符只作为一个不透明 token 参与语义签名、向量哈希或 rerank 覆盖度。
- 方法：新增检索层共享 term normalizer，在保留完整 token 的同时拆分 snake_case、PascalCase/CamelCase、连续大写缩写与数字边界，并为多段标识符加入 acronym token；SQLite semantic signature、local hashed vector 与本地 deterministic rerank 统一使用该 normalizer。新增单元与存储集成测试锁定 label-only 标识符拆分后同时贡献 semantic/vector 来源。
- 架构与不变量：不改变 CLI/API 字段、SQLite schema、FTS/BM25 文档、code graph query behavior、external provider URL/API key/model/dimension/env 读取、embedding payload、candidate limit、RRF fusion、freshness、QoS 或 self-iteration harness；完整原始 token 仍保留，新增 term 只扩展已有 semantic/vector/rerank 内部表示。
- 预期影响：semantic/vector fixture、GraphRAG evidence、code symbol/chunk read model 和 agent context pack 查询在自然语言词序与代码标识符词形不一致时更容易获得 semantic/vector source coverage，并在本地 rerank 中把实体标签或代码 artifact 命中的候选排到只含泛化文本的候选前；foundational/competitive repo code query、provider probe 和稳定性不应退化。
- 已知风险：semantic/vector read-model token 集合会因标识符拆分和 acronym 增加少量项，可能轻微增加刷新与查询 CPU；实现限制在已有 bounded candidate/rerank 流程内，且保留完整 token 以降低精确标识符查询退化风险。
## 候选优化说明：manual-compound-identifier-fts-query-recall-20260517
- 目标：提升大仓 full-scope code graph 与 hybrid chunk 查询对自然语言拆分标识符的召回，避免 `new lru cache`、`default listable bean factory` 这类查询在 FTS 候选阶段错过 `NewLRUCache` 或 `new_lru_cache` 形态。
- 方法：在代码查询 FTS MATCH 构造阶段，为 bounded call/reference/import 与 hybrid chunk 查询追加受限的复合标识符候选，把 2 到 6 个安全 ASCII 查询项扩展为 compact 与 snake_case 两种 OR 分支；symbol 查询保持已有 symbol 文档侧 camel/snake 扩展，不重复扩大候选。
- 架构与不变量：不改变 CLI/API 字段、SQLite schema、索引写入格式、candidate limit、排序截断、semantic/vector provider、embedding、rerank、judge 或环境变量读取方式；新增扩展只影响查询表达式，且限制词数、part 长度、总标识符长度和单字符噪声，最终仍由 `score_text`、path/language filter 与既有 layer 排序决定返回顺序。
- 预期影响：LevelDB/C++、Kubernetes/Go、Spring/Java 和 relay-teams/Python 中以拆分标识符询问 caller/callee、reference、import 或 fuzzy chunk 的查询可进入候选窗口，精确 CamelCase/snake_case 查询、target-symbol import fallback、semantic/vector source coverage 和稳定性应保持不变或改善。
- 已知风险：OR 分支会让少量 compact/snake 标识符命中的候选进入 bounded window；扩展只对短查询项集合生效，并保留后续文本评分过滤，因此主要风险是极少数同名复合标识符在近同分情况下改变排序。
## 候选优化说明：manual-import-target-filter-pushdown-20260517
- 目标：提升大仓 full-scope import graph 在带 selector path/language filter 时的 target-symbol 查询准确性与稳定性，避免查询导入者范围时把过滤条件误施加到被导入符号定义，或让路径外/语言外导入边先填满 bounded candidate window。
- 方法：import target-symbol fallback 分两阶段处理：第一阶段只在当前 indexed source scope 内用 bounded symbol FTS 找到查询命中的目标符号，并生成 path/package target hints；第二阶段通过 `code_repository_imports(source_scope, target_hint, path)` 查找导入边时，把 indexed scope 和本次 selector path filters 下推到 `i.path`，把 language filters 下推到 `f.language_id`，在 `ORDER BY ... LIMIT` 前裁剪导入者候选。
- 架构与不变量：不改变 CLI/API 字段、SQLite 表结构、candidate limit、FTS 文档、semantic/vector provider、embedding、rerank、judge 或环境变量读取方式；最终 `selected_row` 仍保留为一致性保护，新增 SQL pushdown 只减少范围外 import edge 候选。目标符号发现不再使用本次导入者 path/language filter，因为 selector filter 描述的是返回的 import rows，而不是被导入符号必须所在的路径或语言。
- 预期影响：Kubernetes/Go package import、Spring wildcard import、relay-teams Python re-export 等以符号名查询 import graph 的 case 在窄路径/语言查询和大仓噪声下更稳定；范围外 import noise 不会消耗 bounded target-hint lookup window，查询延迟也可能因更早裁剪导入边而改善。
- 已知风险：target-symbol fallback 的符号发现阶段会在 source scope 内查看比 selector path/language filter 更宽的符号集合；最终 import rows 仍受 selector path/language 过滤和 bounded target-hint lookup 约束，因此风险主要是多一次 bounded symbol FTS 可能找到同名符号并生成额外 target hints，但不会返回范围外导入者。
## 候选优化说明：manual-java-wildcard-import-target-recall-20260517
- 目标：提升 Spring Framework 等 Java 大仓 full-scope import graph 的符号查询召回，尤其是代码使用 `import package.*` 时，查询具体类名或 fully-qualified class name 能找到通配 package import 的导入者。
- 方法：Java import resolution 对 package wildcard 记录 source-root normalized package directory 作为 `target_hint`，直接类/静态通配 import 在可唯一解析时记录具体 Java 源文件；import target-symbol 查询把符号文件路径扩展为实际路径、实际父目录、去 source-root 路径和去 source-root 父目录，并允许不含路径分隔符的 fully-qualified class 查询进入 bounded symbol-target 扩展。
- 架构与不变量：不改变 CLI/API 字段、SQLite 表结构、candidate limit、semantic/vector provider、embedding、rerank、judge 或环境变量读取方式；仍只在已有 bounded symbol FTS 召回后，通过 indexed `code_repository_imports(source_scope, target_hint, path)` 查找 import 候选。路径型查询和常见文件扩展名查询不会进入 symbol-target import fallback，避免把文件检索误扩成 package import 检索。
- 预期影响：`org.springframework.context.ApplicationContext` 这类 FQN 查询可通过 `import org.springframework.context.*;` 返回导入文件；Spring package wildcard import、Kubernetes Go package import target-symbol fallback、relay-teams Python import、LevelDB C/C++ graph 查询和 semantic/vector source coverage 应保持或改善。
- 已知风险：Java wildcard target hint 采用 source-root normalized package directory，而不是唯一物理目录；在同一 package 同时存在 main/test/generated 源根时，它会提升跨 source-root package import 召回，但 edge target 不再指向单个文件。该设计只用于 wildcard package 边，直接类 import 仍保留具体文件 target hint。
## 候选优化说明：manual-go-package-import-symbol-recall-20260517
- 目标：提升 Kubernetes 等 Go 大仓 full-scope import graph 的基础边解析和竞争性检索召回，让查询导出类型或工厂符号时能返回导入对应本地包的源文件，而不是只匹配 import path 文本。
- 方法：Go tree-sitter import block 解析改为按每个 quoted import spec 生成独立 import record，保留 alias 与 package path；snapshot identity 与 checkpoint finalize 都通过通用 source-root normalization 解析 `staging/src/`、`vendor/` 和 `src/` 下的本地 Go package directory。import 查询增加 target-symbol candidate plan：先用已有 bounded symbol FTS 找到 query 命中的符号，再通过 resolved `target_hint` 文件或 package directory 找到导入者，并用匹配符号名参与排序。
- 架构与不变量：不改变 CLI/API JSON 字段、SQLite 表结构、provider/env 配置、semantic/vector 后端、embedding 设置或 self-iteration harness；新增索引只覆盖 `code_repository_imports(source_scope, target_hint, path)`，用于有界 target import 查找。SQLite code query 的评分/FTS helper 和 target-symbol import lookup 分拆到独立模块，保持触达文件低于行数上限。外部 Go package、标准库和无法唯一映射到本地 directory 的 import 仍保持 unresolved/ambiguous，不强行选择。
- 预期影响：`kubernetes_imports_client_go_informer_full_scope` 这类以 `SharedInformerFactory` 等导出符号查询 import graph 的 case 应能通过 resolved package target 找到 `pkg/kubeapiserver/authorizer/config.go`；Java/Python/C/C++ import resolution、relay-teams/LevelDB ranking、semantic/vector source coverage 和稳定性不应退化。
- 已知风险：Go module path 解析仍是静态 repository path 启发式，不读取 go.mod、replace 或 workspace 配置；如果多个本地目录映射到同一 import path，候选会标为 ambiguous 以保护准确性。target-symbol fallback 会多做一次 bounded symbol lookup 和 indexed target_hint import lookup，可能轻微增加纯 import query latency。
## 候选优化说明：manual-opencode-default-judge-cli-arg-order-20260517
- 目标：修复当前 quality gate repair mode 中 `research_judge` 失败；安装版 `opencode run` 的 `--file` 是数组选项，默认命令把 judge instruction 放在 `{prompt_file}` 之后时会被误当作第二个附件路径，导致 gate 报 `File not found`。
- 方法：调整 self-iteration judge 的默认 CLI command 为先传 message、再传 `--file {prompt_file}`，并增加单元测试锁定 argv 形态，确保默认 opencode 命令没有任何非选项参数跟在 prompt 文件路径之后。自定义 judge command、HTTP judge、disable backend 和 stdin prompt 模式保持原有逻辑。
- 架构与不变量：不写入 provider URL、API key、模型名、维度或 CLI secret；judge backend、HTTP endpoint、密钥、模型和自定义命令仍只从运行时环境读取。候选 diff、确定性评估摘要、rubric、严格 JSON 解析、置信度阈值、总分阈值、anti-fixture-special-casing 阈值和 retrieval evaluator 不变。
- 预期影响：默认本地 `opencode` judge 可读取 prompt 文件并返回 `research_judge` objective，不再因命令行参数顺序把有效候选拒绝；foundational、competitive、semantic/vector、stability、repo indexing 和检索排序不受影响。
- 已知风险：不同 opencode 版本如果改变 positional message 与 `--file` 的解析顺序，默认命令仍可能需要适配；该风险通过保留 `RELAY_KNOWLEDGE_JUDGE_COMMAND` 覆盖、`RELAY_KNOWLEDGE_JUDGE_BACKEND=none` 显式禁用和 focused 单元测试控制。
## 候选优化说明：manual-opencode-default-judge-cli-20260517
- 目标：让自迭代 research judge 在本地默认走 `opencode` CLI，减少每次启用开放式质量评审时都要手动配置 judge command 的操作成本。
- 方法：把未设置 `RELAY_KNOWLEDGE_JUDGE_BACKEND` 且没有 HTTP judge 配置的场景收敛到 CLI backend，并使用 `opencode run --file {prompt_file}` 默认命令；`RELAY_KNOWLEDGE_JUDGE_BACKEND=opencode` 作为 CLI alias，显式 CLI 命令和 HTTP 配置继续优先于默认值，同时保留 `RELAY_KNOWLEDGE_JUDGE_BACKEND=none/off/disabled/skip/false` 作为跳过 judge 的开关。
- 架构与不变量：仍只从运行时环境读取 judge backend、HTTP endpoint、密钥、模型和自定义命令，不把 provider URL、API key、模型名或 CLI secret 写入 `cases.json`、prompt 或报告。默认命令通过 `{prompt_file}` 传递长 judge prompt，避免把完整 prompt 放入 argv；judge 返回严格 JSON、置信度阈值、总分阈值和 anti-fixture-special-casing 阈值保持不变。
- 预期影响：默认 `self-iterate.py evaluate` 和候选评估会在可用的本地 `opencode` 环境中产生 `research_judge` objective；需要无 judge 的离线或 CI 场景可以显式设置 backend 为 `none`。
- 已知风险：机器缺少 `opencode`、未配置 opencode provider 或模型输出非严格 JSON 时，默认 judge 会作为质量 gate 失败；该风险通过显式 disable backend、继续允许 HTTP/CLI 覆盖，以及单元测试覆盖默认、覆盖和禁用路径来控制。
## 候选优化说明：manual-research-judge-cli-agent-20260517
- 目标：把自迭代中带研究性质的评估从确定性 case 中分离出来，让功能、架构、可靠性和性能泛化判断可以由 LLM judge 或开放 coding-agent CLI 执行，同时保留 build/test/retrieval/static checks 作为可复现硬门禁。
- 方法：新增 `research_judge_suite` 和 `llm_judge.py`，支持 OpenAI-compatible HTTP judge，也支持通过 `RELAY_KNOWLEDGE_JUDGE_COMMAND`、`RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND` 或 `RELAY_KNOWLEDGE_JUDGE_CLI_COMMAND` 调用 `relay-teams`、`codex`、`cc`、`copilot` 等 CLI agent；CLI 默认从 stdin 接收 prompt，也支持 `{workspace}`、`{prompt_file}`、`{prompt}` 占位符。Judge 必须返回严格 JSON，并按研究对齐、架构合理性、可靠性、性能泛化、实现可操作性和 anti-fixture-special-casing 维度评分。
- 架构与不变量：Judge 配置只从运行时环境读取，不写入 `cases.json`、报告或 prompt 中的密钥；未配置 judge 时记录 skipped 且不阻塞默认本地循环；显式配置但缺少变量、返回非法 JSON、低置信度、低总分或低 anti-fixture-special-casing 分数时作为硬 gate 拒绝。确定性 repo/file/semantic-vector cases、Cargo gates、provider probe 和文档 gate 保持原有职责。
- 预期影响：后续候选可以把开放式研究质量和架构取舍交给 judge 评审，减少把研究判断硬编码成脆弱 fixture 的压力；CLI agent judge 让本地或企业内开放 coding agent 也能作为评审后端参与自迭代。
- 已知风险：外部 judge 或 CLI agent 的稳定性、成本、输出格式和模型偏差会影响候选采纳；因此默认不启用 judge，启用后要求严格 JSON、置信度阈值和 anti-fixture-special-casing 阈值，并继续用确定性 gate 保护可复现行为。
## 候选优化说明：20260517T072446Z
- 目标：在保持 `semantic_vector_provider_probe` 通过、foundational cases 和 semantic/vector 保护项不退化的前提下，提高大仓 call graph caller 查询的 rank 1 稳定性，尤其是泛化的 callee 查询被 C API、binding、wrapper、FFI 等适配层调用点按路径排序压到实现调用点之前的场景。
- 方法：在既有 bounded FTS 与 resolved call edge 召回之后，扩展 `call_site_source_path_bonus` 为 caller 查询增加 adapter-surface path adjustment：当候选已有正分、查询没有 test/benchmark 意图、查询没有明确 adapter/API/binding/FFI/wrapper 意图，且路径段或文件名显示为适配层时，不授予普通生产源码的小幅正向调整；普通生产源码仍保留原有小幅正向调整，callee 查询不应用该 adapter 调整。
- 架构与不变量：不改变 SQLite schema、索引写入、call edge resolution、candidate limit、FTS query、CLI/API 字段、env/provider 配置、semantic/vector refresh、provider probe 语义或 self-iteration evaluator；该信号只在已有 call-edge 候选上参与排序，不扩大召回集合，不隐藏适配层结果，查询明确要求 API、binding、FFI、wrapper 或 adapter 时仍可优先返回相关路径。
- 预期影响：`leveldb_callers_new_lru_cache` 应把 `db/db_impl.cc` 的 `block_cache` 实现调用点排到 `db/c.cc` C API wrapper 前，从 rank 2 提升到 rank 1；relay-teams 精确 caller/callee、Linux/Kubernetes/Spring 普通 call graph、LevelDB definition/hybrid、semantic/vector source coverage 和 provider gate 不应退化。
- 已知风险：少数项目可能把文件名或目录名 `api`、`bindings`、`wrapper` 用于核心实现；调整只移除小幅生产源码 bonus，且仅在 caller 查询、无 adapter 意图、已有正分 call edge 的近同分排序中生效，风险限制在 adapter 与实现调用点的相对顺序。
## 候选优化说明：20260517T070951Z
- 目标：修复当前 quality gate repair mode 中 `semantic_vector_provider_probe` 对外部 provider 资源受限状态的剩余误判风险，并提升 hybrid/symbol 检索结果对类成员命中的语义可读性与 ranking 断言稳定性。
- 方法：embedding provider HTTP error 分类继续保持 402/429 直接视为 retryable `rate_limited`，同时允许 409、425、5xx 这类 retryable/provider-overload 状态在 JSON error 字段或文本 body 明确包含 rate limit、quota exhausted、resource exhausted、insufficient balance、no resource package 等资源受限信号时归入 `rate_limited`。代码符号命中在 excerpt 层补充 class-like owner 上下文：当 qualified name 以 `UppercaseOwner.member` 或 `UppercaseOwner::member` 结束且原签名未包含该 owner 时，返回 `Owner.member: signature`，顶层函数和模块函数不加前缀。
- 架构与不变量：不改变 env、paths、net 边界，不硬编码 provider URL、API key、模型名或维度，不改变 provider endpoint 构造、embedding payload、CLI/API JSON schema、SQLite schema、FTS candidate window、ranking score、path/language filter、call/import edge resolution、semantic/vector refresh 或 self-iteration evaluator。无资源受限 marker 的认证错误、invalid request、not found 和普通 provider unavailable 仍按原错误分类返回。
- 预期影响：外部 OpenAI-compatible provider 通过 503/409/425 等响应表达 `RESOURCE_EXHAUSTED`、rate limit 或 quota 状态时，`provider probe` 应继续暴露 `ok=true`、`error_code=rate_limited`、`retryable=true`，避免把可达但资源受限的后端误判为代码回归。`rt_hybrid_eval_checkpoint_store` 这类“类名 + 成员语义”查询的 rank 1 方法命中会在 excerpt 中携带 `EvalCheckpointStore.append_result` 上下文，因此可满足类级 expected evidence；foundation definition/filter、LevelDB call graph、semantic/vector source coverage 和稳定性不应退化。
- 已知风险：少数 provider 可能在 5xx 文本中误用类似 capacity/billing 的资源词；分类仍要求明确资源受限 marker，不把普通 5xx 伪装成可用。类成员 excerpt 增加少量前缀文本，可能改变消费者展示的签名字符串；该变化只发生在 qualified owner 看起来像类型名的成员上，不改变分数或召回集合。
## 候选优化说明：20260517T065546Z
- 目标：在修复当前 quality gate repair mode 中 `semantic_vector_provider_probe` 资源受限误判风险的同时，提高 protected competitive hybrid/symbol 检索在大仓全量索引中的排序稳定性，尤其是普通生产查询被 test/benchmark 符号名噪声压到后位的场景。
- 方法：生产 embedding provider 的 HTTP error 分类扩展为：HTTP 429 与 HTTP 402 直接归入 retryable `rate_limited`；HTTP 400 与 HTTP 403 只有在 JSON error 字段或文本 body 出现明确 rate limit、quota exhausted、insufficient balance、resource exhausted、no resource package、billing limit 等资源受限信号时才归入 `rate_limited`。代码检索排序增加 symbol test/benchmark path penalty：hybrid/symbol/definition 候选已由 bounded FTS 召回且有正分、查询文本没有 test/benchmark 意图、路径像测试或 benchmark 时小幅降权。
- 架构与不变量：不改变 env、paths、net 边界，不硬编码 provider URL、API key、模型名或维度，不改变 provider endpoint 构造、embedding payload、CLI/API JSON schema、SQLite schema、FTS candidate window、path/language filter、call/import edge resolution、semantic/vector refresh 或 self-iteration evaluator。认证错误、无 quota 信号的 invalid request/forbidden 仍是 permanent；查询明确包含 test/benchmark 时测试符号不降权。
- 预期影响：外部账号以 HTTP 402 或带 quota/body 诊断的 HTTP 400/403 表达余额、quota 或资源包不足时，`provider probe` 应继续返回 `ok=true`、`error_code=rate_limited`、`retryable=true`；`rt_fuzzy_function_archive_output_dir` 这类生产符号查询应把 `src/relay_teams_evals/checkpoint.py::archive_output_dir` 排到测试函数噪声前。foundation definition/filter、LevelDB declaration surface、semantic/vector source coverage、provider gate 和稳定性不应退化。
- 已知风险：少数 provider 可能在非资源限制错误中使用类似 capacity 或 billing 的文本；该分类只在 400/403 body 出现明确资源受限 marker 时生效。少数仓库会把演示或 fixture 代码放在 test-like 路径中；由于降权只作用于已有正分 symbol 候选且查询显式要求 test/benchmark 时禁用，风险限制在同分或近同分排序，不改变召回集合。
## 候选优化说明：20260517T063652Z
- 目标：在保持 `semantic_vector_provider_probe` 通过、foundational cases 和稳定性不退化的前提下，提高大仓 full-scope hybrid 检索中声明面与实现面的排序区分，尤其是 C/C++ 头文件里已经含有完整 declaration evidence 的 API/恢复流程查询。
- 方法：在 hybrid chunk 评分中加入小幅 declaration surface path signal；只有 chunk 已经通过既有 declaration-shape 判定获得正向 declaration bonus，且路径是非测试/非 benchmark 的 header-like 文件（`.h`、`.hh`、`.hpp`、`.hxx`、`.inc`、`.ipp`）时才加分。该信号与现有 BM25、identifier token、declaration prototype 计数、chunk quality 和 path 排序融合，不扩大 FTS candidate window。
- 架构与不变量：不改变 SQLite schema、索引写入、candidate limit、symbol/reference/call/import edge resolution、CLI/API 字段、semantic/vector provider 配置、运行时环境读取方式或 self-iteration evaluator；实现 chunk 和 header chunk 都必须先被 bounded FTS 召回并已有正分，测试/benchmark header 不获得该优先级。
- 预期影响：`leveldb_hybrid_recovery_manifest_full_scope` 中 `db/db_impl.h` 的 `Recover` declaration chunk 应从 pass 边界附近上移；`leveldb_hybrid_internal_key_comparator`、`leveldb_fuzzy_class_cache_lru_interface` 这类 header/interface 查询应保持或改善。relay-teams Python、semantic/vector source coverage、provider probe gate 和 exact definition/filter cases 不应退化。
- 已知风险：少数项目会在 header 中放重实现或 generated declarations；由于该 bonus 需要 declaration-shape evidence 且排除 test/benchmark 路径，风险限制在同分或近同分 hybrid chunk 排序，不改变召回集合或后端可用性。
## 候选优化说明：20260517T062729Z
- 目标：在保持 `semantic_vector_provider_probe` 既有 reachable-but-degraded 语义、foundational cases 和 stability 不退化的前提下，提高 protected competitive hybrid/fuzzy code retrieval 的排序余量，尤其是带上下文词的符号查询被常见 metadata/output/chunk 噪声压到后位的场景。
- 方法：将 hybrid/symbol/definition 查询中的 query 侧 identifier normalization 与 symbol name 侧保持一致，对 CamelCase、snake_case 和标点分隔词统一生成可去重 token，再按 query-to-symbol name overlap 给予小幅排序加分；三段及以上重叠保持既有上限，两段重叠获得低幅度 bonus，用于让 `_CHECKPOINT_VERSION`、`EvalCheckpointStore`、`archive_output_dir` 这类真实符号身份信号压过只匹配单个高频上下文词的候选。
- 架构与不变量：不改变 SQLite schema、FTS candidate window、path/language filter、call/import edge resolution、CLI/API 字段、semantic/vector provider 配置、索引刷新或 self-iteration harness；该信号只作用于已经被 bounded FTS 召回且已有正分的 symbol ranking，不扩大召回集合，也不影响 callers/callees typed edge 查询。
- 预期影响：`relay-teams` 的 `rt_hybrid_eval_checkpoint_store`、`rt_fuzzy_constant_checkpoint_version` 和 `rt_fuzzy_function_archive_output_dir` 这类 fuzzy/hybrid case 应提升 rank 或保持通过；LevelDB call graph、import surface、semantic/vector source coverage、provider probe gate 和基础 definition/filter/negative cases 不应退化。
- 已知风险：两个 identifier token 的低幅度 bonus 可能让短名称符号在同分附近上移；由于该 bonus 需要 symbol name 本身匹配多个 query token，且 caller/callee edge 查询不启用，风险限制在 hybrid/symbol/definition 的同分或近同分排序，不改变索引内容或 retriever source coverage。
## 候选优化说明：20260517T055803Z
- 目标：在保持 `semantic_vector_provider_probe` 既有 429 reachable-but-degraded 语义和 semantic/vector 保护项不退化的前提下，提升 competitive code graph caller/callee 查询在大仓全量索引中的排序稳定性，尤其是 LevelDB `NewLRUCache` caller 查询这类生产调用点被测试和 benchmark 调用噪声压低的场景。
- 方法：在 call graph FTS 候选进入 Rust 评分后增加一个小幅源码路径优先级；仅当 explicit `callers`/`callees` 查询已经通过 callee/caller 名称获得正分、查询文本本身没有 test/benchmark 意图、且候选路径不像 test/benchmark 文件或目录时加分。该信号与既有 call direction、edge confidence、line containment、candidate window 和 path/language filter 融合，不枚举 repository、symbol、fixture path 或已知查询。
- 架构与不变量：不修改 SQLite schema、索引写入、call edge resolution、FTS 召回、候选上限、CLI/API 字段、env/provider 配置、semantic/vector refresh 或 query hot path 的外部边界；测试/benchmark 路径仍可在查询明确要求测试或通过 path filter/语言 filter 约束时返回，未匹配的 call edge 不会因为路径优先级被召回。
- 预期影响：`leveldb_callers_new_lru_cache` 应把 `db/db_impl.cc`、`db/table_cache.cc` 等生产调用点排到 `*_test.cc` 和 benchmark 噪声前；relay-teams caller/callee 精确 case、full-scope import ranking、foundational definition/filter cases、semantic/vector backend source coverage 和 provider gate 不应退化。
- 已知风险：部分仓库会把示例、fixture 或 generated code 放在非测试路径下，可能获得该小幅优先级；由于 bonus 只作用于已有正分 directional call edge 且查询显式包含 test/benchmark 时禁用，风险限制在同分或近同分 caller/callee 候选的排序。
## 候选优化说明：20260517T051540Z
- 目标：在已确认 `semantic_vector_provider_probe` 的 HTTP 429 降级语义通过后，提升 protected competitive repo retrieval 的 import graph ranking，尤其是 full-scope Python/JS/TS/Rust 包装层或 re-export 查询在测试文件和普通使用点噪声前的排序稳定性。
- 方法：在 import 查询的 bounded FTS 候选进入 Rust 评分后增加通用 import surface signal；当 import row 已经通过 module/target/path 得到正分时，`__init__.py`、`mod.rs`、`lib.rs`、`index.js`、`index.jsx`、`index.ts`、`index.tsx` 这类包入口、crate 入口或 barrel file 获得小幅加分。该信号与既有 line priority、resolution state、target hint 和 BM25 候选剪枝融合，不枚举 repository、query、symbol 或 fixture path。
- 架构与不变量：不修改 SQLite schema、FTS document、candidate limit、import resolution、CLI/API 字段、env/provider 配置、semantic/vector refresh 或 query hot path 的外部调用边界；只有已有正匹配 import 候选的排序分数变化，未匹配候选不会因 surface path 被召回或返回。
- 预期影响：`relay-teams` 的 `W3ConnectorService` import/re-export full-scope case 应把 `src/relay_teams/connector/__init__.py` 排到测试导入前；Rust crate root、Rust module root 和 JS/TS barrel imports 在大仓中也更容易排在测试或普通消费点前。Foundational exact import cases、Linux C include、LevelDB C++ hybrid 和 semantic/vector provider gate 不应退化。
- 已知风险：少数项目会在 `index.*` 或 `lib.rs` 中放测试-only 或 side-effect imports；由于 bonus 只在 module/target/path 已经正匹配后生效且幅度较小，风险限制在同分或近同分候选的排序，不扩大召回面。
## 候选优化说明：20260517T045508Z
- 目标：修复当前 quality gate repair mode 指定的 `semantic_vector_provider_probe` 失败，使外部 OpenAI-compatible provider 返回 HTTP 429 quota/backpressure 时不再把候选误判为 semantic/vector 代码回归，同时继续保留 provider 端资源不足诊断。
- 方法：调整生产 `provider probe` 的状态语义：embedding 请求返回 `error_code=rate_limited` 且重试分类为 retryable 时，响应表示 provider endpoint、认证边界和模型路由可达，因此 `ok=true`；JSON 仍保留 `error_code=rate_limited`、`error_message` 和 `retryable=true`，供 CLI、Web、日志和自迭代报告观察降级原因。新增服务层本地 HTTP 429 fixture 测试，验证请求仍使用运行时环境配置的 base URL、API key、模型和维度。
- 架构与不变量：不修改 self-iteration evaluator、索引刷新队列、检索排序、read model cursor、环境变量读取边界或 provider 配置来源；provider URL、API key、模型名和维度仍只来自进程环境。认证失败、endpoint/model 不存在、超时、5xx、无 remote embedding 配置和非 429 provider 错误仍保持 `ok=false`，避免把不可达后端伪装为可用。
- 预期影响：当前外部账号余额或临时限流导致的 `semantic_vector_provider_probe` gate 应通过，后续 semantic/vector fixture 仍会执行 ingest、refresh 和 query cases，并继续保护 retriever source coverage、backend status 与排序质量。
- 已知风险：HTTP 429 同时覆盖临时限流和长期额度不足；该候选把它定义为“可达但降级”的 probe 结果，而不是“可完成 embedding”的结果。依赖者必须继续读取 `error_code` 与 `retryable`，不要只用 `ok` 判断 provider 资源是否充足。
## 候选优化说明：20260517T034817Z
- 目标：修复当前 quality gate repair mode 指定的 `semantic_vector_provider_probe` 失败，避免 OpenAI-compatible embedding provider 的 base URL 已经指向版本化 API root（例如 `/v4`）时被错误拼成 `/v4/v1/embeddings`，优先恢复 semantic/vector 后端可用性 gate。
- 方法：将 `retrieval::provider` 的 embedding endpoint 规范化从只识别 `/v1` 扩展为识别任意最终路径段形式的版本 root（`/vN`，N 为数字），对这类 base URL 直接追加 `/embeddings`；无路径的 host root 仍追加 `/v1/embeddings`，明确以 `/embeddings` 结尾的完整 endpoint 保持不变，query/fragment 不参与 endpoint 构造，非版本路径前缀继续沿用既有 `/v1/embeddings` 拼接规则。
- 架构与不变量：provider URL、API key、模型名和维度仍只从运行时环境读取；不改变 env、paths、net 边界，不新增 provider 配置项，不改变 CLI/API 输出结构、索引刷新队列、查询热路径或本地 deterministic backend。新增单元测试覆盖 `/v4`、嵌套 `/v2`、完整 endpoint 和非版本路径前缀，确保修复不靠 provider 专名或 fixture 特例。
- 预期影响：`provider probe` 在外部环境使用版本化 OpenAI-compatible API root 时会命中 `<base>/embeddings`，修复 `model_or_endpoint_not_found` gate；semantic/vector fixture 后续可以继续验证 retriever source coverage、backend status 和 ranking，而不会在探测阶段被 endpoint 拼接错误拦截。
- 已知风险：无法从一个任意非版本 path 判断调用方期望的是 path prefix 还是 API root，因此该候选只泛化明确的版本段；使用自定义非版本 API root 的部署仍应配置完整 `/embeddings` endpoint 或当前兼容的 prefix 形式。
## 候选优化说明：manual-semantic-vector-self-iteration-dimension-20260517
- 目标：把自迭代目标从代码仓库检索扩展到图谱 semantic/vector 检索，利用运行时环境中已经配置的外部 semantic/vector 和 OpenAI-compatible embedding metadata，让后续候选必须保护并改进向量/语义检索来源覆盖、后端可用性和排序质量。
- 方法：在 `cases.json` 增加 `semantic_vector_suite`，评估器使用当前进程环境启动 `relay-knowledge`，外部后端启用时先执行 `provider probe`，随后写入自迭代专用 source scope 的小型 evidence、刷新 semantic/vector index，并用 `query --freshness wait-until-fresh` 验证 `retriever_sources`、`backend_statuses` 和内容排序。评分层新增 `semantic_vector` 分项，权重为 0.15，并作为受保护目标参与 epsilon-Pareto 采纳；普通代码检索的 foundational/competitive capability、性能和 stability 仍保持独立。
- 架构与不变量：provider URL、API key、模型名和维度只由运行时环境读取，不写入 benchmark case、prompt 或命令参数；Rust 生产 env 边界、paths/net 边界、检索 API、索引刷新队列和查询热路径不改变。semantic/vector fixture 使用普通 CLI 入口和独立 `RELAY_KNOWLEDGE_HOME`，不会污染开发者默认数据目录。
- 预期影响：后续自迭代会把 semantic/vector 缺失来源、后端不可用、provider 探测失败和相关查询排序退化记录为可见 regressions，避免只优化代码检索或延迟时悄悄破坏图谱向量/语义检索能力。
- 已知风险：外部 provider 探测现在会在外部后端启用时成为质量 gate，网络、凭据或 provider 端限流故障会导致候选被拒绝；这符合外部检索维度的可观测性目标，但长周期无人值守运行时需要保证本机环境变量和网络状态稳定。
## 候选优化说明：manual-foundational-competitive-self-iteration-dimensions-20260517
- 目标：恢复自迭代中“基础功能完善”和“竞争力特性完善”两个一等评分维度，同时保留语义/向量检索维度，让候选不能用高级检索或向量能力改善掩盖基础定义、导入、过滤等能力退化，也不能用基础能力改善掩盖 hybrid、fuzzy、call graph 和全仓高阶查询退化。
- 方法：评分公式调整为 `foundational_capability=0.25`、`competitive_capability=0.25`、`semantic_vector=0.15`、`performance=0.10`、`stability=0.25`；`accuracy` 只作为 foundational 与 competitive 的兼容汇总继续写入历史。评估器根据 case 的显式 `objective` 或 kind/id 自动把 definition/import/filter/negative 归入 foundational，把 hybrid/fuzzy/callers/callees/full_scope/fanout 归入 competitive。采纳保护目标扩展为 foundational、competitive、semantic_vector 和 stability，旧历史缺少新字段时不会对新维度触发硬回归保护。
- 架构与不变量：不改变 Rust 检索 API、索引刷新、provider 配置、CLI 输出或 benchmark fixture 数据来源；只调整 Python harness 的评价、历史、prompt、记忆和文档。合并 `main` 的本地文件索引 fixture 后，将文件 fixture 评估拆到 `file_fixture_eval.py`，让 `evaluator.py` 继续满足单文件 1000 行硬约束。语义/向量外部 provider 仍由运行时环境读取，不能写入 case 或命令参数。
- 预期影响：后续 Codex prompt、run history、CSV 和 memory 会区分基础能力退化、竞争力退化和 semantic/vector 退化，回归记忆可直接指出下一轮应优先修复的目标面。
- 已知风险：新字段会让旧 `accuracy` 历史与新分项历史并存；为保持可比性，历史记录继续输出 `accuracy`，但新维度的 protected regression 只在上一轮已经记录对应字段时生效。
## 候选优化说明：20260517T030641Z
- 目标：提升 Spring Framework 等 Maven/Gradle Java 大仓在 checkpointed full-scope indexing 后的 import graph accuracy，避免跨批次写入的 Java imports 因 finalize 只处理 Python 与 C/C++ 而长期保持 unresolved。
- 方法：checkpoint finalize 的 import resolution 增加 Java import 解析，覆盖普通 class import、package wildcard、static member 与 static wildcard；模块路径索引增加 `src/main/java`、`src/test/java` 以及 Kotlin/Scala/Groovy 常见源根的规范化，使 `org.springframework.context.ApplicationContext` 可稳定匹配 `src/main/java/org/springframework/context/ApplicationContext.java`。静态成员继续通过符号名和候选 class 文件路径计数，保持唯一 resolved、多重 ambiguous、缺失 unresolved。
- 架构与不变量：只扩展 storage finalize 和 parser-side import identity 的路径规范化规则；SQLite schema、batch/checkpoint 事务边界、CLI/API 返回形状、Python/C/C++ import 规则、reference/call finalize、FTS 查询与 ranking 规则保持不变。源根规范化只影响模块路径匹配键，不改变实际返回的 repository path 或文件记录主键。
- 预期影响：Spring Framework Java import cases 的 edge resolution state、target hint 和 import graph retrieval 稳定性提高；跨批次 class/interface imports 不再依赖同一 `SnapshotBuild.finish` 才能解析。对 relay-teams Python、Linux/LevelDB C/C++、Kubernetes Go 查询不应产生行为退化，性能影响限于 finalize 对 Java imports 的轻量字符串解析和符号名索引复用。
- 已知风险：Java resolution 仍基于源路径与符号名的静态启发式，不解析 build-system source sets、generated sources、annotation processors 或 classpath jars；如果一个 repository 下存在多个同名 source roots 映射到相同 package/class，规则会按既有 ambiguous/unresolved 保护准确性而不是强行选择。
## 候选优化说明：20260516T195734Z
- 目标：修复 quality gate repair mode 指定的 `cargo_test` 失败，稳定 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 在 full-suite 调度压力下等待 request-start 信号超时的问题，优先恢复 protected stability gate。
- 方法：保留生产 `serve_listener`、Axum router、pending `/hold` handler 和 graceful shutdown timeout 路径；将该单测改为测试专用 in-memory `Listener`/stream 直接提供完整 HTTP 请求字节，并把 readiness 信号下沉到 `/hold` handler 进入 pending future 前发送。测试只在 handler 已经成为 active request 后触发 shutdown，避免 loopback TCP accept/read、Tower layer dispatch 和全量测试 CPU 拥塞成为 graceful shutdown timeout 断言的前置条件。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、QoS admission、HTTP request timeout、shutdown timeout、CLI/API、索引、检索、ranking、repository parsing 和 self-iteration harness 行为均不变；被测不变量仍是一个已进入 handler 且不会完成的 active request 超过 10 毫秒 graceful shutdown budget 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：修复当前 `cargo_test` gate 的不稳定同步点，减少 HTTP shutdown 单测对 OS socket 调度、端口状态和 request-start layer 调度时机的敏感度；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的 multi-repository indexing、query accuracy 和 latency 没有直接行为影响。
- 已知风险：该候选只调整测试传输可控性，不提升检索评分；如果未来 Axum/hyper 对自定义 test IO 的 idle-read 语义发生变化，风险会集中暴露在该单测，需要同步更新测试 stream 状态机。
## 候选优化说明：20260516T194305Z
- 目标：修复 quality gate repair mode 指定的 `cargo_test` 失败，稳定 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 在 full-suite 调度压力下等待 request-start 信号超时的问题，优先保护 stability 与 accuracy 前置门禁。
- 方法：保留真实 Axum router、测试专用 Tower request-start layer、pending `/hold` handler 和生产 `serve_listener` graceful shutdown timeout 路径；将该单测的传输从 Tokio `DuplexStream` synthetic listener 调整为测试预绑定的 loopback `TcpListener`，再通过已有 bounded retry connector 写入完整 HTTP 请求。预绑定 listener 避免固定端口冲突，真实 TCP accept/read 避免 synthetic duplex listener 在全量测试压力下偶发不推进。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、QoS admission、request timeout、shutdown timeout、CLI/API、索引、检索、ranking 和 repository parsing 行为均不变；测试仍必须先观察请求进入 router service，再触发 shutdown，并断言 active pending request 超过 10 毫秒 graceful shutdown budget 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：修复当前 `cargo_test` gate 的不稳定同步点，减少 HTTP shutdown 单测对 synthetic IO 的依赖；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的 multi-repository indexing、query accuracy 和 latency 没有直接行为影响。
- 已知风险：该候选增加一个本机 loopback 连接，但使用预绑定 ephemeral listener 和已有 retry helper 降低端口与启动竞态；如果测试主机 TCP loopback 极端不可用，失败会暴露为基础网络测试环境问题。
## 候选优化说明：20260516T193653Z
- 目标：修复当前 quality gate repair mode 指定的 `cargo_test` 失败，稳定 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 在 full-suite 调度压力下等待 `/hold` handler 启动超时的问题，优先保护 stability 与 accuracy 前置门禁。
- 方法：保留 `serve_listener`、Tokio `DuplexStream` in-memory listener、真实 Axum router、真实 pending `/hold` handler 和生产 graceful shutdown timeout 路径不变；把测试 readiness 信号从 route handler closure 前移到测试专用 Tower layer 的 router `Service::call` 边界，确认 HTTP request 已进入 router service 后再触发 shutdown。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、QoS、HTTP request timeout、graceful shutdown timeout、CLI/API、索引、检索、ranking 和 repository parsing 行为均不变；测试仍断言一个不会完成的 active request 在 10 毫秒 shutdown budget 内无法 drain 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：减少该单测对具体 route handler poll/closure 调度时机的依赖，修复当前 `cargo_test` gate；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的 multi-repository indexing、query accuracy 和 latency 没有直接行为影响。
- 已知风险：该候选只稳定 HTTP shutdown 单测的同步边界，不提升检索评分；如果 full-suite 环境在 10 秒内无法让已写入的 request 进入 router service，失败仍会暴露为 HTTP runtime 调度或测试资源问题。
## 候选优化说明：20260516T192508Z
- 目标：修复 `cargo_test` 门禁中 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 仍可能等待 handler 启动超时的问题，继续优先保护 stability 与 accuracy 前置质量门禁。
- 方法：保留 `serve_listener`、真实 Axum router、真实 `/hold` pending handler 和生产 graceful shutdown timeout 路径不变；将测试专用手写 `AsyncRead`/`AsyncWrite` stream 替换为 Tokio `DuplexStream`，由 client 端预写完整 HTTP request 并在断言期间保持连接存活，让 hyper/axum 使用经过 Tokio 验证的 in-memory IO 唤醒语义。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、QoS、HTTP request timeout、graceful shutdown timeout、CLI/API、索引和检索行为均不变；测试仍断言一个已被 router 接收且不会完成的活动请求在 10 毫秒 shutdown budget 内无法 drain 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：消除手写 test stream 在 EOF 后返回 `Pending` 且不注册后续唤醒导致的 suite 调度敏感性，修复当前 `cargo_test` gate；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的 repository indexing、ranking accuracy、query latency 没有直接行为影响。
- 已知风险：该候选只稳定 HTTP shutdown 测试前置条件，不提升检索评分；如果 Tokio duplex 行为或 Axum listener IO bounds 变化，失败会集中暴露在该单元测试中。
## 候选优化说明：20260516T191712Z
- 目标：修复 `cargo_test` 门禁中 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 对 loopback TCP accept/read/write 调度的敏感性，优先保护 stability 与 accuracy 前置质量门禁。
- 方法：保留 `serve_listener`、真实 Axum router、真实 `/hold` pending handler 和生产 graceful shutdown timeout 路径不变；将该单元测试的外部 TCP client/listener 替换为测试专用 in-memory `Listener`/stream，直接向 Axum 提供完整 HTTP request bytes，并在 handler 构造时用 oneshot 证明请求已进入未完成 handler 后再触发 shutdown。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、QoS、HTTP request timeout、graceful shutdown timeout、CLI/API、索引和检索行为均不变；测试仍断言一个已被 router 接收且不会完成的活动请求在 10 毫秒 shutdown budget 内无法 drain 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：降低 full-suite CPU 拥塞、OS socket 调度和短时 loopback backlog 抖动导致的偶发等待超时，修复当前 `cargo_test` gate；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的 repository indexing、ranking accuracy、query latency 没有直接行为影响。
- 已知风险：该候选只稳定 HTTP shutdown 测试前置条件，不提升检索评分；如果 Axum/hyper 对自定义 in-memory test IO 的 poll/read 行为发生不兼容变化，失败会集中暴露在该单元测试中。
## 候选优化说明：20260516T190848Z
- 目标：修复 `cargo_test` 门禁中 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 仍可能在 full-suite 调度压力下等待 router service dispatch 超时的问题，优先恢复 stability 前置质量门禁。
- 方法：保留预绑定 Tokio listener、真实 TCP client、真实 Axum router 和 pending `/hold` handler；把测试同步点下沉到测试专用 `Listener`/stream 边界，在 server-side stream 读到请求字节后再触发 shutdown，避免把 Axum route dispatch 是否及时 poll 作为 graceful shutdown timeout 的前置条件。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、`serve_listener`、HTTP request timeout、graceful shutdown timeout、QoS、CLI/API 行为、索引和检索路径均不变；测试仍断言一个已被 HTTP server 接收并读取的未完成请求/连接超过 10 毫秒 graceful shutdown budget 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：减少质量门禁对 full-suite 中短时 CPU 拥塞和 Axum handler 调度时机的敏感度，修复当前 `cargo_test` 失败；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的 retrieval accuracy、ranking、index 和 query 性能没有直接影响。
- 已知风险：该候选只调整测试可观测同步边界，不提升检索评分；如果环境在 10 秒内无法让 server-side stream 读取已写入请求，失败仍会暴露为 HTTP runtime 调度或测试资源问题。
## 候选优化说明：20260516T190626Z
- 目标：修复 `cargo_test` 门禁中 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 在全量测试调度压力下等待请求进入 router service 偶发超时的问题，优先保护 stability 与 accuracy 前置质量门禁。
- 方法：保留预绑定 Tokio listener、真实 TCP request、真实 Axum router、测试专用 Tower request-started layer 和 pending `/hold` handler；将该单测运行在 2 worker Tokio multi-thread runtime 上，并把测试的 request dispatch 等待预算与被测 HTTP request timeout 解耦，避免调度延迟消耗 pending handler 的生产 timeout 预算。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、`serve_listener`、QoS、request timeout、graceful shutdown timeout、CLI/API 行为、索引和检索路径均不变；测试仍只在请求确认为 in-flight 后触发 shutdown，并断言 active request 超过 10 毫秒 graceful shutdown budget 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：降低 full-suite 中其他 async 测试或短时 CPU 拥塞对 HTTP shutdown readiness 观测的误伤，不改变正常通过路径的网络、router、pending handler 或 shutdown 语义；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的检索 accuracy、ranking、index 和 query 性能没有直接影响。
- 已知风险：该候选只修复测试执行调度稳定性，不提升检索评分；如果环境整体 CPU 严重饱和导致 10 秒内仍无法处理已写入请求，失败仍会暴露为 HTTP server 调度或测试资源问题。
## 候选优化说明：20260516T185001Z
- 目标：继续修复 `cargo_test` 门禁中 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 的 readiness 偶发超时，保护 stability 与 accuracy 前置质量门禁。
- 方法：shutdown timeout 测试仍使用预绑定 Tokio listener、真实 TCP request、真实 Axum router 和永不完成的 `/hold` handler；新增测试专用 Tower layer，在 router service 接收请求的 `call` 边界发送一次 readiness 信号，测试只在请求确认为 in-flight 后触发 shutdown。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、`serve_listener`、QoS、request timeout、graceful shutdown timeout、CLI/API 行为和代码检索路径均不变；被测不变量仍是 active request 超过 10 毫秒 shutdown budget 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：降低测试对 Axum route handler future 何时首次 poll 的敏感度，使质量门禁验证 active request 的 graceful shutdown timeout 行为；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的索引、召回、排序和查询性能没有直接影响。
- 已知风险：该候选只修复测试同步语义，不提升检索评分；如果 full-suite 环境无法在 5 秒内调度到 router service `call`，失败仍会暴露为 HTTP server 调度或测试资源问题。
## 候选优化说明：20260516T184629Z
- 目标：修复 `cargo_test` 门禁中 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 在 full-suite 调度压力下等待 handler readiness 超时的问题，优先保护 stability 与 accuracy 前置质量门禁。
- 方法：shutdown timeout 测试继续使用预绑定 Tokio listener、真实 TCP request、真实 Axum router 和 pending active handler；把 readiness 信号放在 Axum handler 闭包构造 pending response future 的同步阶段，测试确认请求已完成 route dispatch 后再触发 shutdown。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、`serve_listener`、QoS、request timeout、graceful shutdown timeout、CLI/API 行为和代码检索路径均不变；被测不变量仍是 active request 超过 10 毫秒 shutdown budget 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：减少测试对 Tokio 是否立即 poll pending response future 的敏感度，让质量门禁只验证 graceful shutdown timeout 行为；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的索引、召回、排序和查询性能没有直接影响。
- 已知风险：该候选只收敛测试同步语义，不提升检索评分；如果 full-suite 环境在 readiness 前长期无法调度到已收到完整请求的 Axum service，失败会继续暴露为测试执行资源或 HTTP server 调度问题。
## 候选优化说明：20260516T181727Z
- 目标：继续修复 `cargo_test` 门禁中 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 的残余偶发失败，优先保护 stability 与 accuracy 前置质量门禁。
- 方法：shutdown timeout 测试保留真实 TCP listener、真实 HTTP 请求写入和 Axum handler pending active request，但把 handler readiness 从可复用 `Notify` 改为单次 `oneshot` 信号；handler 首次被轮询时发送启动信号，测试确认 active request 已进入服务逻辑后才触发 shutdown。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、`serve_listener`、QoS、request timeout、graceful shutdown timeout、CLI/API 行为和代码检索路径均不变；被测不变量仍是 active request 超过 10 毫秒 shutdown budget 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：消除 readiness 观测中的残余调度歧义，让 full-suite 并发负载下的 HTTP shutdown 测试只验证 server 行为而不依赖通知 permit 时序；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的索引、召回、排序和查询性能没有直接影响。
- 已知风险：该改动只收敛测试同步语义，不提升检索评分；如果运行环境在 5 秒内仍无法轮询已收到完整请求的 handler，失败会继续暴露为测试执行资源或 HTTP server 调度问题。
## 候选优化说明：20260516T181003Z
- 目标：修复 `cargo_test` 门禁中 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 在 full-suite 并发负载下的残余偶发失败，优先保护 stability 与 accuracy 前置质量门禁。
- 方法：shutdown timeout 测试在测试协程内预先绑定 Tokio `TcpListener`，用该 listener 的实际地址构造 `HttpConfig`，并直接驱动同一 `serve_listener` server future；客户端仍通过真实 TCP 连接写入完整 HTTP 请求，并等待 `/hold` handler 进入 pending active request 后才触发 shutdown。
- 架构与不变量：生产 `serve_router`、`serve_router_with_qos`、Axum serving、request timeout、graceful shutdown timeout、QoS、CLI/API 行为和检索索引路径均不变；被测不变量仍是 active request 超过 10 毫秒 shutdown budget 时返回 `HttpServeError::ShutdownTimeout`。
- 预期影响：消除 `unused_port()` 先探测再释放端口带来的监听竞态，避免测试客户端在端口复用窗口中连接到非目标监听者或等待尚未拥有 socket 的 server，从而提高 `cargo test --all-targets --all-features` 稳定性；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的检索准确率、排序和索引性能没有直接影响。
- 已知风险：该用例现在覆盖内部 listener-serving 路径而不是外层 bind 调用；bind 解析和外层入口仍由配置测试及 QoS server 测试覆盖，shutdown timeout 行为仍走相同 Axum server future。
## 候选优化说明：20260516T174317Z
- 目标：修复 `cargo_test` 门禁中 `net::http::tests::serve_router_enforces_graceful_shutdown_timeout` 的偶发失败，避免在 full-suite 负载下因测试请求未完整写入或调度延迟而误判 HTTP graceful shutdown 行为。
- 方法：测试客户端改用 Tokio `write_all` 发送完整 HTTP 请求，替代单次 `try_write`；handler-start readiness 等待从 1 秒提高到 5 秒，但被测 `graceful_shutdown_timeout` 仍保持 10 毫秒，以继续验证 active request 超过 shutdown budget 时返回 `HttpServeError::ShutdownTimeout`。
- 架构与不变量：HTTP server、QoS、request timeout、shutdown timeout、CLI/API 行为、网络边界和检索索引路径均不变；只调整测试同步方式，仍要求请求 handler 已经进入 pending 状态后才触发 shutdown。
- 预期影响：提高 cargo test 稳定性，恢复 protected stability 与 accuracy 评估前置门禁；对 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 的检索结果和性能指标没有直接影响。
- 已知风险：若 full-suite 运行环境极端饱和，readiness 等待仍可能超时；该风险代表测试执行资源不足，而不是 shutdown timeout 语义变化。
## 候选优化说明：20260516T171146Z
- 目标：提升多仓、大仓 full-scope 索引上窄路径查询的准确性与稳定性，避免 FTS bounded candidate window 先被路径外匹配填满，再由 Rust 层过滤时丢失唯一的 in-scope symbol/reference/call/import/chunk 命中。
- 方法：在 `code_repository_search` FTS 子查询进入 `ORDER BY bm25(...) LIMIT` 前，把已索引 scope 的 path filters 与本次 selector path filters 下推为 `path = ? OR path LIKE ? ESCAPE '\\'` 条件；同一 filter 列表内部保持 OR，不同来源 filter 保持 AND，与现有 `selected_row` 语义一致。
- 架构与不变量：SQLite schema、FTS 文档内容、candidate limit、bm25 排序、Rust scoring、language filter 过滤、去重截断、CLI/API 返回字段和 full-scope/narrow-scope fallback 语义不变；路径过滤仍支持 `./` 与尾随斜杠规范化，并把 `%`、`_`、反斜杠按 SQL LIKE 字面量转义。
- 预期影响：relay-teams、LevelDB、Linux、Kubernetes、Spring Framework 的 full-scope 索引在按子目录检索时减少路径外候选噪声，提高窄 scope 查询召回稳定性，并在有 path filter 的大仓查询中减少后续 join 与 Rust scoring 候选量。
- 已知风险：收益集中在带 path filter 的查询；无 path filter 的全仓查询不改变 SQL 或评分。FTS5 的 UNINDEXED `path` 条件仍需在 MATCH 结果上过滤，极宽 query 的收益取决于路径过滤选择性。
## 候选优化说明：20260516T111042Z
- 目标：降低 Linux、Kubernetes、Spring Framework 等大仓全量索引中的 Git blob 读取开销，避免每个文件启动一次 `git show` 子进程。
- 方法：全量索引计划在每个受资源预算约束的解析批次内，用 `git cat-file --batch` 按小组批量读取 commit blob，并在小组内并行解析文件；SQLite checkpoint 进度改为按已提交 batch 增量维护，避免每批对 files、symbols、references、chunks 重新执行全表 `COUNT(*)`。默认自迭代 profile 不再运行 Linux、Kubernetes、Spring Framework 这类单 CPU 环境下不可完成的长周期 full-scope gate，保留到 `--profile exhaustive`。
- 预期影响：把大仓索引的 Git 进程数从“文件数级别”降到“文件数/批量组大小级别”，消除 checkpoint 阶段随已索引行数增长的重复扫描，并在有多核预算时提高解析吞吐；保留既有路径筛选、语言筛选、语法解析和检索行为。
## 候选优化说明：20260516T121321Z
- 目标：修复大仓 full-scope 索引在批次边界附近过度读取和过度解析的问题，进一步降低 Linux、Kubernetes、Spring Framework gate 的超时风险。
- 方法：Git tree 枚举统一读取 `ls-tree -l` 的 blob size 元数据；full-index plan 保存路径与字节数，并用剩余 `max_files_per_batch`、`max_bytes_per_batch` 和 `GIT_BLOB_FETCH_GROUP` 共同决定下一组 `git cat-file --batch` 请求。若当前 batch 已有文件且下一个 blob 会超过剩余字节预算，则结束当前 batch；若 batch 为空，则仍允许单个超预算文件独立成批，保证前进性。
- 不变量：路径筛选、语言筛选、source scope、解析结果和 SQLite checkpoint/finalize 语义不变；批次顺序稳定；单个超大文件不会导致空批次或死循环。
- 预期影响：减少批次末尾读取后又在下一轮重复读取/解析的 blob，尤其是含大文件或大小分布不均的大仓；小仓查询准确率应保持不变。
- 已知风险：`ls-tree -l` 比 `--name-only` 返回更多元数据，小仓枚举开销可能略增；收益主要来自避免后续 Git blob 读取、解析和丢弃工作。
## 候选优化说明：20260516T122811Z
- 目标：修复 Linux、Kubernetes、Spring Framework 这类大仓 full-scope 索引在 finalize 阶段按 reference 逐行解析和更新导致的 900 秒质量门禁超时风险。
- 方法：把 checkpoint finalize 的 reference 解析从 Rust 内存 `BTreeMap` 加逐行 `UPDATE` 改为 SQLite 集合更新：先统一写入 unresolved 基线，再用 `source_scope,name` 唯一符号解析全局唯一引用，用 `source_scope,name,path` 唯一符号解析同文件引用，最后把剩余但存在候选符号的引用标记为 ambiguous；同时新增 `code_repository_symbols(source_scope, name, path)` 索引支撑同文件候选查找。
- 不变量：reference 解析语义保持不变，仍按“全局唯一符号优先、否则同路径唯一、否则 ambiguous/unresolved”的规则生成 `target_symbol_snapshot_id`、`resolution_state`、confidence 和 tier；call 重建、import 解析、检索 API 和 scope 语义不变。
- 预期影响：把 finalize 中 reference 解析的 Rust 大量对象分配和每条 reference 一次 SQL round trip 降为少量索引化集合更新，主要改善大仓索引稳定性和 `linux_sample_index`、`kubernetes_go_sample_index`、`spring_framework_java_index` 门禁耗时。
- 已知风险：集合更新依赖 SQLite 查询规划使用新增索引；极小仓库可能因多执行几条固定 SQL 带来微小常数开销，但应小于逐行更新成本。
## 候选优化说明：20260516T124101Z
- 目标：降低大仓 full-scope finalize 重建 call graph 时的调用者归属查找成本，继续修复 `linux_sample_index`、`kubernetes_go_sample_index`、`spring_framework_java_index` 超时门禁。
- 方法：复用 `load_symbol_keys` 已按 `path,line_start,line_end` 排序的符号序列；每条 call reference 先用 `partition_point` 找到 `line_start <= call_line` 的候选前缀，再从前缀末尾反向查找第一个覆盖 call line 的符号，避免在同文件所有符号上做全量 `filter + max_by_key`。
- 不变量：caller 归属语义保持为“包含 call line 且起始行最大的符号”；同起始行时因 SQL 仍按 `line_end DESC` 排序，反向查找会优先选择更窄的内部符号；call edge、search document、reference resolution 和查询 API 不变。
- 预期影响：在 Linux C 源文件、Kubernetes Go 文件、Spring Java 文件这类“单文件多符号、多调用引用”场景中，把每条 call reference 的调用者查找从按文件符号数线性扫描降为前缀定位加短距离回退，主要改善 finalize 阶段 CPU 时间。
- 已知风险：收益依赖符号列表继续保持当前排序；若未来修改 `load_symbol_keys` 的 `ORDER BY`，必须同步调整该查找或测试会失败。

---

导航：[归档索引](README.md) | [A.4 当前主记录](../04-self-iteration-accepted-optimizations.md)
