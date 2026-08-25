# Agent Protocol Graph Retrieval Research

[English](06-agent-protocol-graph-retrieval-research.md) | [中文](../../zh/04-research/06-agent-protocol-graph-retrieval-research.md)

[Documentation index](../README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

> Document version: 1.0
> Prepared: 2026-05-12
> Scope: exposing graph retrieval from a resident `relay-knowledge` process to
> other agents through an MCP server and an Agent Client Protocol adapter
> Conclusion: MCP and ACP can both expose graph retrieval, but solve different
> integration problems. They should be peer protocols over one core service.
> Protocol refresh: MCP references use the 2025-11-25 specification; the early
> HTTP+SSE transport is a compatibility path, not the default for new work.

## Research Positioning

| Dimension | Conclusion |
| --- | --- |
| Sources | MCP 2025-11-25 official specifications, ACP access shape, this repository's MCP/ACP implementation, and the A2A ecosystem direction. |
| Goal | Keep agent protocols as access layers for graph retrieval rather than duplicating business logic or turning the core into an agent runtime. |
| Competitive focus | Shared API, permissions, QoS, freshness, audit, and error semantics give different agent clients the same governable graph context. |
| Scenarios and future | Targets IDE/agent hosts, MCP tools/resources/prompts, ACP sessions, local services, and future A2A specialist-knowledge gateways. |

## 1. Background

`relay-knowledge` converges CLI, Web, MCP, local ACP, and future HTTP access on a
unified API layer. Hybrid retrieval, graph inspection, index refresh, health,
and background-service state are application-service capabilities. The current
implementation provides an MCP Streamable HTTP tool/resource/prompt adapter, a
local ACP session adapter, an optional persistent JSONL audit sink, and a
Prometheus metrics exporter. Remaining work centers on the documented service
installation, orchestration, and remote-agent productization boundaries.

This research uses **ACP** to mean **Agent Client Protocol**, not Agent
Communication Protocol or Agent Control Protocol.

The core questions are:

- Is an MCP server a suitable graph-retrieval tool entry point for other agents?
- Is an ACP-facing adapter suitable as a graph-retrieval session entry point for
  an IDE, agent client, or agent host?
- How can the protocols share permissions, QoS, retrieval freshness, audit, and
  error semantics?
- How can a resident process prevent a protocol adapter from becoming a second
  business layer?

## 2. Protocol Facts

### 2.1 MCP

MCP uses a host/client/server architecture. The host manages client lifecycle,
permissions, authorization, LLM integration, and context aggregation. A server
exposes a specialized capability and declares resources, tools, and prompts
through capability negotiation. References:

- [MCP Architecture](https://modelcontextprotocol.io/specification/2025-11-25/architecture)
- [MCP Tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
- [MCP Resources](https://modelcontextprotocol.io/specification/2025-11-25/server/resources)
- [MCP Prompts](https://modelcontextprotocol.io/specification/2025-11-25/server/prompts)

Implications for `relay-knowledge`:

- `relay-knowledge` is a natural MCP server for graph retrieval, graph state,
  index state, and diagnostic resources.
- The MCP host remains responsible for model calls, tool selection, user
  confirmation, cross-server orchestration, and complete conversation state.
- The server should neither read the complete conversation nor take over the
  host's agent-runtime responsibilities.
- Tool output should prefer `structuredContent` while also providing concise
  text for older clients and debugging.

### 2.2 Agent Client Protocol

Agent Client Protocol is a JSON-RPC protocol between an agent and a client
application. It defines initialization, authentication, session creation or
resumption, prompt turns, `session/update`, `session/cancel`, permission
requests, tool-call progress, and `_meta` extension points. References:

- [ACP Overview](https://agentclientprotocol.com/protocol/overview)
- [ACP Architecture](https://agentclientprotocol.com/get-started/architecture)
- [ACP Tool Calls](https://agentclientprotocol.com/protocol/tool-calls)
- [ACP Extensibility](https://agentclientprotocol.com/protocol/extensibility)

Implications for `relay-knowledge`:

- ACP is a suitable way to expose `relay-knowledge` as a client-driven
  knowledge-retrieval session.
- `session/prompt` can carry a natural-language retrieval request, while
  `session/update` can carry progress, index freshness, degradation, and result
  readiness.
- Tool-call `kind=search` fits graph retrieval, `kind=read` fits graph metadata
  or resource reads, and `kind=other` fits health and service state.
- Permission requests can protect high-cost operations such as a manual index
  refresh; ordinary reads still pass the local access policy.

## 3. Protocol Roles

| Dimension | MCP server | ACP adapter |
| --- | --- | --- |
| Primary relationship | A host/client calls a server capability | A client drives an agent session |
| Best fit | Another agent treats graph retrieval as a tool | An IDE or agent client treats the knowledge graph as a conversational retrieval agent |
| Main entry points | `tools/list`, `tools/call`, `resources/read`, `prompts/get` | `initialize`, `session/new`, `session/prompt`, `session/update`, `session/cancel` |
| Progress | Tool result or streaming-transport state | `session/update` and tool-call updates |
| Permission model | Host UI plus server-side policy | Client permission request plus adapter policy |
| Output | Structured tool result, resource, or prompt | Session updates, tool-call content, and final prompt response |
| Main risk | Treating the server as a runtime with excessive planning responsibility | Treating the knowledge service as a general code-editing agent |

Conclusions:

- MCP and ACP cover different integration surfaces and should both map to the
  same core capabilities.
- They must not implement retrieval independently; both map to the same unified
  API request, response, error, and metadata contracts.
- MCP is the recommended agent-to-tool entry point; ACP is the agent-client
  session entry point.
- The ACP adapter should not expose file editing, terminal execution, code
  modification, or general agent planning by default.

## 4. Shared Capability Model

The protocol layer translates requests; it does not own graph-retrieval rules:

```text
+----------------------+      +----------------------+
| MCP Server Adapter   |      | ACP Session Adapter  |
| tools/resources      |      | session/tool updates |
+----------+-----------+      +----------+-----------+
           |                             |
           +-------------+---------------+
                         |
                         v
               Agent Access Policy
                         |
                         v
                 Unified API Contract
                         |
                         v
              RelayKnowledgeService
                         |
       +-----------------+-----------------+
       v                 v                 v
   Retrieval          Storage          Indexing
```

The shared surface must include:

- source-scope resolution and authorization;
- freshness policy;
- graph version and indexed graph version;
- stale and degraded index state;
- result limits, context-byte budgets, and timeouts;
- QoS admission, rate limits, and cancellation;
- traces, metrics, audit, and stable error mapping.

## 5. Security

Agent protocol endpoints connect external agents, hosts, and user input directly
to graph retrieval. Security boundaries are therefore required on both sides of
the protocol adapter:

- Every request passes `AgentAccessPolicy` before a unified API request is
  constructed.
- Read-only capability is the default. Mutation, commit, entity merge, delete,
  and cross-scope writes are not implicitly authorized.
- `refresh_indexes` is not a domain write, but consumes CPU, I/O, and index
  capacity, so it should be disabled by default or require permission.
- Prompt-injection text is evidence/context only; it cannot change access
  policy, freshness policy, or authorization.
- MCP tool annotations, ACP `_meta`, and client-supplied identities are
  untrusted input until validated for audit context.
- Errors must not disclose unauthorized scopes, full local paths, secrets,
  original proxy configuration, or internal SQL.

## 6. Runtime Shape

The OS service manager should host the resident process. Protocol adapters are
external entry points to that process, not new background schedulers.

Recommended shape:

- Service mode binds to a local address or stdio by default.
- MCP supports stdio and local Streamable HTTP integration; remote listening is
  explicit.
- ACP should prefer stdio when clients launch an agent subprocess on demand. A
  local launcher or proxy can connect a client to an existing resident service.
- Every protocol request enters the same `net::qos` budgets. Even stdio work is
  subject to in-flight and queue-depth limits.
- Shutdown first stops new MCP/ACP admission, then cancels or completes accepted
  requests, and finally flushes telemetry.

## 7. Retrieval Experience

An external agent needs explainable context rather than matching text alone.
Recommended context-pack fields are:

- `metadata`: trace, request, graph version, indexed graph version, and stale
  state;
- `source_scope`: the scope actually searched;
- `freshness`: the policy actually applied;
- `retrieval_mode`: hybrid, graph-only, or degraded hybrid;
- `results`: graph, evidence, and result hits;
- `citations`: evidence id, entity id, scope id, and displayable location;
- `indexes`: status for each index family;
- `degraded_reason`: unavailable index, stale state, timeout, budget truncation,
  or another bounded cause;
- `truncated`: whether a budget truncated the result.

For MCP, this object belongs in tool `structuredContent`. For ACP, it belongs in
`session/update` under `_meta.relayKnowledge`; the final prompt response needs
only a stop reason and artifact id or a short summary.

## 8. Engineering Conclusions

The v1 direction and current implementation follow these rules:

1. **Peer protocols:** MCP and ACP are first-class adapters to the resident
   process.
2. **Read-first:** retrieval, graph inspection, health, service state, and index
   state are externally available; refresh is restricted by default; writes
   require a separate authority boundary.
3. **One core:** both adapters call the unified API rather than copying
   retrieval, indexing, or storage logic.
4. **Safe local defaults:** local access, narrow scopes, disabled refresh, and
   disabled remote listening are the defaults.
5. **Observable:** each request records trace, runtime identity, policy and QoS
   decisions, freshness, and result truncation.
6. **Cancellable:** ACP cancellation and MCP disconnect paths release budgets
   and stop unnecessary work.

---

Navigation: Previous: [5. Code Repository Tree-sitter Retrieval Research](05-code-repository-tree-sitter-retrieval-research.md) | Next: [7. relay-knowledge Implementation Reference](07-relay-knowledge-implementation-reference.md)
