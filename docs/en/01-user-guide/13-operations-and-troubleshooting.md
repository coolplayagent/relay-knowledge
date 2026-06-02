# Chapter 13: Operations and Troubleshooting

[English](../../en/01-user-guide/13-operations-and-troubleshooting.md) | [中文](../../zh/01-user-guide/13-operations-and-troubleshooting.md)

Troubleshooting should narrow the problem before fixing a single error. Do not rely only on Web page summaries; complete diagnostics live in JSON APIs and CLI responses.

## 13.1 Health Checks

Start with project status:

```bash
relay-knowledge status --format json
```

Then check configuration, health, and service diagnostics:

```bash
relay-knowledge setup doctor --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

Look first at `configuration_ready`, `live_health_checked`, `checks`, and `recommended_actions` from `setup doctor`. Passing setup configuration does not mean live health passed. Next, inspect graph version, index lag, refresh queue diagnostics, `index_refresh.stale_reasons`, runtime directories, HTTP bind, QoS budgets, agent protocol status, telemetry status, and degraded reason from `health` or `service doctor`.

## 13.2 Index Freshness

When a query returns stale or degraded results, inspect the graph and indexes:

```bash
relay-knowledge graph inspect --format json
relay-knowledge index refresh --kind bm25 --format json
relay-knowledge index refresh --kind semantic --kind vector --format json
```

If the caller cannot accept stale indexes, query with:

```bash
relay-knowledge query "topic" --freshness wait-until-fresh --format json
```

If only graph facts are needed:

```bash
relay-knowledge query "topic" --freshness graph-only --format json
```

`health`, `service doctor`, and `index refresh` return `index_refresh.stale_reasons`. Handle reasons containing failed state or `last_error` first. If only lag is reported, usually run `relay-knowledge index refresh --format json` or query with `--freshness wait-until-fresh`.

## 13.3 Common Errors

`missing value for --source` or `missing value for --content`: `ingest` requires both source scope and content.

`invalid --freshness value`: only `allow-stale`, `wait-until-fresh`, and `graph-only` are accepted.

`invalid --kind value`: `index refresh` accepts only `bm25`, `semantic`, or `vector`; `repo query` accepts only `hybrid`, `symbol`, `definition`, `references`, `callers`, `callees`, `imports`, or `sbom`; `repo software` accepts only `dependencies`, `sdks`, `build`, `iac`, `design`, or `all`.

`source_scope is required by the MCP access policy`: the MCP graph tool request lacks a scope, or unspecified scope is not allowed.

`source_scope '<scope>' is not authorized for this MCP policy`: the requested scope is not in `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES` and is not a code repository alias registered in the current runtime. Register the repository with `relay-knowledge repo register <path>` so the Git root or filesystem root directory name becomes the default alias, or pass `--alias <scope>` when a custom scope is required, then index it. Otherwise add `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=<scope>` and restart the service.

Path configuration error: advanced directory overrides and `RELAY_KNOWLEDGE_HOME` must be absolute paths and must not contain `..`. See [Chapter 12](12-advanced-configuration.md) for the full variable list.

OTLP Collector unavailable: `runtime.telemetry.last_error` in `service doctor --format json` records recent exporter initialization or export errors. This affects observability only; it does not mean graph retrieval is unavailable.

MCP HTTP 400: common causes are missing `Mcp-Session-Id`, missing `MCP-Protocol-Version`, invalid initialize payload, or a session flow that did not send `notifications/initialized`.

MCP HTTP 404: common causes are unknown, expired, or retired session IDs. Run initialize again and store the new `Mcp-Session-Id`.

`version does not support --format streaming-json`: `version` supports only `text` and `json`.

`repo impact` reports missing head snapshot: index or update the target head before impact analysis.

`repo status` shows `active_task.state=running` but `checkpoint.parsed_file_count` stays at 0: inspect `active_task.lease_expires_at_ms` and `checkpoint.updated_at_ms` in `repo status --format json`. In a non-interactive agent session, do not repeatedly start `service run`; it is a foreground resident process. If the task is queued or retrying, run `repo index-worker --task-id <active_task.task_id> --format json` to make one bounded worker attempt. If a previous service process claimed the lease and exited, wait for lease recovery before retrying; do not kill relay-knowledge processes or bypass task leases.

`repo query` reports a source fallback candidate or budget degraded reason: only the exact-text fallback layer is degraded. Tree-sitter code graph and SQLite FTS results remain usable. Narrow `--path`, `--language`, `--ref`, or the registered scope, then use `repo status --format json` to confirm that the target snapshot is fresh.

## 13.4 Isolated Reproduction

When debugging user data or local machine contamination, use a separate runtime home:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-repro \
  relay-knowledge status --format json
```

Add the same variable to ingest, query, repo, and service commands to reproduce behavior under an isolated data directory.

For Web or MCP issues, also fix bind, scope, and QoS variables:

```bash
RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-repro \
RELAY_KNOWLEDGE_HTTP_BIND=127.0.0.1:8791 \
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
  relay-knowledge service run --web --mcp streamable-http
```

Check from another terminal:

```bash
curl http://127.0.0.1:8791/api/health
curl http://127.0.0.1:8791/mcp/metrics
```

## 13.5 PR Verification Commands

Rust quality gates:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo llvm-cov --all-targets --all-features --fail-under-lines 90
```

Web and browser integration tests:

```bash
npm --prefix web ci
npm --prefix web run build
./build.sh
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

For documentation-only changes, at minimum check new links and Markdown file paths, and state in the PR why code tests were not run.

## 13.6 Diagnostic Order

When results do not match expectations, narrow the problem in this order:

1. `status --format json`: confirm runtime directories, configuration, and project status.
2. `setup doctor --format json`: read configuration checks, `configuration_ready`, and recommended actions.
3. `health --format json`: confirm graph version, index freshness, queue/dead-letter state, provider state, and QoS status.
4. `graph inspect --format json`: confirm evidence, entity, structured facts, and code counts.
5. `index refresh --format json`: attempt explicit refresh and read stale reasons.
6. Add `--format json` to the relevant business command: preserve metadata, degraded reason, and audit correlation.
7. `audit query --limit 50 --format json`: check whether recent CLI/Web/service/agent operations reached the unified API.
