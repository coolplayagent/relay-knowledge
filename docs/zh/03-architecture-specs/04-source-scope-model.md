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
JVM source root。默认排除 preset 仍保护普通 `vendor/`、`third_party/` 这类
高容量依赖转储；这些路径必须通过显式 path filter opt in 后才能进入仓库 scope。

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
