# Semantic/Vector Provider Backend

[中文](../zh/semantic-vector-provider-backend.md) | [English](../en/semantic-vector-provider-backend.md)

`relay-knowledge` supports two semantic/vector read-model modes:

- `local`: deterministic in-process token and hashed-vector read models.
- `external`: remote embedding provider metadata and provider probe support for
  OpenAI-compatible embedding APIs.
- `disabled`: skip the read model family and report fallback status.

The query path never calls a remote model. Remote provider work belongs to index
refresh, startup recovery, maintenance workers, and explicit probes. BM25,
graph evidence, graph path, temporal, and community retrieval remain available
when semantic/vector backends are disabled or degraded.

## Configuration

All provider settings are read through the `env` boundary:

```bash
relay-knowledge setup profile external-embedding --format json
```

```bash
RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external
RELAY_KNOWLEDGE_VECTOR_BACKEND=external
RELAY_KNOWLEDGE_LLM_PROVIDER=openai_compatible
RELAY_KNOWLEDGE_EMBEDDING_BASE_URL=https://api.example.com/v1
RELAY_KNOWLEDGE_EMBEDDING_API_KEY=...
RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL=text-embedding-3-small
RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL=clip-vit-b32
RELAY_KNOWLEDGE_EMBEDDING_DIMENSION=1536
RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE=32
RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS=30000
RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY=4
```

`external` mode requires base URL, API key, model name, and dimension. Status
and health responses expose only a redacted endpoint, provider name, model
metadata, budgets, and whether a key is configured.

## Runtime Behavior

OpenAI-compatible providers use `POST /v1/embeddings` with:

```json
{
  "model": "text-embedding-3-small",
  "input": ["..."]
}
```

Responses must include exactly one embedding per input, with the configured
dimension and finite float values. HTTP 408, 429, 5xx, timeout, and transport
errors are retryable. HTTP 400, 401, 403, and 404 are permanent configuration or
authorization failures.

Index cursors persist model name, dimension, source hash, and backend cursor so
health diagnostics can explain stale, degraded, or failed states by index
family, scope, and modality.

## Web UI

The Web diagnostics workspace includes a `Providers` section showing:

- semantic and vector backend mode;
- model and dimension metadata;
- redacted remote endpoint and key-configured state;
- batch, timeout, and concurrency budgets;
- semantic/vector cursor rows with model, dimension, scope, and backend cursor.

The Web UI does not save provider settings or submit API keys. Provider
configuration remains an installation/runtime concern until a secret store is
introduced.
