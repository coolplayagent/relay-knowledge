# Semantic/Vector Provider Backend Specification

[English](../../en/03-architecture-specs/semantic-vector-provider-backend.md) | [中文](../../zh/03-architecture-specs/semantic-vector-provider-backend.md)

This is the English documentation page for `specs/semantic-vector-provider-backend.md`. It follows the same structure, examples, commands, and implementation contracts as the Chinese edition so readers can switch languages without changing document location.

> Translation status: the English edition preserves the current technical source text below while the full prose translation is maintained incrementally. Command examples, API paths, environment variables, filenames, and configuration contracts are authoritative.

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

## Source Content

> Version: 1.1
> Date: 2026-05-15

## Summary

Semantic and vector read models may be backed by local deterministic models or a
remote OpenAI-compatible embedding provider. Remote calls are allowed only in
provider probes and refresh/maintenance boundaries, never in query hot paths.

## Contracts

- `env` owns provider environment parsing and validation.
- `net::http` owns outbound HTTP client construction, proxy/TLS policy, and
  timeout configuration.
- `retrieval::provider` owns provider-neutral embedding requests, response
  validation, OpenAI-compatible wire mapping, and retry classification.
- `application` owns runtime composition, probe orchestration, and cursor model
  metadata passed to index refresh completion.
- `model_provider` owns Web Settings chat/completion provider profiles,
  fallback policies, public catalog cache, endpoint probes, and model discovery.
- `storage` persists only provider-neutral read-model data and cursor metadata.

Provider configuration:

- `RELAY_KNOWLEDGE_LLM_PROVIDER`: `openai_compatible` or `echo`.
- `RELAY_KNOWLEDGE_EMBEDDING_BASE_URL`: HTTP(S) endpoint base.
- `RELAY_KNOWLEDGE_EMBEDDING_API_KEY`: secret bearer token.
- `RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE`: positive integer, default `32`.
- `RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS`: positive integer, default `30000`.
- `RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY`: positive integer, default `4`.

When either semantic or vector backend is `external`, base URL, API key, model,
and dimension are required.

Model provider profile files are resolved only through the `paths` boundary:

- `model-profiles.json`: config-directory file for named provider profiles and
  the default profile.
- `model-fallback.json`: config-directory file for fallback policies.
- `model-catalog-cache.json`: cache-directory file for the public `models.dev`
  catalog cache.

Profile API keys and secret headers may appear only in save requests and the
local config file. Any Web/API read response must return a redacted view and
must not include raw secret values.

## Web Contract

`RuntimeStatus` includes secret-free provider diagnostics:

- backend modes for semantic/vector;
- provider type;
- redacted base URL;
- API key configured boolean;
- text/image model names;
- embedding dimension, batch size, timeout, and max concurrency.

The Web `Providers` panel must remain read-only and must not include API key
values in DOM, staged operation payloads, logs, or browser requests.

The Web `Settings` model provider panel may write profile/fallback
configuration and must satisfy these contracts:

- Validate profile name, provider, base URL, sampling, timeout, and duplicate
  headers before saving.
- MaaS, CodeAgent, and Echo profiles can be saved without a configured secret;
  providers that require API keys must receive a new key on save or keep an
  existing configured secret.
- Redacted secret headers sent back during profile updates must preserve the
  stored values, and `clear_api_key=true` must explicitly clear a stored API
  key without relying on ambiguous omitted/null fields.
- `Probe` and `Discover` must use the `net::http` outbound client, timeouts, and
  QoS budgets instead of bypassing the network boundary.
- Catalog refresh failures must preserve the built-in catalog fallback or the
  most recent cache and must not affect query hot paths.

## Failure Modes

- Missing required remote config fails runtime configuration.
- Invalid provider response is permanent and records a diagnostic error.
- 408, 429, 5xx, timeout, and transport failures are retryable.
- 400, 401, 403, and 404 are permanent.
- A stale or failed semantic/vector backend must not prevent BM25 or graph
  retrieval from returning context.

## Tests

- Unit tests cover env parsing, runtime composition, URL normalization,
  response validation, retry classification, and cursor metadata.
- Web build and browser tests cover the Providers panel, readiness display, and
  secret-free operation preview.
- Required gates are `cargo fmt`, `cargo clippy`, `cargo test`,
  `npm run build --prefix web`, and Playwright browser tests.
