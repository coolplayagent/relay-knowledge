# Chapter 3: CLI Command Reference

[English](../../en/01-user-guide/03-cli-command-reference.md) | [中文](../../zh/01-user-guide/03-cli-command-reference.md)

This chapter is an executable command index. Workflow details live in later chapters; use this page to find entry points and diagnostics quickly.

When `--format json` or `--format streaming-json` is requested, parse diagnostics and runtime API failures written to stderr are JSON. Runtime API failures use the stable API error shape with `error_kind`, `message`, and optional `metadata`; text and markdown formats keep human-readable stderr messages.

## 3.1 Common Status Commands

Project status:

```bash
relay-knowledge status --format json
```

Health check:

```bash
relay-knowledge health --format json
```

Service diagnostics:

```bash
relay-knowledge service status --format json
relay-knowledge service doctor --format json
```

`service status` and `service doctor` currently share the same unified API output, covering service mode, background update state, service definition path, agent protocol status, and refresh queue diagnostics.

Version checks:

```bash
relay-knowledge version
relay-knowledge version check --format json
```

`version` prints only the current binary version. It does not load runtime
configuration or use the network. `version check` queries GitHub Releases and
crates.io through `net::http` according to runtime configuration and caches the
result under the runtime cache directory. Ordinary interactive text/markdown CLI
commands only print a short stderr notice when a newer stable version is found;
the primary command stdout is emitted first, and they never replace the binary
automatically.

## 3.2 Provider Diagnostics

```bash
relay-knowledge provider probe --format json
```

`provider probe` reads remote embedding provider configuration through the environment boundary and performs a lightweight probe. The JSON response includes `ok`, `provider`, `model`, `dimension`, optional `latency_ms`, and on failure `error_code`, `error_message`, and `retryable`. HTTP 429, HTTP 402, and quota/backpressure-shaped HTTP 400 or HTTP 403 responses mean the endpoint, auth boundary, and model route were reachable, so they report `ok=true` while keeping `error_code=rate_limited` and `retryable=true` as observable degraded diagnostics. Plain authentication, endpoint, model, timeout, and malformed-response failures still report `ok=false`. It does not print raw API keys or bypass the `env` module.

The OpenAI-compatible embedding base URL may be a host root, a versioned API root such as `/v1` or `/v4`, or a full `/embeddings` endpoint. Non-version path prefixes keep resolving as `<prefix>/v1/embeddings`, and query or fragment suffixes are ignored during endpoint construction.

Endpoint host, batch, timeout, concurrency, and cursor metadata belong to runtime diagnostics in `status`, `health`, or the Web Providers panel.

## 3.3 Setup Doctor and Profiles

`setup doctor` is a storage-free read-only diagnostic:

```bash
relay-knowledge setup doctor --format json
```

It reads only parsed runtime configuration. It does not open or migrate SQLite and does not refresh indexes. `configuration_ready=true` only means configuration checks passed; `live_health_checked=false` means graph storage, index freshness, and worker/service live health still need `health` or `service doctor`.

`setup profile` writes no files and installs no service. It prints recommended environment variables, commands, and notes:

```bash
relay-knowledge setup profile local --format json
relay-knowledge setup profile agent-readonly --format json
relay-knowledge setup profile service --format json
relay-knowledge setup profile external-embedding --format json
```

The profiles cover zero-config local use, read-only MCP agent access, platform service-manager preview, and external embedding provider metadata. Persisting those suggestions into a shell, service manager, or deployment tool is always explicit caller work.

## 3.4 Command Overview

```bash
relay-knowledge status
relay-knowledge help [command...] [--format text|json]
relay-knowledge ingest --source <scope> --content <text> [--entity <label>]
relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness allow-stale|wait-until-fresh|graph-only]
relay-knowledge map init
relay-knowledge map show [--topic <id>]
relay-knowledge map route <topic>
relay-knowledge map source add --id <id> --topic <id> --kind repo|file|doc|config|db|ci|runtime|wiki|monitoring --uri <uri> [--scope <source_scope>] [--description <text>]
relay-knowledge map source update --id <id> [--topic <id>] [--kind repo|file|doc|config|db|ci|runtime|wiki|monitoring] [--uri <uri>] [--scope <source_scope>] [--description <text>]
relay-knowledge map source remove --id <id>
relay-knowledge map validate
relay-knowledge map agent-snippet
relay-knowledge repo register <path> [--alias <name>] [--path <filter>]
relay-knowledge repo index <alias> [--ref <ref>] [--dry-run]
relay-knowledge repo scope preview <alias> [--ref <ref>]
relay-knowledge repo update <alias> --base <ref> --head <ref>
relay-knowledge repo query <alias> --query <text> [--kind hybrid|symbol|definition|references|callers|callees|imports]
relay-knowledge repo feature-flags <alias> [--query <text>] [--ref <ref>] [--path <filter>] [--language <id>] [--limit <n>]
relay-knowledge repo impact <alias> --base <ref> --head <ref>
relay-knowledge repo report <alias> [--format markdown|json]
relay-knowledge repo software <alias> [--ref <ref>] [--kind dependencies|sdks|all] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
relay-knowledge repo status <alias>
relay-knowledge graph inspect
relay-knowledge index refresh [--kind bm25|semantic|vector]
relay-knowledge worker status|run-once [--kind embedding|ocr|vision|extractor]
relay-knowledge proposal list [--state proposed|accepted|rejected|superseded] [--limit <n>]
relay-knowledge proposal show <proposal-id>
relay-knowledge proposal accept|reject|supersede <proposal-id> --by <actor> [--reason <text>]
relay-knowledge audit query [--operation <name>] [--limit <n>]
relay-knowledge provider probe
relay-knowledge health
relay-knowledge service status
relay-knowledge service doctor
relay-knowledge service plan install|uninstall
relay-knowledge service definition write
relay-knowledge service operator status|pause|resume
relay-knowledge service run [--web] [--mcp streamable-http]
relay-knowledge setup doctor
relay-knowledge setup profile local|agent-readonly|service|external-embedding
relay-knowledge version
relay-knowledge version check
```

Cold full `repo index` requests return a durable task handle immediately and start a bounded background worker from the CLI process. `service run` drains the same code-index queue for installed or foreground service operation. Use `repo status --format json` to inspect `active_task`, checkpoint counters, and scope retention while a cold repository index is running. Index writes use a single writer lane; queries, reports, graph reads, file queries, and health diagnostics use bounded read-only connections to read committed snapshots where SQLite WAL permits it.

`repo query` runs `definition`, `references`, and `hybrid` queries through the indexed tree-sitter graph and SQLite FTS read model first. With `--freshness allow-stale`, if the target ref is still being full-indexed and has not finalized, the query reads the previous completed committed scope and marks the response stale/degraded; `wait-until-fresh` still requires the target scope to be fresh. Only when those structured layers leave a specific recall gap does the query start bounded internal exact-text source fallback against the same indexed commit. JSON hits are marked with `retrieval_layers=["lexical","text_fallback"]`; definition fallback may also include `definition`. Candidate-path lookup, candidate-file, materialized-byte, or line-length budget exhaustion degrades only the fallback layer and appears in `degraded_reason`; structured code graph results remain valid.

`repo feature-flags` reads configuration-driven feature-flag graph facts written during indexing. By default it lists flags, configuration sources, and code-usage edges in the selected repository scope; `--query` filters by flag name, config key, path, or excerpt. The extractor recognizes environment variables, config/settings keys, boolean config declarations, and common SDK evaluation calls such as OpenFeature, LaunchDarkly, and Unleash clients. It does not sync provider control-plane state, strategies, segments, or rollout variants. The command does not scan the whole source tree at query time; after extractor changes or newly added flags, run `repo index` or `repo update` before expecting new facts.

`repo software` reads the software global-model projection for the selected repository scope. `--kind dependencies` returns package components derived from manifests and lockfiles; `--kind sdks` returns unresolved external import/include targets as SDK or API-surface usage candidates with `resolution_state`, `target_hint`, evidence path, and line range. The command does not scan package caches, SDK directories, or unindexed external source; rerun `repo index` or `repo update` to refresh the projection after source-scope changes.

`map` commands maintain the repository-local `.knowledge/knowledge-map.yaml` knowledge navigation contract. The YAML stores topic, source, route, and history metadata only; it does not copy authoritative knowledge out of documents, code, config, CI, runtime systems, or external sources. One topic can contain multiple sources, and `map source add` appends distinct source ids to that topic route order. LLM agents should use `map show` and `map route` to locate sources, maintain the contract through `map source add/update/remove`, and run `map validate --format json` after changes. AGENTS.md should keep only a stable reference such as `Knowledge map: .knowledge/knowledge-map.yaml`.

## 3.5 Read and Write Impact

Status, health, help, setup doctor/profile, provider probe, version check, report, map show/route/validate/agent-snippet, and audit query are diagnostic entry points and should not mutate graph facts. `health` is a liveness fast path: it does not queue index refresh work and does not wait for a code-index writer to finish; when storage is busy it may return stale/degraded `storage_busy`. `version check` may only refresh the version-check cache under the runtime cache directory. `ingest`, `map init`, `map source add/update/remove`, `repo index`, `repo update`, `index refresh`, `worker run-once`, proposal state changes, and service definition write can write runtime state, derived indexes, proposals/audit, the knowledge navigation contract, or service definitions.

Automated callers should read operation and read/write metadata from `help --format json` before exposing a command in CI, agents, or the Web operation surface.

## 3.6 Skill-over-CLI

The repository ships `skills/relay-knowledge-cli`, a ClawHub-compatible skill
for LLM agents that should operate relay-knowledge by invoking the local CLI and
parsing JSON output. It covers installation checks, `version check`, setup and
health diagnostics, knowledge graph ingestion/query, and code repository
registration, indexing, query, update, impact, and report workflows.

The skill intentionally does not configure MCP, call MCP tools, or manage ACP
sessions. Use the MCP/ACP chapters for protocol-level agent access.
