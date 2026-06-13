# Chapter 7: MCP Agent Access

[English](../../en/01-user-guide/07-mcp-agent-access.md) | [中文](../../zh/01-user-guide/07-mcp-agent-access.md)

MCP Streamable HTTP exposes local graph retrieval capabilities to external agent runtimes. It reuses the unified API, QoS, scope policy, and audit instead of exposing storage or indexing internals.

## 7.1 Start an MCP Service

Before starting, generate a read-only agent configuration profile:

```bash
relay-knowledge setup profile agent-readonly --format json
```

Minimal local startup:

```bash
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=docs \
relay-knowledge service run --mcp streamable-http
```

Default endpoint:

```text
http://127.0.0.1:8791/mcp
```

For same-port Web/API/MCP, see [Chapter 9: Resident Service](09-resident-service.md).

## 7.2 Policy Variables

Common MCP policy variables:

```text
RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED
RELAY_KNOWLEDGE_MCP_ENDPOINT
RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS
RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES
RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE
RELAY_KNOWLEDGE_MCP_MAX_LIMIT
RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES
RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS
```

Default policy requires explicit allowed scopes. Without `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES`, graph tools reject unspecified scopes unless `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true` is set or the requested scope is already a code repository alias registered in the current runtime.

Registered repository aliases are added to a process-local dynamic allow-list on first MCP access. Repository-set aliases are not cached this way: `relay_code_repository_set_query` revalidates the current set members on each call. A repository-set alias is allowed only when the set alias is explicitly allowed and does not collide with a registered repository alias, or when every current member repository alias or member `source_scope` is already allowed by static policy or runtime repository authorization.

Unknown scopes are still rejected and return a remediation hint such as `RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=<scope>`. Remote binds are rejected by default; non-localhost listening requires `RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS=true`.

Before allowing remote clients, verify HTTP bind, origin allow-list, scope allow-list, QoS budgets, and audit policy. Do not make remote bind plus unspecified scope the default configuration.

## 7.3 Session Flow

Clients must follow the MCP Streamable HTTP session flow:

1. Call `initialize` and provide a supported `MCP-Protocol-Version`.
2. Store the server-returned `Mcp-Session-Id`.
3. Send `notifications/initialized`.
4. Send later requests with `Mcp-Session-Id` and `MCP-Protocol-Version`.

Missing session headers return HTTP 400. Unknown or retired session IDs return HTTP 404. Tool requests, `ping`, and `notifications/cancelled` are all bound to server-issued sessions.

When Web/API/MCP share one HTTP service, `notifications/cancelled` uses a bounded priority admission path so a client can cancel an active tool call even when the normal in-flight request budget is saturated. The bypass applies only to small `/mcp` JSON notifications that already carry a valid session.

The `initialize` to `tools/list` discovery path is storage-cold: MCP registers static tool schemas and returns exploration instructions without opening SQLite or running schema migration. Storage is opened lazily on the first storage-backed tool call, and concurrent first calls share the service storage initialization guard. The first `tools/list` for each session records an initialize-to-tools-list cold-start sample in agent protocol metrics and `/mcp/metrics`.

## 7.4 Tools, Resources, and Prompts

Current MCP tool surface:

- Graph retrieval.
- Graph inspection.
- Health status.
- Service status.
- Index status.
- Authorized code graph query.
- Authorized software global-model query.
- Authorized repository-set code graph query.
- Authorized code impact analysis.

Agent kind selection uses existing product kinds rather than a separate MCP taxonomy. `relay_code_query` accepts `hybrid`, `symbol`, `definition`, `references`, `callers`, `callees`, `imports`, and `sbom`. `relay_software_query` accepts `dependencies`, `sdks`, `files`, `topics`, `relationships`, `build`, `iac`, `design`, and `all`. Singular aliases are accepted for agent ergonomics, and `configuration` maps to software `relationships` while `model` or `models` maps to software `design`; configuration-driven feature flags stay on `relay_code_feature_flags`.

`relay_retrieve_context` returns GraphRAG context with `indexes`, `index_cursors`, and `index_refresh` diagnostics so agents can inspect BM25, semantic, vector, and scoped cursor lag before trusting derived context.

`relay_code_query` and `relay_code_feature_flags` return the same code graph freshness object as CLI and Web, including `freshness.state`, `freshness.index_lag`, `freshness.pending`, `freshness.cursor`, and `freshness.direct_source_read_required`. When direct source reads are required, agents must follow `freshness.agent_instructions` and verify `freshness.direct_source_read_paths` before using stale graph evidence for changed files.

Code query responses include an `explore_budget` object derived from indexed repository size. Repositories below 500 indexed files budget 1 exploration call, 15,000 output characters, and 5 returned files; 500-4,999 files budget 2 calls, 30,000 characters, and 10 files; 5,000-14,999 files budget 3 calls, 45,000 characters, and 15 files; larger repositories budget 5 calls, 75,000 characters, and 25 files. MCP applies the file cap to `relay_code_query` and `relay_code_repository_set_query` results and reports truncation under `agent_output`.

All MCP free-text queries are capped at 10,000 characters, and path filter entries are capped at 4,096 characters. `relay_code_query` and `relay_code_repository_set_query` also accept `include_code=true`; container-like class, struct, interface, enum, and trait hits are returned as compact signature-and-line outlines instead of large source bodies.

Current MCP resource surface:

- `relay://service/status`
- `relay://service/health`
- `relay://indexes/status`
- `relay://graph/summary`, exposed only when `RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE=true`.
- `relay://metrics/prometheus`

Current MCP prompt surface:

- `relay_retrieve_context_prompt`
- `relay_code_impact_prompt`

Resources and prompts provide read-only diagnostics, context, and invocation templates. They cannot bypass access policy and do not enable mutation, index refresh, or repository indexing.

## 7.5 Write Permission Boundary

MCP does not expose index refresh or repository indexing. Repository indexing must be triggered explicitly through `relay-knowledge repo index` or `relay-knowledge repo update`; derived index refresh must be triggered through CLI/Web operational workflows.

Agent requests are recorded as bounded in-process audit events with runtime identity, scope, freshness, QoS decision, budget, truncation, result count, and status. Durable audit writes do not open cold storage for discovery or read-only diagnostics; once a storage-backed tool has opened storage, audit events are mirrored to the durable store. Repository-set query audit entries record the `request.set_alias` value as the scope so multi-repository reads remain visible in the audit trail. See [Chapter 10](10-workers-proposals-audit.md) for persistent audit sink configuration.

## 7.6 Metrics Endpoint

`GET /mcp/metrics` returns a Prometheus text-format snapshot covering current graph version, index refresh queue depth, dead-letter count, QoS in-flight/queued request count, MCP cold-start sample count and duration totals, and stale state for each index. The endpoint still enters through the MCP router and QoS admission.

MCP clients should use Streamable HTTP `/mcp`. `/mcp/sse` and `/mcp/message` are no longer compatibility entry points.
