# 第 3 章 知识图谱工作流

## 3.1 写入 evidence

最小写入命令需要 source scope 和文本内容:

```bash
relay-knowledge ingest \
  --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust \
  --entity SQLite \
  --format json
```

`--source` 用于隔离来源范围，例如 `docs`、`repo:core` 或某个产品域。`--entity` 可以重复，用于给 evidence 绑定实体标签。写入成功后会产生新的 `graph_version`，并驱动 BM25、semantic 和 vector 等派生索引的新鲜度状态。

## 3.2 查询 context pack

普通查询:

```bash
relay-knowledge query "SQLite graph state" \
  --source docs \
  --limit 8 \
  --format json
```

要求索引追上最新图版本:

```bash
relay-knowledge query "SQLite graph state" \
  --source docs \
  --freshness wait-until-fresh \
  --limit 8 \
  --format json
```

只读图事实路径:

```bash
relay-knowledge query "SQLite graph state" \
  --source docs \
  --freshness graph-only \
  --format json
```

JSON 响应同时包含兼容展示用的 `results` 和面向 agent 的 `context_pack`。需要可审计引用时，优先读取 `context_pack.items[*].ranking`、`graph_facts`、`source_span`、`backend_statuses`、`budget_used`、`truncated` 和 `degraded_reason`。

## 3.3 检查图状态

查看图谱统计:

```bash
relay-knowledge graph inspect --format json
```

刷新一个或多个索引族:

```bash
relay-knowledge index refresh --kind bm25 --format json
relay-knowledge index refresh --kind semantic --kind vector --format json
```

不传 `--kind` 时刷新当前服务认为需要处理的索引族。刷新路径使用 bounded refresh queue、lease、retry、dead-letter 和 stale diagnostics，显式刷新失败时不会伪装成已新鲜。
JSON 响应里的 `diagnostics.stale_reasons` 会列出仍未新鲜或失败的索引族和 scoped cursor；
字段包含 kind、source scope、modality、reason、lag versions 和 last error。

## 3.4 结构化事实说明

CLI 的 `ingest` 写入普通 evidence 和 entity labels。共享 API 还支持更丰富的事实:

- evidence `source_path`、span、confidence、status 和 multimodal extraction metadata。
- typed relation、claim 和 event。
- 支持 evidence id 的结构化 facts，且反序列化后会重新校验 span、confidence 和 version range。

检索只把 `accepted` 或 `proposed` evidence 作为上下文候选。`rejected` 或 `superseded` evidence 仍可被图检查看到，但不会进入 BM25 和 graph-evidence 检索候选。

## 3.5 多模态 evidence

当前 schema 可记录 `text_span`、`image_asset`、`ocr_text`、`caption`、`image_embedding`、`table` 和 `layout_region`。OCR、caption 和 image embedding 这类派生 evidence 可以引用 parent evidence，检索时会按 parent 合并，避免同一图片的多个派生命中重复占用 context pack 预算。
