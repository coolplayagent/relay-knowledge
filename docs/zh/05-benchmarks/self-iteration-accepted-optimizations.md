# 自迭代采纳优化记录

本文档由自迭代 harness 在候选通过质量门禁并被采纳时追加，用于把本轮采用的优化思路传递给后续 Codex 迭代。人工维护的总结可以继续补充在对应条目下。

## 记录格式

- `patch`: 本轮候选补丁在 `.git/relay-knowledge-self-iteration/patches/` 下的路径。
- `score`: 采纳时的总分和 accuracy、performance、stability 分项。
- `cases`: 采纳时通过的检索 case 数量。
- `changed paths`: 本轮变更的主要文件。
- `key improvements`: 相对上一轮改善的 case、gate 或 metric。
- `known degradations`: 相对上一轮已观测到的退化，后续迭代必须优先保护或修复。
- `Adopted optimization notes`: Codex 输出中提取的优化说明，用作下一轮 prompt 的上下文。

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
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/code/changes.rs`, `src/relay_knowledge/code/pipeline.rs`, `src/relay_knowledge/code/scope.rs`, `src/relay_knowledge/code/tests.rs`
- key improvements: score_component:score 0.804178->0.916555; score_component:accuracy 0.8->0.9; score_component:performance 0.758045->0.905184; score_component:stability 0.911765->1.0; case:leveldb_definition_db_open {'passed': False, 'rank': 2, 'false_positive_count': 0}->{'passed': True, 'rank': 1, 'false_positive_count': 0} failed_to_passed; case:leveldb_definition_write_batch_put {'passed': False, 'rank': 3, 'false_positive_count': 0}->{'passed': True, 'rank': 1, 'false_positive_count': 0} failed_to_passed
- known degradations: metric:cargo_build_release_ms 27983.0->40757; metric:cargo_fmt_check_ms 485.0->692; metric:cargo_clippy_ms 151.0->193; metric:cargo_test_ms 6336.0->7479; metric:relay_teams_index_ms 79134.0->81836; metric:relay_teams_query_p95_ms 10977.0->11574.0
- latency metrics: cargo_build_release_ms=40757ms; cargo_fmt_check_ms=692ms; cargo_clippy_ms=193ms; cargo_test_ms=7479ms; relay_teams_index_ms=81836ms; relay_teams_query_p50_ms=122ms; relay_teams_query_p95_ms=11574ms; leveldb_cpp_index_ms=19215ms

Adopted optimization notes:

dd", "."]); +    repo.git(["commit", "-m", "base"]); +    let budget = CodeIndexResourceBudget::new(128, "fn a() {}\nfn b() {}\n".len(), 50_000) +        .expect("budget should validate"); +    let plan = prepare_full_index_plan(repo.registration(), repo.selector(), budget) +        .expect("plan should prepare"); + +    let (plan, first_batch) = plan.parse_next_batch().expect("first batch should parse"); +    let (plan, second_batch) = plan.parse_next_batch().expect("second batch should parse"); +    let (_, third_batch) = plan.parse_next_batch().expect("third batch should parse"); + +    let first_batch = first_batch.expect("first batch should exist"); +    let second_batch = second_batch.expect("second batch should exist"); +    assert!(third_batch.is_none()); +    assert_eq!(first_batch.files.len(), 2); +    assert_eq!(first_batch.files[0].path, "src/a.rs"); +    assert_eq!(first_batch.files[1].path, "src/b.rs"); +    assert_eq!(second_batch.files.len(), 1); +    assert_eq!(second_batch.files[0].path, "src/c.rs"); +} + +#[test] fn explicit_default_exclusion_opt_in_supports_dataset_and_lock_paths() { let registration = CodeRepositoryRegistration::new( "repo", tokens used 165,514
## 20260516T122811Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T122811Z.patch`
- score: 0.916543 (accuracy=0.9, performance=0.905145, stability=1.0)
- cases: 18/20 passed
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`, `src/relay_knowledge/storage/sqlite/code_batch_finalize_tests.rs`, `src/relay_knowledge/storage/sqlite/code_schema.rs`
- key improvements: metric:cargo_fmt_check_ms 724.0->688; metric:relay_teams_query_p95_ms 12222.0->11662.0
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=49471ms; cargo_fmt_check_ms=688ms; cargo_clippy_ms=194ms; cargo_test_ms=7428ms; relay_teams_index_ms=81898ms; relay_teams_query_p50_ms=127ms; relay_teams_query_p95_ms=11662ms; leveldb_cpp_index_ms=19586ms

Adopted optimization notes:

            row.get::<_, u16>(3)?, +                        row.get::<_, String>(4)?, +                    ), +                )) +            })?; + +            rows.collect::<Result<BTreeMap<_, _>, _>>() +                .map_err(crate::storage::StorageError::from) +        }) +        .await +        .expect("reference rows should load") +} + fn file( source_scope: &str, file_id: &str, diff --git a/src/relay_knowledge/storage/sqlite/code_schema.rs b/src/relay_knowledge/storage/sqlite/code_schema.rs index f3aec34be9ed352e5e1013106078e718c3bc9168..d7b8cb2e9a40adc1e7c21eb825c6220fb8fd9877 --- a/src/relay_knowledge/storage/sqlite/code_schema.rs +++ b/src/relay_knowledge/storage/sqlite/code_schema.rs @@ -210,6 +210,8 @@ CREATE INDEX IF NOT EXISTS code_repository_symbols_lookup ON code_repository_symbols(source_scope, name, qualified_name, path); +        CREATE INDEX IF NOT EXISTS code_repository_symbols_name_path_lookup +            ON code_repository_symbols(source_scope, name, path); CREATE INDEX IF NOT EXISTS code_repository_references_lookup ON code_repository_references(source_scope, name, kind, path); CREATE INDEX IF NOT EXISTS code_repository_calls_lookup tokens used 143,069
## 20260516T124101Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T124101Z.patch`
- score: 0.916549 (accuracy=0.9, performance=0.905163, stability=1.0)
- cases: 18/20 passed
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`
- key improvements: metric:cargo_fmt_check_ms 724.0->684; metric:cargo_test_ms 8230.0->7430; metric:relay_teams_index_ms 86865.0->81070; metric:relay_teams_query_p95_ms 12179.0->11621.0; metric:leveldb_cpp_index_ms 21294.0->19352
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=51187ms; cargo_fmt_check_ms=684ms; cargo_clippy_ms=187ms; cargo_test_ms=7430ms; relay_teams_index_ms=81070ms; relay_teams_query_p50_ms=116ms; relay_teams_query_p95_ms=11621ms; leveldb_cpp_index_ms=19352ms

Adopted optimization notes:

e) } fn load_symbol_keys( @@ -797,3 +799,39 @@ hash } + +#[cfg(test)] +mod tests { +    use super::{SymbolKey, caller_for_line}; +    use crate::domain::RepositoryCodeRange; + +    #[test] +    fn caller_lookup_uses_sorted_prefix_and_prefers_innermost_symbol() { +        let symbols = vec![ +            symbol("outer", 10, 100), +            symbol("same_start_outer", 20, 80), +            symbol("same_start_inner", 20, 40), +            symbol("after_call", 60, 70), +        ]; + +        let caller = caller_for_line(Some(&symbols), 30).expect("caller should match"); + +        assert_eq!(caller.name, "same_start_inner"); +    } + +    #[test] +    fn caller_lookup_ignores_symbols_that_start_after_call_line() { +        let symbols = vec![symbol("before", 1, 5), symbol("after", 20, 30)]; + +        assert!(caller_for_line(Some(&symbols), 10).is_none()); +    } + +    fn symbol(name: &str, start: u32, end: u32) -> SymbolKey { +        SymbolKey { +            symbol_snapshot_id: format!("symbol:{name}"), +            path: "src/lib.rs".to_owned(), +            name: name.to_owned(), +            line_range: RepositoryCodeRange { start, end }, +        } +    } +} tokens used 88,978
## 20260516T130000Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T130000Z.patch`
- score: 0.916501 (accuracy=0.9, performance=0.905002, stability=1.0)
- cases: 18/20 passed
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/code/pipeline.rs`, `src/relay_knowledge/code/tests.rs`
- key improvements: metric:cargo_test_ms 8118.0->7706; metric:relay_teams_index_ms 89353.0->83470; metric:leveldb_cpp_index_ms 21112.0->20417
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=54020ms; cargo_fmt_check_ms=717ms; cargo_clippy_ms=198ms; cargo_test_ms=7706ms; relay_teams_index_ms=83470ms; relay_teams_query_p50_ms=129ms; relay_teams_query_p95_ms=11995ms; leveldb_cpp_index_ms=20417ms

Adopted optimization notes:

3b88 --- a/src/relay_knowledge/code/tests.rs +++ b/src/relay_knowledge/code/tests.rs @@ -265,6 +265,31 @@ } #[test] +fn full_index_plan_preserves_order_across_bounded_parallel_parse_chunks() { +    let repo = TempGitRepo::create("parallel-fetch-order"); +    for index in 0..40 { +        repo.write( +            &format!("src/file_{index:02}.rs"), +            &format!("fn f_{index}() {{}}\n"), +        ); +    } +    repo.git(["add", "."]); +    repo.git(["commit", "-m", "base"]); +    let budget = +        CodeIndexResourceBudget::new(40, 1024 * 1024, 50_000).expect("budget should validate"); +    let plan = prepare_full_index_plan(repo.registration(), repo.selector(), budget) +        .expect("plan should prepare"); + +    let (_, batch) = plan.parse_next_batch().expect("batch should parse"); +    let batch = batch.expect("batch should exist"); + +    assert_eq!(batch.files.len(), 40); +    for (index, file) in batch.files.iter().enumerate() { +        assert_eq!(file.path, format!("src/file_{index:02}.rs")); +    } +} + +#[test] fn explicit_default_exclusion_opt_in_supports_dataset_and_lock_paths() { let registration = CodeRepositoryRegistration::new( "repo", tokens used 84,035
## 20260516T132656Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T132656Z.patch`
- score: 0.971355 (accuracy=1.0, performance=0.904515, stability=1.0)
- cases: 20/20 passed
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code.rs`, `src/relay_knowledge/storage/sqlite/code_batch.rs`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`
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
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_batch/finalize.rs`, `src/relay_knowledge/storage/sqlite/code_batch_finalize_tests.rs`, `src/relay_knowledge/storage/sqlite/code_schema.rs`
- key improvements: none recorded
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=59850ms; cargo_fmt_check_ms=842ms; cargo_clippy_ms=209ms; cargo_test_ms=8835ms; relay_teams_index_ms=87582ms; relay_teams_query_p50_ms=136ms; relay_teams_query_p95_ms=13219ms; leveldb_cpp_index_ms=19926ms

Adopted optimization notes:

w( +                    " +                    SELECT COUNT(*) +                    FROM code_repository_search +                    WHERE source_scope = ?1 AND document_kind = ?2 +                    ", +                    (&source_scope, &document_kind), +                    |row| row.get(0), +                ) +                .map_err(crate::storage::StorageError::from) +        }) +        .await +        .expect("search document count should load") +} diff --git a/src/relay_knowledge/storage/sqlite/code_schema.rs b/src/relay_knowledge/storage/sqlite/code_schema.rs index d7b8cb2e9a40adc1e7c21eb825c6220fb8fd9877..0946af7022d8361c66c6443600975234efc916e8 --- a/src/relay_knowledge/storage/sqlite/code_schema.rs +++ b/src/relay_knowledge/storage/sqlite/code_schema.rs @@ -373,7 +373,8 @@ source_scope, document_kind, record_id, path, language_id, content ) SELECT source_scope, 'call', call_id, path, '', -               coalesce(caller_name, '') || ' ' || callee_name || ' ' || coalesce(target_hint, '') +               coalesce(caller_name, '') || ' ' || callee_name || ' ' || +               coalesce(target_hint, '') || ' ' || path FROM code_repository_calls ", [], tokens used 135,337
## 20260516T135933Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T135933Z.patch`
- score: 0.939577 (accuracy=0.923077, performance=0.904871, stability=1.0)
- cases: 24/26 passed
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
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
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
- key improvements: metric:cargo_build_release_ms 56550.0->48946; metric:cargo_fmt_check_ms 755.0->728; metric:relay_teams_query_p95_ms 12317.0->11797.0
- known degradations: none recorded
- latency metrics: cargo_build_release_ms=48946ms; cargo_fmt_check_ms=728ms; cargo_clippy_ms=200ms; cargo_test_ms=8023ms; relay_teams_index_ms=81709ms; relay_teams_query_p50_ms=130ms; relay_teams_query_p95_ms=11797ms; leveldb_cpp_index_ms=19861ms

Adopted optimization notes:

rs: Vec::new(), +        language_filters: Vec::new(), +        full_replace: true, +        changed_path_count: 3, +        skipped_unchanged_count: 0, +        deleted_paths: Vec::new(), +        tombstones: Vec::new(), +        files: vec![ +            file( +                "first-noise-file", +                "src/a_noise.py", +                "python", +                CodeParseStatus::Parsed, +                None, +            ), +            file( +                "second-noise-file", +                "src/b_noise.py", +                "python", +                CodeParseStatus::Parsed, +                None, +            ), +            file( +                "exact-file", +                "src/z_exact_owner.py", +                "python", +                CodeParseStatus::Parsed, +                None, +            ), +        ], +        symbols: Vec::new(), +        references: Vec::new(), +        imports: Vec::new(), +        calls: vec![first_noise, second_noise, exact], +        chunks: Vec::new(), +        diagnostics: Vec::new(), +    } +} + fn snapshot_with_call_site_chunk() -> CodeIndexSnapshot { let mut caller = symbol( "sanitize-options", tokens used 132,437
## 20260516T142335Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T142335Z.patch`
- score: 0.939946 (accuracy=0.923077, performance=0.90733, stability=1.0)
- cases: 24/26 passed
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
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
## 20260516T143645Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T143645Z.patch`
- score: 0.963002 (accuracy=0.961538, performance=0.907191, stability=1.0)
- cases: 25/26 passed
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`, `src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs`
- key improvements: score_component:score 0.939954->0.963002; score_component:accuracy 0.923077->0.961538; metric:cargo_fmt_check_ms 711.0->678; metric:relay_teams_index_ms 73237.0->70352; case:rt_fuzzy_constant_checkpoint_version {'passed': False, 'rank': None, 'false_positive_count': 0}->{'passed': True, 'rank': 3, 'false_positive_count': 0} failed_to_passed; case:rt_fuzzy_function_archive_output_dir {'passed': False, 'rank': 13, 'false_positive_count': 0}->{'passed': True, 'rank': 2, 'false_positive_count': 0} failed_to_passed
- known degradations: metric:leveldb_cpp_query_p95_ms 136.0->183.0; case:leveldb_hybrid_recovery_manifest_full_scope {'passed': True, 'rank': 3, 'false_positive_count': 0}->{'passed': False, 'rank': 9, 'false_positive_count': 0} passed_to_failed; case:leveldb_fuzzy_class_cache_lru_interface {'passed': True, 'rank': 1, 'false_positive_count': 0}->{'passed': True, 'rank': 5, 'false_positive_count': 0} rank_worsened
- latency metrics: cargo_build_release_ms=34450ms; cargo_fmt_check_ms=678ms; cargo_clippy_ms=195ms; cargo_test_ms=7340ms; relay_teams_index_ms=70352ms; relay_teams_query_p50_ms=128ms; relay_teams_query_p95_ms=8344ms; leveldb_cpp_index_ms=18666ms

Adopted optimization notes:

us-callee", "cma-source", "mm/cma_debug.c"); ambiguous.caller_name = Some("cma_debugfs_init".to_owned()); diff --git a/src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs b/src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs index 595e4c271c813e5466eb068d26842c18e72d6e06..db6719d0fe07cf265d1d8dff6c31a380334f1108 --- a/src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs +++ b/src/relay_knowledge/storage/sqlite/code_query_unit_tests.rs @@ -32,3 +32,25 @@ assert!(values.len() <= MAX_CANDIDATE_BIND_VALUES); } + +#[test] +fn symbol_fts_query_uses_any_term_for_fuzzy_recall() { +    assert_eq!( +        symbol_fts_match_query("checkpoint metadata version constant"), +        "\"checkpoint\" OR \"metadata\" OR \"version\" OR \"constant\"" +    ); +    assert_eq!( +        fts_match_query("checkpoint metadata version constant"), +        "\"checkpoint\" \"metadata\" \"version\" \"constant\"" +    ); +} + +#[test] +fn score_text_matches_identifier_parts_inside_snake_case_names() { +    let score = score_text( +        "archive output directory", +        ["def archive_output_dir(output_dir: Path) -> Path:"], +    ); + +    assert!(score >= 4.0); +} tokens used 144,045
## 20260516T144537Z

- patch: `/opt/workspace/relay-knowledge-refactor/.git/relay-knowledge-self-iteration/patches/20260516T144537Z.patch`
- score: 0.985951 (accuracy=1.0, performance=0.906342, stability=1.0)
- cases: 26/26 passed
- changed paths: `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md`, `src/relay_knowledge/storage/sqlite/code_query.rs`, `src/relay_knowledge/storage/sqlite/code_query_accuracy_tests.rs`
- key improvements: score_component:score 0.963002->0.985951; score_component:accuracy 0.961538->1.0; metric:cargo_build_release_ms 34450.0->30307; case:leveldb_hybrid_recovery_manifest_full_scope {'passed': False, 'rank': 9, 'false_positive_count': 0}->{'passed': True, 'rank': 5, 'false_positive_count': 0} failed_to_passed; case:leveldb_fuzzy_class_cache_lru_interface {'passed': True, 'rank': 5, 'false_positive_count': 0}->{'passed': True, 'rank': 1, 'false_positive_count': 0} rank_improved
- known degradations: metric:cargo_fmt_check_ms 678.0->708; metric:relay_teams_query_p95_ms 8344.0->9460.0; metric:leveldb_cpp_query_p95_ms 183.0->225.0
- latency metrics: cargo_build_release_ms=30307ms; cargo_fmt_check_ms=708ms; cargo_clippy_ms=182ms; cargo_test_ms=7290ms; relay_teams_index_ms=69032ms; relay_teams_query_p50_ms=118ms; relay_teams_query_p95_ms=9460ms; leveldb_cpp_index_ms=18856ms

Adopted optimization notes:

        base_resolved_commit_sha: None, +        resolved_commit_sha: "commit".to_owned(), +        tree_hash: "tree".to_owned(), +        path_filters: Vec::new(), +        language_filters: Vec::new(), +        full_replace: true, +        changed_path_count: 2, +        skipped_unchanged_count: 0, +        deleted_paths: Vec::new(), +        tombstones: Vec::new(), +        files: vec![ +            file( +                "db-impl-header", +                "db/db_impl.h", +                "cpp", +                CodeParseStatus::Parsed, +                None, +            ), +            file( +                "db-impl-source", +                "db/db_impl.cc", +                "cpp", +                CodeParseStatus::Parsed, +                None, +            ), +        ], +        symbols: Vec::new(), +        references: Vec::new(), +        imports: Vec::new(), +        calls: Vec::new(), +        chunks: vec![target, noise], +        diagnostics: Vec::new(), +    } +} + fn snapshot_with_related_callee_names() -> CodeIndexSnapshot { let mut unrelated = call("unmapped-area", "mmap-source", "mm/mmap.c"); unrelated.caller_name = Some("do_mmap".to_owned()); tokens used 107,884

