# Semantic/Vector Provider Architecture

[English](../../en/03-architecture-specs/10-semantic-vector-provider-architecture.md) | [中文](../../zh/03-architecture-specs/10-semantic-vector-provider-architecture.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Semantic/vector providers are backend choices for derived read models, not knowledge sources of truth. The default deterministic local model keeps zero-configuration behavior available; external embedding providers improve quality but are constrained by env, QoS, redacted diagnostics, caching, dimension checks, and degradation policy.

## 2. Provider Modes

| Mode | Behavior |
| --- | --- |
| `local` | Deterministic local semantic signatures and hashed vectors for tests, default UX, and offline use |
| `external` | Controlled workers call external embedding endpoints and record model, dimension, backend cursor |
| `disabled` | No semantic/vector refresh is scheduled; retrieval declares the missing family |

Provider mode enters the system only through typed `env` config.

OpenAI-compatible embedding endpoint construction accepts a host root, a final version path segment such as `/v1` or `/v4`, or a full `/embeddings` endpoint. Query and fragment suffixes are not part of the provider endpoint identity.

## 3. Data Contract

External embedding output does not write accepted facts. It writes index-family metadata or derived evidence metadata only. Each vector record binds scope, evidence/chunk id, model name, dimension, content hash, and graph version.

## 4. Privacy and Security

- API keys, authorization headers, and endpoint secrets are visible only at save/execution boundaries.
- Web and diagnostics return configured booleans or redacted values only.
- External requests obey source authorization and redaction policy.
- Retries do not write secrets to logs, audit records, or dead-letter payloads.

## 5. Degradation Policy

When an external provider is unavailable, the system falls back to local mode if configured or declares semantic/vector unavailable. Provider probes classify HTTP 402, HTTP 429, and quota/backpressure-shaped HTTP 400, HTTP 403, HTTP 409, HTTP 425, or 5xx bodies as reachable but resource-limited diagnostics; authentication, endpoint, model, timeout, generic provider-unavailable, and malformed-response failures remain distinct. Context packs expose backend availability, model, dimension, last error, and stale lag.

## 6. Acceptance Criteria

- Hybrid retrieval works with no external service by default.
- External provider dimension changes produce explicit stale/rebuild requirements.
- Secrets do not appear in logs, Web responses, MCP resources, or test snapshots.

---

Navigation: Previous: [9. Hybrid Retrieval and Context Packing](09-hybrid-retrieval-and-context-packing.md) | Next: [11. Code Knowledge Graph Model](11-code-knowledge-graph-model.md)
