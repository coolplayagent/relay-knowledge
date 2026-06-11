# Chapter 3: CLI Command Reference

[English](../../en/01-user-guide/03-cli-command-reference.md) | [中文](../../zh/01-user-guide/03-cli-command-reference.md)

This chapter is an executable command index. Workflow details live in later chapters; use this page to find entry points and diagnostics quickly.

When `--format json` or `--format streaming-json` is requested, parse diagnostics and runtime API failures written to stderr are JSON. Runtime API failures use the stable API error shape with `error_kind`, `message`, and optional `metadata`; text and markdown formats keep human-readable stderr messages.

To access a deployed resident service from a local CLI, use global `--remote <base-url>` or `RELAY_KNOWLEDGE_REMOTE_BASE_URL`. Remote mode covers `repo index`, `repo scope preview`, `repo status`, `repo query`, `repo feature-flags`, `repo impact`, `repo report`, and `repo software` for repositories already registered on the service host. `repo index --reset` and `repo index-worker` are rejected while remote mode is selected and must be run on the service host; unrelated local commands such as `status` and `health` keep using local runtime state when only the environment variable is set.

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
relay-knowledge files index [--root <path>] [--source <scope>]
relay-knowledge files query <text> [--source <scope>] [--root <root-id>] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
relay-knowledge map init
relay-knowledge map show [--topic <id>]
relay-knowledge map route <topic>
relay-knowledge map source add --id <id> --topic <id> --kind repo|file|doc|config|db|ci|runtime|wiki|monitoring --uri <uri> [--scope <source_scope>] [--description <text>]
relay-knowledge map source update --id <id> [--topic <id>] [--kind repo|file|doc|config|db|ci|runtime|wiki|monitoring] [--uri <uri>] [--scope <source_scope>] [--description <text>]
relay-knowledge map source remove --id <id>
relay-knowledge map validate
relay-knowledge map agent-snippet
relay-knowledge repo register <path> [--alias <name>] [--path <filter>]
relay-knowledge repo remove <alias>
relay-knowledge repo index <alias> [--ref <ref>] [--dry-run|--reset]
relay-knowledge repo index-worker [--task-id <id>]
relay-knowledge repo scope preview <alias> [--ref <ref>]
relay-knowledge repo update <alias> --base <ref> --head <ref>
relay-knowledge repo query <alias> --query <text> [--kind hybrid|symbol|definition|references|callers|callees|imports|sbom]
relay-knowledge repo feature-flags <alias> [--query <text>] [--ref <ref>] [--path <filter>] [--language <id>] [--limit <n>]
relay-knowledge repo impact <alias> --base <ref> --head <ref>
relay-knowledge repo report <alias> [--format markdown|json]
relay-knowledge repo software <alias> [--ref <ref>] [--kind dependencies|sdks|files|topics|relationships|build|iac|design|all] [--freshness allow-stale|wait-until-fresh|graph-only] [--limit <n>]
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
relay-knowledge service plan install|upgrade|rollback|uninstall [--target-version <version>] [--install-dir <path>]
relay-knowledge service lifecycle install|upgrade|rollback|uninstall [--dry-run|--execute] [--target-version <version>] [--install-dir <path>]
relay-knowledge service definition write
relay-knowledge service operator status|pause|resume
relay-knowledge service worker run [--task-id <id>]
relay-knowledge service run [--web] [--mcp streamable-http]
relay-knowledge setup doctor
relay-knowledge setup profile local|agent-readonly|service|external-embedding
relay-knowledge version
relay-knowledge version check
```

Kind values are scoped to their command family:

- `repo query --kind` and `repo-set query --kind`: `hybrid`, `symbol`,
  `definition`, `references`, `callers`, `callees`, `imports`, `sbom`.
- `repo software --kind`: `dependencies`, `sdks`, `files`, `topics`,
  `relationships`, `build`, `iac`, `design`, `all`.
- `index refresh --kind`: `bm25`, `semantic`, `vector`; omitting `--kind`
  requests all supported index families.
- `worker status|run-once --kind`: `embedding`, `ocr`, `vision`, `extractor`.
- `map source add|update --kind`: `repo`, `file`, `doc`, `config`, `db`,
  `ci`, `runtime`, `wiki`, `monitoring`.

Do not pass kind values across command families. Use `repo impact` for impact
analysis and `repo feature-flags` for feature flags; they are not
`repo query --kind` values.

Cold full `repo index` requests return a durable task handle immediately and start a bounded background worker from the CLI process. Non-interactive agents can run `repo index-worker --task-id <id> --format json` as an explicit single-shot drain command for queued or retrying tasks; `service worker run [--task-id <id>] --format json` is the split-worker preview entrypoint and claims at most one durable code-index task, completing or failing it through task id, lease owner, and attempt count checks; `service run` drains the same code-index queue for installed or foreground service operation. Use `repo status --format json` to inspect `active_task`, checkpoint counters, and scope retention while a cold repository index is running. `repo index <alias> --reset --format json` clears unfinished task leases for the repository without deleting completed indexed scopes or reviving terminal dead-letter history. Index writes use one live writer per repository; queries, reports, graph reads, file queries, and health diagnostics use bounded read-only connections to read committed snapshots where SQLite WAL permits it.

After a bulk code-index snapshot apply or checkpointed finalize succeeds, SQLite storage automatically runs best-effort `PRAGMA optimize` and `PRAGMA wal_checkpoint(PASSIVE)` to refresh planner statistics and fold WAL pages back into the main database. A maintenance failure does not roll back an otherwise successful index result, but `health --format json` and graph inspection expose `graph.sqlite.journal_mode`, `wal_size_bytes`, `last_maintenance_at_ms`, and `last_maintenance_error`. The timestamp and error are persisted in SQLite so they survive service restarts and one-shot worker exits. Under `partitioned_sqlite`, these fields aggregate the control database and active repository shard databases through read-only shard diagnostics; if any active shard cannot be inspected, `wal_size_bytes` is unknown and the shard error remains visible. Large-repository query-plan or indexing-performance regressions should be covered through `tools/self_iteration --categories performance` rather than uncontrolled large fixtures in ordinary CLI paths.

`repo remove <alias>` deletes the registered repository behind that alias from relay-knowledge runtime state, including all aliases for the repository id, code index scopes, code-index tasks, repository-set membership, repository-set overlays, and software projection rows. It does not delete files from the source repository on disk. Removal is rejected while the repository still has a running code-index task lease; after a successful remove, the same path or alias can be registered again.

`query` returns display-compatible `results`, an agent-oriented `context_pack`, per-family `indexes`, scoped `index_cursors`, and `index_refresh` queue/lag diagnostics. `index_refresh.stale_reasons` explains BM25, semantic, vector, and scoped cursor lag or failures; `index_cursors` reports source scope, modality, backend cursor, model metadata, indexed graph version, and last error where present. `--freshness wait-until-fresh` runs the bounded refresh path before answering; `--freshness allow-stale` may return stale read models but marks metadata and degraded reason; `--freshness graph-only` bypasses derived read models and leaves cursor/queue diagnostics empty.

`files index` scans configured or explicit authorized local roots into the bounded file-location index. Explicit roots must be absolute paths allowed by `RELAY_KNOWLEDGE_FILE_INDEX_ROOTS`; omitting `--root` scans the configured roots. `files query` reads that committed file index rather than shelling out to Everything, Spotlight, Windows Search, locate, `rg`, or `grep`. The JSON response includes `freshness.state`, `freshness.index_lag`, `freshness.cursors`, `freshness.stale_reason`, `freshness.degraded_reason`, `freshness.bounded_rescan_required`, `freshness.direct_source_read_required`, `freshness.direct_source_read_paths`, and `freshness.agent_instructions`. `--freshness wait-until-fresh` suppresses pending, stale, degraded, or overflowed file-index answers until a bounded scan has completed. `--freshness allow-stale` may return indexed paths with those diagnostics; agents must directly read returned paths before editing or citing changed files when `direct_source_read_required=true`.

`repo query` runs `definition`, `references`, and `hybrid` queries through the indexed tree-sitter graph and SQLite FTS read model first. With `--freshness allow-stale`, if the target ref is still being full-indexed and has not finalized, the query reads the previous completed committed scope and marks the response stale/degraded; `wait-until-fresh` still requires the target scope to be fresh. The JSON response includes `freshness.state`, `freshness.index_lag`, `freshness.pending`, `freshness.cursor`, `freshness.direct_source_read_required`, and `freshness.agent_instructions` so agents can see checkpoint progress and know when returned paths require direct source reads before editing or citation. Only when structured layers leave a specific recall gap does the query start bounded internal exact-text source fallback against the same indexed commit. JSON hits are marked with `retrieval_layers=["lexical","text_fallback"]`; definition fallback may also include `definition`. Candidate-path lookup, candidate-file, materialized-byte, or line-length budget exhaustion degrades only the fallback layer and appears in `degraded_reason`; structured code graph results remain valid.

`repo query --query` accepts inline filters such as `kind:function`, `lang:rust` or `language:rust`, `path:storage`, and `name:query`. Unknown `prefix:value` tokens remain ordinary search text. Inline language filters intersect explicit `--language`; `kind` and language narrow SQL candidates, while `path` and `name` filter scored hits before truncation. `name:` matches symbol identities and SBOM package identities, not arbitrary excerpt text.

`repo feature-flags` reads configuration-driven feature-flag graph facts written during indexing. By default it lists flags, configuration sources, and code-usage edges in the selected repository scope; `--query` filters by flag name, config key, path, or excerpt. Its JSON response includes the same `freshness` object as `repo query`, including pending task, checkpoint cursor, index lag, stale/degraded reason, and direct-source-read paths for returned feature-flag usage files. The extractor recognizes environment variables, config/settings keys, boolean config declarations, and common SDK evaluation calls such as OpenFeature, LaunchDarkly, and Unleash clients. It does not sync provider control-plane state, strategies, segments, or rollout variants. The command does not scan the whole source tree at query time; after extractor changes or newly added flags, run `repo index` or `repo update` before expecting new facts.

`repo software` reads the software global-model projection for the selected repository scope. `--kind dependencies` returns package components derived from manifests and lockfiles plus `dependency_usages` that link declared packages to matching code/config import evidence; `--kind sdks` returns unresolved external import/include targets as SDK or API-surface usage candidates; `--kind files` returns whole-file nodes for code, config, docs, build manifests, deployments, tests, and templates; `--kind topics` returns topics extracted from Markdown/spec headings and `.knowledge/knowledge-map.yaml`; `--kind relationships` returns cross-domain edges such as `documents`, `depends_on`, `uses_sdk`, and `configures`. `--kind build` returns package, script, target, feature, module, profile, plugin, goal, and job entries extracted from Cargo, npm, Python, Go, Maven effective `pom.xml`, Gradle, CMake, Makefile, and CI workflow evidence. `--kind iac` returns deployment and infrastructure resources extracted from Dockerfile, Compose, Kubernetes YAML, Helm charts, Terraform, systemd, launchd, and CI workflow evidence. `--kind design` returns evidence-backed software systems, modules, components, interfaces, and capabilities from README files, architecture/design Markdown, and package/module manifests. The command does not execute build tools, scan package caches, SDK directories, cloud APIs, unindexed external source, or whole-repository docs at query time; rerun `repo index` or `repo update` to refresh the projection after source-scope changes.

Agent-facing MCP kind access reuses the same kind families rather than introducing parallel names. `relay_code_query` covers code graph kinds, `relay_software_query` covers software global-model kinds, and `relay_code_feature_flags` covers configuration-driven feature flags. Common agent aliases are normalized to existing kinds: `dependency` to `dependencies`, `configuration` to `relationships`, and `model` or `models` to `design`.

`map` commands maintain the repository-local `.knowledge/knowledge-map.yaml` knowledge navigation contract. The YAML stores topic, source, route, and history metadata only; it does not copy authoritative knowledge out of documents, code, config, CI, runtime systems, or external sources. One topic can contain multiple sources, and `map source add` appends distinct source ids to that topic route order. LLM agents should use `map show` and `map route` to locate sources, maintain the contract through `map source add/update/remove`, and run `map validate --format json` after changes. AGENTS.md should keep only a stable reference such as `Knowledge map: .knowledge/knowledge-map.yaml`.

## 3.5 Read and Write Impact

Status, health, help, setup doctor/profile, provider probe, version check, report, map show/route/validate/agent-snippet, and audit query are diagnostic entry points and should not mutate graph facts. `health` is a liveness fast path: it does not queue index refresh work and does not wait for a code-index writer to finish; when storage is busy it may return stale/degraded `storage_busy`. `version check` may only refresh the version-check cache under the runtime cache directory. `ingest`, `map init`, `map source add/update/remove`, `repo remove`, `repo index`, `repo update`, `index refresh`, `worker run-once`, proposal state changes, and service definition write can write runtime state, derived indexes, proposals/audit, the knowledge navigation contract, or service definitions.

Automated callers should read operation and read/write metadata from `help --format json` before exposing a command in CI, agents, or the Web operation surface.

## 3.6 Skill-over-CLI

The repository ships `skills/relay-knowledge-cli`, a ClawHub-compatible skill
for LLM agents that should operate relay-knowledge by invoking the local CLI and
parsing JSON output. It covers installation checks, `version check`, setup and
health diagnostics, knowledge graph ingestion/query, and code repository
registration, indexing, query, update, impact, and report workflows.

The skill intentionally does not configure MCP, call MCP tools, or manage ACP
sessions. Use the MCP/ACP chapters for protocol-level agent access.
