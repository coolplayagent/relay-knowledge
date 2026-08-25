# relay-knowledge Implementation Reference

[English](07-relay-knowledge-implementation-reference.md) | [中文](../../zh/04-research/07-relay-knowledge-implementation-reference.md)

[Documentation index](../README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

> Prepared: 2026-05-13
> Progress refreshed: 2026-05-17
> Scope: compare the repository's Rust implementation with the knowledge-graph,
> GraphRAG, Agentic KG, Tree-sitter code-graph, protocol-access, and background
> service research, recording closed foundations and open productization work.
> Position: an engineering reference, not a replacement for the hard constraints
> and interface specifications in `docs/en/03-architecture-specs/`.

## Research Positioning

| Dimension | Conclusion |
| --- | --- |
| Sources | The previous six research chapters, current code and capability docs, and the GraphRAG, agentic KG, Tree-sitter, temporal graph, and multimodal-evidence directions. |
| Goal | Compress research conclusions into an implementation path, clarifying what is already closed, what remains a gap, and what should stay as a future-facing interface. |
| Competitive focus | The research path should compete through shared core services, versioned facts, explainable context packs, code graphs, and service operations rather than feature count. |
| Scenarios and future | Targets v1 productization, later GraphRAG expansion, agent access, silent updates, evaluation loops, and install/deployment governance. |

## 1. Executive Conclusion

At the time of this refresh, `relay-knowledge` already had an extensible
knowledge-graph foundation: a unified API; an async application service; SQLite
graph state and graph versions; structured facts; index-freshness metadata;
scoped index cursors carrying source hash, backend cursor, and model metadata;
bounded refresh queues; task-lease and reconciler diagnostics; structured stale
reasons; an FTS5 BM25 read model; local semantic/vector read models;
configurable external semantic/vector backend contracts; schema-path,
temporal, and community retrieval; an RRF context pack; deterministic local
reranking; Tree-sitter repository indexing; multimodal evidence schema;
background and maintenance extraction submission; worker proposal lifecycle;
MCP Streamable HTTP; a local ACP session adapter; MCP resources/prompts;
Prometheus metrics; an optional JSONL audit sink; CLI/Web entry points; a
GraphRAG evaluation fixture; foundational `env`/`paths`/`net` boundaries; and
QoS configuration.

It was not yet a complete service-installation product. The recorded open work
included concrete external embedding/OCR/vision/table/layout providers,
privileged service lifecycle and packaging, conflict and valid-time product
semantics, remote ACP/A2A access, a query router, and release-facing evaluation
reports. Later documents may close individual items; this dated research note
keeps the historical boundary rather than silently rewriting it.

The product direction is a **knowledge substrate**, not a copy of one GraphRAG
framework:

- The core owns facts, evidence, versions, scopes, indexes, retrieval,
  diagnostics, and audit.
- An external agent runtime owns planning, tool calls, approvals, long-running
  sessions, and final LLM generation.
- LLM or agent output can enter proposals, diagnostics, summaries, or derived
  indexes, but cannot bypass the graph-mutation contract and overwrite accepted
  facts.
- GraphRAG value lies in retrieval planning and context organization: BM25,
  semantic, vector, and graph expansion cooperate to return an explainable
  context pack rather than only a natural-language answer.

## 2. Implementation Baseline

### 2.1 Reusable Core Foundations

- The `api` layer defines shared request/response types for CLI, Web, HTTP, and
  agent adapters, including ingest, hybrid retrieval, context packs, graph
  inspection, index refresh, health, service state, agent identity, and
  repository APIs. The Web adapter connects its operation composer to the
  application service through same-origin `/api/web/operations/execute`.
- The `application` layer converges business entry points in
  `RelayKnowledgeService`, so CLI and adapters do not access SQLite or
  Tree-sitter directly.
- Storage traits isolate graph facts, mutation logs, index metadata, and code
  queries. The SQLite implementation sends blocking database work to
  `spawn_blocking` workers.
- The domain layer includes `GraphVersion`, `SourceScope`, `FreshnessPolicy`,
  `IndexStatus`, `GraphMutationBatch`, `EvidenceRecord`, and code-graph types.
- The code and code-service layers support Git repositories and non-Git source
  directories, clean snapshots, filesystem synthetic snapshots, incremental
  diffs, worktree overlays, multilingual Tree-sitter parsing, code-graph
  retrieval, bounded internal exact-text source fallback, and diff impact.
- `net::http` and `net::qos` own configuration validation, event-driven HTTP,
  timeouts, request-body budgets, and admission policy. MCP Streamable HTTP runs
  within those boundaries.
- The MCP adapter supports sessions, protocol headers, tool calls, resources,
  prompts, Prometheus metrics, access policy, QoS, cancellation, restricted
  refresh, code query/impact, bounded audit, and an optional JSONL sink.
- The local ACP adapter supports initialization metadata, session creation,
  prompt progress, cancellation, context artifacts, runtime identity, QoS,
  bounded audit, and an optional JSONL sink.

These foundations align with the research: async-first operation, one API,
decoupled graph storage, index freshness, code graphs, and scope isolation.

### 2.2 Capability Boundary at This Refresh

- `retrieve_context` combined SQLite FTS5 BM25, graph-evidence fallback,
  code-graph documents, local semantic tokens, local hashed-vector ANN,
  schema paths, temporal events, community summaries, RRF, and deterministic
  reranking. Context items carried structured facts, fact-derived one-hop
  `graph_paths`, source spans, code artifacts, rerank signals, and backend
  availability metadata. Semantic/vector backend state came from read-model
  cursors plus runtime configuration and supported `local`, `external`, and
  `disabled` modes.
- `index_status` aggregated BM25, semantic, vector, and other family freshness.
  Scoped cursors recorded kind, scope, modality, graph version, source hash,
  and backend cursor. Semantic/vector workers could publish model name and
  dimension. `refresh_indexes` scheduled persistent tasks, acquired leases,
  replayed the mutation log, and advanced cursors. Refresh completion derived
  model metadata from indexed documents rather than separating runtime labels
  from read-model provenance.
- The generic knowledge graph had expanded beyond evidence/entity rows to typed
  relations, claims/events, confidence, source spans, status, version-range
  validation, and worker proposals. Valid time, conflict resolution, and a
  complete fact-review product experience remained open.
- Background-service state was exposed through the API. Foreground
  `service run` executed a minimal startup index reconciler. Refresh had task
  rows, leases, retry, dead-letter counts, reconciler replay, and attributed
  stale reasons. Service-definition generation and silent-update state existed;
  privileged install, rollback, watchdog, and maintenance orchestration were
  still productization work.
- MCP Streamable HTTP and the local ACP session adapter were usable with access
  policy, QoS, bounded audit, optional JSONL output, code query/impact tools,
  MCP resources/prompts, and metrics.

## 3. Reusable Directions

### 3.1 GraphRAG and LightRAG: Explainable Context First

The graph is not a vector-store replacement; it is a layer for retrieval
planning, relationship expansion, and context organization. The priority is to
keep `HybridRetrievalResponse` as an auditable context pack:

- return matching entities, relationships, chunks, source scope, graph and
  index versions, retriever source, and score explanation;
- use entity linking, bounded neighborhoods, and evidence chunks for local
  questions;
- use community/summary read models for future global questions;
- preserve paths for multi-hop questions rather than expanding arbitrary
  k-hop neighborhoods;
- report stale, degraded, truncated, and freshness-policy state on every
  result.

The core need not generate final LLM answers. It organizes grounded context;
an external runtime or UI decides whether to generate an answer.

### 3.2 Agentic KG: Keep the Core out of Runtime Planning

- Adapters perform protocol translation, pre-authorization, identity injection,
  QoS admission, and error mapping.
- MCP tools/resources/prompts are the default knowledge-tool surface for other
  agents.
- ACP is suitable for conversational retrieval but does not grant file editing,
  terminal execution, or code modification by default.
- High-risk operations such as mutation commits, entity merges, or index
  rebuilds require proposals/approval or explicit permission.
- Every agent request records trace, runtime identity, source scope, freshness,
  QoS decision, and result truncation.

The unified API is already downstream of the MCP adapter; MCP and ACP do not
need independent retrieval implementations.

### 3.3 Tree-sitter Code Graph: Productize the Strongest Path

Repository retrieval was the closest capability to delivery. It covered Git
snapshots, incremental diffs, worktree overlays, Tree-sitter parsing for Rust,
Python, JavaScript/JSX, TypeScript/TSX, Go, Java, Kotlin, Scala, C/C++, C#,
Ruby, PHP, Swift, and Bash, plus symbols, references, imports, calls, chunks,
and impact.

Priority improvements were:

- complete scope metadata: repository, resolved commit, tree hash, path filters,
  and indexed ref;
- reliable incremental flow from changed paths through content-hash skips,
  tombstones, reverse dependents, and scoped refresh;
- agent-readable context packs across symbols, references, calls, imports,
  chunks, and impact;
- explicit limit, path/language filters, timeout, truncation, and degradation;
- bounded source fallback only when structured definition/reference/hybrid
  recall is insufficient, with `text_fallback` provenance and no invented
  resolved edge;
- syntax-level fact labels and ambiguous/unresolved state when cross-file
  semantics are uncertain.

### 3.4 Temporal Graphs: Separate System Version from Valid Time

- `graph_version`: committed graph-database state used for mutation replay,
  index cursors, and stale detection.
- `valid_from` / `valid_to`: when a fact holds in the modeled domain.
- `observed_at` / `source_published_at`: when evidence was observed or
  published.
- `as_of` / `time_range`: temporal constraints on retrieval.

Vector similarity cannot resolve temporal conflicts. Similar statements about
one entity can be true at different times, so graph version and fact-validity
time must constrain them together.

### 3.5 Multimodal Evidence: One Provenance Model

PDF pages, images, OCR, captions, tables, and layout regions should not become
isolated indexes:

- Original evidence keeps source URI/hash, media hash, modality, extractor and
  version, scope, and parent evidence.
- OCR, captions, and vision descriptions are derived evidence and never replace
  the source image or page.
- Extraction failure records diagnostics and degradation without blocking other
  modalities.
- Retrieval groups OCR, caption, image, and text hits under their common parent
  instead of displaying duplicates.

The existing fact/evidence API already supported paths, spans, confidence,
status, relations, claims, events, and version-range validation. Multimodal
extensions should preserve that provenance while adding modality, extractor,
parent, and diagnostic fields.

## 4. Gap Analysis at This Refresh

| Area | Baseline | Main gap | Priority |
| --- | --- | --- | --- |
| Unified API | Ingest/query/context pack/status/health/repository/agent identity/API operations/audit | Finer context artifacts and release diagnostics | P1 |
| Graph facts | Evidence/entity, typed relation, claim/event, confidence, source span, status, extraction metadata, proposals, graph version | Valid-time product semantics, conflict resolution, review UI | P1 |
| Hybrid retrieval | BM25, graph evidence, code documents, local semantic/vector, external-backend metadata, path/temporal/community, RRF, local rerank, context pack | Query router, lite-global/DRIFT-like expansion, external rerank provider | P1 |
| Code graph | Multilingual indexing, scope metadata, filters, report, query/impact, MCP tools, bounded exact-text fallback | Federated cross-repository resolution and broader real-world performance evidence | P1 |
| Background service | Status API, foreground service, startup reconciler, queue, leases, dead letters, metrics, definition preview, silent-update state | Privileged install, watchdog, rollback, package manifests, maintenance orchestration | P1 |
| Agent access | MCP Streamable HTTP, local ACP, resources/prompts, access policy, QoS, audit, JSONL, code tools, metrics | Remote ACP, A2A gateway, deeper host integration | P2 |
| Multimodal | Evidence/extraction schema, diagnostics, parent grouping, modality metadata, worker contract | Concrete providers, image embeddings, model-coexistence policy | P2 |
| Temporal graph | Graph version, event `occurred_at`, `as_of`/year retrieval | Valid-time range invalidation and hierarchical time graph | P2 |

## 5. Closed Foundations and Open Productization

The local GraphRAG path had advanced through the four research phases. They
became regression baselines rather than an unchanged backlog:

| Phase | Closed foundation | Open productization |
| --- | --- | --- |
| 1. Real retrieval loop | Typed facts, source spans, confidence, BM25 aliases, context packs, graph paths, code artifacts | Regression protection |
| 2. Recoverable refresh | Mutation log, scoped cursor, bounded queue, leases/retry/dead letter, startup reconciler, stale reasons | More capacity, failure, and recovery coverage |
| 3. Agent/service foundation | MCP Streamable HTTP, local ACP, resources/prompts, metrics, audit, QoS, Web operations | Remote ACP, A2A, host integration, privileged service lifecycle |
| 4. Advanced GraphRAG/multimodal foundation | Local semantic/vector, schema/temporal/community retrieval, multimodal schema, worker proposals, evaluation fixture | Concrete external providers, query router, lite-global/DRIFT, release evaluation |

The Web workspace executed retrieve, ingest, graph inspection, index refresh,
repository workflows, and service snapshots through
`/api/web/operations/execute`, then refreshed diagnostics after success.
`service run` mounted Web endpoints; when MCP Streamable HTTP was enabled, MCP
and Web routes shared one `net::http` listener and QoS budget.

## 6. Engineering Constraints

- `env` owns environment variables, `paths` owns platform paths, and `net` owns
  networking and HTTP.
- I/O, databases, Tree-sitter, embeddings, OCR, index rebuilds, and compaction
  cannot block async-runtime hot paths.
- Every queue is bounded; retrieval and traversal have limits, timeouts,
  cancellation, and truncation/degradation state.
- CLI, Web, HTTP, MCP, and ACP share the application service rather than copying
  business logic.
- A new public API needs a production caller or specification and tests.
- Documentation changes with implementation, especially for configuration,
  environment, paths, networking, QoS, indexes, services, and installation.

## 7. Recommended Next Work at This Refresh

1. Add concrete worker adapters for external embedding, OCR, vision, table, and
   layout providers, with coexistence strategy, provider limits, and diagnostics.
2. Complete service-manager install/upgrade/uninstall, rollback, package
   manifests, watchdog, and maintenance workflows.
3. Productize valid-time, conflict resolution, and fact-review UI semantics.
4. Plan a query router, lite-global/DRIFT-like expansion, external reranking,
   and an A2A gateway while keeping `HybridRetrievalResponse` canonical.
5. Expand GraphRAG evaluation data, longitudinal reports, and release thresholds
   across stale indexes, ambiguous entities, multi-hop, time, and code impact.

This sequence maximized reuse of the implementation while converting the most
important GraphRAG, Agentic KG, Tree-sitter, and freshness research into a
testable engineering loop.

---

Navigation: Previous: [6. Agent Protocol Graph Retrieval Research](06-agent-protocol-graph-retrieval-research.md) | Next: [8. Competitive, High-Performance, and Local File Retrieval Research 2026](08-competitive-performance-research-2026.md)
