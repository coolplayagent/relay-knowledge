# Source Scope 模型

[中文](../../zh/03-architecture-specs/04-source-scope-model.md) | [English](../../en/03-architecture-specs/04-source-scope-model.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

Source scope 是整个系统的授权、版本和索引分区基础。没有 scope，检索结果无法说明它来自哪个知识边界；没有 snapshot，代码和文档检索无法抵抗 rebase、路径过滤和脏工作树造成的语义漂移。

## 2. Scope 类型

| Scope | 用途 | 真源 |
| --- | --- | --- |
| `document_scope` | 文档目录、文件、导入批次 | normalized source path + content hash |
| `repository_snapshot` | clean Git 快照检索 | repository id + commit sha + tree hash + filters |
| `git_changeset` | review、impact、diff 证据 | base/head commit + changed path set |
| `worktree_overlay` | 显式脏工作树分析 | clean snapshot + overlay identity |
| `runtime_scope` | service、worker、diagnostic 事件 | runtime instance + component identity |

所有事实、索引 cursor、audit event 和 context item 都必须携带 scope 或可追溯到 scope。
本地文件定位索引使用显式 document scope，例如 `user-documents` 或
`local-files:<name>`。Root 可以指向用户文档目录、Linux `/opt`、挂载盘或
Windows `D:\` 等盘符根，但扫描器不得把一个已授权 root 自动扩大为全盘扫描。

## 3. 规范化算法

Scope 构造遵循固定流程：

1. 解析用户输入和授权范围。
2. 规范化路径、语言、source kind 和 alias。
3. 把 branch、tag、HEAD、PR ref 解析为 commit/tree。
4. 拒绝可被解释为命令选项的 ref，例如以 `-` 开头的输入。
5. 合并 path filter 和请求期 language filter，并生成稳定 filter hash；仓库注册不会贡献 language filter。
6. 查找或创建 scoped index partition。

查询不得把窄 filter 自动回退到宽 scope；需要新 filter 组合时必须先索引该 scope。

仓库源码规范化必须把 source root 视为一组布局，而不是单一 `src/` 约定。支持的
source-root candidate 包括 `src/`、`lib/`、`Sources/`、`external_deps/`、
`packages/`、`modules/`、`plugins/`、`extensions/`，以及这些目录下的嵌套
JVM source root。对于 clean Git snapshot，注册和请求 path scope 内的 tracked tree
是目录权威：Git 跟踪的 `.cloudbuild/`、`.cid/`、`.build_config/`、`build/`、
`dist/`、`vendor/` 和 `third_party/` 路径都会作为候选，而不会只因目录名被拒绝。
保留的默认 preset 是文件级保护，用于二进制/媒体文件和 `*.jsonl` 这类数据集转储。
source-root discovery 仍不会把刻意收窄的 `--path src` 注册自动扩大到宽泛依赖树；
worktree overlay 也不会递归展开未跟踪的高容量依赖、缓存或构建目录，除非显式
path filter opt in。

非 Git source directory 使用 filesystem synthetic snapshot，而不是 Git history。
由于没有 tracked tree 作为目录权威，默认扫描采用白名单：根层支持文件和 `src/`、
`include/`、`lib/`、`Sources/`、`packages/`、`modules/`、`plugins/`、
`extensions/`、`docs/`、`config/` 等 source-like roots 可以进入索引；宽泛构建、
依赖、缓存、virtualenv 和 coverage 目录必须显式 path opt in 才会扫描。这个
opt in 必须是路径特异的：`--path src` 不允许顺带扫描兄弟级 `node_modules/`、
`target/` 或 `vendor/`，只有 `--path build` 或 `build/` 下的具体路径才允许该宽泛
目录进入 filesystem synthetic snapshot，`--path .` 则显式 opt in 整个非 Git root，
包括宽泛目录。没有 path filter 的默认扫描只能进入会贡献根层白名单文件或白名单
source/config/documentation root 的目录。带过滤条件的扫描只能进入请求路径和有界的
可发现 source root；无关兄弟目录必须在 `read_dir` 可能失败或消耗无关时间之前跳过。
如果 Git 探测发现 Git metadata，但因为 unsafe ownership、缺少 Git 支持或 `.git`
metadata 损坏而无法解析仓库，注册和 ref resolution 必须失败，不能静默降级为非 Git
filesystem source。

`filesystem:` synthetic commit 必须绑定到 source-layout discovery 后的有效索引
scope，而不是所有已扫描文件。注册/请求 path、language 和非 Git 默认白名单之外的
未索引文件变化，不能移动这个 scoped commit。Pre-scope filesystem hash pass 也必须先应用
file preset 再读取内容：media、binary、`*.jsonl` 和其他默认排除文件不会被读取或 hash，
除非显式 path filter opt in 到该文件。非 Git moving-ref resolution 用于 query、status、
repository set 和 impact 时，必须使用识别 indexed scope 的同一组有效 path 和 language
filters。repository-set 的更窄 filter 成员必须先通过兼容的 active 更宽非 Git commit
解析，不能要求 storage 中已经存在更窄 synthetic commit；repository-set freshness
check 也必须复用同一个兼容 commit 后才能把 moving member 标为 stale。全量索引任务重放排队的
`filesystem:` ref 时，必须重新计算同一个 scoped hash；如果 live indexed scope
已经变化，任务必须失败或重试，不能把另一个 live tree 静默索引到已排队的 commit
名下。非 Git full-index plan 或同步 full-snapshot build 接受 synthetic scope 后，
每次后续读取 live bytes 前都必须校验计划中的逐文件 content hash；filesystem 增量和
worktree-overlay delta 在解析前也必须拒绝不再匹配计划 content hash 的 bytes。非 Git 文件 byte、hash 和
metadata materialization 必须用 `symlink_metadata` 逐段重新检查 selected path，并在
读取或计量文件前拒绝最终路径或祖先目录 symlink 替换。显式的已存储
`filesystem:` ref 必须先于动态 source-kind 或 Git 探测按身份解析给 storage lookup，
让本地编辑后或 source directory 后来加入 Git metadata 后的旧 indexed scope 仍可查询；
source byte materialization 在读取前校验 live tree，且 `filesystem:` ref 必须先走
filesystem snapshot 校验路径而不是重新探测 Git，并使用与 indexed scope 相同的有效
path 和 language filters。非 Git 增量更新遇到显式 `base_ref` 时，必须解析并加载该已存储 scope 的
fingerprints，不能静默改用当前 active scope。增量更新必须同时使用当前和上一版
source-layout discovery，这样删除某个 discovered root 中最后一个文件时，旧 indexed
path 仍会被删除。当更宽的非 Git index task 正在运行时，较窄的 stale-read 请求只有在用
task 的有效 path/language filters 同时比较 task scope 和 requested selector scope 后才能被
服务。非 Git impact changed-path 收集必须先于 source-kind 探测检查显式 `filesystem:`
base/head refs，并带着这些 path 和 language filters 先解析并比较 scoped base/head refs；
二者相同时必须返回空 changeset，否则必须使用 indexed effective filesystem filters，
包括显式 opt in 的宽泛目录。Impact path partition 和 deleted-symbol extraction
也必须先于 Git 探测处理显式 `filesystem:` ref；空 filesystem changeset 不能强制作
snapshot 重新探测。Git 的 query/status ref normalization
和 fresh full-index reuse check 必须保留廉价 `rev-parse`/tree-id 路径，不能只为
解析 ref 或证明已有 scope fresh 而执行接近索引规模的 tree walk。

## 4. Git 快照规则

- `repository_id` 是本地稳定身份，不等于 alias。
- 同一 tree hash 可以复用索引分区，但保留 requested ref 的审计元数据。
- rebase 或 force move 产生新 head 时必须产生新 scope。
- dirty worktree 不能混入 clean snapshot；只能作为 `worktree_overlay` 或 `git_changeset`。

## 5. 授权和隔离

Scope policy 先于检索、索引、MCP tool、Web operation 和 worker task 生效。任何 adapter 只能请求授权后的 scope；不能在查询过程中扩大路径、语言或仓库范围。

## 6. 验收标准

- 每个 context item 能说明 `scope_id`、source kind、版本和 filter。
- 代码仓库查询在同 commit 不同 path filter 下不会交叉污染。
- rebase 后旧 scope 只用于历史审计或显式指定查询。

---

导航: 上一章: [3. 基础运行时层](03-foundational-runtime.md) | 下一章: [5. 多模态证据摄取](05-multimodal-evidence-ingestion.md)
