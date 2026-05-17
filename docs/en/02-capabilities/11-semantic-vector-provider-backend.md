# Semantic/Vector Provider Backend

[English](./11-semantic-vector-provider-backend.md) | [中文](../../zh/02-capabilities/11-semantic-vector-provider-backend.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Semantic/vector provider backends provide controlled switching between local and external models. Default local mode preserves offline use; external mode records model metadata; disabled mode explicitly exits the read model.

## User-visible Behavior

```bash
RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external RELAY_KNOWLEDGE_VECTOR_BACKEND=external RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL=text-embed-3-small RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL=clip-vit-b32 RELAY_KNOWLEDGE_EMBEDDING_DIMENSION=1536 relay-knowledge index refresh --kind semantic --kind vector --format json
```

`RELAY_KNOWLEDGE_SEMANTIC_BACKEND` and `RELAY_KNOWLEDGE_VECTOR_BACKEND` accept `local`, `external`, or `disabled`.

## Competitive Features

Provider state enters `backend_statuses`, including configured backend, model, dimension, scope post-filter, and indexed graph version. External models are not a fact source; they are derived read-model backends.

## Web User Interface

The Web Settings page shows model provider profiles, fallback policy, endpoint probes, and model discovery state. Secrets are accepted only at save time; browser responses expose configured booleans or redacted headers only.

## Degradation and Diagnostics

Disabled mode does not run semantic/vector retrievers and does not schedule matching refresh. Model names must be non-empty after trimming; dimension changes trigger explicit rebuild/freshness requirements.

## Related Architecture Chapters

- [Semantic/Vector Provider Architecture](../03-architecture-specs/10-semantic-vector-provider-architecture.md)

---

Navigation: Previous: [10. Code Impact and Reporting](10-code-impact-and-reporting.md) | Next: [12. Web Workspace Capabilities](12-web-workspace-capabilities.md)
