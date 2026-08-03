# Code Index Fact Versioning

代码索引的 `source_scope` 不只由仓库、tree hash、路径过滤器和语言过滤器决定，还包含代码事实版本。这个版本用于区分不同的持久化事实语义，例如解析器新增或修正的定义、引用、依赖、边、搜索文档和检索证据。

## 必须升级版本的变更

以下变更会改变已经持久化的代码事实，必须同步升级代码事实版本：

- 解析器新增、删除或重新分类定义、引用、调用、导入、依赖、边或搜索文档。
- 检索依赖的事实形态发生变化，例如 Python 或 TypeScript 类型注解开始作为 `type` 引用参与图检索。
- freshness、查询或增量索引对 `source_scope` 兼容性的判断发生变化。
- 旧索引即使 tree hash 未变，也会因为缺少新增事实而导致 benchmark、self-iteration 或用户查询召回错误。

版本升级后，freshness 检查会期望新的 `source_scope`，旧 scope 只作为历史数据保留。重新索引仍然通过既有的 durable task、lease、checkpoint、bounded batch 和状态观测流程完成，不能通过 fixture 查询、路径、符号或仓库名特判绕过。

查询、feature flag 查询和 impact 分析必须使用 freshness 解析出的当前事实版本 `source_scope`，不能让底层存储再次只按仓库、ref 和过滤器选择旧 scope。增量索引也只能从当前事实版本的 base scope 克隆未变更文件；如果 base scope 来自旧事实版本，必须要求先全量重建 base，而不是把旧事实复制进新 scope。

每个 checkpointed batch 在事实发布前都有 durable staging manifest，发布事务会原子地推进 manifest、事实计数和 checkpoint。`staged` 不是可查询事实，也不是成功状态；只有 checkpoint 和 scope 发布都完成后，事实才可被 freshness 查询使用。

存储层按 ref 或最新 checkpoint 查找生成的 `git_snapshot:<16 hex>` scope 时，必须在扫描候选 scope 的过程中优先选择当前事实版本，而不是先按 checkpoint 或 `source_scope` 排序缩窄到单行后再过滤。这样旧事实版本 scope 即使是当前 active 行或更新时间更晚，也不会遮蔽已经存在的当前事实版本索引；如果没有当前事实版本 scope，存储层不能把旧 `git_snapshot:<16 hex>` scope 作为兼容结果返回。非生成 scope（例如测试或外部调用显式传入的自定义 `source_scope`）不参与这个生成 scope 事实版本判定，仍按普通存储兼容性处理。

Repository-set member 也必须在状态和查询前重新校验事实版本。既有 member 若保存了旧 `source_scope`，只能在找到同 commit/filter 的当前事实版本 scope 后使用当前 scope 查询并把 member/overlay 标记为 stale；如果执行 repository-set refresh，刷新 overlay 前必须先把替换后的 member scope 写回持久层，确保 overlay edges 和 member version manifest 基于当前事实版本重建。如果找不到当前 scope，查询必须跳过旧 scope，refresh 也不能通过旧 scope 重建 overlay 或通过 `AllowStale` 继续服务旧事实。member 自身的 path/language filters 仍是查询、source fallback 和 freshness 语义边界；事实版本校验需要使用实际 indexed scope filters，不能用宽 scope 覆盖 member row filters。

仓库重新注册到新的 root path、path filters 或 language filters 后，仓库行必须保持 stale/registered，不能因为旧 `code_repository_scopes` 行仍是 fresh 就清除仓库级 stale。Fresh full-index fast path 可以复用当前 fact-version 的代码 scope，但仍必须刷新同 scope 的 software global projection；否则缺失、stale 或失败的 projection 会在代码索引成功响应后继续影响 `repo software` 和 MCP software 查询。

## 本次约束

语法解析被有界预算取消或在进入 grammar 前识别出高复杂度、无声明容器的 C/C++ 指定初始化片段时，文件必须持久化为 `failed` 并保留诊断，同时继续写入文件级 source chunk、FTS 搜索文档、依赖、feature flag 与 route 投影。因为旧索引中的同类失败文件没有 source chunk，本次优化升级代码事实版本，强制 freshness 对这些 scope 执行完整、可恢复的重建。

该有界失败路径不能把任意条件编译块或函数体中的损坏代码标记为 `parsed`，也不能省略 durable task、lease、checkpoint、批量 DML、FTS 或 edge finalize。`index_performance_c_fragment` 生成式性能仓库用不含真实仓库名、路径或符号的重复指定初始化片段保护 5 秒 cold-index 预算；回归到无界 tree-sitter 恢复会让该任务无法在预算内完成。

C 文件跳过重复 tags query 后，manual tree traversal 必须显式物化 translation unit 直属或由顶层预处理条件包裹的 `struct`、`union` 与 `enum` tag。该规则按语法节点形态工作，不依赖 fixture 名称；对应 symbol facts、FTS 和查询召回必须与启用 tags query 时等价，并由 `c-composite-tags-v1` 事实版本强制旧 scope 重建。

YAML、JSON、TOML、Markdown 等结构化文档继续保留完整 symbol facts 与 symbol search documents，但 source chunk 从“每个 symbol 一条重复 excerpt，再附加文件前缀”改为覆盖完整已授权文本的连续有界窗口。窗口最多 8 KiB、200 行，互不重复，全部写入 chunk 表和 FTS；因此不是关闭 search-document 写入或丢弃尾部源码。该 derived fact 布局由 `bounded-config-chunks-v1` 进入代码事实版本，避免旧 scope 的重复 chunk 与新 scope 混用。

同样的完整源码窗口策略扩展到至少 64 个 symbol 且密度超过每个估算窗口 4 个 symbol 的代码文件；只省去重复的非 callable symbol-linked chunk，可调用 symbol 的 body-linked chunk、全部 symbol/reference facts、symbol FTS 和窗口 FTS 均保留。该布局由 `dense-source-windows-v1` 进入事实版本，不能用来省略 FTS、edge finalize 或 freshness 重建。

Python 协议方法和服务方法中的类型注解需要成为代码图引用事实。若评估缓存是在该事实提取能力之前生成的，`W3ConnectorSaveRequest` 这类注解引用会缺失，但旧索引可能仍被 tree hash freshness 认为可用。

对应修复是升级代码事实版本，使 `relay-teams` 等仓库在 freshness wait 或全量评估中重新构建代码事实。该修复不改变 schema、不放宽 stale/degraded 状态、不跳过索引阶段，也不修改 task lease、checkpoint、retry 或 writer 互斥语义。

## 验证

修改代码事实版本或解析器引用事实后，至少运行相关 parser 单测和 foundational self-iteration：

```bash
cargo test python_protocol_method_annotations_are_reference_facts --all-targets --all-features
./self-iterate.sh evaluate --profile fast --categories foundational --jobs 8 --repo-jobs 4 --query-jobs 8 --command-timeout-seconds 900
```

如需证明 `categories=all` 下的 foundational 分数，运行全类目评估并检查报告中的 `foundational_capability`：

```bash
./self-iterate.sh evaluate --profile fast --categories all --jobs 8 --repo-jobs 4 --query-jobs 8 --command-timeout-seconds 900
```
自迭代的大仓索引预算采用基线驱动的弹性约束：`index_budget_mode=elastic` 时，评估器优先用目标 Git 仓库的 `git ls-files` 实测文件数覆盖 `expected_file_count`；若配置了 `baseline_files_per_second`，则按观测吞吐计算预算，否则按 `baseline_index_budget_ms × expected_file_count / baseline_file_count` 计算，并受 `max_index_budget_ms` 上限约束；注册加上独立的有界开销预算。这样 180s 只是历史基线参考点，不再被错误地当成所有仓库的硬超时。

任务被 reset 或 orphan worker 接管时，已有 `indexing` checkpoint 和已发布批次会作为恢复起点保留；只有新的 full-replace scope 才会清理旧事实。这样弹性超时不会把长任务退化成重复冷索引。
