# 自迭代采纳优化记录归档 20260803

本文件保存主记录在 2026-08-03 为满足 tracked documentation 1000 行硬上限而迁出的历史段落。原始 patch、报告和渐进式记忆仍保留在 `.git/relay-knowledge-self-iteration/`。

## run-1780397808-to-run-1780398992 compacted

- summary: accepted Rust workflow chunk-first (`run-1780397808`, score 0.978577, 108/108 cases) and import query shape scoring (`run-1780398992`, score 0.979356, 108/108 cases) preserved foundational=1.000000, semantic_vector=1.000000, and stability=1.000000 while improving TypeScript dynamic import rank from 4 to 1 and competitive capability to 0.996795. Full patches, changed paths, improvements, degradations, and latency metrics remain in `.git/relay-knowledge-self-iteration/patches-v2/`, reports, and progressive memory.
- performance memory: `run-1780398992` improved competitive capability but recorded performance=0.889226 with software-global, LevelDB, index-performance, and project-alias latency regressions. Later candidates should prefer general read-path or planning work reduction over more local scorer-only import tweaks.

## run-1780401183 compacted

- score: 0.980895 with 108/108 cases passed; Hybrid direct-evidence coverage gate improved performance from 0.875658 to 0.897779 while preserving foundational=1.000000, semantic_vector=1.000000, and stability=1.000000. Full patch, metrics, improvements, and latency degradations remain in `.git/relay-knowledge-self-iteration/patches-v2/run-1780401183.patch`, its report, and progressive memory.

## 2026-06-03 exact-symbol-miss, read-model outage, and fact-version scan

- 算法/架构：Symbol/Definition 单标识符查询先执行精确 `name = ?` lookup；若未饱和且无直接命中，直接返回空结果，不再启动宽 FTS 证明负例。Hybrid chunk-first 在 FTS/read-model 暂时不可用时可用既有 graph/API identity rows 或 bounded symbol-table LIKE 候选继续返回有 `degraded_reason` 的结构化命中，source fallback 对无 indexed hit、无 path anchor 的精确定义 miss 直接退出，SQLite read pool busy timeout 降为固定短等待；`latest_repository_scope_status` 扫描候选时同步验证 `code_snapshot_expected_scope_id`，跳过 legacy fact-version scope 后继续寻找 current scope。自迭代 harness 评分同步升级为 `base_score + capability_ceiling_bonus` 的动态天花板策略，macro explore prompt 改为有边界 biological mutation，research judge 要求完整维度 JSON 和 `min_dimension_score`。
- 不变量/预期影响/风险：不改变 parser facts、SQLite schema、FTS 写入、candidate 上限、任务 lease/checkpoint、repo-set overlay、semantic/vector read model、env/paths/net、QoS 或安装发布；Hybrid 仍只在 bounded candidates 与 dense evidence gate 下提前返回，Symbol/Definition 精确 kind 与无锚点 source fallback 不再用宽文本召回补空。自迭代策略变更只影响独立 `tools/self_iteration` harness、cases 与文档，保留 failed gate、missing diff、受保护目标回退和 anti-fixture 约束；预期让高基线阶段的 competitive/research/performance 突破继续产生有界采纳信号，并让 LLM judge 输出可审计维度证据。风险是 judge 配置不完整时更早失败，受 self-iteration unit tests、README 参数覆盖测试和 judge dimension tests 控制。

## run-1780646058817570176-validate

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches-v2/run-1780645380018333834-explore.patch`
- score: 1.000000 (foundational=1.000000, competitive=1.000000, accuracy=1.000000, semantic_vector=1.000000, research_judge=n/a, performance=0.993317, stability=1.000000)
- cases: 62/62 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/graph_tests.rs`, `src/relay_knowledge/storage/sqlite/retrieval/derived.rs`
- key improvements: none recorded
- known degradations: none recorded
- latency metrics: cargo_fmt_check_ms=4835ms; self_iteration_cargo_fmt_check_ms=463ms; linux_glibc_compatibility_policy_ms=161ms; skill_metadata_policy_cases_ms=322ms; cargo_build_debug_ms=31774ms; self_iteration_cargo_check_ms=685ms; code_index_recovery_cases_ms=8897ms; code_index_sqlite_lock_cases_ms=8771ms

Adopted optimization notes:

Rust self-iteration v2 accepted this candidate through the independent tools/self_iteration harness. The candidate is expected to improve the general retrieval, indexing, evaluation, or harness behavior described by the changed paths and recorded metrics.

## 2026-06-05 research self-iteration planning mode

- 算法/架构：`tools/self_iteration` 新增只读 `research-plan` 模式，把本次 arXiv、X.com、Reddit、开源项目与系统工程深度研究流程抽象为 Markdown 计划，覆盖来源台账、可信度分层、综合矩阵、竞品 issue 拆解、双语文档、归档验证和远端 main 发布证据。
- 不变量/预期影响/风险：该模式不调用 Codex、不运行评估、不写 self-iteration 历史、不修改产品 CLI/API、索引、存储、网络或发布行为；后续 research 迭代可先生成计划底稿，再按来源真实、issue 独立可验收和文档归档完整性推进。风险主要是输出模板过于保守，由 harness 单元测试和双语 README 参数覆盖约束。

## 2026-08-02 冷索引与增量索引分钟级性能评估

- 目标与测量修复：Codex 候选生成默认升级为 `gpt-5.6-sol` + `xhigh`；性能 fixture 使用每仓隔离 runtime home，并验证 cold task/parsed-file 完成证据，禁止把缓存命中的 `changed_path_count=0` no-op 计作冷索引改善。
- 算法：Git 冷索引先直接尝试有界 `cat-file --batch`，仅在缺失对象或 submodule 路径时回退 batch-check/逐路径读取；普通 Git 增量变更在 512 文件、16 MiB 默认预算内预取 blob，避免每个变更文件启动一次 `git show`。
- 回归面：1024 文件 fast fixture 和 2048 文件 full fixture 都创建第二个含修改、新增、删除的提交，执行 `repo update`，记录 `*_cold_index_ms`、`*_cold_register_index_ms`、`*_incremental_index_ms`，并限制 delta blob read/parse 数量；PR benchmark workflow 会直接审计 JSON 完成证据和预算。
- 架构不变量与风险：任务租约、单仓单 writer、checkpoint、重试、资源预算、FTS/边终结、freshness 和查询事实不变；额外内存严格受现有 batch 文件/字节上限约束。submodule 或 batch 读取失败仍走原有有界回退，风险由 Git/submodule 测试、增量 batch-read 测试和真实 CLI 性能 gate 覆盖。

## run-1785677193

- patch: `/opt/workspace/relay-knowledge/.git/relay-knowledge-self-iteration/patches-v2/run-1785677193.patch`
- score: 0.986247 (foundational=1.000000, competitive=0.993590, accuracy=0.996795, semantic_vector=1.000000, research_judge=n/a, performance=0.931431, stability=1.000000)
- cases: 109/109 passed
- changed paths: `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`, `src/relay_knowledge/application/code_repository/indexing/mod.rs`
- key improvements: none recorded
- known degradations: none recorded
- latency metrics: cargo_fmt_check_ms=4516ms; self_iteration_cargo_fmt_check_ms=504ms; linux_glibc_compatibility_policy_ms=121ms; skill_metadata_policy_cases_ms=243ms; cargo_build_debug_ms=263ms; self_iteration_cargo_check_ms=464ms; code_index_recovery_cases_ms=1228ms; code_index_sqlite_lock_cases_ms=946ms

Adopted optimization notes:

Rust self-iteration v2 accepted this candidate through the independent tools/self_iteration harness. The candidate is expected to improve the general retrieval, indexing, evaluation, or harness behavior described by the changed paths and recorded metrics.
