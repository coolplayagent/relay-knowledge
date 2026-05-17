# 第 10 章 Worker、Proposal 与 Audit

[中文](../../zh/01-user-guide/10-workers-proposals-audit.md) | [English](../../en/01-user-guide/10-workers-proposals-audit.md)

Worker 负责把 CPU-heavy 或 I/O-heavy 工作移出查询热路径。Proposal 负责人工审核模型或外部 worker 产出的图谱变更。Audit 负责让 CLI、Web、service 和 agent 操作可追踪。

## 10.1 Worker 配置

多模态 evidence 写入后会进入持久 worker 队列。可配置外部 HTTP worker endpoint:

```text
RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT
RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT
RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED
```

worker endpoint 负责 embedding、OCR、视觉 caption、表格/layout 抽取等重任务。worker 结果先进入 proposal 或 multimodal extraction commit path，不在查询热路径里同步调用外部服务。

## 10.2 常用命令

```bash
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
relay-knowledge proposal show <proposal-id> --format json
relay-knowledge proposal accept <proposal-id> --by <actor> --reason "reviewed"
relay-knowledge audit query --limit 50 --format json
```

未配置外部 endpoint 时，`worker run-once` 使用 deterministic fallback 生成 proposal，不阻塞 BM25、graph retrieval 或 ingest。proposal 必须人工 accept 后才会通过 graph mutation pipeline 写入 accepted facts。

## 10.3 Extractor Contract

设置 `RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT` 后，foreground worker 会通过 `net::http` 按全局 request timeout 发送 `contract_version=2` 的 JSON 请求。请求携带 manual-review policy、timeout/lease/max-attempts/max-in-flight 预算，以及 provenance 要求。

外部 extractor 返回的 `ingest_request` 会继续走 proposal 存储，不会直接提交 graph mutation。其中 relation、claim 和 event 即使声明为 `accepted`，也会在 proposal payload 中被降为 `proposed`，避免模型抽取或关系推断绕过事实审批。

## 10.4 Provenance

Worker 返回值可以附带 `provenance` 对象，字段包括 `producer`、`provider`、`model`、`prompt_id`、`prompt_version`、`schema_version`、`input_source_hash`、`input_fact_ids`、`stale_when` 和 `budget_notes`。这些 metadata 会随 proposal 持久化，供 CLI/Web/API 审核和 audit 查询使用。

## 10.5 Audit Sink

Agent audit 持久化默认关闭。开启后，MCP 和本地 ACP audit events 会通过有界 async queue 写入 `paths` 管理的 log 目录:

```text
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH
```

队列深度在运行时 capped 到 65536。队列满时持久镜像可以丢弃事件，内存 audit log 仍保留最近事件。CLI/Web/service operation 还写入持久 audit sink，可通过 `audit query` 检查最近操作。
