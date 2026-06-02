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

存储层按 ref 或最新 checkpoint 查找生成的 `git_snapshot:<16 hex>` scope 时，必须在扫描候选 scope 的过程中优先选择当前事实版本，而不是先按 checkpoint 或 `source_scope` 排序缩窄到单行后再过滤。这样旧事实版本 scope 即使是当前 active 行或更新时间更晚，也不会遮蔽已经存在的当前事实版本索引；如果没有当前事实版本 scope，存储层不能把旧 `git_snapshot:<16 hex>` scope 作为兼容结果返回。非生成 scope（例如测试或外部调用显式传入的自定义 `source_scope`）不参与这个生成 scope 事实版本判定，仍按普通存储兼容性处理。

Repository-set member 也必须在状态和查询前重新校验事实版本。既有 member 若保存了旧 `source_scope`，只能在找到同 commit/filter 的当前事实版本 scope 后使用当前 scope 查询并把 member/overlay 标记为 stale；如果执行 repository-set refresh，刷新 overlay 前必须先把替换后的 member scope 写回持久层，确保 overlay edges 和 member version manifest 基于当前事实版本重建。如果找不到当前 scope，查询必须跳过旧 scope，refresh 也不能通过旧 scope 重建 overlay 或通过 `AllowStale` 继续服务旧事实。

## 本次约束

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
