# Linux 代码图检索准确性测试 2026-05-15

日期：2026-05-15

测试目标：以 `/opt/workspace/linux` 为测试仓库，验证
`relay-knowledge repo query` 对编码领域检索的准确性，覆盖宏定义、函数定义、
头文件声明到实现、函数引用、调用方、被调用方和 C `#include` 关系。

## 环境和样本

- 测试仓库：`/opt/workspace/linux`
- 分支：`master`
- HEAD：`70eda68668d1476b459b64e69b8f36659fa9dfa8`
- Tree：`26fe996ae3fd74753bac73ea6f08e724b4f604a9`
- 工作树：测试前 `git status --short` 无输出
- 测试二进制：`target/release/relay-knowledge`
- 原始输出目录：`/tmp/relay-knowledge-linux-accuracy-20260515/`

## 全量索引结果

全量 scope preview：

- 选中文件：93,376
- 选中字节：1,578,724,800
- C/头文件：63,168 个文件 / 1,402,875,576 字节
- Rust：349 个文件 / 4,840,395 字节
- Bash：1,124 个文件 / 5,586,987 字节
- Python：374 个文件 / 4,129,828 字节
- Unknown：28,356 个文件 / 161,238,752 字节
- 预计降级文件：28,523

全量索引命令：

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-linux-accuracy-20260515/home \
  target/release/relay-knowledge repo index linux-accuracy --ref HEAD --format json
```

实际结果：失败，退出码 134，未产生索引 JSON。

stderr：

```text
thread 'tokio-rt-worker' (...) has overflowed its stack
fatal runtime error: stack overflow, aborting
real 170.43
user 103.53
sys 54.30
```

因此 Linux 全量仓库当前无法完成代码图索引，后续准确性验证使用真实 Linux 文件组成的
受控样本 scope。

## 中等 Scope 尝试

为了接近真实核心子系统，尝试注册：

- `mm`
- `fs`
- `kernel`
- `init`
- `include/linux`
- `--language c`

preview 结果：

- 选中文件：5,801
- 选中字节：89,192,590
- 预计降级文件：5

索引运行 502.70 秒后仍未写入索引结果，SQLite 文件仍停留在注册后的大小，手动中断。
该结果说明当前索引器对中等规模 Linux C scope 缺少可观察进度、增量落盘和可恢复边界。

## 精确样本 Scope

最终用于准确性验证的样本 scope 选取真实 Linux 文件：

- `init/main.c`
- `kernel/fork.c`
- `kernel/sched/core.c`
- `mm/mmap.c`
- `mm/nommu.c`
- `mm/vma.c`
- `mm/vma.h`
- `mm/cma_debug.c`
- `fs/read_write.c`
- `fs/exec.c`
- `fs/debugfs/inode.c`
- `include/linux/mm.h`
- `include/linux/debugfs.h`
- `include/linux/fs.h`
- `include/linux/container_of.h`
- `include/linux/module.h`
- `include/linux/export.h`
- `include/linux/build_bug.h`
- `include/linux/compiler.h`
- `include/linux/slab.h`
- `include/linux/sched.h`

preview：

- 选中文件：21
- 选中字节：1,241,748
- 预计降级文件：0
- 语言：C，21 个文件

索引结果：

- `repo index`：1.06 秒
- `indexed_file_count`：21
- `symbol_count`：2,813
- `reference_count`：6,979
- `chunk_count`：2,816
- `degraded_file_count`：18
- Edge resolution：resolved 1,727 / ambiguous 323 / unresolved 5,561

注意：preview 预计 0 个降级文件，但实际 18 个文件因
`tree-sitter produced error nodes; indexed syntax facts may be partial` 被标记为
degraded。

## Tree-sitter C 能力检查

本轮检查了本地依赖中的 `tree-sitter-c-0.24.2/queries/tags.scm`：

- 原生 tags query 能捕获 `struct_specifier`、`union_specifier`、
  `function_declarator`、`type_definition` 和 `enum_specifier`。
- 原生 tags query 没有 `preproc_def` / `preproc_function_def` 的
  `@definition.macro` 捕获，因此 C 宏定义不会自动进入 symbol 索引。
- C 函数捕获点是 `function_declarator`，不是完整 `function_definition`。如果直接
  使用该范围，函数 symbol 只覆盖声明行，调用图无法把函数体内 call site 归属到
  caller。
- `preproc_include` 可由语法树遍历发现，但需要项目侧把
  `#include <linux/debugfs.h>` 解析到 `include/linux/debugfs.h` 这类本地文件路径。

修复策略是在 tree-sitter tags 之外增加 C 语言感知抽取层：使用显式栈遍历 AST，
从 `function_definition` 建完整函数范围，从顶层 `declaration` 建
`function_declaration`，从 `preproc_def` / `preproc_function_def` 建 `macro`，
并解析 C include 目标。

## 修复后复测

修复后原始输出目录：
`/tmp/relay-knowledge-linux-accuracy-20260515-fixed2/`。

精确样本 scope 复测：

- `repo index`：1.97 秒
- `indexed_file_count`：21
- `symbol_count`：3,605
- `reference_count`：6,988
- `chunk_count`：3,605
- `degraded_file_count`：18
- Edge resolution：resolved 2,136 / ambiguous 478 / unresolved 5,006

较修复前，symbol 数从 2,813 增至 3,605，resolved edge 从 1,727 增至
2,136，unresolved edge 从 5,561 降至 5,006。

代表性修复后结果：

- `definition container_of`：rank 1 命中
  `include/linux/container_of.h:19-25`。
- `definition module_init`：rank 1/2 命中
  `include/linux/module.h:89-90` 和 `include/linux/module.h:131-137`。
- `definition EXPORT_SYMBOL`：rank 1 命中 `include/linux/export.h:89-90`。
- `definition BUILD_BUG_ON`：rank 1 命中 `include/linux/build_bug.h:50-52`。
- `definition likely`：rank 1/2 命中 `include/linux/compiler.h:44-45` 和
  `include/linux/compiler.h:76-77`。
- `callers cma_debugfs_add_one`：返回
  `cma_debugfs_init calls cma_debugfs_add_one`，不再是 `<module>`。
- `callees cma_debugfs_init`：返回 `debugfs_create_dir` 和
  `cma_debugfs_add_one`。
- `callees do_mmap`：返回 `__get_unmapped_area`、
  `find_vma_intersection`、`mmap_region` 等函数体内调用。
- `imports linux/debugfs.h`：只返回 `fs/debugfs/inode.c:23-24` 和
  `mm/cma_debug.c:9-10`，并解析为 `include/linux/debugfs.h`。
- `imports linux/mm.h`：返回 `fs/exec.c`、`kernel/fork.c`、`mm/mmap.c`、
  `mm/nommu.c` 等真实 include source，均解析为 `include/linux/mm.h`。

中等 scope 复测：

- scope：`mm`、`fs`、`kernel`、`init`、`include/linux`，共 5,801 个 C 文件。
- 命令设置 220 秒上限。
- 结果：220 秒超时，未完成索引，但没有再次出现栈溢出。

结论：C 语义准确性问题已修复；深层语法树栈溢出已消除。后续修复已把 full index
从单次内存 snapshot 改为 checkpointed batch pipeline：解析阶段按资源预算分批落
SQLite，`code_repository_index_checkpoints` 持久化 scope 级进度，`repo status`
在运行中显示 `indexing` 和已提交计数，finalize 阶段再基于已落库事实解析引用、
include 和调用边，并在完成后原子切换 active scope。

## Checkpointed Pipeline 回归

本轮针对残余架构问题增加了两个回归门禁：

```bash
cargo test checkpointed_batches_finalize_cross_batch_call_edges --all-targets --all-features
cargo test --test relay_knowledge indexes_tree_sitter_repository_and_queries_code_graph --all-features
```

覆盖结果：

- 两个 batch 分别提交目标符号和调用引用，finalize 能跨 batch 解析
  `target_symbol_snapshot_id`，并物化 `call` edge。
- 第一批提交后，`code_repositories.state = indexing`，`indexed_file_count = 1`，
  证明进度不再只存在于进程内存。
- 完成后 summary 返回 `progress.batch_count = 2`、
  `progress.checkpoint_file_count = 2` 和实际 `resource_budget`。
- 应用层真实 Git fixture 的 `repo index` 已走 checkpointed full-index path，
  definition/reference/import 查询和后续 incremental update 仍通过。

本地 Linux smoke 复核：

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-linux-checkpoint-smoke \
  target/debug/relay-knowledge repo register /opt/workspace/linux \
  --alias linux-checkpoint-smoke \
  --path init/main.c \
  --path include/linux/module.h \
  --language c \
  --format json

RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-linux-checkpoint-smoke \
  target/debug/relay-knowledge repo index linux-checkpoint-smoke --ref HEAD --format json

RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-linux-checkpoint-smoke \
  target/debug/relay-knowledge repo query linux-checkpoint-smoke \
  --query module_init --kind definition --ref HEAD --limit 3 --format json
```

结果：

- Linux HEAD：`70eda68668d1476b459b64e69b8f36659fa9dfa8`，tree：
  `26fe996ae3fd74753bac73ea6f08e724b4f604a9`。
- 受控 scope：`init/main.c`、`include/linux/module.h`，语言 `c`。
- index summary：2 files、255 symbols、401 references、255 chunks、2 degraded files。
- progress：`batch_count = 1`、`checkpoint_file_count = 2`、
  `resource_budget = {max_files_per_batch:128, max_bytes_per_batch:16777216,
  max_rows_per_batch:50000}`。
- `definition module_init` rank 1/2 命中 `include/linux/module.h:89-90` 和
  `include/linux/module.h:131-137`。

当前结论：RK-LINUX-ACC-002 的架构阻断已解除。Linux 子系统级 scope 仍需要独立的
吞吐 benchmark 来给出完成时间和推荐 budget，但不再依赖单次巨大内存 snapshot，也不再
在中断前零落盘、零 checkpoint、零进度。

## 修复前查询用例汇总

真值来自 Linux 源码行号，并用 `nl`/`rg` 复核。原始 JSON 位于
`/tmp/relay-knowledge-linux-accuracy-20260515/sample-queries/`。

| 用例 | 查询 | 结果摘要 |
| --- | --- | --- |
| 函数定义 | `definition start_kernel` | Pass，命中 `init/main.c:1017` |
| 函数定义 | `definition copy_process` | Pass，rank 1 命中 `kernel/fork.c:1969`，同时召回 `rcu_copy_process` |
| 函数定义 | `definition find_vma` | Partial，命中 `include/linux/mm.h:4183`、`mm/mmap.c:903`、`mm/nommu.c:640`，但声明和实现都标为 definition |
| 函数定义 | `definition debugfs_create_dir` | Partial，命中 `fs/debugfs/inode.c:570`、`include/linux/debugfs.h:150`、`include/linux/debugfs.h:298`，但无法区分实现、声明和配置 fallback |
| 函数定义 | `definition vfs_read` | Partial，命中 `fs/read_write.c:554` 和 `include/linux/fs.h:2079`，但无声明到实现关系 |
| 函数定义 | `definition do_mmap` | Partial，命中声明与 `mm/mmap.c:336`、`mm/nommu.c:1012` 两个配置实现 |
| 函数定义 | `definition mmap_region` | Partial，命中 `mm/vma.c:2830` 与 `mm/vma.h:462` |
| 宏定义 | `definition container_of` | Fail，0 条结果；真值为 `include/linux/container_of.h:19` |
| 宏定义 | `definition module_init` | Fail，返回 `within_module_init` false positive；真值为 `include/linux/module.h:89` 和 `:131` |
| 宏定义 | `definition EXPORT_SYMBOL` | Fail，0 条结果；真值为 `include/linux/export.h:89` |
| 宏定义 | `definition BUILD_BUG_ON` | Fail，0 条结果；真值为 `include/linux/build_bug.h:50` |
| 宏定义 | `definition likely` | Fail，返回 `ftrace_likely_update` false positive；真值为 `include/linux/compiler.h:44` 和 `:76` |
| 引用 | `references find_vma` | Partial，命中真实调用点，但也包含 `find_vma_prev`/`find_vma_intersection` 子串相关结果 |
| 引用 | `references debugfs_create_dir` | Pass，命中 `mm/cma_debug.c:168`、`:178`、`:182`、`:205` |
| 引用 | `references EXPORT_SYMBOL` | Partial，能召回宏调用，但 50 条样本全部 unresolved |
| 调用方 | `callers cma_debugfs_add_one` | Fail，命中 `mm/cma_debug.c:208`，但 caller 显示为 `<module>`，未归属到 `cma_debugfs_init` |
| 调用方 | `callers find_vma` | Fail，能列出 call sites，但所有 caller 均为 `<module>` |
| 调用方 | `callers vfs_read` | Fail，能列出 call sites，但 caller 均为 `<module>` |
| 被调用方 | `callees cma_debugfs_init` | Fail，0 条结果；函数体内应有 `debugfs_create_dir` 和 `cma_debugfs_add_one` |
| 被调用方 | `callees cma_debugfs_add_one` | Fail，0 条结果；函数体内应有多次 `debugfs_create_dir` / `debugfs_create_file` |
| 被调用方 | `callees do_mmap` | Fail，0 条结果；函数体内应有 `__get_unmapped_area`、`find_vma_intersection`、`mmap_region` 等 |
| 被调用方 | `callees start_kernel` | Fail，0 条结果；函数体内有大量启动流程调用 |
| Include | `imports linux/debugfs.h` | Partial，召回 `fs/debugfs/inode.c:23` 和 `mm/cma_debug.c:9`，但也混入 `include/linux/debugfs.h` 自身 include，且全部 unresolved |
| Include | `imports linux/mm.h` | Partial，召回 `fs/exec.c:30`，但大量结果来自 `include/linux/mm.h` 文件内部 include，且全部 unresolved |

## 问题清单

### RK-LINUX-ACC-001：Linux 全量索引栈溢出

严重度：Critical

状态：已修复栈安全；full-index 已改为 checkpointed batch pipeline。

全量 `/opt/workspace/linux` scope 在 release 二进制下 170.43 秒后崩溃：

```text
thread 'tokio-rt-worker' (...) has overflowed its stack
fatal runtime error: stack overflow, aborting
```

修复后：

- AST 手写遍历从递归改为显式栈。
- 5,801 文件核心 C scope 在 220 秒上限内未再出现栈溢出。

剩余影响：

- Linux 全仓库完成时间仍需单独 benchmark 量化。
- 子系统级和全仓库 scope 可观测性不再被单次 snapshot 架构阻断。

### RK-LINUX-ACC-002：中等规模 Linux C scope 索引耗时不可控且无增量落盘

严重度：High

状态：已修复架构阻断；吞吐预算待 benchmark 调优。

`mm`、`fs`、`kernel`、`init`、`include/linux` 共 5,801 个 C/头文件、约 89MB。
索引运行 502.70 秒后仍无 JSON 输出，SQLite 文件未增长，手动中断。

修复后：

- Full index 使用 `CodeIndexResourceBudget` 控制每批文件数、字节数和 SQLite 行数。
- 每批提交后更新 `code_repository_index_checkpoints`，并把 repository status 标为
  `indexing`，暴露已落库文件、符号、引用和 chunk 计数。
- 查询继续读取上一版 fresh scope；新 scope 只有 finalize 成功后才变成 active。
- 引用解析、C/C++ include 解析和 call edge 物化移动到 finalize 阶段，避免跨 batch
  依赖被错误限制在单批内。

### RK-LINUX-ACC-003：preview 降级预估与实际索引降级严重不一致

严重度：High

状态：未修复。

精确样本 scope preview 预计：

- `expected_degraded_file_count = 0`

实际索引报告：

- `degraded_file_count = 18 / 21`
- 降级原因均为 `tree-sitter produced error nodes; indexed syntax facts may be partial`

影响：

- 用户在索引前无法判断 Linux C 代码会大面积降级。
- 查询响应会带仓库级 degraded 状态，准确性置信度降低。

### RK-LINUX-ACC-004：C 宏定义没有作为符号索引

严重度：High

状态：已修复。

失败样本：

- `definition container_of` 返回 0，真值为 `include/linux/container_of.h:19`。
- `definition EXPORT_SYMBOL` 返回 0，真值为 `include/linux/export.h:89`。
- `definition BUILD_BUG_ON` 返回 0，真值为 `include/linux/build_bug.h:50`。
- `definition module_init` 返回 `within_module_init` false positive，真值为
  `include/linux/module.h:89` 和 `include/linux/module.h:131`。
- `definition likely` 返回 `ftrace_likely_update` false positive，真值为
  `include/linux/compiler.h:44` 和 `include/linux/compiler.h:76`。

修复后：

- `container_of`、`module_init`、`EXPORT_SYMBOL`、`BUILD_BUG_ON`、`likely`
  均可通过 `definition` 查询召回，精确宏定义排在 false positive 之前。
- 宏调用如 `EXPORT_SYMBOL(...)` 和 `container_of(...)` 可解析到宏 symbol。

### RK-LINUX-ACC-005：C 函数符号范围只覆盖声明行，导致调用归属失败

严重度：Critical

状态：已修复。

示例：

- `definition cma_debugfs_init` 返回 `mm/cma_debug.c:200-200`，真实函数体为
  `mm/cma_debug.c:200-211`。
- `definition cma_debugfs_add_one` 返回 `mm/cma_debug.c:161-161`，真实函数体为
  `mm/cma_debug.c:161-198`。

调用方查询：

- `callers cma_debugfs_add_one` 命中 `mm/cma_debug.c:208`，但结果为
  `<module> calls cma_debugfs_add_one`，`symbol_snapshot_id = null`。
- 真值应为 `cma_debugfs_init` 调用 `cma_debugfs_add_one`。

修复后：

- C 函数定义使用完整 `function_definition` 范围。
- `callers cma_debugfs_add_one` 返回 `cma_debugfs_init calls cma_debugfs_add_one`。
- call graph 行保留 caller symbol id 和 caller name。

### RK-LINUX-ACC-006：C `callees` 查询在样本中全部失败

严重度：Critical

状态：已修复。

失败样本：

- `callees cma_debugfs_init` 返回 0；真值包含 `debugfs_create_dir` 和
  `cma_debugfs_add_one`。
- `callees cma_debugfs_add_one` 返回 0；真值包含多次 `debugfs_create_dir` 和
  `debugfs_create_file`。
- `callees do_mmap` 返回 0；真值包含 `__get_unmapped_area`、
  `find_vma_intersection`、`mmap_region` 等。
- `callees start_kernel` 返回 0；真值包含大量启动流程调用。

修复后：

- `callees cma_debugfs_init` 返回函数体内调用。
- `callees cma_debugfs_add_one` 返回多次 debugfs 调用。
- `callees do_mmap` 返回 mmap 路径内调用。

### RK-LINUX-ACC-007：头文件声明、实现和配置 fallback 被混作 `definition`

严重度：High

状态：部分修复。

样本：

- `find_vma` 同时返回 `include/linux/mm.h:4183` 声明、
  `mm/mmap.c:903` 实现和 `mm/nommu.c:640` 配置替代实现。
- `debugfs_create_dir` 同时返回 `fs/debugfs/inode.c:570` 实现、
  `include/linux/debugfs.h:150` 声明和 `include/linux/debugfs.h:298` static inline
  fallback。
- `vfs_read` 同时返回 `fs/read_write.c:554` 实现和 `include/linux/fs.h:2079` 声明。

修复后：

- 顶层函数声明标记为 `function_declaration`，实现使用 `function` 并排在声明之前。
- `definition vfs_read` rank 1 为 `fs/read_write.c:554-583`，声明仍可通过
  `--path include/linux/fs.h` 收窄查询。

剩余影响：

- 还没有独立 declaration-to-implementation typed edge。
- `debugfs_create_dir` 这类配置 fallback 与实现仍需结合 path 或 build config 判断。

### RK-LINUX-ACC-008：C include 边可召回但全部 unresolved，且 target 查询混入 source 文件内部 include

严重度：Medium

状态：已修复。

样本：

- `imports linux/debugfs.h` 召回 `fs/debugfs/inode.c:23` 和 `mm/cma_debug.c:9`，
  但也混入 `include/linux/debugfs.h` 文件内部的 `#include <linux/fs.h>`、
  `#include <linux/seq_file.h>` 等。
- `imports linux/mm.h` 召回 `fs/exec.c:30`，但大量结果来自
  `include/linux/mm.h` 文件内部 include。
- 所有 C include 命中的 `edge_resolution_state` 均为 `unresolved`。

修复后：

- import 查询评分不再用 source path 匹配目标文本，避免把头文件内部 include 混入。
- `<linux/debugfs.h>` 和 `<linux/mm.h>` 能解析到 indexed header path，并以
  `edge_resolution_state=resolved` 返回。

### RK-LINUX-ACC-009：宏调用被当作普通 call reference，但无法解析到宏定义

严重度：Medium

状态：已修复。

样本：

- `references EXPORT_SYMBOL` 能召回 `EXPORT_SYMBOL(...)` 用法，但 top 50 全部
  `unresolved`。
- `hybrid container_of` 能召回 `container_of(...)` 用法，但都是
  `call/unresolved`，没有宏定义目标。

修复后：

- 宏定义以 `macro` kind 入库。
- 宏调用仍作为 call/reference 检索层返回，但 `target_hint` 和
  `target_symbol_snapshot_id` 指向宏 symbol，不再全部 unresolved。

## 结论

修复后，Linux C 精确样本中的宏定义、普通函数定义、调用方、被调用方、宏引用和
include 目标解析均达到可用准确性。tree-sitter-c 原生 tags 的宏缺失和
function_declarator 单行范围问题由项目侧 C 抽取层补齐。

剩余主要问题转为规模化索引能力：Linux 子系统级 scope 在 220 秒内仍未完成，说明后续
需要把当前单次内存 snapshot 架构升级为分批解析、分批 SQLite 落盘、可恢复
checkpoint、进度观测和资源预算模型。
