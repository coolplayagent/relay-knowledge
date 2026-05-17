# 竞争力、高性能与本机文件检索研究 2026

[中文](../../zh/04-research/08-competitive-performance-research-2026.md) | [English](../../en/04-research/08-competitive-performance-research-2026.md)

> 文档版本: 1.0
> 编制日期: 2026-05-17
> 范围: GraphRAG、混合搜索、向量索引、代码搜索、本机文件检索、图存储、权限图、高性能算法和 SRE 实践。

## 1. 研究定位

| 维度 | 结论 |
| --- | --- |
| 研究来源 | 官方产品/规范文档、系统论文、数据库和搜索引擎工程材料、内部基准与现有架构约束。 |
| 研究目标 | 把外部高性能系统经验转化为 relay-knowledge 可执行的竞争力、索引、检索、后台服务和基准建议。 |
| 竞争力判断 | 长期优势不是单一 GraphRAG 算法，而是本地优先、版本一致、权限可控、代码/文件/图谱统一检索、Context Pack 可解释和后台可恢复。 |
| 性能原则 | 先缩小候选，再排序；先过滤授权/scope，再召回；冷热路径隔离；所有索引和后台任务都带预算、新鲜度和降级诊断。 |

## 2. 跨领域结论

- GraphRAG 论文和产品实践共同指向 query router、local/global 检索、社区摘要、路径组织和增量刷新；盲目扩大 k-hop 或 top-k 会增加噪声和 token 成本。
- 向量检索的高性能来自 HNSW、PQ/IVF、磁盘图索引、量化、过滤前置和多阶段重排；向量索引只能作为候选和排序信号，不能成为事实真源。
- 全文与混合搜索的成熟模式是倒排索引、BM25/BM25F、trigram/posting list、RRF 和 phased ranking；不同召回器分数不可直接相加时，rank-based fusion 更稳。
- 代码搜索的竞争力来自精确符号、trigram/regex、BM25、AST 结构、引用/调用/import 边、语言和路径过滤、版本 scope 与影响分析，而不是“代码 embedding”单一路径。
- 本机文件高速检索必须把文件名/路径、metadata、内容和变更游标拆成独立 read model；Everything、Spotlight、Windows Search、plocate 和 ripgrep 是机制参考，不应成为 relay-knowledge 的运行依赖。
- 大规模图和权限系统强调缓存、关系模型、因果一致性、权限过滤前置和低延迟检查；Context Pack 和文件/代码查询不能在最终截断后才做授权。
- 高性能后台运行依赖 bounded queue、lease、dead-letter、replay、adaptive concurrency、timeout、cancellation、trace/metric/log 关联和明确的 overload 行为。

## 3. 参考地图

| 领域 | 代表参考 | 可吸收经验 |
| --- | --- | --- |
| GraphRAG 与 Hybrid RAG | Microsoft GraphRAG/DRIFT、LightRAG、E^2GraphRAG、ROGRAG、Practical GraphRAG、PolyG、EA-GraphRAG | query mode 选择、局部/全局融合、实体-chunk 双向索引、增量构图、路径剪枝和结果验证。 |
| 向量检索 | HNSW、FAISS、DiskANN、ScaNN、ACORN、Vespa constrained ANN | 图近邻、量化、磁盘驻留、过滤感知 ANN、target hits、召回率/延迟/内存权衡。 |
| 全文与混合搜索 | Lucene BM25、Vespa hybrid/phased ranking、OpenSearch RRF、Azure AI Search RRF | 倒排索引、RRF、分阶段排序、过滤前置、可解释 rank contribution。 |
| 代码搜索 | GitHub Code Search/Blackbird、Sourcegraph/Zoekt、Tree-sitter、ripgrep、persistent trigram index | trigram 候选、regex literal 提取、符号优先、AST chunk、ignore 规则、多线程遍历和版本化索引。 |
| 本机文件搜索 | Everything、Windows Search、Spotlight/FSEvents、Linux inotify/fanotify、plocate | 文件名和 metadata 优先、系统变更日志、posting list/trigram、权限可见性、游标溢出后重扫。 |
| 图存储与权限图 | Facebook TAO/RAMP-TAO、Google Zanzibar | 关系图缓存、读多写少优化、权限关系前置、外部一致性和低延迟授权检查。 |
| 存储与更新 | RocksDB/LSM、WAL、mutation log、materialized view | 写入批处理、后台 compaction、崩溃恢复、增量视图和热查询隔离。 |
| 运行可靠性 | Google SRE overload、Envoy adaptive concurrency、OpenTelemetry | overload 防护、自适应并发、retry 抑制、端到端 trace、指标和错误分类。 |

## 4. 本机文件系统高速检索建议

本机文件检索应成为独立派生索引族，而不是普通 evidence 或代码仓库索引的附属功能。建议拆为四个 read model：

| Read model | 内容 | 设计要求 |
| --- | --- | --- |
| `local_file_path` | normalized path、basename、目录 token、扩展名、路径 trigram/posting list | 面向毫秒级文件名/路径检索；必须先应用 source scope、授权 root、ignore/exclude 和 freshness policy。 |
| `local_file_metadata` | size、mtime、hash、mime、language、owner/permission snapshot、hidden/system 属性、symlink 状态 | 用于过滤、排序和诊断；metadata 缺失不应阻断路径查询。 |
| `local_file_content` | 文本 chunk、BM25/trigram、可选 semantic/vector metadata | 内容索引按文件类型、大小和资源预算选择性启用；大文件、二进制、OCR、压缩包进入后台 worker。 |
| `local_file_change_cursor` | Windows USN、macOS FSEvents、Linux inotify/fanotify 或 bounded rescan cursor | 记录 last event、overflow、missed event、scan watermark、stale reason 和下一次 reconcile 入口。 |

推荐查询流程：

```text
normalize file query
  -> resolve authorized local file scope
  -> apply path/exclude/permission/freshness filters
  -> path and metadata candidate recall
  -> optional content/semantic recall
  -> RRF or phased rerank
  -> return hits with freshness, cursor, permission, truncation, and degraded metadata
```

关键约束：

- 不依赖 Everything、Spotlight、Windows Search、locate 或外部守护进程才能工作；平台能力只能作为后续 watcher 后端或导入源。
- 文件名/路径索引和内容索引分离，避免慢内容抽取拖慢交互式文件定位。
- 变更事件不可靠或溢出时，系统返回 degraded/stale reason 并触发 bounded rescan，不静默声称 fresh。
- 权限和 scope 过滤必须在候选窗口前生效，避免未授权路径进入排序、trace 或 context pack。
- 文件系统 worker 必须受 queue capacity、scan timeout、max file bytes、max files per root 和 IO budget 控制。

## 5. 高性能算法落点

- **候选收缩**: 用倒排表、trigram、path token、symbol name、scope/path/language filter 先把候选压到有界窗口，再做 expensive scoring。
- **混合融合**: 对 BM25、语义、向量、图路径、代码边和文件路径候选使用 RRF 或 phased ranking；只有同源同量纲分数才直接线性组合。
- **路径剪枝**: 多跳图检索采用 query intent、schema path、edge confidence、时间范围和最大 token/edge/hop 预算，不做无界邻域扩展。
- **增量优先**: Git diff、mutation log、file change cursor 和 source hash 驱动刷新；全量重扫只能作为 cold start、reconcile 或 cursor 失效后的受控操作。
- **冷热隔离**: 查询热路径只读已提交 read model；OCR、embedding、parser、content extraction、compaction 和大文件 hash 在 worker/maintenance 边界执行。
- **缓存与失效**: 缓存 key 必须包含 scope、graph version、index cursor、query policy 和权限摘要；任何 graph/file/code 变更都必须能解释受影响索引。
- **并发控制**: admission control、adaptive concurrency、timeout、cancellation 和 retry backoff 是性能功能，不是运维附属项。

## 6. 改进建议

| 优先级 | 建议 | 验收信号 |
| --- | --- | --- |
| P0 | 在架构和能力文档中把本机文件检索定义为 `local_file_path`、`local_file_metadata`、`local_file_content`、`local_file_change_cursor` 四层 read model。 | 文档说明文件名查询不依赖内容索引，所有文件查询返回 freshness/degraded reason。 |
| P0 | 为代码、文件、图谱混合检索统一记录 candidate window、filter count、RRF contribution、truncation reason 和 stale lag。 | context pack 和 benchmark 文档有可观测字段和 p95/p99 指标。 |
| P1 | 增加文件内容索引路线：文本 chunk BM25/trigram 优先，semantic/vector 作为可选后端，OCR/压缩包/大文件走 worker。 | 文件名查询和内容查询有独立延迟预算，内容索引失败不影响路径索引。 |
| P1 | 引入 query router：区分 exact term、conceptual、multi-hop、code symbol、file path、impact 和 temporal 查询。 | 每类查询有明确 retriever family、预算和降级规则。 |
| P1 | 将 cold indexing、incremental update、no-op refresh、watcher lag、queue lag 纳入基准门禁。 | 基准章节记录目标、采集命令和回归阈值。 |
| P2 | 评估平台 watcher 后端和 ANN 后端的可插拔实现。 | 后端能力缺失时可降级为 bounded rescan 或 local lexical read model。 |

## 7. 来源

- Microsoft GraphRAG query engine: https://microsoft.github.io/graphrag/query/overview/
- Microsoft Research DRIFT Search: https://www.microsoft.com/en-us/research/blog/introducing-drift-search-combining-global-and-local-search-methods-to-improve-quality-and-efficiency/
- LightRAG: https://arxiv.org/abs/2410.05779
- E^2GraphRAG: https://arxiv.org/abs/2505.24226
- ROGRAG: https://aclanthology.org/2025.acl-demo.58/
- HNSW: https://arxiv.org/abs/1603.09320
- FAISS billion-scale similarity search: https://arxiv.org/abs/1702.08734
- DiskANN: https://papers.nips.cc/paper/9527-diskann-fast-accurate-billion-point-nearest-neighbor-search-on-a-single-node
- Google ScaNN: https://research.google/blog/announcing-scann-efficient-vector-similarity-search/
- Vespa nearest neighbor and hybrid search: https://docs.vespa.ai/en/querying/nearest-neighbor-search
- OpenSearch RRF hybrid search: https://opensearch.org/blog/introducing-reciprocal-rank-fusion-hybrid-search/
- Sourcegraph Code Search: https://sourcegraph.com/docs/code-search/features
- Zoekt: https://github.com/sourcegraph/zoekt
- ripgrep performance notes: https://burntsushi.net/ripgrep/
- Everything indexes and USN journal: https://www.voidtools.com/support/everything/indexes
- Everything FAQ: https://www.voidtools.com/faq/
- Apple FSEvents: https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/TechnologyOverview/TechnologyOverview.html
- Linux inotify: https://man7.org/linux/man-pages/man7/inotify.7.html
- plocate: https://plocate.sesse.net/
- Google Zanzibar: https://www.usenix.org/conference/atc19/presentation/pang
- Meta RAMP-TAO: https://engineering.fb.com/2021/08/18/core-infra/ramp-tao/
- RocksDB: https://rocksdb.org/index.html
- Google SRE cascading failures: https://sre.google/sre-book/addressing-cascading-failures/
- Google SRE overload: https://sre.google/workbook/overload/
