# Industry Capability Snapshot 2026

[English](../../en/04-research/01-industry-capability-snapshot-2026.md) | [中文](../../zh/04-research/01-industry-capability-snapshot-2026.md)

[Documentation index](../../en/README.md) | [GitHub repository](https://github.com/coolplayagent/relay-knowledge)

> Document version: 1.0
> Prepared: 2026-05-13
> Scope: GraphRAG, agent protocols, managed retrieval, local graph retrieval,
> and relay-knowledge usability gaps.

## Research Positioning

| Dimension | Conclusion |
| --- | --- |
| Sources | Primary sources are official Microsoft GraphRAG/DRIFT, MCP, A2A, OpenAI File Search, and Neo4j GraphRAG materials, cross-checked against this repository's current capability gaps. |
| Goal | Convert industry signals into relay-knowledge product direction, separating capabilities that should be productized now from interfaces that should remain forward-looking. |
| Competitive focus | Local-first operation, three-layer retrieval, explainable graph paths, authorization boundaries, service operation, and agent-protocol interoperability should combine into differentiation instead of chasing one managed-retrieval feature. |
| Scenarios and future | Targets zero-config onboarding, enterprise knowledge retrieval, repository understanding, agent workspaces, and long-running background index operation. |

## Summary

By 2026, the mainstream direction has moved beyond vector-only RAG toward
explainable, governable, interoperable knowledge systems:

- GraphRAG has expanded from local graph search to global/community search,
  DRIFT, and query routing.
- MCP has become a mainstream protocol through which agents access local
  tools, resources, and context; Streamable HTTP replaces the earlier HTTP+SSE
  transport.
- A2A has moved from an early specification into a production ecosystem for
  cross-framework agent collaboration and capability discovery.
- Managed-retrieval products hide complex indexing and execution details behind
  defaults and expose only a small set of result-count, filtering, and citation
  controls.
- Graph-database ecosystems combine full-text, vector, graph traversal, agent
  framework integration, and explainable paths as standard GraphRAG features.

The current `relay-knowledge` architecture is broadly aligned with that
direction: local-first operation, a unified API, three-layer retrieval,
freshness, QoS, audit, MCP/ACP adapters, and a code graph are already present.
The main gap is not the absence of advanced features. It is that advanced
configuration reaches new users too early, concrete providers and service
installation still need productization, and query-routing or DRIFT-like
strategies are not yet explicit interfaces.

## Industry Signals

The Microsoft GraphRAG query engine distinguishes local search, global search,
DRIFT search, basic search, and question generation. Local search serves
questions around specific entities; global search performs map-reduce over
community reports; DRIFT injects community information into local search to
broaden starting points and factual diversity. Microsoft Research's DRIFT
experiment on more than 5,000 news articles and 50 local questions also reports
more wins than local search on comprehensiveness and diversity.

The MCP 2025-11-25 specification defines stdio and Streamable HTTP as standard
transports. Streamable HTTP requires one MCP endpoint to support POST and GET,
and emphasizes Origin validation, localhost as the local default,
authentication, session ids, protocol-version headers, cancellation,
resumability, and backward compatibility. `relay-knowledge` implements
Streamable HTTP, session headers, protocol-version validation,
resources/prompts, and session termination through DELETE. GET/SSE resumability
remains a future enhancement.

The official A2A specification positions Agent2Agent as an interoperability
standard across agent frameworks, languages, and vendors. On 2026-04-09, the
Linux Foundation announced support from more than 150 organizations, major
cloud-platform integrations, and production use cases; A2A v1.0 was also
published as its first stable production release. For `relay-knowledge`, A2A is
better suited to a future specialist-knowledge-agent gateway than to turning
the core into an agent runtime.

OpenAI File Search illustrates the usability direction for managed retrieval:
users create a vector store, upload files, and use `file_search` as a Responses
API tool, while the platform manages retrieval and combines semantic and
keyword search. It exposes only a small control surface, such as result count,
reducing the burden of balancing token use, latency, and quality.

The Neo4j GraphRAG ecosystem presents GraphRAG as a system capability that
combines knowledge graphs, vector search, full-text search, graph traversal,
communities/clustering, agent frameworks, and MCP/A2A integration. Its emphasis
on factual sources, paths, relationships, and explainability aligns with
`relay-knowledge` context packs, graph paths, and provenance.

## relay-knowledge Current Fit

- **Aligned:** local-first storage, SQLite read models, BM25,
  semantic/vector retrieval, graph paths, temporal/community context,
  structured facts, freshness/versioning, QoS, MCP Streamable HTTP, an ACP
  adapter, and a repository code graph.
- **Partially aligned:** global/community retrieval has community-summary
  context items but no query router, lite-global, or DRIFT-like strategy
  interface.
- **Partially aligned:** the MCP tool surface is available, while resumability
  and a more complete session lifecycle still need productization.
- **Partially aligned:** architecture documents reserve an A2A gateway
  direction but do not specify agent cards, task lifecycle, artifact mapping,
  or signed identity in detail.
- **Open gaps:** concrete external embedding/OCR/vision providers, remaining
  privileged service installation and release diagnostics, and other documented
  productization work.
- **Usability gap:** earlier README and guide paths put too many environment
  variables on the main path, implying that a new user had to understand every
  configuration option first.

## Product Direction

- **Zero configuration by default:** local deterministic read models are the
  default path; users should not first have to select an embedding provider,
  HTTP budget, QoS policy, or MCP policy.
- **Layered advanced configuration:** Basic keeps CLI arguments; Advanced owns
  embedding, QoS, HTTP, and MCP; Deployment owns service-manager and remote
  access; Diagnostic owns CI and reproduction variables.
- **One onboarding loop:** `status -> ingest -> query -> health` is the minimum
  loop; repository graphs, Web, MCP, and external backends follow later.
- **Stable core boundary:** keep the existing context pack rather than adding a
  core final-answer API; query routers, lite-global retrieval, and DRIFT-like
  expansion can evolve above it.

## Sources

- Microsoft GraphRAG Query Engine: https://microsoft.github.io/graphrag/query/overview/
- Microsoft Research DRIFT Search: https://www.microsoft.com/en-us/research/blog/introducing-drift-search-combining-global-and-local-search-methods-to-improve-quality-and-efficiency/
- MCP Streamable HTTP transport, 2025-11-25: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- A2A Protocol specification: https://a2a-protocol.org/dev/specification/
- A2A Protocol v1.0 announcement: https://a2a-protocol.org/latest/announcing-1.0/
- Linux Foundation A2A adoption update, 2026-04-09: https://www.linuxfoundation.org/press/a2a-protocol-surpasses-150-organizations-lands-in-major-cloud-platforms-and-sees-enterprise-production-use-in-first-year
- OpenAI File Search guide: https://platform.openai.com/docs/guides/tools-file-search/
- Neo4j GraphRAG Labs: https://neo4j.com/labs/genai-ecosystem/graphrag/

---

Navigation: Next: [2. Knowledge Graph Research Summary](02-knowledge-graph-research.md)
