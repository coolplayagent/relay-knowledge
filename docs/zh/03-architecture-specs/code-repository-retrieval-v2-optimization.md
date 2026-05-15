# 代码仓库检索 v2 优化

[中文](../../zh/03-architecture-specs/code-repository-retrieval-v2-optimization.md) | [英文](../../en/03-architecture-specs/code-repository-retrieval-v2-optimization.md)

> 状态：实现规格
> 范围：仓库索引易用性、代码查询性能、诊断，以及本地确定性 semantic/vector 检索

## 总结

本阶段把 relay-teams E2E 后续问题清单转化为产品行为，并将混合检索从 Phase 1 的 “semantic/vector unavailable” 状态继续推进。v2 后端刻意保持本地和确定性：SQLite 存储概念 token 文档，以及哈希 token/字符 n-gram 向量，使测试、离线安装和 CI 不需要 embedding 服务。

未来 embedding provider 可以在同一检索来源契约后替换确定性向量写入器和搜索器。公开响应形状已经报告 `semantic` 与 `vector` 后端状态、排序信号、索引版本、新鲜度和降级信息。

## 关键变化

- `repo index --dry-run` 和 `repo scope preview <alias>` 返回不修改状态的 scope preview，包含选中文件数、字节数、语言分布、最大的选中文件、排除路径、不支持文件、生成/重型文件估算，以及预期降级文件。
- 默认 source preset 排除常见生成路径、重型路径和资源，例如 `dist`、`build`、`target`、`node_modules`、cache、vendored 目录、PDF、archive、font、image、video、source map、wasm、`*.jsonl` 这类按行数据集 dump，以及 `uv.lock` 这类 lockfile snapshot。
- Git 仓库根目录下的 `.relay-knowledgeignore` 提供可重复的仓库本地排除规则。支持空行、注释、目录名、锚定路径和 `*.extension` 模式。ignore 规则只能收窄有效 scope。
- `repo index` 摘要现在包含 Git 枚举/blob 读取、已解析文件、SQLite 写入、跳过未变更文件和降级文件的进度计数。
- `repo impact` 通过 `path_groups.in_scope_changed_paths` / `path_groups.out_of_scope_changed_paths` 返回 scope 内外路径分组。
- `repo report <alias> --format markdown|json` 输出注册 scope、已解析 commit/tree、索引总量、降级摘要、代表性查询、延迟样本和新鲜度状态。
- `graph inspect` 和 `health` 在图代码计数中包含仓库代码总量，使 API consumer 不会看到误报为空的代码图；同一仓库专用计数仍保留在 `repository_code_totals` 下。
- 代码仓库查询在对 symbol、reference、call、import 和 chunk 进行内存评分前，先使用 SQLite candidate predicate 和 limit。
- `repo query <alias> --query runtime tools role` 接受未加引号的尾随查询词，直到遇到下一个 option flag。
- 重复根目录注册会保留现有 alias，并把新 alias 添加到同一 repository id。不同 repository id 之间的 alias 冲突会被拒绝。
- 即使活动仓库状态已经指向另一个 head，`repo update` 也可以使用持久化的匹配 base snapshot。

## 检索模型

本地 v2 检索模型会在写入 BM25 文档时同步写入派生文档：

- `retrieval_semantic_documents` 存储从内容、实体标签、来源路径、代码符号和代码块中提取的归一化概念词项。
- `retrieval_vector_documents` 存储向量文档元数据和向量范数。
- `retrieval_vector_terms` 存储确定性 FNV 哈希 token 和字符 trigram 权重。

混合检索现在使用互惠排名融合整合 BM25、图证据、代码图、semantic 和 vector 候选。Context pack 排序元数据会记录每个贡献检索器的来源、排名、来源得分和解释。使用本地 SQLite 读模型时，`semantic` 与 `vector` 后端状态为 `available`。

## 接口

- CLI:
  - `relay-knowledge repo index <alias> --dry-run [--ref <ref>]`
  - `relay-knowledge repo scope preview <alias> [--ref <ref>]`
  - `relay-knowledge repo report <alias> --format markdown|json`
  - `relay-knowledge repo query <alias> --query multi word query`
- API:
  - `CodeRepositoryScopePreviewResponse`
  - `CodeRepositoryReportResponse`
  - `CodeIndexProgressSummary`
  - `CodeImpactPathGroups`
  - `CodeRepositoryTotals`，包含仓库解析状态计数
- 存储：
  - repository totals 和 report 仍位于 `CodeRepositoryStore` 后面。
  - repository alias 仍位于 storage 边界后面；调用方通过同一 repository status contract 解析 alias。
  - semantic/vector 行仍是 SQLite 读模型，不是 domain fact。

## 测试

必要覆盖：

- preview 计数、语言 bucket、默认 preset 排除和 `.relay-knowledgeignore` 排除。
- impact 的 in-scope/out-of-scope 路径分组。
- dry-run、scope preview、report、markdown 格式和多词代码查询的 CLI 解析。
- identifier 变体的确定性 semantic/vector 检索。
- 重复根目录 alias 保留和 alias 冲突处理。
- 索引另一个活动 head 后的持久化 base 增量更新。
- no-op index、persisted-base update、hybrid query 和 impact 延迟预算的 benchmark 回归门禁。
- graph inspect 和 health 响应中的 repository totals。
- 优化后的代码查询路径保留现有 definition/reference/import 结果。

质量门禁保持不变：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --test benchmarks --all-features -- --nocapture
```

## 假设

- 本阶段不配置外部 embedding provider。
- 默认 source preset 默认启用，除非用户显式把注册或请求路径收窄到生成区域，或收窄到特定 `*.jsonl` 文件、`uv.lock` 等已排除资源。
- `.relay-knowledgeignore` 排除规则不能扩大已注册 scope，也不能替代 Git 授权或 selector 校验。
- 真实 embedding、跨仓库检索、LLM reranking 和多模态向量仍是同一检索来源契约后的未来扩展。
