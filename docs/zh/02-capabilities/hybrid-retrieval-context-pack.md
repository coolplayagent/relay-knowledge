# 混合检索 Context Pack 功能文档

[中文](./hybrid-retrieval-context-pack.md) | [英文](../../en/02-capabilities/hybrid-retrieval-context-pack.md)

本文档说明当前由 `RelayKnowledgeService::retrieve_context` 实现的第四阶段检索行为。

## 功能概览

`retrieve_context` 现在返回可审计的上下文包，而不是只返回扁平的证据命中列表。响应仍保留既有的 `results` 数组，供 CLI 与 Web 兼容使用，同时补充：

- `context_pack`：图版本、来源范围、新鲜度策略、截断状态、后端可用性，以及每个条目的来源和排序元数据。
- `fusion`：排序算法和候选数量。当前阶段使用 `k = 60` 的互惠排名融合。
- `budget_used`：请求限制、候选数量、返回数量和打包后的上下文字节数。
- `truncated`：是否因为请求限制省略了一个或多个匹配候选。

每个结果都包含 `retriever_sources`、`ranking`、实体投影、可选来源范围、结构化事实、直接图路径证据，以及可选的代码工件元数据。`ranking` 会记录检索器来源、来源内排名、原始来源得分和简短解释，方便 agent 引用条目入选原因。

## 检索来源

检索层使用以下召回路径：

- `bm25`：基于 SQLite FTS5，对证据内容、实体标签、生成的实体标签别名、来源范围、来源路径、代码符号、生成的代码符号别名和代码块进行 BM25 检索。
- `graph_evidence`：基于图证据和实体词项重叠的确定性回退检索。
- `code_graph`：由 tree-sitter 代码图写入共享 BM25 读模型的代码符号与代码块文档。
- `semantic`：本地 token-signature 读模型，覆盖证据和派生多模态证据，并在排序解释中携带模型、维度、来源哈希、作用域和图版本元数据。
- `vector`：本地哈希向量 ANN 读模型，使用确定性向量、作用域后过滤、图版本过滤、模型元数据和来源哈希。
- `graph_path`：围绕已接受关系、声明、事件及其支持证据的 schema 指导遍历。
- `temporal`：面向年份词项和 `as_of:<date>` 约束的事件检索。
- `community_summary`：面向全局、概览或社区类查询的作用域摘要命中。

`semantic` 和 `vector` 是新鲜度元数据中的显式索引族。`backend_statuses` 记录已配置的 `local`、`external` 或 `disabled` 读模型模式、模型名称、维度、作用域后过滤、已索引图版本，以及过期或不可用原因。派生后端禁用或过期时，BM25 与图证据检索仍可使用，响应也会继续报告索引新鲜度。

BM25、semantic、vector、图路径、时序和社区命中通过 RRF 融合。默认 semantic/vector 实现是本地确定性读模型。外部 OpenAI 兼容 embedding provider 可以通过同一游标和后端状态契约提供读模型元数据与探测诊断，而不改变 context pack 响应形状。

健康检查和索引刷新诊断也会暴露这些索引族的作用域游标元数据：来源哈希、后端游标，以及配置的后端 worker 提供的模型名称和维度。同一诊断中还包含 `stale_reasons`，用于解释失败状态、图版本滞后和最近错误。

当前 v2 基线使用本地确定性 SQLite 读模型：semantic 文档存储归一化概念 token，vector 文档存储哈希 token 与字符 n-gram 权重。未来后端不可用时，`backend_statuses` 仍会记录不可用状态和回退原因。

## 图事实

图变更现在支持证据元数据和结构化事实：

- 证据来源路径、来源范围、置信度、状态和图版本。
- `text_span`、`image_asset`、`ocr_text`、`caption`、`image_embedding`、`table`、`layout_region` 的证据模态和提取元数据。
- 实体标签之间的类型化关系。
- 带有主体、谓词、客体、证据 ID、置信度、状态和版本范围的声明。
- 带有关联实体、可选有效时间文本、置信度、状态和版本范围的事件。

结构化事实持久化到 SQLite，并计入图检查和变更日志响应。实体清理会保留仍被证据、关系、声明和事件引用的实体。

检索上下文条目还会暴露由这些事实派生的直接 `graph_paths`。每条路径保留参与节点标签、关系/声明/事件边、支持证据 ID、置信度、生命周期状态和图版本有效范围。

Ingest API 接受结构化事实及其证据。基础 CLI 仍写入证据和实体标签；API adapter 可以额外提供证据 `source_path`、`span`、`confidence`、`status`，以及引用证据 ID 的关系、声明和事件记录。

结构化事实必须引用支持证据 ID，才能通过检索返回。Ingest 在持久化前会重新校验反序列化得到的范围、置信度分数和版本范围。状态为 `rejected` 或 `superseded` 的证据仍可在图中检查，但会从 BM25 与图证据检索候选中排除。

OCR、字幕、表格、布局和图像嵌入维护流程通过 `commit_multimodal_extraction` 提交派生证据。该路径会先校验父证据归属和提取器身份，再复用常规摄取与索引刷新路径。检索使用父证据 ID 作为合并键，因此同一图片的 OCR 与字幕命中会作为一个分组上下文条目返回，而不会产生重复结果。

## 新鲜度与快照行为

检索始终针对显式图版本执行。BM25 文档会存储 `created_graph_version`，因此查询不会返回请求快照之后写入的证据或代码图文档。

新鲜度策略保持不变：

- `allow_stale`：返回结果，并在元数据中标记过期状态。
- `wait_until_fresh`：查询前刷新过期索引元数据。
- `graph_only`：绕过索引元数据，返回仅图谱的降级上下文。

当 `RELAY_KNOWLEDGE_SEMANTIC_BACKEND` 或 `RELAY_KNOWLEDGE_VECTOR_BACKEND` 设置为 `disabled` 时，对应检索器不会执行候选召回，也不会调度读模型刷新工作。Semantic 和 vector 游标的模型元数据来自已索引文档，而不是运行时覆盖标签。

当任一后端设置为 `external` 时，远端 provider 通过 `env` 边界配置。查询执行仍读取本地读模型表，不在热路径调用 provider。

## CLI 示例

```bash
relay-knowledge ingest \
  --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust \
  --format json

relay-knowledge query SQLite \
  --source docs \
  --freshness wait-until-fresh \
  --format json
```

查询响应同时包含 `results` 和 `context_pack`。简单展示可使用 `results`。当 agent 需要来源归因、事实/路径溯源或降级处理时，应使用 `context_pack.items[*].ranking`、`context_pack.items[*].graph_facts`、`context_pack.items[*].graph_paths`、`context_pack.items[*].source_span` 和 `context_pack.backend_statuses`。
