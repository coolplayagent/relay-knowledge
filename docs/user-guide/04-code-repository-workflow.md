# 第 4 章 代码仓库工作流

## 4.1 注册仓库

把 Git 仓库注册为代码检索源:

```bash
relay-knowledge repo register /path/to/repo \
  --alias core \
  --path src \
  --language rust \
  --format json
```

`--alias` 是后续命令使用的短名。`--path` 和 `--language` 可以重复，注册时定义的范围会限制索引、查询和影响分析，后续请求只能收窄范围，不能扩大范围。

## 4.2 全量索引

索引当前 `HEAD`:

```bash
relay-knowledge repo index core --ref HEAD --format json
```

索引不可变提交更适合复现实验:

```bash
relay-knowledge repo index core --ref <commit-sha> --format json
```

全量索引通过 Git 读取 clean tree，tree-sitter 解析 Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift 和 Bash。Unsupported、invalid UTF-8、binary、oversized 或 parser 失败文件会降级为 text-only 或 failed diagnostics，不会让整个批次失败。

## 4.3 代码查询

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

结果包含 repository id、alias、`scope_id`、requested ref、resolved commit、tree hash、path、language、byte range、line range、symbol/file id、retrieval layer、index version、freshness、score 和 excerpt。branch、tag 和 `HEAD` 会先解析到 commit/tree；同一 tree hash 的多个 branch 复用同一 scope，但响应仍保留本次请求的 ref 作为审计信息。rebase 或 force-move 后的新 head 必须先重新索引，否则查询会失败而不是返回旧 branch 内容。

## 4.4 增量更新

索引两个 ref 之间的变化:

```bash
relay-knowledge repo update core --base main --head HEAD --format json
```

增量路径读取 `git diff --name-status --find-renames -z`，只重建新增、修改、复制、重命名或类型变化的文件。删除和重命名源路径会从 active index 移除，rename lineage 会保留为 tombstone。

## 4.5 Worktree overlay

需要索引未提交工作区时使用 `--ref worktree`:

```bash
relay-knowledge repo index core --ref worktree --format json
relay-knowledge repo query core --query retry_policy --ref worktree --format json
```

overlay 绑定当前 checked-out `HEAD`，使用合成 snapshot 标识，包含已修改和未跟踪文件。overlay 活跃时，clean commit ref 查询会被拒绝，避免把未提交内容误标成 clean Git snapshot。

## 4.6 影响分析

分析 diff 影响:

```bash
relay-knowledge repo impact core \
  --base main \
  --head HEAD \
  --limit 100 \
  --format json
```

影响分析会验证 `head_ref` 对应已索引 snapshot，按注册 scope 过滤 changed paths，再用模块、符号、caller、import 和已删除符号名推导受影响位置。

## 4.7 仓库状态

```bash
relay-knowledge repo status core --format json
```

状态输出用于确认当前索引 ref、文件数量、symbol/reference/chunk 总量、诊断和 freshness。若 `repo status` 与 `graph inspect` 的 code counts 看起来不一致，以 `repo status` 为代码索引诊断入口；`graph inspect` 更偏向通用图状态。
