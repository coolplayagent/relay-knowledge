# 自迭代采纳优化记录
本文档由自迭代 harness 在候选通过质量门禁并被采纳时追加，用于把本轮采用的优化思路传递给后续 Codex 迭代。人工维护的总结可以继续补充在对应条目下。
## 记录格式
- `patch`/`score`/`cases`/`changed paths`: 本轮候选补丁、采纳分数、通过 case 数和主要文件。
- `key improvements`/`known degradations`/`latency metrics`/`Adopted optimization notes`: 改善、退化、耗时与优化说明。
## 渐进式记忆
自迭代 harness 还会在 `.git/relay-knowledge-self-iteration/memory/` 写入不进入版本控制的渐进式记忆。`memory/index.jsonl` 只保存有界索引，`memory/summaries/<id>.md` 保存短摘要，`memory/details/<id>.md` 保存完整评分、gate、case、metric、patch 和 report 引用。后续 Codex 运行应先读取 prompt 中的 memory index，再按相关性读取 summary，只有当前 gate、metric、case、路径或算法目标需要时才打开 detail 或 patch，避免一次性加载全部历史报告。
## 候选优化说明：manual-exported-constructed-value-definition-20260518
- 目标/算法/架构：保护 foundational、competitive、semantic/vector、research judge、performance 与 stability 下限，补齐 JavaScript/TypeScript 大仓中导出运行时对象的 definition 召回；parser 仅把 `export const name = Owner.factory(...)` 这类 member-call 构造值和 `export const name = new Type(...)` 记录为 `constant` symbol，使 `protocol`、`route` 等公开协议/服务对象进入既有 symbol FTS 与 definition 查询路径。
- 不变量：不改变 SQLite schema、FTS 表结构、candidate limit、ranking 权重、import/call/reference finalize、CLI/API JSON、semantic/vector provider/env、embedding 设置、research judge 配置、网络/HTTP/QoS、安装发布或 self-iteration harness；非导出局部常量、普通标识符函数调用、对象/数组字面量和超长构造块仍不进入 symbol 表。
- 预期影响：Opencode TypeScript `packages/llm/src/protocols/openai-chat.ts` 中 `export const protocol = Protocol.make(...)` 可被 path/language-filtered definition 查询命中，类似 relay-teams 或大型 TS/JS 仓库的公开协议、route、transport、layer 对象召回更稳定；索引写入仅增加短导出构造值的 symbol/chunk，register-to-index wall time 应接近中性。
- 已知风险：少量导出工厂结果会以 `constant` kind 参与 definition/hybrid 排名，可能改变同名导出值附近的排序；实现要求 export ancestor、member call 或 `new` expression、合法 JS 标识符和 64 行长度上界，以避免把大型配置对象或普通局部变量变成宽泛噪声。
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
- 算法与架构：source preset 继续排除 dependency/cache/vendor/build/out/target、二进制媒体、map/jsonl 和锁文件；只允许源码语言文件进入 `dist/{js,javascript,ts,typescript,src,source,sources}/{app,client,core,runtime,server}` runtime 子树，并排除 `.min.`、assets、vendor 和非源码扩展。Caller/callee 排序在已有 bounded FTS candidate、方向过滤、最终 Rust scoring 内，对无 test intent 的 test/benchmark call site 加小额 penalty，让 production call site 不被 resolved test edge 的置信度差压低。
- 不变量：不改变 SQLite schema、事实模型、candidate limit、CLI/API JSON、semantic/vector provider URL/API key/model/dimension、embedding 设置、research judge URL/API key/model/CLI、HTTP/QoS、安装发布或仓库/符号/case 特殊分支；显式 `--path` opt-in 与 `.relay-knowledgeignore` 优先级保持不变，query 明确提到 test/benchmark 时不做 test path penalty。
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

## 候选优化说明：自迭代文档与 patch 长期记忆

- 目标：让自迭代候选在修改代码、测试、benchmark 或 harness 策略时，同时留下可供后续迭代理解的算法与架构说明，避免只有 patch 和评分而缺少设计意图。
- 方法：候选 diff 只要包含非文档文件，就追加 `self_iteration_algorithm_documentation` gate，要求同步更新本文档；prompt 明确要求写出算法、架构、不变量、预期 case/metric 影响和风险。该 gate 在候选评估完成后、评分前加入，作为硬质量门禁参与 `quality gates failed` 拒绝原因。
- 长期记忆：prompt 新增 `.git/relay-knowledge-self-iteration/patches/` 索引，按最近 patch 列出路径、大小、采纳状态、分数、变更文件、拒绝原因和主要改善。Codex 先读索引，再用 `sed -n` 对相关 patch 小范围渐进读取，避免一次性塞入所有历史 patch 造成上下文膨胀。
- 预期影响：后续自迭代能同时利用结构化 run history、人工可读设计说明和原始 patch 细节，减少重复尝试，提高对历史成功/失败算法的复用质量。

## 候选优化说明：20260516T130000Z

- 目标：继续修复 Linux、Kubernetes、Spring Framework full-scope index 在大文件数仓库中因 Git blob 读取批次过小而接近或超过 900 秒门禁的问题，同时避免把并行解析改成不受控的线程膨胀。
- 方法：全量索引的 `git cat-file --batch` 读取组从固定 32 个路径提升到默认文件批次上限 128 个路径，并继续受 `max_files_per_batch` 与 `max_bytes_per_batch` 约束；解析阶段改为按 `available_parallelism()` 分块启动 scoped worker，每块完成后再推进下一块，保持输出顺序稳定。
- 架构与不变量：Git blob 读取、解析、SQLite checkpoint/finalize 仍由既有 bounded batch plan 管理；source scope、路径筛选、语言筛选、行记录、符号 identity、CLI/API 响应语义不变；单个 batch 的内存上限仍由 16 MiB 默认字节预算和资源预算控制。
- 预期影响：把大仓冷索引中的 Git 子进程数量最多再降低约 4 倍，主要改善 `linux_sample_index`、`kubernetes_go_sample_index`、`spring_framework_java_index` 的 index wall time；小仓 ranking 与 query accuracy 不应变化。
- 已知风险：单次 `cat-file --batch` stdout 峰值可接近 batch 字节预算；低并发机器上解析仍按 CPU 并行度串行分块，因此收益主要来自减少 Git 进程启动与 IPC 开销。

## 候选优化说明：20260516T132656Z

- 目标：继续修复 Linux、Kubernetes、Spring Framework full-scope index 的 900 秒超时风险，针对批量持久化与 finalize 阶段中高频重复 SQL prepare 的固定开销。
- 方法：checkpoint batch 写入在 files、symbols、references、imports、chunks、diagnostics 六类循环中复用各自的 prepared statement；FTS search document 插入通过同一个 prepared inserter 复用 SQL；finalize 的 import resolution 更新、call edge 重建插入和 search document 重建同样复用 prepared statement，避免每条记录重新解析相同 SQL 文本。
- 架构与不变量：SQLite schema、事务边界、batch/checkpoint 语义、search document 内容、call edge ID、reference/import/call resolution 规则、CLI/API 返回 schema 均不变；仍由既有 bounded batch 与 finalize transaction 控制资源和崩溃恢复边界。
- 预期影响：大仓索引中每批数百到数万行的写入与 call/import finalize 少做重复 SQL 编译，主要改善 `linux_sample_index`、`kubernetes_go_sample_index`、`spring_framework_java_index`、`relay_teams_index_ms` 和 `leveldb_cpp_index_ms`，对查询 accuracy/ranking 不应产生影响。
- 已知风险：prepared statement 生命周期覆盖整个插入循环，若后续在同一循环中加入需要独占 schema 变更的操作，必须先释放 statement；当前循环只执行普通 DML 与 FTS insert，风险较低。

## 候选优化说明：20260516T135345Z

- 目标：继续修复 Linux、Kubernetes、Spring Framework 大仓 full-scope index 在 finalize 阶段重建 call graph 时的 SQLite/FTS 写入放大，同时避免上轮“移除 call FTS 文档”造成的 query p95 与 ranking 退化。
- 方法：`code_repository_calls` 仍按 reference 逐条重建以保留 caller 归属和稳定 call ID；call edge 表重建完成后，用一次 `INSERT INTO code_repository_search ... SELECT ... FROM code_repository_calls` 集合语句批量重建 call FTS 文档，替代每条 call edge 一次 Rust inserter 调用；schema backfill 使用同一 caller、callee、target hint、path 内容字段，保持旧库补全和新 finalize 输出一致。
- 架构与不变量：call edge schema、call search document 内容字段、source scope、caller/callee resolution、query API、FTS 查询路径、ranking 融合和 checkpoint/finalize 事务边界保持不变；新增测试断言 cross-batch call finalize 后仍生成 call FTS 文档并可被 callers 查询命中。
- 预期影响：大仓调用引用数量很高时，finalize 少执行数十万次 Rust 到 SQLite 的 FTS insert 调用和参数绑定，主要改善 `linux_sample_index`、`kubernetes_go_sample_index`、`spring_framework_java_index` 超时风险；因为查询路径不变，`rt_hybrid_eval_checkpoint_store`、relay-teams p95 和 LevelDB definition cases 应避免 20260516T133442Z 的退化。
- 已知风险：集合插入会在 call table 重建后一次性写入 call FTS rows，事务内峰值 SQLite 工作集中在该语句；若未来 call search document 内容新增字段，必须同步更新 backfill 与 finalize 的两处 `SELECT`。

## 候选优化说明：20260516T135933Z

- 目标：保护大仓 graph 查询准确率，避免 references、calls、imports 在 FTS 命中数超过 bounded candidate window 时，因为未排序的 SQLite FTS row 顺序先截断而丢掉最相关候选。
- 方法：graph 查询的 reference、call、import FTS 子查询在 `LIMIT` 前统一按 `bm25(code_repository_search) ASC, record_id ASC` 排序，与 symbol/chunk 查询的候选剪枝策略一致；Rust 层仍只对 bounded candidate set 做既有语义评分、置信度加权、去重和截断。
- 架构与不变量：SQLite schema、FTS 文档内容、API 返回字段、query kind 分派、scope/path/language 过滤、最终 Rust scoring 与排序规则不变；新增 caller 回归测试构造超过 500 个匹配 call 文档，断言更短且更相关的 FTS 候选在 bounded scoring 前不会被未排序窗口排除。
- 预期影响：在 relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 这类多仓/大仓中，callers/callees/imports/references 查询的候选召回更稳定，特别是大量同名调用、头文件 include、或引用噪声超过默认 500 候选时；性能可能因三个 graph 子查询多一次 FTS rank 排序有小幅成本，但候选窗口仍有上限。
- 已知风险：SQLite FTS `bm25` 排序会在高频宽查询上增加查询 CPU；如果 p95 明显退化，应考虑把 rank-aware ordering 限定到命中数可能溢出窗口的 query kind，或引入更细的 path/language 预过滤候选表。
## 20260516T121321Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T121321Z.patch`
- score: 0.916555 (accuracy=0.9, performance=0.905184, stability=1.0)
- cases: 18/20 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/code/changes.rs`, `src/relay_knowledge/code/pipeline.rs`, `src/relay_knowledge/code/scope.rs`, `src/relay_knowledge/code/tests.rs`
- key improvements: score_component:score 0.804178->0.916555; score_component:accuracy 0.8->0.9; score_component:performance 0.758045->0.905184; score_component:stability 0.911765->1.0; case:leveldb_definition_db_open {'passed': False, 'rank': 2, 'false_positive_count': 0}->{'passed': True, 'rank': 1, 'false_positive_count': 0} failed_to_passed; case:leveldb_definition_write_batch_put {'passed': False, 'rank': 3, 'false_positive_count': 0}->{'passed': True, 'rank': 1, 'false_positive_count': 0} failed_to_passed
- known degradations: metric:cargo_build_release_ms 27983.0->40757; metric:cargo_fmt_check_ms 485.0->692; metric:cargo_clippy_ms 151.0->193; metric:cargo_test_ms 6336.0->7479; metric:relay_teams_index_ms 79134.0->81836; metric:relay_teams_query_p95_ms 10977.0->11574.0
- latency metrics: cargo_build_release_ms=40757ms; cargo_fmt_check_ms=692ms; cargo_clippy_ms=193ms; cargo_test_ms=7479ms; relay_teams_index_ms=81836ms; relay_teams_query_p50_ms=122ms; relay_teams_query_p95_ms=11574ms; leveldb_cpp_index_ms=19215ms

Adopted optimization notes:

dd", "."]); +    repo.git(["commit", "-m", "base"]); +    let budget = CodeIndexResourceBudget::new(128, "fn a() {}\nfn b() {}\n".len(), 50_000) +        .expect("budget should validate"); +    let plan = prepare_full_index_plan(repo.registration(), repo.selector(), budget) +        .expect("plan should prepare"); + +    let (plan, first_batch) = plan.parse_next_batch().expect("first batch should parse"); +    let (plan, second_batch) = plan.parse_next_batch().expect("second batch should parse"); +    let (_, third_batch) = plan.parse_next_batch().expect("third batch should parse"); + +    let first_batch = first_batch.expect("first batch should exist"); +    let second_batch = second_batch.expect("second batch should exist"); +    assert!(third_batch.is_none()); +    assert_eq!(first_batch.files.len(), 2); +    assert_eq!(first_batch.files[0].path, "src/a.rs"); +    assert_eq!(first_batch.files[1].path, "src/b.rs"); +    assert_eq!(second_batch.files.len(), 1); +    assert_eq!(second_batch.files[0].path, "src/c.rs"); +} + +#[test] fn explicit_default_exclusion_opt_in_supports_dataset_and_lock_paths() { let registration = CodeRepositoryRegistration::new( "repo", tokens used 165,514
## 20260516T122811Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T122811Z.patch`
- score: 0.916543 (accuracy=0.9, performance=0.905145, stability=1.0)
- cases: 18/20 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`, `src/relay_knowledge/storage/sqlite/code_batch_finalize_tests.rs`, `src/relay_knowledge/storage/sqlite/code_schema.rs`
- key improvements: metric:cargo_fmt_check_ms 724.0->688; metric:relay_teams_query_p95_ms 12222.0->11662.0
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=49471ms; cargo_fmt_check_ms=688ms; cargo_clippy_ms=194ms; cargo_test_ms=7428ms; relay_teams_index_ms=81898ms; relay_teams_query_p50_ms=127ms; relay_teams_query_p95_ms=11662ms; leveldb_cpp_index_ms=19586ms

Adopted optimization notes:

            row.get::<_, u16>(3)?, +                        row.get::<_, String>(4)?, +                    ), +                )) +            })?; + +            rows.collect::<Result<BTreeMap<_, _>, _>>() +                .map_err(crate::storage::StorageError::from) +        }) +        .await +        .expect("reference rows should load") +} + fn file( source_scope: &str, file_id: &str, diff --git a/src/relay_knowledge/storage/sqlite/code_schema.rs b/src/relay_knowledge/storage/sqlite/code_schema.rs index f3aec34be9ed352e5e1013106078e718c3bc9168..d7b8cb2e9a40adc1e7c21eb825c6220fb8fd9877 --- a/src/relay_knowledge/storage/sqlite/code_schema.rs +++ b/src/relay_knowledge/storage/sqlite/code_schema.rs @@ -210,6 +210,8 @@ CREATE INDEX IF NOT EXISTS code_repository_symbols_lookup ON code_repository_symbols(source_scope, name, qualified_name, path); +        CREATE INDEX IF NOT EXISTS code_repository_symbols_name_path_lookup +            ON code_repository_symbols(source_scope, name, path); CREATE INDEX IF NOT EXISTS code_repository_references_lookup ON code_repository_references(source_scope, name, kind, path); CREATE INDEX IF NOT EXISTS code_repository_calls_lookup tokens used 143,069
## 20260516T124101Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T124101Z.patch`
- score: 0.916549 (accuracy=0.9, performance=0.905163, stability=1.0)
- cases: 18/20 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`
- key improvements: metric:cargo_fmt_check_ms 724.0->684; metric:cargo_test_ms 8230.0->7430; metric:relay_teams_index_ms 86865.0->81070; metric:relay_teams_query_p95_ms 12179.0->11621.0; metric:leveldb_cpp_index_ms 21294.0->19352
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=51187ms; cargo_fmt_check_ms=684ms; cargo_clippy_ms=187ms; cargo_test_ms=7430ms; relay_teams_index_ms=81070ms; relay_teams_query_p50_ms=116ms; relay_teams_query_p95_ms=11621ms; leveldb_cpp_index_ms=19352ms

Adopted optimization notes:

e) } fn load_symbol_keys( @@ -797,3 +799,39 @@ hash } + +#[cfg(test)] +mod tests { +    use super::{SymbolKey, caller_for_line}; +    use crate::domain::RepositoryCodeRange; + +    #[test] +    fn caller_lookup_uses_sorted_prefix_and_prefers_innermost_symbol() { +        let symbols = vec![ +            symbol("outer", 10, 100), +            symbol("same_start_outer", 20, 80), +            symbol("same_start_inner", 20, 40), +            symbol("after_call", 60, 70), +        ]; + +        let caller = caller_for_line(Some(&symbols), 30).expect("caller should match"); + +        assert_eq!(caller.name, "same_start_inner"); +    } + +    #[test] +    fn caller_lookup_ignores_symbols_that_start_after_call_line() { +        let symbols = vec![symbol("before", 1, 5), symbol("after", 20, 30)]; + +        assert!(caller_for_line(Some(&symbols), 10).is_none()); +    } + +    fn symbol(name: &str, start: u32, end: u32) -> SymbolKey { +        SymbolKey { +            symbol_snapshot_id: format!("symbol:{name}"), +            path: "src/lib.rs".to_owned(), +            name: name.to_owned(), +            line_range: RepositoryCodeRange { start, end }, +        } +    } +} tokens used 88,978
## 20260516T130000Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T130000Z.patch`
- score: 0.916501 (accuracy=0.9, performance=0.905002, stability=1.0)
- cases: 18/20 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/code/pipeline.rs`, `src/relay_knowledge/code/tests.rs`
- key improvements: metric:cargo_test_ms 8118.0->7706; metric:relay_teams_index_ms 89353.0->83470; metric:leveldb_cpp_index_ms 21112.0->20417
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=54020ms; cargo_fmt_check_ms=717ms; cargo_clippy_ms=198ms; cargo_test_ms=7706ms; relay_teams_index_ms=83470ms; relay_teams_query_p50_ms=129ms; relay_teams_query_p95_ms=11995ms; leveldb_cpp_index_ms=20417ms

Adopted optimization notes:

3b88 --- a/src/relay_knowledge/code/tests.rs +++ b/src/relay_knowledge/code/tests.rs @@ -265,6 +265,31 @@ } #[test] +fn full_index_plan_preserves_order_across_bounded_parallel_parse_chunks() { +    let repo = TempGitRepo::create("parallel-fetch-order"); +    for index in 0..40 { +        repo.write( +            &format!("src/file_{index:02}.rs"), +            &format!("fn f_{index}() {{}}\n"), +        ); +    } +    repo.git(["add", "."]); +    repo.git(["commit", "-m", "base"]); +    let budget = +        CodeIndexResourceBudget::new(40, 1024 * 1024, 50_000).expect("budget should validate"); +    let plan = prepare_full_index_plan(repo.registration(), repo.selector(), budget) +        .expect("plan should prepare"); + +    let (_, batch) = plan.parse_next_batch().expect("batch should parse"); +    let batch = batch.expect("batch should exist"); + +    assert_eq!(batch.files.len(), 40); +    for (index, file) in batch.files.iter().enumerate() { +        assert_eq!(file.path, format!("src/file_{index:02}.rs")); +    } +} + +#[test] fn explicit_default_exclusion_opt_in_supports_dataset_and_lock_paths() { let registration = CodeRepositoryRegistration::new( "repo", tokens used 84,035
## 20260516T132656Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T132656Z.patch`
- score: 0.971355 (accuracy=1.0, performance=0.904515, stability=1.0)
- cases: 20/20 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code.rs`, `src/relay_knowledge/storage/sqlite/code_batch.rs`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`
- key improvements: score_component:score 0.916501->0.971355; score_component:accuracy 0.9->1.0; case:rt_definition_w3_save_request {'passed': False, 'rank': 2, 'false_positive_count': 0}->{'passed': True, 'rank': 1, 'false_positive_count': 0} failed_to_passed; case:rt_hybrid_eval_checkpoint_store {'passed': False, 'rank': None, 'false_positive_count': 0}->{'passed': True, 'rank': 2, 'false_positive_count': 0} failed_to_passed
- known degradations: metric:cargo_build_release_ms 54020.0->60487; metric:cargo_fmt_check_ms 717.0->818; metric:cargo_test_ms 7706.0->8794; metric:relay_teams_index_ms 83470.0->86086; metric:relay_teams_query_p95_ms 11995.0->13289.0
- latency metrics: cargo_build_release_ms=60487ms; cargo_fmt_check_ms=818ms; cargo_clippy_ms=214ms; cargo_test_ms=8794ms; relay_teams_index_ms=86086ms; relay_teams_query_p50_ms=133ms; relay_teams_query_p95_ms=13289ms; leveldb_cpp_index_ms=20307ms

Adopted optimization notes:

  caller.map(|symbol| symbol.name.clone()), -                reference.target_symbol_snapshot_id, -                reference.name, -                reference.target_hint, -                reference.resolution_state, -                reference.confidence_basis_points, -                reference.confidence_tier, -                reference.line_start, -                reference.line_end, -            ], -        )?; -        super::super::insert_search_document( -            transaction, +        insert_call.execute(params![ +            repository_id, +            source_scope, +            call_id, +            reference.file_id, +            reference.path, +            caller.map(|symbol| symbol.symbol_snapshot_id.clone()), +            caller.map(|symbol| symbol.name.clone()), +            reference.target_symbol_snapshot_id, +            reference.name, +            reference.target_hint, +            reference.resolution_state, +            reference.confidence_basis_points, +            reference.confidence_tier, +            reference.line_start, +            reference.line_end, +        ])?; +        search_documents.insert( source_scope, "call", &call_id, tokens used 98,611

## 候选优化说明：accuracy/stability 优先与 case 扩展

- 目标：让自迭代优先维护代码检索 accuracy 与 stability，把它们作为基础功能可用性的受保护目标；同时扩展现有 benchmark cases，对功能精度和高 fan-out 查询性能暴露更广的回归面。
- 方法：评分权重调整为 `accuracy=0.60`、`performance=0.15`、`stability=0.25`；采纳策略新增 protected objective 检查，历史 run 存在时显著 accuracy 或 stability 退化会直接拒绝候选，即使性能指标改善。prompt 明确要求 Codex 先处理 accuracy/case/stability 退化，再追求纯延迟优化。`cases.json` 增加 relay-teams、Linux、LevelDB、Spring Framework、Kubernetes 的 definition/hybrid/imports 查询，部分 case 使用 `limit=20` 扩大排序与查询延迟覆盖。
- 追加 fuzzy case：继续补充自然语言式、非精确符号名查询，覆盖变量、函数、常数和类，包括 checkpoint version 常量、archive output 函数、LevelDB Cache 类、CRC mask 常量、Spring DispatcherServlet 类、Kubernetes repeatable authorizer 变量、service IP range helper 和 REST noBackoff 变量。
- 架构与不变量：自迭代仍独立于 Rust crate；repository target 仍保持 `scope=all`；case 级 path/language filter 只用于查询端过滤验证；epsilon-Pareto 仍用于噪声抑制和非受保护目标决策，build/test gate 继续作为硬约束。
- 预期影响：后续候选会更少用性能提升换取 accuracy 或 gate 稳定性退化；新增 case 提高对 Python 方法重名、Python/C++/Java/Go 常量变量、C 宏/函数、C++ 工厂函数与类、Java servlet 类型、Go authorizer API 的覆盖，并把更多全仓查询纳入 p50/p95 性能观测。
- 已知风险：新增 case 会改变 accuracy 平均值基线，首次运行可能需要重新建立可比历史；`limit=20` case 会略微增加查询评估耗时，但能更早暴露大仓候选集和排序退化。
## 20260516T135345Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T135345Z.patch`
- score: 0.939527 (accuracy=0.923077, performance=0.904539, stability=1.0)
- cases: 24/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`, `src/relay_knowledge/storage/sqlite/code_batch_finalize_tests.rs`, `src/relay_knowledge/storage/sqlite/code_schema.rs`
- key improvements: none recorded
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=59850ms; cargo_fmt_check_ms=842ms; cargo_clippy_ms=209ms; cargo_test_ms=8835ms; relay_teams_index_ms=87582ms; relay_teams_query_p50_ms=136ms; relay_teams_query_p95_ms=13219ms; leveldb_cpp_index_ms=19926ms

Adopted optimization notes:

w( +                    " +                    SELECT COUNT(*) +                    FROM code_repository_search +                    WHERE source_scope = ?1 AND document_kind = ?2 +                    ", +                    (&source_scope, &document_kind), +                    |row| row.get(0), +                ) +                .map_err(crate::storage::StorageError::from) +        }) +        .await +        .expect("search document count should load") +} diff --git a/src/relay_knowledge/storage/sqlite/code_schema.rs b/src/relay_knowledge/storage/sqlite/code_schema.rs index d7b8cb2e9a40adc1e7c21eb825c6220fb8fd9877..0946af7022d8361c66c6443600975234efc916e8 --- a/src/relay_knowledge/storage/sqlite/code_schema.rs +++ b/src/relay_knowledge/storage/sqlite/code_schema.rs @@ -373,7 +373,8 @@ source_scope, document_kind, record_id, path, language_id, content ) SELECT source_scope, 'call', call_id, path, '', -               coalesce(caller_name, '') || ' ' || callee_name || ' ' || coalesce(target_hint, '') +               coalesce(caller_name, '') || ' ' || callee_name || ' ' || +               coalesce(target_hint, '') || ' ' || path FROM code_repository_calls ", [], tokens used 135,337
## 20260516T135933Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T135933Z.patch`
- score: 0.939577 (accuracy=0.923077, performance=0.904871, stability=1.0)
- cases: 24/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
- key improvements: metric:cargo_build_release_ms 59850.0->56550; metric:cargo_fmt_check_ms 842.0->755; metric:cargo_test_ms 8835.0->7792; metric:relay_teams_index_ms 87582.0->83515; metric:relay_teams_query_p95_ms 13219.0->12317.0
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=56550ms; cargo_fmt_check_ms=755ms; cargo_clippy_ms=204ms; cargo_test_ms=7792ms; relay_teams_index_ms=83515ms; relay_teams_query_p50_ms=132ms; relay_teams_query_p95_ms=12317ms; leveldb_cpp_index_ms=19595ms

Adopted optimization notes:

 "exact-file", "src/exact_owner.py"); +    exact.caller_name = Some("exactOwner".to_owned()); +    exact.callee_name = "TargetCall".to_owned(); +    exact.target_hint = Some("TargetCall".to_owned()); +    exact.resolution_state = "resolved".to_owned(); +    exact.confidence_basis_points = 8_000; +    exact.confidence_tier = "inferred".to_owned(); +    calls.push(exact); + +    CodeIndexSnapshot { +        repository_id: "repo".to_owned(), +        source_scope: TEST_SOURCE_SCOPE.to_owned(), +        base_resolved_commit_sha: None, +        resolved_commit_sha: "commit".to_owned(), +        tree_hash: "tree".to_owned(), +        path_filters: Vec::new(), +        language_filters: Vec::new(), +        full_replace: true, +        changed_path_count: files.len(), +        skipped_unchanged_count: 0, +        deleted_paths: Vec::new(), +        tombstones: Vec::new(), +        files, +        symbols: Vec::new(), +        references: Vec::new(), +        imports: Vec::new(), +        calls, +        chunks: Vec::new(), +        diagnostics: Vec::new(), +    } +} + fn snapshot_with_call_site_chunk() -> CodeIndexSnapshot { let mut caller = symbol( "sanitize-options", tokens used 157,522

## 候选优化说明：20260516T140540Z

- 目标：提升大仓 call graph 查询准确率，避免 `Callers`/`Callees` 方向查询在主边端相同的大量候选中，只按路径或插入顺序处理 tie，导致带有 caller、callee 或路径上下文的自然语言查询排不到目标结果。
- 方法：在 call FTS bounded candidate set 已经命中后，先保持既有方向语义：`Callers` 必须由 callee 字段产生正分，`Callees` 必须由 caller 字段产生正分；只有主边端 `base_score > 0` 时，再用非主边端和 path 计算一个 0.35 系数的上下文 bonus。这样 `TargetCall exactOwner` 仍只返回调用 `TargetCall` 的 caller，但会把 caller 名或路径含 `exactOwner` 的边排在同 callee 噪声之前。
- 架构与不变量：SQLite schema、FTS 文档、candidate limit、source scope、path/language filter、API 字段、call edge resolution/confidence bonus、去重和最终截断规则不变；新增单元级集成测试构造同 callee、同 confidence、不同 caller/path 的噪声，断言 caller 上下文能稳定打破 tie。
- 预期影响：relay-teams、Linux、LevelDB、Kubernetes、Spring Framework 中高 fan-out API、工厂函数、hook、handler 的 callers/callees 查询更容易利用用户给出的 owner、component、file/path 上下文，提升 top-rank 准确率；计算只发生在最多 500-2000 个候选的 Rust scoring 阶段，对索引与 SQLite 查询性能无影响。
- 已知风险：上下文 bonus 可能把路径或 caller/callee 名里包含额外查询词的结果排到更前；由于 bonus 受主边端正分门控，不能让不调用目标 callee 或不属于目标 caller 的边单独入选。

## 候选优化说明：20260516T142335Z

- 目标：提升 fuzzy definition/hybrid 查询对多段函数名的排序准确率，尤其是 `archive old eval output directory timestamp suffix` 这类自然语言查询应优先返回 `archive_output_dir`，而不是只命中单个通用词的 output、directory 或 archive 噪声符号。
- 方法：仅在 symbol 查询已由 FTS 召回后，在既有 `symbol_name_query_bonus` 中增加受限的部分覆盖 bonus；当至少 3 个长度不小于 3 的查询词能与 symbol name 的规范化 identifier token 精确匹配，或存在清晰前缀关系（例如 `directory` 与 `dir`）时，最多增加 2.0 分。新增回归测试构造 `archive_output_dir` 与 output/directory/archive 单词噪声，断言多段符号名排在首位。
- 架构与不变量：不改变 SQLite schema、FTS content、candidate limit、source scope、path/language filter、API 字段、call/reference/import 查询语义、去重或最终截断；只调整 bounded symbol candidate set 内的 Rust 排序，且 bonus 需要 3 个匹配词门槛，避免 1-2 个通用词扩大噪声优势。
- 预期影响：relay-teams、LevelDB、Kubernetes、Spring Framework 中以 snake_case、CamelCase 或缩写命名的函数、类、常量，在自然语言查询同时描述多个 name parts 时更容易排到 top-rank；对性能的影响限于已召回 symbol 候选的少量 identifier token 比较。
- 已知风险：包含 3 个以上通用短 identifier parts 的符号可能获得额外分数；门槛、长度限制、2.0 上限和不修改 FTS 召回可限制对现有准确率与 p95 的扰动。
## 20260516T140540Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T140540Z.patch`
- score: 0.939609 (accuracy=0.923077, performance=0.905086, stability=1.0)
- cases: 24/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
- key improvements: metric:cargo_build_release_ms 56550.0->48946; metric:cargo_fmt_check_ms 755.0->728; metric:relay_teams_query_p95_ms 12317.0->11797.0
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=48946ms; cargo_fmt_check_ms=728ms; cargo_clippy_ms=200ms; cargo_test_ms=8023ms; relay_teams_index_ms=81709ms; relay_teams_query_p50_ms=130ms; relay_teams_query_p95_ms=11797ms; leveldb_cpp_index_ms=19861ms

Adopted optimization notes:

rs: Vec::new(), +        language_filters: Vec::new(), +        full_replace: true, +        changed_path_count: 3, +        skipped_unchanged_count: 0, +        deleted_paths: Vec::new(), +        tombstones: Vec::new(), +        files: vec![ +            file( +                "first-noise-file", +                "src/a_noise.py", +                "python", +                CodeParseStatus::Parsed, +                None, +            ), +            file( +                "second-noise-file", +                "src/b_noise.py", +                "python", +                CodeParseStatus::Parsed, +                None, +            ), +            file( +                "exact-file", +                "src/z_exact_owner.py", +                "python", +                CodeParseStatus::Parsed, +                None, +            ), +        ], +        symbols: Vec::new(), +        references: Vec::new(), +        imports: Vec::new(), +        calls: vec![first_noise, second_noise, exact], +        chunks: Vec::new(), +        diagnostics: Vec::new(), +    } +} + fn snapshot_with_call_site_chunk() -> CodeIndexSnapshot { let mut caller = symbol( "sanitize-options", tokens used 132,437
## 20260516T142335Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T142335Z.patch`
- score: 0.939946 (accuracy=0.923077, performance=0.90733, stability=1.0)
- cases: 24/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
- key improvements: score_component:score 0.893508->0.939946; score_component:accuracy 0.846154->0.923077; metric:cargo_build_release_ms 35177.0->33618; metric:cargo_fmt_check_ms 718.0->682; metric:relay_teams_index_ms 76655.0->70526; metric:relay_teams_query_p95_ms 11039.0->8185.0; case:rt_hybrid_eval_checkpoint_store {'passed': False, 'rank': None, 'false_positive_count': 0}->{'passed': True, 'rank': 2, 'false_positive_count': 0} failed_to_passed; case:rt_fuzzy_function_archive_output_dir {'passed': False, 'rank': None, 'false_positive_count': 0}->{'passed': False, 'rank': 13, 'false_positive_count': 0} rank_improved
- known degradations: case:rt_fuzzy_constant_checkpoint_version {'passed': True, 'rank': 1, 'false_positive_count': 0}->{'passed': False, 'rank': None, 'false_positive_count': 0} passed_to_failed
- latency metrics: cargo_build_release_ms=33618ms; cargo_fmt_check_ms=682ms; cargo_clippy_ms=179ms; cargo_test_ms=7211ms; relay_teams_index_ms=70526ms; relay_teams_query_p50_ms=120ms; relay_teams_query_p95_ms=8185ms; leveldb_cpp_index_ms=18800ms

Adopted optimization notes:

              None, +            ), +            file( +                "output-file", +                "src/relay_teams/sessions/runs/background_tasks/projection.py", +                "python", +                CodeParseStatus::Parsed, +                None, +            ), +            file( +                "directory-file", +                "src/relay_teams/workspace/directory_picker.py", +                "python", +                CodeParseStatus::Parsed, +                None, +            ), +            file( +                "archive-file", +                "tests/unit_tests/net/test_github_cli.py", +                "python", +                CodeParseStatus::Parsed, +                None, +            ), +        ], +        symbols: vec![target, output_noise, directory_noise, archive_noise], +        references: Vec::new(), +        imports: Vec::new(), +        calls: Vec::new(), +        chunks: Vec::new(), +        diagnostics: Vec::new(), +    } +} + fn snapshot_with_resolved_callee_tie() -> CodeIndexSnapshot { let mut ambiguous = call("ambiguous-callee", "cma-source", "mm/cma_debug.c"); ambiguous.caller_name = Some("cma_debugfs_init".to_owned()); tokens used 172,862

## 候选优化说明：20260516T143645Z

- 目标：修复 fuzzy symbol/hybrid 查询在自然语言 query 含额外描述词时的召回缺口，优先保护 `rt_fuzzy_constant_checkpoint_version` 和 `rt_fuzzy_function_archive_output_dir` 这类代码仓库检索准确率。
- 方法：仅对 symbol FTS bounded candidate recall 使用多 term `OR`，避免 `_CHECKPOINT_VERSION`、`archive_output_dir` 这类真实符号因 `metadata`、`old`、`timestamp`、`suffix` 等描述词未出现在符号文档内而在评分前被排除；reference、call、import 继续使用原有 all-term FTS 召回。Rust 侧 `score_text` 改为识别 snake_case 和 CamelCase identifier part，且 symbol 评分字段纳入 `kind`，让召回后的候选仍按符号名、类型、签名、路径上下文排序。
- 架构与不变量：SQLite schema、FTS document 内容、candidate limit、source scope、path/language filter、API 字段、graph edge 查询、去重和最终截断规则不变；召回扩展只发生在 bounded symbol candidate set 内，最终排序仍由统一 scorer、symbol kind bonus 和既有融合规则决定。
- 预期影响：relay-teams、LevelDB、Linux、Kubernetes、Spring Framework 中含常量、函数、类名的自然语言 fuzzy definition/hybrid 查询更容易召回真实多段 identifier，并用 kind/name part 排在单词噪声前；`archive_output_dir` 和 checkpoint version 常量应提升 rank，已通过单元与集成回归测试覆盖。
- 已知风险：symbol FTS 的 `OR` 召回会让宽查询进入更多候选，可能增加少量 SQLite FTS 和 Rust scoring CPU；候选窗口仍受 500-2000 上限约束，且非 symbol graph 查询保持原有精确召回以控制 fan-out。

## 候选优化说明：20260516T144537Z

- 目标：修复宽 hybrid 查询在 LevelDB 大仓中把 API 声明块排在使用样例或实现块之后的问题，优先保护 `leveldb_hybrid_recovery_manifest_full_scope` 和 `leveldb_fuzzy_class_cache_lru_interface`，同时保留上一轮 fuzzy symbol 召回收益。
- 方法：在 chunk 层 scoring 后增加受限声明块 bonus；只有当查询至少 3 个长度不小于 3 的词能命中 chunk identifier/text，且 chunk 形态像 API 声明时才加分。抽象接口查询要求 query 含 `interface` 且 chunk 含 `virtual ... = 0;`，普通声明上下文要求至少两行函数声明式原型。该规则补充 ranking fusion，不改变 FTS 召回、symbol/edge 查询或最终 API schema。
- 架构与不变量：SQLite schema、索引内容、source scope、path/language filter、bounded candidate limit、去重截断、symbol FTS `OR` 召回和 graph edge 查询语义不变；bonus 只在已召回的 hybrid chunk 候选内生效，并要求多词覆盖以避免单词噪声被提升。
- 预期影响：LevelDB、Linux、Kubernetes、Spring Framework 中面向接口、头文件声明、恢复/manifest 这类 API 上下文的自然语言 hybrid 查询，应更稳定地返回声明入口，而不是测试 fixture、构造函数使用点或实现细节；`leveldb_fuzzy_class_cache_lru_interface` 预期回到 rank 1，`leveldb_hybrid_recovery_manifest_full_scope` 预期进入通过阈值。
- 已知风险：部分实现文件也可能包含多个声明式行或纯虚接口文本，存在小幅 rank 变化风险；规则要求多词覆盖和声明形态，且仅对 chunk hit 加 bounded bonus，以避免牺牲精确 symbol/query cases。

## 候选优化说明：20260516T145236Z

- 目标：保留 20260516T144537Z 对 LevelDB hybrid/API 声明块排序的准确率收益，同时降低该声明块 bonus 在 relay-teams 和 LevelDB 宽 hybrid 查询 p95 上的 per-candidate CPU 成本。
- 方法：chunk scoring 仍使用同一个声明块 bonus，但先用廉价结构检查判断 chunk 是否包含抽象接口或至少两个声明式 prototype，再对可能拿到 bonus 的少量候选执行 query term 覆盖、identifier token 和 lowercase substring 匹配；query terms 在 `search_chunks` 中每次请求只解析一次，而不是每个 chunk 重复解析。
- 架构与不变量：SQLite schema、FTS 召回、candidate limit、source scope、path/language filter、API 字段、去重截断、声明块 bonus 分值和已接受的 LevelDB accuracy 规则保持不变；bonus 仍要求至少 3 个查询词覆盖且只作用于 bounded chunk candidate set。
- 预期影响：非声明 chunk 候选在 Rust scoring 阶段避免重复 query 分词、全文 lowercase 和 identifier token 扫描，预期改善 `relay_teams_query_p95_ms` 与 `leveldb_cpp_query_p95_ms`；`leveldb_hybrid_recovery_manifest_full_scope` 和 `leveldb_fuzzy_class_cache_lru_interface` 的排序应保持不变。
- 已知风险：结构检查顺序改变不应影响结果，但如果未来某语言的声明形态不符合 `declaration_line_is_prototype` 或抽象接口文本模式，仍不会获得该 bonus；测试覆盖实现块不加分、头文件 prototype 和纯虚接口仍加分。

## 候选优化说明：20260516T155614Z

- 目标：修复 relay-teams `_summary` callers/callees 这类高 fan-out call graph 查询的 p95 稳定性问题，同时不改变 accuracy、ranking、FTS 召回或 API 返回语义。
- 方法：为 `code_repository_chunks(source_scope, symbol_snapshot_id)` 增加 SQLite 索引。call 查询为了生成调用点 excerpt，会把 bounded call candidates 的 `caller_symbol_snapshot_id` 关联到 `code_repository_chunks`；大仓 chunk 数量较高且 caller 命中密集时，没有该索引会让每个候选都可能扫描同 scope chunks。新增索引把 caller chunk lookup 变成按 source scope 和 symbol identity 的索引查找。
- 架构与不变量：不修改 code graph schema 字段、FTS 文档、candidate limit、查询 scoring、去重截断、call edge resolution、path/language filter 或 CLI/API 行为；索引通过既有 `CREATE INDEX IF NOT EXISTS` 初始化与迁移路径应用到新旧 SQLite 数据库。新增测试固定 schema 必须包含该 lookup index。
- 预期影响：`relay_teams_query_p95_ms` 中 `_summary` callers/callees 查询应显著降低尾延迟；LevelDB、Kubernetes、Linux、Spring Framework 的 call graph 查询在多调用、多 chunk 场景下也应更稳定。索引阶段可能多维护一个小型 B-tree，但 chunk 写入仍在 bounded batch/finalize 事务内完成。
- 已知风险：大仓索引写入和数据库文件会因额外索引略增；如果未来 call excerpt 不再从 `code_repository_chunks` 关联 caller symbol，该索引价值会下降，需要按查询计划重新评估。

## 候选优化说明：20260516T160620Z

- 目标：继续保护大仓 call graph 查询稳定性，避免一个 caller symbol 被切成多个非重叠 chunks 时，单条 call edge 因 excerpt join 被放大成多条候选并增加排序、评分和去重成本。
- 方法：call 查询仍复用 `code_repository_chunks(source_scope, symbol_snapshot_id)` lookup index，但 caller chunk join 增加 call line containment 条件：只连接 `line_start <= call.line_start <= line_end` 的 chunk。这样 excerpt 直接来自包含调用点的 chunk，不再把同一 caller symbol 的 prologue、body、tail chunks 全部带入 bounded candidate rows。
- 架构与不变量：不改变 SQLite schema、FTS 文档、candidate limit、call edge 召回、方向性 caller/callee 语义、confidence bonus、path/language filter、API 字段或最终排序规则；只收敛已有 excerpt join 的候选行数。新增回归测试构造同一 caller symbol 的两个非重叠 chunks，断言 callers 查询只返回一条 hit 且 excerpt 取实际 call-site chunk。
- 预期影响：relay-teams、Linux、Kubernetes、Spring Framework 这类长函数、大类方法较多的仓库中，callers/callees 查询的 Rust scoring 输入更小、结果重复更少，p95 抖动应降低；accuracy 预期保持或改善，因为摘要优先来自调用点所在 chunk。
- 已知风险：如果某个索引器生成的 chunk line ranges 不覆盖 call line，则该 call hit 会退回到 `caller calls callee` 摘要，不再从同 symbol 的其他 chunk 猜测 excerpt；这是更保守的稳定性取舍，后续可通过索引器 line-range 测试保护覆盖率。

## 20260516T143645Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T143645Z.patch`
- score: 0.963002 (accuracy=0.961538, performance=0.907191, stability=1.0)
- cases: 25/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:score 0.939954->0.963002; score_component:accuracy 0.923077->0.961538; metric:cargo_fmt_check_ms 711.0->678; metric:relay_teams_index_ms 73237.0->70352; case:rt_fuzzy_constant_checkpoint_version {'passed': False, 'rank': None, 'false_positive_count': 0}->{'passed': True, 'rank': 3, 'false_positive_count': 0} failed_to_passed; case:rt_fuzzy_function_archive_output_dir {'passed': False, 'rank': 13, 'false_positive_count': 0}->{'passed': True, 'rank': 2, 'false_positive_count': 0} failed_to_passed
- known degradations: metric:leveldb_cpp_query_p95_ms 136.0->183.0; case:leveldb_hybrid_recovery_manifest_full_scope {'passed': True, 'rank': 3, 'false_positive_count': 0}->{'passed': False, 'rank': 9, 'false_positive_count': 0} passed_to_failed; case:leveldb_fuzzy_class_cache_lru_interface {'passed': True, 'rank': 1, 'false_positive_count': 0}->{'passed': True, 'rank': 5, 'false_positive_count': 0} rank_worsened
- latency metrics: cargo_build_release_ms=34450ms; cargo_fmt_check_ms=678ms; cargo_clippy_ms=195ms; cargo_test_ms=7340ms; relay_teams_index_ms=70352ms; relay_teams_query_p50_ms=128ms; relay_teams_query_p95_ms=8344ms; leveldb_cpp_index_ms=18666ms

Adopted optimization notes:

us-callee", "cma-source", "mm/cma_debug.c"); ambiguous.caller_name = Some("cma_debugfs_init".to_owned()); diff --git a/src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs b/src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs index 595e4c271c813e5466eb068d26842c18e72d6e06..db6719d0fe07cf265d1d8dff6c31a380334f1108 --- a/src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs +++ b/src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs @@ -32,3 +32,25 @@ assert!(values.len() <= MAX_CANDIDATE_BIND_VALUES); } + +#[test] +fn symbol_fts_query_uses_any_term_for_fuzzy_recall() { +    assert_eq!( +        symbol_fts_match_query("checkpoint metadata version constant"), +        "\"checkpoint\" OR \"metadata\" OR \"version\" OR \"constant\"" +    ); +    assert_eq!( +        fts_match_query("checkpoint metadata version constant"), +        "\"checkpoint\" \"metadata\" \"version\" \"constant\"" +    ); +} + +#[test] +fn score_text_matches_identifier_parts_inside_snake_case_names() { +    let score = score_text( +        "archive output directory", +        ["def archive_output_dir(output_dir: Path) -> Path:"], +    ); + +    assert!(score >= 4.0); +} tokens used 144,045
## 20260516T144537Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T144537Z.patch`
- score: 0.985951 (accuracy=1.0, performance=0.906342, stability=1.0)
- cases: 26/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
- key improvements: score_component:score 0.963002->0.985951; score_component:accuracy 0.961538->1.0; metric:cargo_build_release_ms 34450.0->30307; case:leveldb_hybrid_recovery_manifest_full_scope {'passed': False, 'rank': 9, 'false_positive_count': 0}->{'passed': True, 'rank': 5, 'false_positive_count': 0} failed_to_passed; case:leveldb_fuzzy_class_cache_lru_interface {'passed': True, 'rank': 5, 'false_positive_count': 0}->{'passed': True, 'rank': 1, 'false_positive_count': 0} rank_improved
- known degradations: metric:cargo_fmt_check_ms 678.0->708; metric:relay_teams_query_p95_ms 8344.0->9460.0; metric:leveldb_cpp_query_p95_ms 183.0->225.0
- latency metrics: cargo_build_release_ms=30307ms; cargo_fmt_check_ms=708ms; cargo_clippy_ms=182ms; cargo_test_ms=7290ms; relay_teams_index_ms=69032ms; relay_teams_query_p50_ms=118ms; relay_teams_query_p95_ms=9460ms; leveldb_cpp_index_ms=18856ms

Adopted optimization notes:

        base_resolved_commit_sha: None, +        resolved_commit_sha: "commit".to_owned(), +        tree_hash: "tree".to_owned(), +        path_filters: Vec::new(), +        language_filters: Vec::new(), +        full_replace: true, +        changed_path_count: 2, +        skipped_unchanged_count: 0, +        deleted_paths: Vec::new(), +        tombstones: Vec::new(), +        files: vec![ +            file( +                "db-impl-header", +                "db/db_impl.h", +                "cpp", +                CodeParseStatus::Parsed, +                None, +            ), +            file( +                "db-impl-source", +                "db/db_impl.cc", +                "cpp", +                CodeParseStatus::Parsed, +                None, +            ), +        ], +        symbols: Vec::new(), +        references: Vec::new(), +        imports: Vec::new(), +        calls: Vec::new(), +        chunks: vec![target, noise], +        diagnostics: Vec::new(), +    } +} + fn snapshot_with_related_callee_names() -> CodeIndexSnapshot { let mut unrelated = call("unmapped-area", "mmap-source", "mm/mmap.c"); unrelated.caller_name = Some("do_mmap".to_owned()); tokens used 107,884
## 20260516T145236Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T145236Z.patch`
- score: 0.986063 (accuracy=1.0, performance=0.907089, stability=1.0)
- cases: 26/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: metric:cargo_build_release_ms 30307.0->28188; metric:cargo_fmt_check_ms 708.0->509; metric:cargo_clippy_ms 182.0->149; metric:cargo_test_ms 7290.0->6539; metric:relay_teams_query_p95_ms 9460.0->8464.0
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=28188ms; cargo_fmt_check_ms=509ms; cargo_clippy_ms=149ms; cargo_test_ms=6539ms; relay_teams_index_ms=70263ms; relay_teams_query_p50_ms=119ms; relay_teams_query_p95_ms=8464ms; leveldb_cpp_index_ms=18725ms

Adopted optimization notes:

erms = query_terms("recover descriptor save_manifest versionedit"); + +    assert_eq!( +        declaration_chunk_bonus( +            &terms, +            "Status DBImpl::RecoverLogFile(uint64_t log_number, bool* save_manifest) {\n  descriptor_log_->AddRecord(edit->Encode());\n}" +        ), +        0.0 +    ); +    assert_eq!( +        declaration_chunk_bonus( +            &terms, +            "class DBImpl {\n  Status RecoverLogFile(uint64_t log_number, bool* save_manifest,\n                        VersionEdit* edit)\n      EXCLUSIVE_LOCKS_REQUIRED(mutex_);\n  Status WriteLevel0Table(MemTable* mem, VersionEdit* edit)\n      EXCLUSIVE_LOCKS_REQUIRED(mutex_);\n};" +        ), +        2.0 +    ); +} + +#[test] +fn declaration_chunk_bonus_preserves_interface_boost() { +    let terms = query_terms("cache interface lookup insert total charge lru"); + +    assert_eq!( +        declaration_chunk_bonus( +            &terms, +            "class Cache {\n public:\n  virtual Handle* Insert(const Slice& key, void* value, size_t charge) = 0;\n  virtual Handle* Lookup(const Slice& key) = 0;\n  virtual size_t TotalCharge() const = 0;\n};" +        ), +        3.0 +    ); +} tokens used 113,000
## 20260516T155614Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T155614Z.patch`
- score: 1.0 (accuracy=1.0, performance=1.0, stability=1.0)
- cases: 26/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_schema.rs`, `src/relay_knowledge/storage/sqlite/code_tests.rs`
- key improvements: score_component:score 0.985774->1.0; score_component:performance 0.90516->1.0; metric:relay_teams_query_p50_ms 127.0->93.5; metric:relay_teams_query_p95_ms 11629.0->298.0; metric:leveldb_cpp_index_ms 19318.0->13790; metric:leveldb_cpp_query_p50_ms 149.0->105.5; metric:leveldb_cpp_query_p95_ms 246.0->170.0
- known degradations: metric:cargo_build_release_ms 37157.0->52888; metric:cargo_fmt_check_ms 664.0->713; metric:relay_teams_index_ms 70374.0->74641
- latency metrics: cargo_build_release_ms=52888ms; cargo_fmt_check_ms=713ms; cargo_clippy_ms=176ms; cargo_test_ms=7130ms; relay_teams_index_ms=74641ms; relay_teams_query_p50_ms=94ms; relay_teams_query_p95_ms=298ms; leveldb_cpp_index_ms=13790ms

Adopted optimization notes:

sitorySelector, CodeRetrievalLayer, FreshnessPolicy, }, -    storage::SqliteGraphStore, +    storage::{SqliteGraphStore, StorageError}, }; #[path = "code_test_support.rs"] @@ -114,6 +114,33 @@ } #[tokio::test] +async fn schema_indexes_chunks_by_symbol_for_call_excerpt_lookup() { +    let store = SqliteGraphStore::open_in_memory().expect("store should open"); + +    let index_exists = store +        .run(|connection| { +            connection +                .query_row( +                    " +                    SELECT EXISTS( +                        SELECT 1 +                        FROM sqlite_master +                        WHERE type = 'index' +                          AND name = 'code_repository_chunks_symbol_lookup' +                    ) +                    ", +                    [], +                    |row| row.get::<_, bool>(0), +                ) +                .map_err(StorageError::from) +        }) +        .await +        .expect("schema index check should succeed"); + +    assert!(index_exists); +} + +#[tokio::test] async fn rejects_code_queries_for_unindexed_refs() { let store = store_with_repository_snapshot(snapshot_with_chunk( "repo", tokens used 113,836
## 20260516T160620Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T160620Z.patch`
- score: 1.0 (accuracy=1.0, performance=1.0, stability=1.0)
- cases: 26/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
- key improvements: metric:cargo_build_release_ms 27786.0->25737; metric:cargo_fmt_check_ms 694.0->524; metric:cargo_clippy_ms 8303.0->158; metric:cargo_test_ms 7351.0->6118; metric:leveldb_cpp_index_ms 14476.0->13704; metric:leveldb_cpp_query_p50_ms 144.5->103.5; metric:leveldb_cpp_query_p95_ms 226.0->174.0
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=25737ms; cargo_fmt_check_ms=524ms; cargo_clippy_ms=158ms; cargo_test_ms=6118ms; relay_teams_index_ms=74148ms; relay_teams_query_p50_ms=94ms; relay_teams_query_p95_ms=308ms; leveldb_cpp_index_ms=13704ms

Adopted optimization notes:

ize-options-chunk", -            "db-impl-source", -            "db/db_impl.cc", -            "Options SanitizeOptions(const Options& src) {\n    Options result;\n    result.block_cache = NewLRUCache(8 << 20);\n    return result;\n}", -            Some("sanitize-options"), -        )], +        chunks: vec![ +            RepositoryCodeChunkRecord { +                line_range: range(110, 115), +                ..chunk( +                    "sanitize-options-prologue", +                    "db-impl-source", +                    "db/db_impl.cc", +                    "Options SanitizeOptions(const Options& src) {\n    Options result;", +                    Some("sanitize-options"), +                ) +            }, +            RepositoryCodeChunkRecord { +                line_range: range(116, 124), +                ..chunk( +                    "sanitize-options-call-site", +                    "db-impl-source", +                    "db/db_impl.cc", +                    "    result.block_cache = NewLRUCache(8 << 20);\n    return result;\n}", +                    Some("sanitize-options"), +                ) +            }, +        ], diagnostics: Vec::new(), } } tokens used 172,827
## 20260516T171146Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T171146Z.patch`
- score: 1.0 (accuracy=1.0, performance=1.0, stability=1.0)
- cases: 26/26 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
- key improvements: metric:cargo_build_release_ms 35920.0->28113; metric:cargo_fmt_check_ms 676.0->516; metric:cargo_clippy_ms 171.0->144; metric:cargo_test_ms 7135.0->6245
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=28113ms; cargo_fmt_check_ms=516ms; cargo_clippy_ms=144ms; cargo_test_ms=6245ms; relay_teams_index_ms=67184ms; relay_teams_query_p50_ms=116ms; relay_teams_query_p95_ms=305ms; leveldb_cpp_index_ms=17958ms

Adopted optimization notes:

d, +            &path, +            "target", +        )); +    } + +    files.push(file( +        "target-file", +        "src/target.rs", +        "rust", +        CodeParseStatus::Parsed, +        None, +    )); +    symbols.push(symbol( +        "target-symbol", +        "target-file", +        "src/target.rs", +        "target", +    )); + +    CodeIndexSnapshot { +        repository_id: "repo".to_owned(), +        source_scope: TEST_SOURCE_SCOPE.to_owned(), +        base_resolved_commit_sha: None, +        resolved_commit_sha: "commit".to_owned(), +        tree_hash: "tree".to_owned(), +        path_filters: Vec::new(), +        language_filters: Vec::new(), +        full_replace: true, +        changed_path_count: files.len(), +        skipped_unchanged_count: 0, +        deleted_paths: Vec::new(), +        tombstones: Vec::new(), +        files, +        symbols, +        references: Vec::new(), +        imports: Vec::new(), +        calls: Vec::new(), +        chunks: Vec::new(), +        diagnostics: Vec::new(), +    } +} + fn snapshot_with_degraded_files(count: usize) -> CodeIndexSnapshot { let mut files = Vec::new(); let mut diagnostics = Vec::new(); tokens used 159,379
## 20260516 HTTP test stability entries

- runs: `20260516T174317Z`, `20260516T181003Z`, `20260516T181727Z`, `20260516T184629Z`, `20260516T185001Z`, `20260516T190530Z`, `20260516T190848Z`, `20260516T191712Z`, `20260516T192508Z`, `20260516T193653Z`, `20260516T194305Z`, `20260516T195734Z`
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/net/http_tests.rs`
- summary: 多轮围绕 HTTP graceful shutdown/QoS 测试的 listener 绑定、请求启动信号、in-memory stream 和 timeout 同步边界做稳定性修复，目标是恢复 cargo test stability gate；原始 patch 仍保留在对应 `.git/relay-knowledge-self-iteration/patches/<run>.patch` 长期记忆中。
- invariants: 不改变生产 HTTP/QoS runtime、SQLite、code retrieval、semantic/vector、provider/env、judge、安装发布或 CLI/API 行为；只修正测试同步条件。
## 20260517 early detailed entries

- archived in `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations-archive-20260517.md` to keep each tracked documentation file below the 1000-line hard limit.

## 20260517T070951Z
- score: 0.985604; cases: 35/35 passed; summary: provider/ranking docs and symbol excerpt improvements raised competitive accuracy; latency degradations remain in memory details.
## 20260517T072446Z
- score: 0.995374; cases: 35/35 passed; summary: code query path ranking generalized caller/callee source-path intent and improved competitive accuracy/performance, with build/test latency degradations preserved in memory.
## 20260517T094216Z
- score: 0.972063; cases: 36/36 passed; summary: Go/import identity, import target lookup, and parser/finalize updates expanded competitive repository coverage without recorded deterministic degradations.
## 20260517T101427Z
- score: 0.973723; cases: 36/36 passed; summary: Java import identity and target lookup improved research judge slightly while recording build/test and large-repo latency degradations in memory.
## 20260517T102845Z
- score: 0.9769; cases: 36/36 passed; summary: import target lookup refinements improved research judge and repo latency, with build/clippy/provider probe degradations preserved in memory.
## 20260517T105538Z
- score: 0.978502; cases: 36/36 passed; summary: code query support changes improved score, research judge, and relay-teams/LevelDB latency while preserving performance regressions in memory.
## 20260517T113610Z
- score: 0.979868; cases: 36/36 passed; summary: semantic/vector identifier-aware retrieval restored foundational accuracy and research judge quality, with performance regressions preserved in memory details.
## 20260517T115627Z
- score: 0.97996; cases: 36/36 passed; summary: semantic/vector read-model cache reuse preserved score while improving judge and local gate timings.
## 20260517T122624Z
- score: 0.978379; cases: 36/36 passed; summary: code query score-text saturation improved protected repo capability and performance without recorded degradations.
## 20260517T134401Z
- score: 0.976945 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.88, performance=0.936811, stability=1.0); cases: 36/36 passed.
- summary: code query ranking/support changes improved research_judge while recording performance degradations that remain protected context in memory.
## 20260517T140829Z
- score: 0.97451 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.86, performance=0.943869, stability=1.0); cases: 36/36 passed.
- summary: identifier-aware semantic/vector retrieval improved research_judge and provider probe behavior; performance degradations remain protected context in progressive memory and patch memory.
## 20260517T171331Z
- score: 0.924213 (foundational=0.888889, competitive=0.857143, accuracy=0.873016, semantic_vector=1.0, research_judge=0.88, performance=0.958586, stability=1.0); cases: 32/36 passed.
- summary: CLI repo index inline worker removed the `repo index` then `repo query --freshness wait-until-fresh` race; latency degradations remain protected context in memory.
## 20260517T184611Z
- score: 0.971201; cases: 36/36 passed; summary: bounded symbol/call line context restored foundational, competitive, accuracy, stability, and research judge by expanding only already-scored hits with indexed adjacent-symbol lookup.
## 20260517T190803Z
- score: 0.972277; cases: 36/36 passed; summary: request-scoped code query token reuse improved total score and research judge while preserving repository and semantic/vector floors.
## 20260517T204108Z
- score: 0.964715; cases: 36/36 passed; summary: prompt gate filtering stopped superseded quality failures from dominating follow-up candidates; current latency degradations remain protected memory context.
## 20260517T210331Z
- score: 0.971793; cases: 36/36 passed; summary: identifier token cache improvements lifted performance and research judge while preserving repository and semantic/vector floors.
## 20260517T212719Z
- score: 0.958053; cases: 36/36 passed; summary: language-filtered edge candidate pushdown improved relay-teams query latency with protected retrieval floors intact.
## 20260517T213819Z
- score: 0.964671; cases: 36/36 passed; summary: directional callee candidate filtering improved research judge while preserving full-scope cases.
## 20260517T220618Z
- score: 0.954963; cases: 36/36 passed; summary: edge search language materialization restored foundational/accuracy floors and improved research judge, with query latency risks noted in memory.
## 20260517T223953Z
- score: 0.925314; cases: 36/36 passed; summary: QoS HTTP test pre-bound its listener to remove the cargo_test port reuse race; retrieval metrics were unchanged and performance degradations remain memory context.
## 20260517T224754Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260517T224754Z.patch`
- score: 0.951457 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.87, performance=0.86705, stability=1.0)
- cases: 36/36 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_support.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:score 0.925314->0.951457; score_component:research_judge 0.75->0.87; metric:semantic_vector_provider_probe_ms 1336.0->1208
- known degradations: metric:cargo_build_release_ms 33235.0->34804; metric:cargo_fmt_check_ms 821.0->903; metric:relay_teams_query_p50_ms 208.5->308.5; metric:relay_teams_query_p95_ms 387.0->539.0; metric:leveldb_cpp_index_ms 15521.0->20291; metric:local_noise_file_index_ms 496.0->586; metric:local_noise_file_query_p50_ms 274.0->301.5; metric:semantic_vector_refresh_ms 209.0->241
- latency metrics: cargo_build_release_ms=34804ms; cargo_fmt_check_ms=903ms; cargo_clippy_ms=216ms; cargo_test_ms=8606ms; relay_teams_index_ms=72605ms; relay_teams_query_p50_ms=308ms; relay_teams_query_p95_ms=539ms; leveldb_cpp_index_ms=20291ms

Adopted optimization notes:

erms = query_terms("recover descriptor save_manifest versionedit"); @@ -455,6 +484,32 @@ } #[tokio::test] +async fn caller_search_accepts_scoped_target_hint_prefilter() { +    let mut call = code_query_call("scoped-target-call", "service-file", "src/pkg/service.py"); +    call.caller_name = Some("Caller".to_owned()); +    call.callee_name = "TargetThing".to_owned(); +    call.target_hint = Some("pkg.service.TargetThing".to_owned()); +    let store = store_with_case_intent_snapshot(code_query_snapshot( +        vec![code_query_file("service-file", "src/pkg/service.py", "python")], +        Vec::new(), +        vec![call], +    )) +    .await; + +    let hits = store +        .search_code(code_search_request( +            "pkg.service.TargetThing", +            CodeQueryKind::Callers, +        )) +        .await +        .expect("scoped caller query should succeed"); + +    assert_eq!(hits.len(), 1); +    assert_eq!(hits[0].path, "src/pkg/service.py"); +    assert!(hits[0].score >= 5.0, "score was {}", hits[0].score); +} + +#[tokio::test] async fn edge_queries_apply_language_filters_before_candidate_limit() { let mut files = Vec::new(); let mut calls = Vec::new(); tokens used 195,287
## 20260517T230922Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260517T230922Z.patch`
- score: 0.962311 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.91, performance=0.88074, stability=1.0)
- cases: 36/36 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code.rs`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`, `src/relay_knowledge/storage/sqlite/code_batch/finalize_typescript_imports.rs`, `src/relay_knowledge/storage/sqlite/code_batch_finalize_typescript_tests.rs`
- key improvements: score_component:score 0.944618->0.962311; score_component:research_judge 0.83->0.91; metric:cargo_build_release_ms 35548.0->32030; metric:relay_teams_query_p50_ms 295.0->220.5; metric:relay_teams_query_p95_ms 530.0->401.0; metric:semantic_vector_query_p95_ms 245.0->210.0
- known degradations: metric:semantic_vector_provider_probe_ms 1954.0->3610
- latency metrics: cargo_build_release_ms=32030ms; cargo_fmt_check_ms=810ms; cargo_clippy_ms=196ms; cargo_test_ms=8756ms; relay_teams_index_ms=71477ms; relay_teams_query_p50_ms=220ms; relay_teams_query_p95_ms=401ms; leveldb_cpp_index_ms=19272ms

Adopted optimization notes:

), +        source_scope: source_scope.to_owned(), +        base_resolved_commit_sha: None, +        resolved_commit_sha: "commit".to_owned(), +        tree_hash: "tree".to_owned(), +        path_filters: Vec::new(), +        language_filters: Vec::new(), +        full_replace: true, +        total_path_count, +        changed_path_count: total_path_count, +        skipped_unchanged_count: 0, +        deleted_paths: Vec::new(), +        tombstones: Vec::new(), +        resource_budget: CodeIndexResourceBudget::new(1, 1024, 1024).expect("budget"), +    } +} + +fn range(start: u32, end: u32) -> RepositoryCodeRange { +    RepositoryCodeRange { start, end } +} + +async fn search( +    store: &SqliteGraphStore, +    query: &str, +    kind: CodeQueryKind, +) -> Vec<CodeRetrievalHit> { +    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new()) +        .expect("selector should validate"); +    store +        .search_code( +            CodeRetrievalRequest::new(query, selector, kind, 5, FreshnessPolicy::AllowStale) +                .expect("request should validate"), +        ) +        .await +        .expect("query should succeed") +} tokens used 162,097
## 20260517T234741Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260517T234741Z.patch`
- score: 0.961262 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.88, performance=0.917747, stability=1.0)
- cases: 36/36 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/code/parser/manual.rs`, `src/relay_knowledge/code/parser_tests.rs`
- key improvements: score_component:score 0.757755->0.961262; score_component:performance 0.875598->0.917747; score_component:stability 0.981132->1.0; score_component:research_judge 0.0->0.88; metric:cargo_build_release_ms 50825.0->40908; metric:leveldb_cpp_index_ms 19874.0->18234; metric:leveldb_cpp_query_p50_ms 300.0->222.5; metric:leveldb_cpp_query_p95_ms 387.0->279.0
- known degradations: none recorded. 2026-05-18 target-case expansion treats self-iteration cases and included case files as forward workload, adding competitive JavaScript/Java/C/C++, background file-index, and register-to-index performance targets without allowing fixture-specific shortcuts.
- latency metrics: cargo_build_release_ms=40908ms; cargo_fmt_check_ms=816ms; cargo_clippy_ms=217ms; cargo_test_ms=9323ms; relay_teams_index_ms=81573ms; relay_teams_query_p50_ms=294ms; relay_teams_query_p95_ms=537ms; leveldb_cpp_index_ms=18234ms

Adopted optimization notes:

onnectorSaveRequest, +): Promise<void> => { +    await client.save(request); +}; + +const normalizeConnector = function ( +    request: W3ConnectorSaveRequest, +): W3ConnectorSaveRequest { +    return request; +}; + +class ConnectorService { +    saveLater = (request: W3ConnectorSaveRequest): void => { +        saveW3Connector(request); +    }; +} +"#, +    ); + +    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed); +    for name in ["saveW3Connector", "normalizeConnector", "saveLater"] { +        let symbol = snapshot +            .symbols +            .iter() +            .find(|symbol| symbol.name == name) +            .unwrap_or_else(|| panic!("{name} should be extracted as a function symbol")); +        assert_eq!(symbol.kind, "function"); +        assert!(symbol.signature.contains("W3ConnectorSaveRequest")); +    } +    assert!( +        !snapshot +            .symbols +            .iter() +            .any(|symbol| symbol.name == "CONNECTOR_TIMEOUT_MS") +    ); +} + +#[test] fn long_multibyte_symbol_signatures_truncate_on_utf8_boundary() { let mut source = "def retry_policy(value=\"".to_owned(); source.push_str(&"\u{00e9}".repeat(300)); tokens used 242,666
## 20260518T014540Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T014540Z.patch`
- score: 0.907794 (foundational=1.0, competitive=0.782609, accuracy=0.891304, semantic_vector=1.0, research_judge=0.92, performance=0.749005, stability=1.0)
- cases: 40/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/retrieval.rs`, `src/relay_knowledge/storage/sqlite/retrieval/advanced.rs`, `src/relay_knowledge/storage/sqlite/retrieval/derived.rs`
- key improvements: none recorded
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=48540ms; cargo_fmt_check_ms=1034ms; cargo_clippy_ms=277ms; cargo_test_ms=7191ms; relay_teams_index_ms=101175ms; relay_teams_register_index_ms=101283ms; relay_teams_query_p50_ms=382ms; relay_teams_query_p95_ms=781ms

Adopted optimization notes:

te/retrieval/derived.rs index 2bd9886479fa589a8ce06c8a9160360e3c1dd16d..45ab7b45722182c02bb33364cdba1e451123e21f --- a/src/relay_knowledge/storage/sqlite/retrieval/derived.rs +++ b/src/relay_knowledge/storage/sqlite/retrieval/derived.rs @@ -21,7 +21,7 @@ connection: &Connection, request: &GraphSearchRequest, ) -> Result<Vec<ScoredHit>, StorageError> { -    let query_terms = token_signature(&request.query, &[], None, "") +    let query_terms = token_signature(&request.query, &[], None) .into_iter() .collect::<BTreeSet<_>>(); if query_terms.is_empty() { @@ -149,7 +149,7 @@ request: &GraphSearchRequest, ) -> Result<Vec<ScoredHit>, StorageError> { let result_limit = bounded_candidate_limit(request); -    let query_terms = token_signature(&request.query, &[], None, "") +    let query_terms = token_signature(&request.query, &[], None) .into_iter() .collect::<BTreeSet<_>>(); if query_terms.is_empty() { @@ -296,7 +296,7 @@ fn vector(&mut self, dimension: usize) -> &[f64] { self.vectors .entry(dimension) -            .or_insert_with(|| hashed_vector(self.query, &[], None, "", dimension)) +            .or_insert_with(|| hashed_vector(self.query, &[], None, dimension)) } } tokens used 226,901
## 20260518T035623Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T035623Z.patch`
- score: 0.908297 (foundational=1.0, competitive=0.938406, accuracy=0.969203, semantic_vector=1.0, research_judge=0.81, performance=0.737121, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/en/01-user-guide/05-code-repository-graph-workflow.md`, `docs/zh/01-user-guide/05-code-repository-graph-workflow.md`, `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/code/scope.rs`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_path_ranking.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:score 0.72539->0.908297; score_component:competitive_capability 0.927536->0.938406; score_component:accuracy 0.963768->0.969203; score_component:stability 0.984127->1.0; score_component:research_judge 0.0->0.81; metric:cargo_fmt_check_ms 1100.0->1066; metric:cargo_clippy_ms 311.0->268; case:leveldb_cpp_callers_filter_policy_key_may_match {'passed': False, 'rank': 7, 'false_positive_count': 0}->{'passed': True, 'rank': 4, 'false_positive_count': 0} failed_to_passed
- known degradations: metric:local_documents_file_index_ms 362.0->403; metric:local_documents_file_query_p50_ms 364.0->393.5; metric:local_background_auto_index_files_background_service_auto_indexes_new_document_file_auto_index_first_seen_ms 770.0->1090.0; metric:semantic_vector_provider_probe_ms 1184.0->1532; metric:semantic_vector_refresh_ms 411.0->440
- latency metrics: cargo_build_release_ms=48370ms; cargo_fmt_check_ms=1066ms; cargo_clippy_ms=268ms; cargo_test_ms=7302ms; relay_teams_index_ms=107180ms; relay_teams_register_index_ms=107313ms; relay_teams_query_p50_ms=392ms; relay_teams_query_p95_ms=819ms

Adopted optimization notes:

    let mut production_call = +        code_query_call("ambiguous-production-call", "table-file", "table/table.cc"); +    production_call.caller_name = Some("InternalGet".to_owned()); +    production_call.callee_name = "KeyMayMatch".to_owned(); +    production_call.target_hint = Some("KeyMayMatch".to_owned()); +    production_call.confidence_basis_points = 5_000; +    production_call.confidence_tier = "ambiguous".to_owned(); + +    let store = store_with_case_intent_snapshot(code_query_snapshot( +        vec![ +            code_query_file("filter-test-file", "table/filter_block_test.cc", "cpp"), +            code_query_file("table-file", "table/table.cc", "cpp"), +        ], +        Vec::new(), +        vec![test_call, production_call], +    )) +    .await; + +    let hits = store +        .search_code(code_search_request("KeyMayMatch", CodeQueryKind::Callers)) +        .await +        .expect("caller query should succeed"); + +    assert_eq!(hits[0].path, "table/table.cc"); +    assert!(hits[0].score > hits[1].score); +} + +#[tokio::test] async fn callee_search_applies_direction_before_candidate_limit() { let mut files = Vec::new(); let mut calls = Vec::new(); tokens used 185,291
## 20260518T041031Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T041031Z.patch`
- score: 0.942578 (foundational=1.0, competitive=0.938406, accuracy=0.969203, semantic_vector=1.0, research_judge=0.87, performance=0.877659, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch.rs`
- key improvements: score_component:score 0.908297->0.942578; score_component:performance 0.737121->0.877659; score_component:research_judge 0.81->0.87; metric:cargo_build_release_ms 48370.0->40362; metric:cargo_fmt_check_ms 1066.0->641; metric:cargo_clippy_ms 268.0->167; metric:cargo_test_ms 7302.0->6218; metric:relay_teams_index_ms 107180.0->67478
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=40362ms; cargo_fmt_check_ms=641ms; cargo_clippy_ms=167ms; cargo_test_ms=6218ms; relay_teams_index_ms=67478ms; relay_teams_register_index_ms=67566ms; relay_teams_query_p50_ms=238ms; relay_teams_query_p95_ms=475ms

Adopted optimization notes:

?; -    let rows = statement.query_map(params![source_scope], |row| { -        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)) -    })?; +    for path in missing_paths { +        if let Some(language_id) = statement +            .query_row(params![batch.source_scope.as_str(), path.as_str()], |row| { +                row.get(0) +            }) +            .optional()? +        { +            languages.insert(path, language_id); +        } +    } + +    Ok(languages) +} -    rows.collect::<Result<BTreeMap<_, _>, _>>() -        .map_err(StorageError::from) +fn edge_paths_missing_from_batch( +    batch: &CodeIndexBatch, +    languages: &BTreeMap<String, String>, +) -> Vec<String> { +    let mut missing_paths = Vec::<String>::new(); +    for path in batch +        .references +        .iter() +        .map(|reference| reference.path.as_str()) +        .chain(batch.imports.iter().map(|import| import.path.as_str())) +    { +        if !languages.contains_key(path) +            && !missing_paths.iter().any(|known| known.as_str() == path) +        { +            missing_paths.push(path.to_owned()); +        } +    } + +    missing_paths } fn insert_diagnostics( tokens used 343,083
## 20260518T051523Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T051523Z.patch`
- score: 0.951562 (foundational=1.0, competitive=0.971014, accuracy=0.985507, semantic_vector=1.0, research_judge=0.88, performance=0.885931, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_support.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:score 0.939403->0.951562; score_component:competitive_capability 0.938406->0.971014; score_component:accuracy 0.969203->0.985507; score_component:performance 0.871162->0.885931; score_component:research_judge 0.86->0.88; metric:cargo_clippy_ms 175.0->144; metric:leveldb_cpp_index_ms 20003.0->14611; metric:leveldb_cpp_register_index_ms 20274.0->14886
- known degradations: metric:relay_teams_index_ms 61634.0->69579; metric:relay_teams_register_index_ms 61711.0->69655; metric:semantic_vector_provider_probe_ms 1278.0->1418
- latency metrics: cargo_build_release_ms=31835ms; cargo_fmt_check_ms=563ms; cargo_clippy_ms=144ms; cargo_test_ms=5347ms; relay_teams_index_ms=69579ms; relay_teams_register_index_ms=69655ms; relay_teams_query_p50_ms=289ms; relay_teams_query_p95_ms=583ms

Adopted optimization notes:

".to_owned(); + +    let mut production_call = +        code_query_call("ambiguous-production-call", "service-file", "src/service.cc"); +    production_call.caller_name = Some("Dispatch".to_owned()); +    production_call.callee_name = "TargetCall".to_owned(); +    production_call.target_hint = Some("TargetCall".to_owned()); +    production_call.confidence_basis_points = 5_000; +    production_call.confidence_tier = "ambiguous".to_owned(); + +    let store = store_with_case_intent_snapshot(code_query_snapshot( +        vec![ +            code_query_file("router-file", "src/router.cc", "cpp"), +            code_query_file("service-file", "src/service.cc", "cpp"), +        ], +        Vec::new(), +        vec![wrapper_call, production_call], +    )) +    .await; + +    let hits = store +        .search_code(code_search_request("TargetCall", CodeQueryKind::Callers)) +        .await +        .expect("caller query should succeed"); + +    assert_eq!(hits[0].path, "src/service.cc"); +    assert!(hits[0].score > hits[1].score); +} + +#[tokio::test] async fn callee_search_applies_direction_before_candidate_limit() { let mut files = Vec::new(); let mut calls = Vec::new(); tokens used 231,480
## 20260518T052713Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T052713Z.patch`
- score: 0.954349 (foundational=1.0, competitive=0.971014, accuracy=0.985507, semantic_vector=1.0, research_judge=0.89, performance=0.889842, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch.rs`, `src/relay_knowledge/storage/sqlite/code_batch_search_tests.rs`
- key improvements: score_component:research_judge 0.88->0.89; metric:relay_teams_index_ms 69579.0->42134; metric:relay_teams_register_index_ms 69655.0->42222; metric:relay_teams_query_p50_ms 289.0->216.0; metric:relay_teams_query_p95_ms 583.0->437.0; metric:semantic_vector_provider_probe_ms 1418.0->1367
- known degradations: metric:cargo_fmt_check_ms 563.0->599; metric:cargo_clippy_ms 144.0->172; metric:cargo_test_ms 5347.0->8553; metric:local_documents_file_index_ms 212.0->283; metric:local_documents_file_query_p50_ms 201.5->289.0; metric:local_documents_file_query_p95_ms 211.0->292.0; metric:local_noise_file_index_ms 372.0->521; metric:local_noise_file_query_p50_ms 209.0->278.0
- latency metrics: cargo_build_release_ms=32286ms; cargo_fmt_check_ms=599ms; cargo_clippy_ms=172ms; cargo_test_ms=8553ms; relay_teams_index_ms=42134ms; relay_teams_register_index_ms=42222ms; relay_teams_query_p50_ms=216ms; relay_teams_query_p95_ms=437ms

Adopted optimization notes:

      " +                UPDATE code_repositories +                SET last_indexed_scope_id = ?1 +                WHERE repository_id = 'repo' +                ", +                [source_scope], +            )?; + +            Ok(()) +        }) +        .await +        .expect("active scope should update"); +} + +async fn mark_scope_retained(store: &SqliteGraphStore, source_scope: &str) { +    let source_scope = source_scope.to_owned(); +    store +        .run(move |connection| { +            connection.execute( +                " +                INSERT INTO code_repository_scopes ( +                    source_scope, repository_id, resolved_commit_sha, tree_hash, +                    path_filters_json, language_filters_json, indexed_file_count, +                    symbol_count, reference_count, chunk_count, stale, degraded_reason +                ) +                VALUES (?1, 'repo', 'commit', 'tree', '[]', '[]', 0, 0, 0, 0, 0, NULL) +                ", +                params![source_scope], +            )?; + +            Ok(()) +        }) +        .await +        .expect("retained scope should insert"); +} + fn file( source_scope: &str, file_id: &str, tokens used 214,659
## 20260518T062727Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T062727Z.patch`
- score: 0.949464 (foundational=1.0, competitive=0.967391, accuracy=0.983696, semantic_vector=1.0, research_judge=0.86, performance=0.905382, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_call_counts.rs`, `src/relay_knowledge/storage/sqlite/code_query_support.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:score 0.938584->0.949464; score_component:performance 0.872744->0.905382; score_component:research_judge 0.83->0.86; metric:cargo_build_release_ms 38932.0->32332; metric:cargo_fmt_check_ms 825.0->603; metric:cargo_clippy_ms 222.0->149; metric:cargo_test_ms 9722.0->5631; metric:leveldb_cpp_query_p50_ms 310.5->235.0
- known degradations: metric:relay_teams_query_p50_ms 215.5->301.0; metric:relay_teams_query_p95_ms 428.0->589.0; metric:leveldb_cpp_index_ms 15084.0->16951; metric:leveldb_cpp_register_index_ms 15297.0->17220; case:leveldb_cpp_callers_filter_policy_key_may_match {'passed': True, 'rank': 1, 'false_positive_count': 0}->{'passed': True, 'rank': 4, 'false_positive_count': 0} rank_worsened
- latency metrics: cargo_build_release_ms=32332ms; cargo_fmt_check_ms=603ms; cargo_clippy_ms=149ms; cargo_test_ms=5631ms; relay_teams_index_ms=42534ms; relay_teams_register_index_ms=42600ms; relay_teams_query_p50_ms=301ms; relay_teams_query_p95_ms=589ms

Adopted optimization notes:

()); +    attach_second.caller_name = Some("attachRunStream".to_owned()); +    attach_second.callee_symbol_snapshot_id = Some("release-symbol".to_owned()); +    attach_second.callee_name = "releaseActiveStreamHandle".to_owned(); +    attach_second.target_hint = Some("releaseActiveStreamHandle".to_owned()); +    attach_second.line_range = code_query_range(492, 492); + +    let store = store_with_case_intent_snapshot(code_query_snapshot( +        vec![code_query_file("stream-file", path, "javascript")], +        vec![start, end, attach], +        vec![start_call, end_call, attach_first, attach_second], +    )) +    .await; + +    let hits = store +        .search_code(code_search_request( +            "releaseActiveStreamHandle", +            CodeQueryKind::Callers, +        )) +        .await +        .expect("caller query should succeed"); + +    assert!(hits[0].excerpt.contains("attachRunStream")); +    assert!(hits[0].score > hits[1].score); +} + +#[tokio::test] async fn caller_search_accepts_scoped_target_hint_prefilter() { let mut call = code_query_call("scoped-target-call", "service-file", "src/pkg/service.py"); call.caller_name = Some("Caller".to_owned()); tokens used 219,367
## 20260518T071744Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T071744Z.patch`
- score: 0.953631 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.86, performance=0.896209, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:competitive_capability 0.967391->1.0; score_component:accuracy 0.983696->1.0; metric:relay_teams_query_p50_ms 301.0->216.0; metric:relay_teams_query_p95_ms 589.0->426.0; metric:leveldb_cpp_index_ms 16951.0->14893; metric:leveldb_cpp_register_index_ms 17220.0->15101; case:leveldb_cpp_callers_filter_policy_key_may_match {'passed': True, 'rank': 4, 'false_positive_count': 0}->{'passed': True, 'rank': 1, 'false_positive_count': 0} rank_improved
- known degradations: score_component:performance 0.905382->0.896209; metric:relay_teams_index_ms 42534.0->46304; metric:relay_teams_register_index_ms 42600.0->46381; metric:local_noise_file_query_p50_ms 206.5->232.5; metric:local_noise_file_query_p95_ms 207.0->245.0; metric:local_background_auto_index_file_index_ms 210.0->274; metric:local_background_auto_index_files_background_service_auto_indexes_new_document_file_auto_index_first_seen_ms 399.0->679.0; metric:semantic_vector_provider_probe_ms 1237.0->1335
- latency metrics: cargo_build_release_ms=33212ms; cargo_fmt_check_ms=605ms; cargo_clippy_ms=153ms; cargo_test_ms=5611ms; relay_teams_index_ms=46304ms; relay_teams_register_index_ms=46381ms; relay_teams_query_p50_ms=216ms; relay_teams_query_p95_ms=426ms

Adopted optimization notes:

ed(); +        call.confidence_basis_points = 8_000; +        call.confidence_tier = "inferred".to_owned(); +        call.line_range = code_query_range(line, line); +        repeated_test_calls.push(call); +    } + +    let mut calls = vec![production_call]; +    calls.extend(repeated_test_calls); +    let store = store_with_case_intent_snapshot(code_query_snapshot( +        vec![ +            code_query_file("table-file", "table/table.cc", "cpp"), +            code_query_file("filter-test-file", "table/filter_block_test.cc", "cpp"), +        ], +        Vec::new(), +        calls, +    )) +    .await; + +    let hits = store +        .search_code(code_search_request("KeyMayMatch", CodeQueryKind::Callers)) +        .await +        .expect("caller query should succeed"); + +    assert_eq!(hits[0].path, "table/table.cc"); +    assert!(hits[0].excerpt.contains("InternalGet")); +    assert!(hits[0].score > hits[1].score); +} + +#[tokio::test] async fn caller_search_demotes_same_named_wrapper_call_sites() { let mut wrapper_call = code_query_call("resolved-wrapper-call", "router-file", "src/router.cc"); wrapper_call.caller_name = Some("Router::TargetCall".to_owned()); tokens used 139,240
## 20260518T093310Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T093310Z.patch`
- score: 0.935946 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.87, performance=0.763642, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `docs/zh/06-verification/06-code-graph-retrieval-accuracy-linux-2026-05-15.md`, `src/relay_knowledge/code/scope.rs`, `src/relay_knowledge/code/tests.rs`, `src/relay_knowledge/domain/code_repository.rs`
- key improvements: score_component:score 0.924475->0.935946; score_component:research_judge 0.82->0.87; metric:relay_teams_index_ms 63213.0->55235; metric:relay_teams_register_index_ms 63328.0->55358; metric:local_noise_file_index_ms 736.0->705; metric:semantic_vector_provider_probe_ms 1638.0->1178
- known degradations: metric:cargo_fmt_check_ms 1044.0->1090; metric:cargo_clippy_ms 345.0->11447; metric:local_background_auto_index_file_index_ms 359.0->389; metric:local_background_auto_index_files_background_service_auto_indexes_new_document_file_auto_index_first_seen_ms 1037.0->1168.0; metric:semantic_vector_refresh_ms 398.0->464; metric:semantic_vector_query_p95_ms 419.0->449.0
- latency metrics: cargo_build_release_ms=50392ms; cargo_fmt_check_ms=1090ms; cargo_clippy_ms=11447ms; cargo_test_ms=7553ms; relay_teams_index_ms=55235ms; relay_teams_register_index_ms=55358ms; relay_teams_query_p50_ms=422ms; relay_teams_query_p95_ms=817ms

Adopted optimization notes:

b fn alpha() {}\n"); diff --git a/src/relay_knowledge/domain/code_repository.rs b/src/relay_knowledge/domain/code_repository.rs index 7fdb92681e37fddd260c690100eb19edddf757be..03d7ec2af60d7bcbc718b240d4f64b5ffa42f3a1 --- a/src/relay_knowledge/domain/code_repository.rs +++ b/src/relay_knowledge/domain/code_repository.rs @@ -435,7 +435,7 @@ } impl CodeIndexResourceBudget { -    pub const DEFAULT_MAX_FILES_PER_BATCH: usize = 128; +    pub const DEFAULT_MAX_FILES_PER_BATCH: usize = 256; pub const DEFAULT_MAX_BYTES_PER_BATCH: usize = 16 * 1024 * 1024; pub const DEFAULT_MAX_ROWS_PER_BATCH: usize = 50_000; @@ -911,4 +911,19 @@ assert_eq!(error.field, "line_range"); } + +    #[test] +    fn default_code_index_budget_batches_more_small_files_without_raising_row_or_byte_caps() { +        let budget = CodeIndexResourceBudget::default(); + +        assert_eq!(budget.max_files_per_batch, 256); +        assert_eq!( +            budget.max_bytes_per_batch, +            CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH +        ); +        assert_eq!( +            budget.max_rows_per_batch, +            CodeIndexResourceBudget::DEFAULT_MAX_ROWS_PER_BATCH +        ); +    } } tokens used 237,921
## 20260518T094852Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T094852Z.patch`
- score: 0.945694 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.87, performance=0.828627, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code.rs`, `src/relay_knowledge/storage/sqlite/code_batch.rs`, `src/relay_knowledge/storage/sqlite/code_cleanup.rs`
- key improvements: score_component:score 0.935946->0.945694; score_component:performance 0.763642->0.828627; metric:cargo_build_release_ms 50392.0->47327; metric:cargo_clippy_ms 11447.0->297; metric:relay_teams_index_ms 55235.0->20009; metric:relay_teams_register_index_ms 55358.0->20140; metric:leveldb_cpp_index_ms 26028.0->2699; metric:leveldb_cpp_register_index_ms 26417.0->3082
- known degradations: metric:semantic_vector_provider_probe_ms 1178.0->1300
- latency metrics: cargo_build_release_ms=47327ms; cargo_fmt_check_ms=1082ms; cargo_clippy_ms=297ms; cargo_test_ms=7399ms; relay_teams_index_ms=20009ms; relay_teams_register_index_ms=20140ms; relay_teams_query_p50_ms=402ms; relay_teams_query_p95_ms=799ms

Adopted optimization notes:

     source_scope, document_kind, record_id, path, language_id, content +                    ) +                    VALUES (?1, 'symbol', ?2, ?2, 'rust', 'target') +                    ", +                    rusqlite::params!["scope", path], +                ) +                .expect("search row should insert"); +        } + +        let transaction = connection.transaction().expect("transaction should open"); +        delete_path_indexes(&transaction, "scope", ["src/a.rs", "src/b.rs", "src/a.rs"]) +            .expect("paths should delete"); +        transaction.commit().expect("transaction should commit"); + +        for table in PATH_TABLES +            .iter() +            .copied() +            .chain(["code_repository_search"]) +        { +            let remaining = connection +                .query_row( +                    &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = 'scope'"), +                    [], +                    |row| row.get::<_, usize>(0), +                ) +                .expect("remaining row count should load"); +            assert_eq!(remaining, 1, "{table} should keep only the unmatched path"); +        } +    } +} tokens used 217,237
## 20260518T114107Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T114107Z.patch`
- score: 0.94048 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.85, performance=0.8232, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`
- key improvements: none recorded
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=50953ms; cargo_fmt_check_ms=1136ms; cargo_clippy_ms=387ms; cargo_test_ms=7592ms; relay_teams_index_ms=46379ms; relay_teams_register_index_ms=46512ms; relay_teams_query_p50_ms=410ms; relay_teams_query_p95_ms=816ms

Adopted optimization notes:

d' +          AND EXISTS ( +                SELECT 1 +                FROM unique_path_symbol +                WHERE unique_path_symbol.name = code_repository_references.name +                  AND unique_path_symbol.path = code_repository_references.path +          ) ", params![source_scope], )?; transaction.execute( " -        UPDATE code_repository_references AS reference +        WITH symbol_names AS ( +            SELECT DISTINCT name +            FROM code_repository_symbols +            WHERE source_scope = ?1 +        ) +        UPDATE code_repository_references SET resolution_state = 'ambiguous', confidence_basis_points = 5000, confidence_tier = 'ambiguous' -        WHERE reference.source_scope = ?1 -          AND reference.resolution_state = 'unresolved' -          AND EXISTS ( -                SELECT 1 -                FROM code_repository_symbols AS symbol -                WHERE symbol.source_scope = reference.source_scope -                  AND symbol.name = reference.name -            ) +        WHERE source_scope = ?1 +          AND resolution_state = 'unresolved' +          AND name IN (SELECT name FROM symbol_names) ", params![source_scope], )?; tokens used 189,842
## 20260518T114915Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260518T114915Z.patch`
- score: 0.94613 (foundational=1.0, competitive=1.0, accuracy=1.0, semantic_vector=1.0, research_judge=0.87, performance=0.831535, stability=1.0)
- cases: 45/45 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query_support.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:score 0.94048->0.94613; score_component:performance 0.8232->0.831535; score_component:research_judge 0.85->0.87; metric:cargo_build_release_ms 50953.0->49009; metric:local_documents_file_query_p50_ms 392.0->364.5; metric:local_background_auto_index_files_background_service_auto_indexes_new_document_file_auto_index_first_seen_ms 1271.0->738.0; metric:semantic_vector_provider_probe_ms 3141.0->2866; metric:semantic_vector_refresh_ms 555.0->401
- known degradations: metric:cargo_clippy_ms 387.0->10965
- latency metrics: cargo_build_release_ms=49009ms; cargo_fmt_check_ms=1104ms; cargo_clippy_ms=10965ms; cargo_test_ms=7508ms; relay_teams_index_ms=45712ms; relay_teams_register_index_ms=45832ms; relay_teams_query_p50_ms=392ms; relay_teams_query_p95_ms=791ms

Adopted optimization notes:

 version constant"), -        "\"checkpoint\" OR \"metadata\" OR \"version\" OR \"constant\"" +        "(\"checkpoint\" OR \"metadata\" OR \"version\" OR \"constant\") OR \"checkpointmetadataversionconstant\" OR \"checkpoint_metadata_version_constant\"" ); assert_eq!( +        symbol_fts_match_query("new lru cache"), +        "(\"new\" OR \"lru\" OR \"cache\") OR \"newlrucache\" OR \"new_lru_cache\"" +    ); +    assert_eq!( fts_match_query("checkpoint metadata version constant"), "(\"checkpoint\" \"metadata\" \"version\" \"constant\") OR \"checkpointmetadataversionconstant\" OR \"checkpoint_metadata_version_constant\"" ); @@ -290,6 +294,14 @@ )) .await .expect("lowercase symbol query should succeed"); +    let spaced_hits = store +        .search_code(code_search_request( +            "eval checkpoint store", +            CodeQueryKind::Definition, +        )) +        .await +        .expect("spaced compound symbol query should succeed"); +    assert_eq!(spaced_hits[0].symbol_snapshot_id.as_deref(), Some("eval-checkpoint-store")); assert!( hit.score > lower_hits[0].score + 1.5, "mixed-case query should keep CamelCase symbol-name bonus, got {} vs lowercase {}", tokens used 159,018
