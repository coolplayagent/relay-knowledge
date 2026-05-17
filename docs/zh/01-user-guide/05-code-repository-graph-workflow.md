# 第 5 章 代码仓库图谱工作流

[中文](../../zh/01-user-guide/05-code-repository-graph-workflow.md) | [English](../../en/01-user-guide/05-code-repository-graph-workflow.md)

代码仓库图谱把 Git tree、文件、符号、引用、调用和 import 关系纳入同一检索面。它不是简单的文件搜索；查询和影响分析都依赖已索引的代码图谱快照。

## 5.1 注册仓库

把 Git 仓库注册为代码检索源:

```bash
relay-knowledge repo register /path/to/repo \
  --alias core \
  --path src \
  --language rust \
  --format json
```

`--alias` 是后续命令使用的短名。`--path` 和 `--language` 可以重复；注册时定义的范围会限制索引、查询和影响分析，后续请求只能收窄范围，不能扩大范围。

注册只记录仓库根路径、alias 和允许 scope，不立即解析文件。路径必须指向本机可读 Git worktree；索引时再解析目标 ref 或 worktree overlay。再次注册同一个 Git root 时会为同一个 repository id 增加 alias，不会让旧 alias 失效；如果 alias 已经属于另一个 repository id，注册会失败。

## 5.2 Scope 预览

索引前预览当前 scope 会覆盖哪些文件:

```bash
relay-knowledge repo scope preview core --ref HEAD --format json
```

`repo index --dry-run` 使用同一个 preview path:

```bash
relay-knowledge repo index core --ref HEAD --dry-run --format json
```

preview 适合在收窄 `--path` 或 `--language` 后确认不会把无关目录写入代码图谱。默认 source preset 会排除常见生成目录、二进制/媒体资产、`*.jsonl` 数据集转储和 `uv.lock` 这类锁文件快照；确实需要检索时，用精确 `--path` 注册或请求对应文件即可显式纳入。

## 5.3 建立代码图谱索引

索引当前 `HEAD`:

```bash
relay-knowledge repo index core --ref HEAD --format json
```

索引不可变提交更适合复现实验:

```bash
relay-knowledge repo index core --ref <commit-sha> --format json
```

全量索引通过 Git 读取 clean tree，tree-sitter 解析 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift 和 Bash。Unsupported、invalid UTF-8、binary、oversized 或 parser 失败文件会降级为 text-only 或 failed diagnostics，不会让整个批次失败。

## 5.4 符号与关系查询

混合查询:

```bash
relay-knowledge repo query core \
  --query retry_policy \
  --kind hybrid \
  --ref HEAD \
  --path src \
  --language rust \
  --freshness wait-until-fresh \
  --limit 10 \
  --format json
```

按窄类型查询:

```bash
relay-knowledge repo query core --query RetryPolicy --kind symbol --format json
relay-knowledge repo query core --query retry_policy --kind definition --format json
relay-knowledge repo query core --query retry_policy --kind references --format json
relay-knowledge repo query core --query retry_policy --kind callers --format json
relay-knowledge repo query core --query retry_policy --kind callees --format json
relay-knowledge repo query core --query crate::retry_policy --kind imports --format json
```

结果包含 repository id、alias、`scope_id`、requested ref、resolved commit、tree hash、path、language、byte range、line range、symbol/file id、retrieval layer、index version、freshness、score 和 excerpt。

branch、tag 和 `HEAD` 会先解析到 commit/tree；同一 tree hash 的多个 branch 复用同一 scope，但响应仍保留本次请求的 ref 作为审计信息。rebase 或 force-move 后的新 head 必须先重新索引，否则查询会失败而不是返回旧 branch 内容。

符号命中同时返回 `canonical_symbol_id`，用于跨快照表达逻辑符号身份。引用、调用和 import 命中会返回 `edge_kind`、`edge_resolution_state`、`edge_target_hint`、`edge_confidence_basis_points` 和 `edge_confidence_tier`。当目标无法唯一解析时，结果会标记为 `unresolved` 或 `ambiguous`，不会把猜测写成确定调用。

## 5.5 增量更新

索引两个 ref 之间的变化:

```bash
relay-knowledge repo update core --base main --head HEAD --format json
```

`repo update` 会把 `base` 到 `head` 的 diff 应用到已持久化的 `base` snapshot。`base` 不需要是当前 active snapshot；只要同一 repository id、path filter 和 language filter 下曾经索引过该 base commit，增量更新就会从对应 persisted scope 克隆并只解析变化文件。

如果 CLI 报告找不到 matching indexed base scope，先索引目标 base:

```bash
relay-knowledge repo index core --ref main --format json
relay-knowledge repo update core --base main --head HEAD --format json
```

增量路径读取 `git diff --name-status --find-renames -z`，只重建新增、修改、复制、重命名或类型变化的文件。删除和重命名源路径会从 cloned base index 移除，rename lineage 会保留为 tombstone。

## 5.6 Worktree Overlay

需要索引未提交工作区时使用 `--ref worktree`:

```bash
relay-knowledge repo index core --ref worktree --format json
relay-knowledge repo query core --query retry_policy --ref worktree --format json
```

overlay 绑定当前 checked-out `HEAD`，使用合成 snapshot 标识，包含已修改和未跟踪文件。overlay 活跃时，clean commit ref 查询会被拒绝，避免把未提交内容误标成 clean Git snapshot。

## 5.7 影响分析

分析 diff 影响:

```bash
relay-knowledge repo impact core \
  --base main \
  --head HEAD \
  --limit 100 \
  --format json
```

影响分析会验证 `head_ref` 对应已索引 snapshot，按注册 scope 过滤 changed paths，再用模块、符号、caller、import 和已删除符号名推导受影响位置。

## 5.8 报告与状态

生成可读报告:

```bash
relay-knowledge repo report core --format markdown
```

脚本使用 JSON:

```bash
relay-knowledge repo report core --format json
relay-knowledge repo status core --format json
```

报告包含 repository id、root、indexed commit、tree hash、文件/符号/reference/chunk 总量、scope、代表性查询、延迟样本和 degradation summary。Markdown 报告适合贴进 PR 或发布说明；JSON 报告适合 CI 比较索引质量。

`repo report --format markdown` 还会汇总 edge resolution: resolved、ambiguous 和 unresolved 数量，用于判断当前代码图谱是否主要来自确定 AST 提取，还是存在大量需要人工或后续解析器改进的模糊边。

## 5.9 排障顺序

`repo query` 结果为空时，按顺序确认:

1. `repo status <alias>` 是否显示已索引的 clean commit 或 worktree overlay。
2. 查询时的 `--ref` 是否与已索引 snapshot 一致。
3. 请求的 `--path` 和 `--language` 是否只是在注册 scope 内进一步收窄。
4. `--kind` 是否过窄；不确定时先用 `--kind hybrid`。
5. 文件是否被诊断为 unsupported、binary、oversized、invalid UTF-8 或 parser failed。

`repo impact` 需要 `--head` 对应已索引 snapshot。先运行 `repo index core --ref <head>` 或 `repo update core --base <base> --head <head>`，再运行 impact。
