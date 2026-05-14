# 代码仓库 Tree-sitter 检索功能文档

[中文](./code-repository-tree-sitter-retrieval.md) | [英文](../../en/02-capabilities/code-repository-tree-sitter-retrieval.md)

`relay-knowledge` 将 Git 仓库作为一等代码来源。CLI 与 application service 共用同一组 async API，覆盖仓库注册、tree-sitter 索引、代码检索和影响分析。

## 命令

```bash
relay-knowledge repo register /path/to/repo --alias core --path src --language rust
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo query core --query retry_policy --kind definition --ref HEAD --path src --language rust --freshness wait-until-fresh --limit 10 --format json
relay-knowledge repo query core --query retry_policy --kind references --ref HEAD --format json
relay-knowledge repo query core --query crate::retry_policy --kind imports --ref HEAD --format json
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo impact core --base main --head HEAD --format json
relay-knowledge repo status core --format json
```

`--kind hybrid` 会同时检索 symbol、definition、reference、import、call 和 chunk。窄类型包括 `symbol`、`definition`、`references`、`callers`、`callees` 和 `imports`。基于 diff 的影响分析通过 `repo impact` 提供；普通 `repo query` 不接受 `impact` 类型，避免把变更集结果与混合检索结果混淆。

`repo query` 还支持 `--limit`、`--ref`、可重复 `--path`、可重复 `--language`，以及 `--freshness allow-stale|wait-until-fresh|graph-only`。Symbol 命中同时包含 snapshot 绑定的 `symbol_snapshot_id` 和稳定的 `canonical_symbol_id`。Reference、caller/callee、import 和 impact 命中会暴露边元数据：`edge_kind`、`edge_resolution_state`、`edge_target_hint`、`edge_confidence_basis_points` 和 `edge_confidence_tier`。

## relay-teams E2E 结论

`relay-teams` 仓库已经通过 CLI 作为端到端代码检索来源验证。可复现的交互式基线使用不可变 commit ref：

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-src-e2e \
  relay-knowledge repo register /opt/workspace/relay-teams \
  --alias relay-teams-src \
  --path src/relay_teams \
  --language python \
  --format json

RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-relay-teams-src-e2e \
  relay-knowledge repo index relay-teams-src \
  --ref a6063949f4c526ce0e4eddf09d627f5f26c69df7 \
  --format json
```

该运行在 Python 生产源码范围内索引了 691 个文件、13,399 个 symbol、82,460 个 reference 和 13,402 个 chunk，没有降级文件。Definition、reference、import、caller 和 hybrid 查询都返回了按 revision 约束的命中，并携带 resolved commit、tree hash、path、line range、retrieval layer、index version、freshness、score 和 excerpt metadata。

较宽的试验范围 `src`、`tests`、`docs` 和 `frontend` 不适合交互式全量索引，因为它会纳入生成的前端资产、PDF、大型文档和大型 UI test fixture。当前 CLI 需要在长时间 full-index 操作开始前提供更多 preflight 和 progress 信息。

后续改进项：

- 增加 `repo index --dry-run` 或 `repo scope preview`，在索引前展示文件数量、字节数、语言分布、最大文件、不支持文件、生成资产和预期降级文件。
- 全量索引时展示 progress 和 budget，包括 Git 文件枚举、blob 读取、parser 工作、SQLite 写入、耗时、跳过文件、降级文件和当前 scope。
- 让默认 source preset 和排除规则匹配实际索引成本。默认 preset 排除 `dist`、build output、cache directory、PDF、vendored asset、`*.jsonl` 数据集 dump 和 `uv.lock`；用户可通过显式 path filter opt in。仓库本地 `.relay-knowledgeignore` 可让额外排除规则可重复。
- `graph inspect` 和 `health` 的代码计数应包含代码仓库索引总量，或明确标注为 graph evidence 计数。E2E 运行中 `repo status` 已显示代码索引总量，但 `graph inspect` 显示零代码文件和零 symbol，容易误读为索引失败。
- `repo impact` 的路径报告应区分 scope 内外变更，或默认只展示注册 scope 内的 `changed_paths`。当前 impact 命中遵守注册 scope，但路径报告仍包含无关 docs、frontend 和 test 变更。
- 大型代码索引需要优化查询执行。Python 源码基线对聚焦场景可接受，但 hybrid retrieval 明显慢于 definition 和 reference lookup。Symbol、reference、call、import 和 chunk search 应先使用 SQLite predicate 或 FTS-backed candidate selection，再进入内存评分。
- 改善多词查询参数体验。类似 `repo query relay-teams-src --query runtime tools role` 的命令目前会在 `runtime` 后失败；错误应解释引号用法，或在无歧义时接受剩余词作为 query。
- 增加 `repo report <alias> --format markdown|json`，输出可复用运维报告，包含注册 scope、resolved commit、tree hash、index totals、degradation summary、representative queries、latency samples 和 freshness state。

v2 实现规格和本地 deterministic semantic/vector 检索基线维护在 [代码仓库检索 v2 优化规格](../03-architecture-specs/code-repository-retrieval-v2-optimization.md)。

## 实现

- Git registration 解析 repository root，并从 `remote.origin.url` 与本地 root 派生稳定 `repository_id`；无 origin 时回退到绝对 root path。Status lookup 先查 `repository_id`，再回退到 alias lookup，因此不与 repository id 冲突的 `repo:` alias 仍可访问。
- Full indexing 使用 `git ls-tree` 和 `git show` 读取干净 Git tree。
- Incremental indexing 使用 `git diff --name-status --find-renames -z`，只重新解析 changed、copied、renamed 或 type-changed path。被选中的 deleted 和 renamed path 会从 active index 删除，copy source 不参与 impact seed，rename lineage 以 tombstone 保留。复用旧 file fingerprint 前，incremental base ref 必须解析到当前已索引 snapshot。
- Worktree overlay 模式会把变更的 worktree file 索引到 synthetic `worktree:<hash>` tree id 和 `worktree:<commit>:<hash>` resolved snapshot identity。查询必须使用 `--ref worktree` 读取 overlay row；overlay 激活时拒绝 clean commit ref，避免把未提交内容标记为干净 Git snapshot。
- Parser work 通过 application-level `spawn_blocking` 边界执行，SQLite 写入也保留在 storage blocking worker 后面。
- Rust、Python、JavaScript/JSX、TypeScript/TSX、Go、Java、Kotlin、Scala、C、C++、C#、Ruby、PHP、Swift 和 Bash 文件使用 tree-sitter grammar。包含 error node 的 syntax tree 会以 `partial` 状态索引并记录 file diagnostic，同时保留可靠的 symbol、reference、import、call 和 chunk。不支持、非法 UTF-8、二进制或超大文件会在可能时降级为 text-only chunk。Parser 或 query failure 只隔离到受影响文件的 `failed` diagnostic，不会中止整个仓库 batch。

## 存储和检索

- 存储层以 repository、scope、file、symbol、reference、chunk、parse status 和 index cursor 为边界建模。
- BM25 read model 会写入代码 symbol 和 chunk document，使代码图命中可以与 graph evidence、semantic 和 vector 命中一起融合。
- Code query 返回 revision-scoped hit，包含 path、line range、kind、score、freshness、symbol identity、edge diagnostics 和 excerpt。
- Impact analysis 从 changed path、deleted symbol name、callee identity 和 import/module seed 出发，避免扫描整个 scope table。

## 测试

测试覆盖注册、重复注册 alias、full index no-op、incremental update、worktree overlay、symbol/reference/import/call/chunk query、impact analysis、unsupported/degraded file、scope filtering、freshness policy，以及 CLI/Web 共享 application service 行为。
