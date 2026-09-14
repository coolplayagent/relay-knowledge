# relay-teams 代码图检索准确性测试 2026-05-15

日期：2026-05-15

测试目标：以 `/opt/workspace/relay-teams` 为样本仓库，验证
`relay-knowledge repo query` 对编码领域查询的准确性，覆盖函数/类定义、引用、
调用方、被调用方、导入、混合检索、负例和请求过滤。

## 环境和样本

- 测试仓库：`/opt/workspace/relay-teams`
- 分支：`improve-memory-skill-draft-status-ui`
- HEAD：`fa3c0ddc9d81400b8d5e58ab7600dd557a056816`
- Tree：`5f3b09c42f4419bfa2db0c3ffef9fbaccde80e65`
- 工作树：测试前 `git status --short` 无输出
- 测试二进制：`target/debug/relay-knowledge`
- 修复前原始输出：`/tmp/relay-knowledge-code-accuracy-20260515/json/`
- 修复后原始输出：`/tmp/relay-knowledge-code-accuracy-20260515-fixed/json/`

修复后复测使用独立运行时：

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-code-accuracy-20260515-fixed/home \
  target/debug/relay-knowledge repo register /opt/workspace/relay-teams \
  --alias relay-teams-accuracy --format json
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-code-accuracy-20260515-fixed/home \
  target/debug/relay-knowledge repo index relay-teams-accuracy --ref HEAD --format json
```

## 索引结果

修复后 scope preview 选中：

- 文件数：1,653
- 字节数：22,063,153
- Python：1,430 个文件 / 19,910,475 字节
- JavaScript：3 个文件 / 4,737 字节
- Bash：2 个文件 / 2,727 字节
- Unknown：218 个文件 / 2,145,214 字节
- 预期降级文件：218

修复后索引和报告：

- `repo index`：232,472ms（debug 构建）
- `repo status`：245ms
- `repo report`：742ms
- `indexed_file_count`：1,653
- `symbol_count`：28,125
- `reference_count`：187,993
- `chunk_count`：28,436
- `degraded_file_count`：218
- Edge resolution：resolved 68,357 / ambiguous 53,883 / unresolved 77,287

修复前 edge resolution 为 resolved 61,964 / ambiguous 53,816 / unresolved
83,747。修复后 local Python import 解析让 resolved 边增加，unresolved 边减少。

## 修复内容

- 查询 scope 解析：当已索引完整或更宽 scope 时，`repo query --path` 和
  `--language` 可在该 scope 上做请求级收窄；无匹配语言返回空结果，不再报
  “requested filters 没有索引”。如果仓库只索引了受限 scope，请求尝试查询该
  scope 外的路径或语言时仍显式报错，不把未索引 scope 伪装成空结果。
- 排序：`definition`、`callers`、`callees`、`hybrid` 等查询增加 exact
  identifier 优先级，避免 `_summary` 被 `get_latest_summary` 这类子串命中淹没。
- Tree-sitter import：继续由 tree-sitter 采集 Python import 语句，再用本地
  module/name 与已索引符号表解析 local imports，输出 `resolved` /
  `ambiguous` / `unresolved`；外部包不会仅因同名本地符号而被误标为 resolved，
  `__init__.py` 会归一化为 Python package module。
- 诊断：单条 hit 不再继承仓库级 `degraded_reason`；全局降级仍保留在响应级和
  report/status 中。

新增回归覆盖：

- `python_tree_sitter_imports_resolve_local_symbols`
- `python_tree_sitter_external_imports_do_not_match_local_symbol_names`
- `python_tree_sitter_package_init_imports_resolve_package_modules`
- `full_scope_serves_narrower_query_filters`
- `restrictive_scope_rejects_query_filters_outside_indexed_scope`
- `exact_identifier_matches_rank_before_substring_matches`
- `parsed_hits_do_not_inherit_repository_degraded_reason`
- `queries_can_narrow_a_full_repository_index_by_path_or_language`
- `restricted_index_rejects_query_filters_outside_indexed_scope`

## 准确性汇总

真值来自 relay-teams 源码的 Python AST 行号扫描，并用 `nl`/`rg` 对关键样本复核。
本轮共 34 个重点用例，覆盖 `ConnectorService`、`W3ConnectorService`、
`ConnectorItem`、`W3ConnectorSaveRequest`、`list_connectors`、
`save_credentials`、`_aggregate_status`、`_slugify`、`_summary`、
`_build_service`、`_normalize_string_fields` 等符号族。

| kind | 修复前 | 修复后 | 备注 |
| --- | ---: | ---: | --- |
| `definition` | 11 Pass / 3 Fail | 14 Pass / 0 Fail | 过滤查询已通过 |
| `references` | 5 Pass / 0 Fail | 5 Pass / 0 Fail | 抽样引用位置均可召回 |
| `callers` | 4 Pass / 1 Fail | 5 Pass / 0 Fail | `_summary` rank 1 命中 |
| `callees` | 3 Pass / 1 Fail | 4 Pass / 0 Fail | `_summary` rank 1 命中 |
| `imports` | 3 Pass / 0 Fail | 3 Pass / 0 Fail | local imports 由 unresolved 变为 resolved |
| `hybrid` | 3 Pass / 0 Fail | 3 Pass / 0 Fail | 类定义保持 rank 1 |

总体：修复前 29 Pass / 5 Fail；修复后 34 Pass / 0 Fail。

代表性修复后结果：

- `callers _summary`：rank 1 命中
  `src/relay_teams/connector/service.py:177`，
  `list_connectors calls _summary`。
- `callees _summary`：rank 1 命中
  `src/relay_teams/connector/service.py:677`，
  `_summary calls ConnectorSummary`。
- `definition ConnectorService --path src/relay_teams/connector/service.py`：
  rank 1 命中 `src/relay_teams/connector/service.py:137`。
- `definition ConnectorService --language python`：rank 1 命中
  `src/relay_teams/connector/service.py:137`。
- `definition ConnectorService --language rust`：返回 0 条结果。
- `imports W3ConnectorSaveRequest`：前 5 条 local import hit 的
  `edge_resolution_state` 均为 `resolved`。
- 解析正常的 `ConnectorService` hit 不再带 hit 级 `degraded_reason`。
- 负例 `DefinitelyMissingRelayTeamsSymbol` 在 `definition` 与 `hybrid` 下均返回
  0 条结果。

## 问题清单

### RK-ACC-001：完整索引后无法用 `--path` 或 `--language` 收窄查询

严重度：High

状态：已修复并通过 relay-teams 复测。

修复前复现：

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-code-accuracy-20260515/home \
  target/debug/relay-knowledge repo query relay-teams-accuracy \
  --query ConnectorService --kind definition \
  --path src/relay_teams/connector/service.py --format json
```

修复前实际结果：

```text
code repository 'relay-teams-accuracy' has no index for ref fa3c0ddc9d81400b8d5e58ab7600dd557a056816 and requested filters
```

修复后：

- `--path src/relay_teams/connector/service.py` 返回目标定义。
- `--language python` 返回目标定义。
- `--language rust` 返回空结果。

### RK-ACC-002：短下划线 helper 的调用图查询缺少精确标识符优先级

严重度：Medium

状态：已修复并通过 relay-teams 复测。

修复前 `--query _summary --kind callers --limit 10` 的目标调用方在 rank 35；
修复后 rank 1。修复前 `--kind callees` 的目标函数体未进入 top 50；修复后 rank
1，并返回 `ConnectorSummary`、`sum`、`len` 等 `_summary` 函数体内调用。

### RK-ACC-003：精确符号查询会混入后缀扩展名，且同分排序不可表达精确性

严重度：Medium

状态：已修复主要排序问题。

修复前 `_build_service` 会把 `_build_service_with_control`、
`_build_service_with_xiaoluban` 与精确 `_build_service` 同分提前返回。修复后精确
identifier 优先于后缀扩展名。仍有多个真正同名 `_build_service` 定义，它们需要
结合已修复的 `--path` 收窄来定位，这是预期的歧义场景。

修复前 `hybrid ConnectorService` 的 top 10 被类方法定义填满；修复后类定义 rank
1，后续优先返回真实引用和调用边。

### RK-ACC-004：本地 import 能召回但全部标记为 unresolved

严重度：Medium

状态：已修复并通过 relay-teams 复测。

修复后 `ConnectorService` 和 `W3ConnectorSaveRequest` 的 local import hit 前 5 条均
为 `resolved`。当前实现用 tree-sitter 采集 import 语句，并在索引完成阶段用本地
Python module/name 与符号表解析；无法唯一判定时保留 `ambiguous` 或 `unresolved`，
不把外部包误报为本地 resolved。

### RK-ACC-005：解析正常的命中继承了仓库级 degraded_reason

严重度：Low

状态：已修复并通过 relay-teams 复测。

修复前 `src/relay_teams/connector/service.py:137` 这类已解析 Python 命中也带有
`218 file(s) degraded during code indexing`。修复后单条 hit 只报告自身文件或
chunk 的降级原因；仓库级降级仍通过 `repo status` / `repo report` 和响应级
`degraded_reason` 表达。

## 结论

修复后，relay-teams 重点样本中的定义、引用、调用方、被调用方、导入、混合检索、
负例和请求过滤全部通过。Tree-sitter 对 Python 定义、调用和 import 语句的采集
能力足够支撑当前代码图检索；新增的本地 import 解析和 exact identifier 排序解决了
本轮发现的主要准确性问题。
