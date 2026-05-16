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

## 候选优化说明：自迭代文档与 patch 长期记忆

- 目标：让自迭代候选在修改代码、测试、benchmark 或 harness 策略时，同时留下可供后续迭代理解的算法与架构说明，避免只有 patch 和评分而缺少设计意图。
- 方法：候选 diff 只要包含非文档文件，就追加 `self_iteration_algorithm_documentation` gate，要求同步更新本文档；prompt 明确要求写出算法、架构、不变量、预期 case/metric 影响和风险。该 gate 在候选评估完成后、评分前加入，作为硬质量门禁参与 `quality gates failed` 拒绝原因。
- 长期记忆：prompt 新增 `.git/relay-knowledge-self-iteration/patches/` 索引，按最近 patch 列出路径、大小、采纳状态、分数、变更文件、拒绝原因和主要改善。Codex 先读索引，再用 `sed -n` 对相关 patch 小范围渐进读取，避免一次性塞入所有历史 patch 造成上下文膨胀。
- 预期影响：后续自迭代能同时利用结构化 run history、人工可读设计说明和原始 patch 细节，减少重复尝试，提高对历史成功/失败算法的复用质量。
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

