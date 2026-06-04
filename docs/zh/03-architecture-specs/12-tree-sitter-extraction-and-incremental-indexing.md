# Tree-sitter 抽取与增量索引

[中文](../../zh/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md) | [English](../../en/03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)

> 文档版本: 2.0
> 编制日期: 2026-05-30
> 适用范围: 第三卷架构与算法白皮书

## 1. 设计结论

Tree-sitter 是代码结构入口，但不是全能语义分析器。架构必须把 grammar、query capture、错误降级、增量候选缩小和索引刷新串成可恢复 pipeline，使 unsupported language 或不可恢复 parse error 只降级局部能力，不破坏整体检索；C/C++ macro、preprocessor 和 decorator 区域的可恢复 parse error 在抽取仍可靠时应继续保留结构化事实。

## 2. 语言注册

每个语言注册项包含：language id、file extensions、tree-sitter grammar、capture queries、comment rules、identifier segmentation 和 fallback chunker。缺失 grammar 时，文件仍进入 text chunk 和 BM25 路径。查询时的 source fallback 不是 grammar 替代品；它只能在已索引源码候选上补充精确文本证据，不能创建图事实。

配置、构建和模板 grammar 注册在代码配置模块下，而不是运行时配置模块。支持面包括 Markdown、XML、Bazel/Starlark、Make、CMake、Dockerfile/Containerfile、Java properties、TOML、INI/`.conf`、YAML、JSON、Go module、Ninja、Jinja2 和 Go template。这些格式写入普通 file、symbol、reference、import、dependency、feature-flag 和 chunk facts，查询 API 不需要为配置检索引入独立 schema。SQL 作为 `.sql` 文件的代码 grammar 注册，会在同一套仓库代码图表中写入 table、view/materialized view、function/procedure、trigger、type 等 schema object 符号，以及 SQL 对象引用和调用引用。层级配置格式必须写入稳定的点分路径；数组和数组表使用 `[]` 而不是数字下标，例如 `server.port`、`containers[].name` 和 `bin[].name`。

结构化文档和配置文件采用 Tree-sitter AST 加产品规则的组合。Tree-sitter 负责识别 Markdown 标题、链接定义、inline link、JSON pair/array 和 INI section/setting 的语法节点、range 与 error node；产品规则负责把 JSON 数组折叠成 `[]` 路径、把 INI section 和 setting 合成点分名称、过滤 Markdown 外部 URL 或 anchor-only 链接，并把本地 Markdown 链接写成 unresolved import metadata。Markdown 和配置文件即使存在 symbol 级 chunk，也必须保留文件级 chunk，保证正文、配置值和局部 parse failure 仍可通过 BM25/hybrid 召回。

Parser 实现必须把语言专属规则放在高内聚的语言目录中。Node kind 分类、语言专属 import 抽取、C/C++ 手工恢复逻辑归入 `src/relay_knowledge/code/parser/languages/<language>/`；通用解析流程、syntax helper、文本校验、依赖清单解析和 chunk 构建保留在 parser 层共享模块。

## 3. Capture Contract

Query captures 输出统一结构：definition、reference、call、import、feature flag/config usage、doc comment、symbol span、body span 和 chunk span。Capture 结果在写入前必须经过 scope、path、line/column 和 content hash 校验。

## 4. 全量构建

```text
resolve snapshot
  -> enumerate authorized files
  -> batch parse and chunk
  -> write file/symbol/reference/feature-flag/chunk facts
  -> finalize cross-batch edges
  -> refresh code/BM25/semantic/vector indexes
  -> mark scope fresh
```

全量构建过程中旧 fresh scope 继续服务查询；新 scope 只有 finalize 成功后才成为 fresh。

## 5. 增量更新

增量算法先缩小工作集：

1. 使用 Git diff/status 和 blob hash 找 changed files。
2. 加入 deleted/renamed/moved files。
3. 用反向依赖和 import/call/reference edge 扩散 affected files。
4. 只刷新受影响的 code facts、chunks 和 index families。

Import 依赖扩散必须优先使用已索引代码地图和版本化 import edge。若 import 指向的外部依赖或跨仓库目标没有代码地图，索引器只记录 unresolved target hint、resolution reason 和受影响的本仓库事实，不为了补齐该依赖而触发未授权全仓扫描；这个 coverage gap 不是 parser、file、scope 或 response 降级。查询层可在同一 scope 内用该 hint 触发受限内部 source fallback。

本地配置关系只在同一 indexed source scope 内解析。Finalize 可以在该 scope 的全部文件写入后解析确定性的本地文件引用、模板 include 和构建目标引用。有歧义的本地匹配以及外部 image、package、remote label 或 template 继续保留为 unresolved 或 ambiguous metadata，而不是 parser degraded state。

Feature flag 抽取属于索引阶段。运行时配置读取、布尔配置声明和 guarded-code 关系必须随文件 scope 写入版本化事实；查询层只能读这些事实和对应 FTS 文档。TOML、YAML、JSON、INI、Java properties 等配置格式中的布尔声明复用 configuration 抽取器产出的结构化 config-key fact，而不是维护第二套 feature-flag 来源。更新抽取规则、配置文件或 guarded 分支后，必须通过 full 或 incremental index 刷新相关 scope。

## 6. 高性能边界

代码索引采用 Sourcegraph/Zoekt、GitHub Code Search、ripgrep 和 Tree-sitter 类系统的共同原则：先用路径、语言、trigram、symbol name 和 blob hash 缩小候选，再做 AST capture、edge resolution 和语义/向量刷新。AST chunk 应沿函数、类型、模块、doc comment 和 import block 边界切分；fallback text chunk 只在结构解析不可用时接管。

全量 cold index、语义 embedding、跨 batch edge finalization、large file skip/hash 和 parser-heavy work 都属于 master 监督的后台 worker 或 maintenance 边界，不能阻塞查询热路径。Code-index worker 必须通过 application service claim durable task，持有 attempt-scoped lease，并执行有界 parse/write batch；接口层和查询路径不能直接调用 tree-sitter full indexing。增量索引必须记录 changed file count、affected file count、parse throughput、write batch count、candidate window 和 stale lag，便于区分真正增量和隐藏全仓扫描。

全量索引批次必须同时受文件数、字节数和写入 row 数约束；为了提升大仓库 cold index 吞吐，可以扩大有界批次、并行 parser worker、减少空 scope 上的冗余 SQLite 探测、复用 prepared statement、使用可索引 FTS metadata 清理或增加分阶段 finalization checkpoint，但不得跳过 FTS/search-document 写入、edge finalization、checkpoint、freshness 校验或 degraded/status 上报。任何注册后索引性能优化都必须在 self-iteration `fast` 或 `--categories performance` 中留下可回归的 `index_ms`、`register_index_ms` 和 finalization 后 ref 可查询性的预算或 guardrail。

Query-time source fallback 与 Git blob 读取一样必须进入 blocking-worker 边界。产品路径使用内部 fixed-string scanner 搜索临时物化的有界已索引 blob，先应用 path/language/scope 过滤，并在候选路径、候选文件预算或物化字节预算触发时返回 degraded reason，而不是把查询热路径退化成全仓扫描。开发者或 agent 检查源码时，可以使用 `rg` 或 `grep -RIn --exclude-dir=.git --exclude-dir=target ...`，但这些命令必须留在产品运行时索引和查询循环之外。

## 7. 降级策略

不可恢复 parse error、grammar panic、capture mismatch 或 unsupported language 生成 parse status 诊断，并回退到 text chunk。C/C++ 文件如果 error node 局限在 macro expansion、有界 preprocessor directive 或 decorator-like export macro，且 symbol、reference 或 import 抽取成功，可以记录为 parsed。降级结果必须出现在 repo status、health 和 context pack metadata 中。外部依赖源码缺失保持 unresolved edge metadata，不写成 `degraded_reason`。查询时 exact-text source fallback 的候选路径或预算降级应出现在 code query 响应 metadata 中，而不是写入索引状态。人工 `rg`/`grep` fallback 是 agent 检查源码的操作说明，不应作为产品 index health 上报。

## 8. 验收标准

- 大仓库索引能报告 progress，不替换旧 fresh scope。
- 增量更新只处理 changed 和 affected files，不能全仓扫描伪装为增量。
- 解析失败文件仍能通过文本检索召回。
- 索引 trace 能说明候选缩小、parse、写入和刷新各阶段耗时。

---

导航: 上一章: [11. 代码知识图谱模型](11-code-knowledge-graph-model.md) | 下一章: [13. 代码检索排序与影响分析](13-code-retrieval-ranking-and-impact-analysis.md)
