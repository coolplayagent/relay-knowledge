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

## 10.6 Software Experience Feedback

`feedback` records bugs, missing capabilities, poor results, workflow friction,
performance concerns and documentation gaps. It has its own durable lifecycle;
it never creates accepted graph facts or reuses proposal approval. Exit code zero
does not establish satisfactory results; missing dependencies and permission
failures do not by themselves establish a product defect.

Use the [feedback schema](../../../skills/relay-knowledge-cli/references/feedback.schema.json)
and [workflow example](../../../skills/relay-knowledge-cli/references/feedback-workflows.md).
Narrative fields (`intent`, `expected`, `actual`, `impact`, observations and optional
reproduction) must contain a minimal public summary. Observation origins are
`agent-observation`, `user-experience`, `cli-fact` or `hypothesis`; these describe
caller claims, not automatic truth certification. Private logs and knowledge
content belong in `evidence`; baseline metadata belongs in `diagnostics`. Both
remain local. Copy existing command `metadata.trace_id`/`metadata.request_id` into
the report to associate an original operation; otherwise the feedback request
supplies its own identifiers. The CLI records its actual version and platform.

```bash
relay-knowledge feedback report --input feedback.json --format json
relay-knowledge feedback status --format json
relay-knowledge feedback preview <feedback-id> --format json
```

The default `local-only` mode saves a durable draft without creating an issue.
Preview shows the exact locally prepared public title/body and its digest, a
separate raw report digest, and fixed omission categories. Both evidence labels
and contents remain private: `omitted_evidence` reports only generic categories
such as `raw evidence`, never caller-supplied labels, including in CLI/Web status
and preview responses. A deduplicated issue can retain
another outbox's nonce, so this local payload is not a claim about the exact
current remote body. Raw evidence, diagnostics and trace IDs never enter
the issue. Sensitive public narrative becomes `evidence-insufficient`; supply a
public minimal summary instead of waiving redaction. The scanner rejects known
credentials, emails, private paths and unsafe control characters, but cannot
establish that arbitrary prose is public. Keep private knowledge out of narrative
fields even when it contains no recognizable secret pattern.

Content bindings have distinct scopes:

| Binding | Content and lifecycle |
| --- | --- |
| `raw_report_digest` (status `evidence.raw_digest`) | Immutable local raw report, including private evidence. Never published. |
| `publication.payload.digest` | Immutable complete locally prepared title/body, including markers. |
| `publication.payload.dedup_marker` | Stable digest of sanitized public title/body before recovery metadata; independent of private evidence and the per-outbox nonce. |
| `publication.issue.body_digest` | Digest of the remote issue body actually observed by the adapter; refreshed by `track` if the remote body changes. |

These bindings intentionally cover different content. Do not compare the local
title/body digest directly with the remote body-only digest or present a reused
issue's body as byte-identical to the local preview.

## 10.7 Explicit Publication and Recovery

Local `feedback configure --input feedback-policy.json` persists a version 1
policy: `mode`, `target_repository`, `allowed_kinds`, `daily_quota` and optional
`validation_runner`. `auto-submit` requires an explicit `owner/repository` target
and nonempty kind allowlist. Quota is 1–100 create attempts per persisted 24-hour
window, with a default of 5. Configuration does not publish earlier drafts.
Supply `RELAY_KNOWLEDGE_FEEDBACK_GITHUB_TOKEN` through the process/managed-service
environment, with read/create-issue permission for the selected repository.
Never put credential values in reports, policy files or issue text.

```bash
relay-knowledge feedback configure --input feedback-policy.json --format json
relay-knowledge feedback submit <feedback-id> --format json
relay-knowledge feedback retry <feedback-id> --format json
relay-knowledge feedback track <feedback-id> --format json
```

After opt-in, `report` can create a real issue through the built-in GitHub adapter
and return its URL. It does not delegate publication to `gh` or execute log
instructions. `submit` and `retry` enforce policy, kind scope, quota and frozen
target/payload. Duplicate local reports with the same public scenario, observations, impact and
CLI version merge occurrence counts. The original raw report remains immutable;
later raw evidence is not appended. Corrected public narrative gets a new ID. A random opaque nonce correlates recovery of one publication. A separate stable
marker hashes only the already-sanitized public title/body, allowing identical
public reports from another outbox to reuse an existing issue. Private content
hashes never enter the issue. Concurrent independent installations can still
race; only processes sharing the same outbox are serialized. This adapter does not comment or reopen.

The outbox serializes across processes and persists send intent before HTTP.
The last transaction/worker owner explicitly releases the OS lock, so unrelated
child-process descriptor inheritance cannot delay the next transaction; a
cancelled commit keeps ownership until its disk worker finishes. It
retains at most 1,000 records/16 MiB, permits at most five create attempts per
record, and bounds retry delays. Capacity exhaustion preserves existing records
and rejects new writes. Offline/authentication/permission/quota failures expose
explicit states, reasons and next-attempt timestamps. A publication failure does
not discard a saved report or alter a preceding knowledge-query result.
`retryable-failed` can retry after its deadline; `blocked` requires resolving the
reported policy, authentication or budget condition.

GitHub create-issue has no client idempotency key. An interrupted or ambiguous
POST is therefore never automatically repeated. `retry` only reconciles its exact
nonce; a match supplies the real URL. No match leaves `awaiting-reconciliation`,
since absence cannot prove an in-flight write will not appear later. Remote lookup uses a repository-scoped nonce search bounded to two candidates,
30 seconds and 4 MiB. Incomplete, ambiguous or inconsistent results fail closed;
an exact marker and target match are required. This prevents duplicate
creates from one journal by accepting potentially unresolved delivery; it does
not promise distributed exactly-once delivery across installations. Do not erase
unresolved state to force a second create.

## 10.8 Fixes and Regression Evidence

`track` reads remote issue state and records it locally. Closure, PR merge and a
new release cannot establish a fix. `link-fix` takes `reference` and
`target_version` and enters `awaiting-validation`. An original
`reproduction.scenario` and `.expected` provide inert scenario text and an exact
comparison criterion.

```bash
relay-knowledge feedback link-fix <feedback-id> --input fix.json --format json
relay-knowledge feedback validate <feedback-id> --input validation.json --format json
```

A separately authorized runner must replay the scenario. Configure its identity
in `validation_runner` independently of publication permission. Validation input
contains matching `runner`, the `scenario_digest` from status, `actual`, `version`,
`environment`, unique `run_id`, and optional `elapsed_ms`/`steps`. The local caller
attests that this evidence came from that run; this is not cryptographic external
runner authentication or an independent CLI rerun. The CLI checks the original
scenario/criterion binding and fix version and computes exact equality; it accepts
no input `passed` flag and executes no command. Matching authorized evidence gives
`verified-fixed`; mismatching evidence gives `regressed`. Missing criteria or
runner authority leave validation pending. Run IDs cannot be rebound, and each
record retains at most 32 runs. Keep hardware/model/index/workload differences in
baseline diagnostics and candidate environment metadata. Retrieval-quality claims
without fixed inputs and criteria need external evaluation first.

Publication grants no repair, download, upgrade, PR or script-execution authority.
The #416 batch-map example is a friction fixture; this feature does not implement
batch map transactions.
