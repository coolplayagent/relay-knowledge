# 混合检索竞争力

[中文](./05-hybrid-retrieval-advantage.md) | [English](../../en/02-capabilities/05-hybrid-retrieval-advantage.md)

> 文档版本: 2.0
> 编制日期: 2026-05-17
> 适用范围: 第二卷能力说明

## 能力定位

混合检索是第二卷最核心的竞争力能力。它同时使用 BM25、local semantic token read model、local hashed-vector ANN、可配置 external semantic/vector backend、graph evidence fallback、code graph documents、有界代码 exact-text source fallback、local file path/content read model、schema path、temporal event、community summary 和 RRF。

## 用户可见行为

- 查询结果带 retriever sources 和 ranking explanation。
- BM25 会索引 entity 和 code symbol 的生成式 lexical alias，但不把 alias 当 canonical label 返回。
- Graph paths 保留节点标签、edge fact id、predicate、supporting evidence ids、confidence、status 和 version range。
- Temporal、community 和 code graph 信号可以与普通 evidence 一起进入 context pack。
- 代码 exact-text 兜底命中以 `lexical`/`text_fallback` provenance 进入结果，不伪装成 resolved graph edge。
- 本机文件结果区分 path、metadata、content 和 change cursor freshness；文件名/路径查询不依赖内容索引。

## 竞争力特性

普通全文搜索容易漏概念相似，普通向量搜索容易漏精确符号，普通图查询缺少自然语言召回，普通桌面文件搜索又常缺少图谱和 agent context。混合检索把这些信号融合后再做预算分配，能同时服务事实问答、代码定位、本机文件定位、多跳关系和 agent 上下文构造。

## 命令/API 入口

```bash
relay-knowledge query "retry policy graph path"   --freshness wait-until-fresh   --limit 10   --format json
```

## 降级与诊断

Semantic/vector backend disabled 或 cursor stale 时，BM25 和 graph evidence 仍可工作。响应的 `context_pack.backend_statuses` 会说明 configured backend、model、dimension、scope post-filter 和 indexed graph version。
代码 source fallback 候选路径或预算耗尽时，只降级 exact-text 兜底层；已有 BM25、code graph edge 和 graph evidence 仍可进入 context pack。
本机文件 content cursor stale 时，path/metadata 仍可服务文件定位；响应需要说明 content stale、watcher lag 或 bounded rescan 状态。

### BM25 多级降级策略

BM25 检索路径内部实现三级降级链，最大化召回率的同时保持排序质量：

```
FTS5 prefix match (BM25 评分)
  ↓ 结果为空且 query ≥ 2 字符
精确名匹配 (JSON-safe entity_labels LIKE / LOWER(content))
  ↓ 结果为空
LIKE 子串搜索 (content LIKE '%query%' ESCAPE '\')
  ↓ 结果为空且 query ≥ 3 字符
Levenshtein fuzzy search (edit distance ≤ 1..2)
```

**性能保底**：
- 精确名匹配使用 JSON 编码后的 `LIKE '%"target"%'` pattern 支持多标签实体和转义后的 label 字符
- LIKE fallback 在参数绑定前转义 `\`、`%` 和 `_`
- 所有 WHERE 子句将 OR 条件包裹在括号内，确保 scope 和 version 过滤对全部分支生效
- Levenshtein 使用维护在 SQLite 中的 `graph_bm25_label_grams` label gram 索引，按 query-specific gram overlap 和 label length bound 收集 scope/version 候选，避免扫描 graph documents 或截断任意 anchor rows
- label gram schema 和 backfill 由 SQLite schema marker version 保护，通过比较每个 document 的 expected grams 恢复未完成升级，并在构造 SQL bind 参数前限制 query grams 数量
- fuzzy 匹配先应用 gram-overlap 候选上限，再由 Rust Levenshtein 评分，并在 matched-name cap 前按 edit distance 排序
- fuzzy 结果通过 label-gram document ids 批量 join 已排序 name，保留该 name 的 edit-distance score 参与结果排序，避免 per-name leading-wildcard 扫描或单次跨 name SQL `LIMIT` 丢掉更近的匹配
- fallback SQL 会先限制 rows，再做确定性内存排序，避免 leading-wildcard LIKE 路径触发无界 SQL sort
- edit distance 上限随 query 长度自适应：≤ 4 字符 → max dist 1，> 4 字符 → max dist 2
- 降级为互斥瀑布式：前级有结果则跳过后续级，结果按 document_id 去重
- 所有 SQL 查询均使用 `graph_bm25.` 表前缀消除歧义

**适用场景**：
- 用户拼写错误（如 `getUssr` → `getUser`）
- 子串查询（如 `sign` → `signInWithGoogle`）
- 短词查询（FTS 前缀匹配噪音太大时）

## 关联架构章节

- [混合检索与 Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Semantic/Vector Provider 架构](../03-architecture-specs/10-semantic-vector-provider-architecture.md)

---

导航: 上一章: [4. 查询与 Context Pack 基础](04-query-and-context-pack-basics.md) | 下一章: [6. 新鲜度与索引恢复](06-freshness-and-index-recovery.md)
