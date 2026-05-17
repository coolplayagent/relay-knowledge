# Chapter 6: Web Workspace

[English](../../en/01-user-guide/06-web-workspace.md) | [中文](../../zh/01-user-guide/06-web-workspace.md)

The Web workspace is for local diagnostics and operation execution. It is not a separate business layer; command previews and results come from the same backend application service.

## 6.1 Build Static Assets

The Web workspace lives in `web/`:

```bash
./build.sh
```

Build output is written to `web/dist`. Browser integration tests build static assets before starting their test static-directory server.

## 6.2 Same-Origin APIs

The current Web client reads and executes through same-origin service endpoints:

```text
/api/project/status
/api/health
/api/service/status
/api/web/graph/canvas
/api/web/operations/execute
/api/configs/model/profiles
/api/configs/model-fallback
/api/configs/model/catalog
/api/configs/model:probe
/api/configs/model:discover
```

The UI shows project health, GraphRAG readiness, provider backend diagnostics, graph counts, Status graph overview, Graph canvas, scoped index freshness, refresh queue diagnostics, stale reasons, runtime budgets, agent/model settings, and the operation composer. Complete diagnostics still live in `/api/health`, `/api/service/status`, and operation result JSON.

The Providers panel shows only redacted semantic/vector backend mode, model, dimension, endpoint host, key configured state, and cursor metadata. The Web UI does not store or echo raw provider API keys.

## 6.3 Page Structure

The Web workspace uses left navigation and a right detail area. On desktop, the navigation stays fixed and the detail area scrolls independently; on narrow screens, navigation becomes a fixed top menu. Selecting Status, Readiness, Graph, Providers, Operations, Indexes, Runtime, or Settings shows only that page instead of stacking all panels in one long page.

The toolbar provides light/dark theme switching. The first load follows the browser system theme; user choice is stored in browser local storage.

The Graph page provides three read-only canvases:

- Knowledge: entity, evidence, relation, claim, and event facts.
- Code: source scope, code file, code symbol, and reference/call/import/define relationships.
- Mixed: knowledge and code graphs combined, including source-scope or source-path links that can be inferred across graphs.

Canvas requests use `/api/web/graph/canvas?kind=knowledge|code|mixed&scope=<scope>&query=<text>&limit=<n>`. The default limit is 250 and the maximum is 1000. The backend always returns a bounded snapshot at the current graph version and marks truncation in `summary.truncated`.

## 6.4 Operation Execution

The Web Operations panel covers typed command/request preview and same-origin execution for:

- Context retrieval and evidence ingest.
- Graph inspection and index refresh.
- Code repository registration, indexing, query, update, impact analysis, and status.
- Provider probe.
- Worker status and run-once.
- Proposal list, show, accept, reject, and supersede.
- Audit query.
- Service status and service run snapshot.

`Run` sends the current snapshot and shows pending, success, or error state. Execution requests do not use the 10-second diagnostic client timeout, so long indexing or maintenance operations are not aborted by the frontend.

The displayed command is a CLI-equivalent preview, not a simulated frontend result. The real result comes from the unified API response returned by `/api/web/operations/execute`. On error, copy operation, command, error kind, and metadata from result JSON, then reproduce with the same CLI arguments.

## 6.5 Same-Port Local Service

Start a local Web/API/MCP service:

```bash
./build.sh
./run.sh start --port 8791 --daemon
```

Open:

```text
http://127.0.0.1:8791/
http://127.0.0.1:8791/api/health
```

`run.sh` does not build automatically. If `target/release/relay-knowledge` or `web/dist/index.html` is missing, run `./build.sh` first.

## 6.6 Browser Integration Tests

Local validation:

```bash
uv sync --extra dev --no-default-groups
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

The tests cover diagnostics, the Status page query entry point, Status graph overview, single-detail navigation, theme switching, GraphRAG readiness, graph canvas controls, operation composer, index table, runtime panel, Settings-generated configuration, model provider profiles, provider probe/discovery, and mobile layout.

## 6.7 Safety Boundary

The Web workspace is for local diagnostics and operations. It is not an installer or daemon manager. Browser service run returns only a runtime snapshot; real resident services are started by CLI, `run.sh`, or the platform service manager.

Remote access is disabled by default and is accepted only when MCP remote-client policy and HTTP bind allow it explicitly. `/api/web/operations/execute` request bodies are limited by `RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES`, and actual retrieve, ingest, index, repository, worker, proposal, audit, and service operations all reuse the backend application service.
