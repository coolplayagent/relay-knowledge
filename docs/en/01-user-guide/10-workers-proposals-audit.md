# Chapter 10: Workers, Proposals, and Audit

[English](../../en/01-user-guide/10-workers-proposals-audit.md) | [中文](../../zh/01-user-guide/10-workers-proposals-audit.md)

Workers move CPU-heavy or I/O-heavy work out of the query hot path. Proposals provide human review for graph changes produced by models or external workers. Audit keeps CLI, Web, service, and agent operations traceable.

## 10.1 Worker Configuration

After multimodal evidence is written, work can enter persistent worker queues. External HTTP worker endpoints can be configured with:

```text
RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT
RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT
RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT
RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT
RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED
```

Worker endpoints own heavy work such as embedding, OCR, visual captions, and table/layout extraction. Worker results enter proposals or the multimodal extraction commit path and are not called synchronously on the query hot path.

## 10.2 Common Commands

```bash
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
relay-knowledge proposal show <proposal-id> --format json
relay-knowledge proposal accept <proposal-id> --by <actor> --reason "reviewed"
relay-knowledge audit query --limit 50 --format json
```

When no external endpoint is configured, `worker run-once` uses a deterministic fallback to create a proposal. It does not block BM25, graph retrieval, or ingest. A proposal must be manually accepted before it writes accepted facts through the graph mutation pipeline.

## 10.3 Extractor Contract

When `RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT` is set, the foreground worker sends a `contract_version=2` JSON request through `net::http` using the global request timeout. The request carries manual-review policy, timeout/lease/max-attempts/max-in-flight budgets, and provenance requirements.

An external extractor's returned `ingest_request` continues through proposal storage and does not directly commit graph mutations. Relations, claims, and events are downgraded to `proposed` in the proposal payload even when the extractor declares them `accepted`, preventing model extraction or relationship inference from bypassing review.

## 10.4 Provenance

Worker responses can include a `provenance` object with `producer`, `provider`, `model`, `prompt_id`, `prompt_version`, `schema_version`, `input_source_hash`, `input_fact_ids`, `stale_when`, and `budget_notes`. This metadata is persisted with proposals for CLI/Web/API review and audit queries.

## 10.5 Audit Sink

Agent audit persistence is disabled by default. When enabled, MCP and local ACP audit events are mirrored through a bounded async queue to the `paths`-managed log directory:

```text
RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED
RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH
```

Queue depth is capped to 65536 at runtime. When the queue is full, the durable mirror may drop events, while the in-memory audit log still retains recent events. CLI/Web/service operations also write to the persistent audit sink and can be inspected with `audit query`.
