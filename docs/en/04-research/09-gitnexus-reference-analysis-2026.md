# GitNexus Feature and UI Implementation Research 2026

[English](../../en/04-research/09-gitnexus-reference-analysis-2026.md) | [中文](../../zh/04-research/09-gitnexus-reference-analysis-2026.md)

> Document version: 1.0
> Prepared: 2026-05-18
> Reference project: <https://github.com/abhigyanpatwari/GitNexus>
> Reference revision: `7d500390b93068dee43c5e507edf5b9116d1c277`, 2026-05-18, `fix: Use Ladybug native read-only enforcement and prepared statement execution for Cypher query paths (#1655)`
> Scope: research GitNexus public capabilities, CLI/MCP/HTTP backend, Web graph UI, agent interaction patterns, and product lessons that can become future `relay-knowledge` improvements. This document does not import source code or copy implementation.

## 1. Research Positioning

| Dimension | Conclusion |
| --- | --- |
| Sources | Official GitHub README, `ARCHITECTURE.md`, main-branch source layout, and selected implementation files. |
| Goal | Identify GitNexus lessons in code knowledge graphs, agent tools, Web visualization, and local bridge UX. |
| Competitive judgment | GitNexus is strongest when it packages precomputed code structure, process flows, impact analysis, MCP tools, and graph UI into an agent-native workflow. |
| Adoption boundary | `relay-knowledge` can adopt capability semantics and interaction patterns, while preserving Rust async-first design, `env`/`paths`/`net` ownership, platform service-manager operation, graph versioning, and derived-index freshness. |

## 2. Executive Conclusion

GitNexus is a TypeScript monorepo with three main parts:

- `gitnexus/`: npm CLI, MCP stdio server, HTTP API, Tree-sitter parsing, LadybugDB graph storage, embeddings, and indexing pipeline.
- `gitnexus-web/`: Vite + React Web UI. Current architecture documentation describes it as a graph explorer and AI chat thin client whose queries go through the `gitnexus serve` HTTP API.
- `gitnexus-shared/`: shared types, constants, and resilience utilities consumed by CLI and Web.

The official README positions GitNexus as a zero-server code intelligence engine and presents both CLI/MCP and Web UI usage paths. One important observation: the README still describes a browser-only WASM path, while `ARCHITECTURE.md` and current Web source show the primary Web path as a local HTTP backend bridge via `gitnexus serve`. For `relay-knowledge`, that difference is itself a useful product signal: quick demos, browser exploration, and daily large-repository agent workflows may share one UI, but backend capability, scale limits, index state, and security boundaries must be explicit.

Lessons worth adopting first:

- Give agents structured tools instead of making models explore raw graph edges through many turns.
- Make community, process, impact, route/tool/shape, and related precomputed read models part of retrieval results.
- Link Web search, graph selection, code references, process panels, and AI chat into one shared interaction state.
- Use a local HTTP bridge so the Web UI can browse already indexed repositories without repeated upload or re-indexing.
- Use repository registry and group/contract registry concepts for multi-repository discovery, cross-service impact analysis, and monorepo partitioning.

Lessons to avoid copying directly:

- Do not copy TypeScript source, frontend styling, prompt text, skills, or license-restricted implementation.
- Do not write runtime state into repositories by default. `relay-knowledge` configuration, indexes, logs, caches, and dead-letter data remain owned by `paths` unless explicitly configured otherwise.
- Do not make browser-stored long-lived provider credentials the default path.
- Do not run full-graph layout, community detection, embedding, cloning, parsing, or large-file scans on query hot paths.
- Do not expose file-mutating rename-style MCP tools by default. Write operations require scope policy, audit, preview, and explicit authorization.

## 3. Capability Map

### 3.1 CLI and Index Lifecycle

GitNexus CLI commands cover a full lifecycle from first setup to maintenance:

- `setup`: auto-configures MCP for Cursor, Claude Code, OpenCode, and Codex.
- `analyze [path]`: indexes a repository with force rebuild, embeddings, worker timeout, max file size, repo aliasing, skip Git discovery, and skip AGENTS/CLAUDE/skills injection options.
- `serve`: starts the local HTTP server for the Web UI and Streamable HTTP MCP.
- `mcp`: starts the stdio MCP server.
- `list`, `status`, `doctor`, `clean`, `remove`: manage indexes, status, and diagnostics.
- `wiki`: generates repository wiki content from the graph.
- `query`, `context`, `impact`, `cypher`, `detect-changes`: call backend tools directly without MCP overhead.
- `group`: manages multi-repository groups, contract sync, cross-repository query/impact/status/contracts.

The useful pattern is that the CLI is not only an indexing command. It also covers setup, diagnostics, cleanup, direct queries, MCP, Web bridge, documentation generation, and multi-repo composition. `relay-knowledge` already has setup profiles, service doctor, repository commands, and MCP/ACP access; future work can make Web, agent, and multi-repository workflows feel like one explainable command surface.

### 3.2 Index Pipeline and Graph Model

GitNexus architecture describes a 12-phase pipeline:

```text
scan -> structure -> [markdown, cobol] -> parse -> [routes, tools, orm]
  -> crossFile -> mro -> communities -> processes
```

Core outputs include:

- File and folder structure: Project, Folder, File, and structure edges such as CONTAINS, DEFINES, and IMPORTS.
- Code symbols: Function, Class, Interface, Method, language-specific nodes, and cross-file references/calls/inheritance/implementation edges.
- Route/tool/ORM read models: API routes, tool definitions, database queries, and corresponding handler/consumer relationships.
- MRO and call resolution: language provider hooks, receiver inference, dispatch decisions, and method override/implement edges.
- Communities: Leiden community detection with functional areas, cohesion, keywords, and membership relationships.
- Processes: execution flows from entry points to terminals, used as first-class units for agent query and the Web `Processes` panel.

Implications for `relay-knowledge`:

- Flow-shaped results are more useful for agents than isolated symbol hits. Query results should be organized by process, module, evidence path, and affected surface.
- Route, tool, and shape mismatch views are high-value product surfaces inside a code graph, and should feed API and agent-review impact analysis.
- An explicit pipeline DAG with typed stage outputs helps incremental refresh, partial rebuilds, metrics, and test localization. `relay-knowledge` already has refresh queues and code-graph modules; route/tool/consumer shape can become independent derived indexes.

### 3.3 Storage, Registry, and Freshness

GitNexus stores each repository index under `.gitnexus/` inside the repository, then registers a pointer in `~/.gitnexus/registry.json`. The MCP server reads that registry and opens LadybugDB connections lazily. Metadata records indexed commit, file hashes, incremental dirty state, schema version, and stats for staleness detection and crash recovery.

That path fits an npm tool and portable demos, but `relay-knowledge` should not default to repository-local runtime state. The transferable semantics are:

- A global registry lets one resident service discover multiple authorized repositories.
- Every index records last commit, schema version, file hashes, dirty flag, and stats.
- Multi-repository connection pools should be lazy, idle-evicted, and concurrency-limited.
- Staleness should be visible in MCP resources, Web badges, query responses, and reindex prompts.

`relay-knowledge` should map those semantics to runtime/data/cache directories owned by `paths`, with source scopes explicitly authorizing repository roots.

### 3.4 MCP and Agent Tools

GitNexus MCP tools are not thin graph query wrappers. They are agent APIs that return pre-organized context:

| Tool | Product meaning |
| --- | --- |
| `list_repos` | Discover indexed repositories and guide later repo-scoped tool calls. |
| `query` | BM25 + semantic + RRF process-grouped search with task context, goal, limits, and max symbols. |
| `context` | 360-degree symbol view with callers, callees, process participation, and file location. |
| `impact` | Blast radius organized by depth, confidence, process, module, and risk summary. |
| `detect_changes` | Map git diff hunks to indexed symbols and affected processes. |
| `rename` | Generate multi-file rename preview/edit candidates from graph and text search. |
| `cypher` | Low-level structural query escape hatch. |
| `route_map`, `tool_map`, `shape_check`, `api_impact` | Specialized views for API routes, MCP/RPC tools, response shapes, and consumer drift. |
| `group_list`, `group_sync` | Manage cross-repository groups and the Contract Registry. |

Resources and prompts also serve agent workflows: `gitnexus://repos`, repo context, clusters, processes, schema, group contracts/status, plus `detect_impact` and `generate_map` prompts. The README also describes Exploring, Debugging, Impact Analysis, and Refactoring skills, plus community-derived repo-specific skills.

Direct improvement points for `relay-knowledge`:

- MCP tool responses should include next-step hints, scope, freshness, degraded reason, and correlation id by default.
- `query` can add intent fields such as `goal`, `task_context`, `max_symbols`, and `include_content` so ranking and context packs are more explainable.
- `context` and `impact` should continue moving from hit lists toward executable flows and risk groups.
- Route/tool/shape views can become high-value specialized tools for API impact analysis and agent review.

### 3.5 HTTP Bridge and Background Jobs

`gitnexus serve` uses Express to expose Web UI files, REST APIs, and MCP over HTTP. Public endpoints include health, heartbeat, info, repos, repo, graph, query, search, file, grep, processes, clusters, analyze, embed, and MCP. The service binds to localhost by default and restricts CORS to localhost, private/LAN origins, and the official Vercel UI.

Web analysis is managed by `JobManager`:

- Only one active analysis job runs at a time.
- Same-repository requests are deduplicated by returning the existing job.
- Analysis runs in a child process and supports cancellation, a 30-minute timeout, SSE progress, and a 1-hour terminal job TTL.
- The Web client consumes analyze/embed progress through SSE and shows reconnecting state when the server disconnects.

Transferable lessons:

- Every long-running Web task should be cancellable, observable, recoverable or at least retryable, instead of being a synchronous request.
- SSE progress should become a shared pattern for Web indexing, embedding, workers, rebuilds, and maintenance.
- `relay-knowledge` keeps HTTP under `net::http`; background tasks must integrate QoS, bounded queues, leases, dead letters, and startup reconciliation.

### 3.6 Web UI Information Architecture

The current GitNexus Web UI is a React workspace:

- `DropZone`/`RepoLanding`/`AnalyzeOnboarding`: detects a local server, shows indexed repositories, or starts repository URL/path analysis.
- `Header`: logo, repository dropdown, re-analyze/delete, global symbol search, settings, help, chat entry, and embedding status.
- `FileTreePanel`: browse files and graph nodes by directory.
- `GraphCanvas`: full-screen Sigma.js graph canvas with hover, selection, zoom, fit, focus, rerun layout, depth/label/edge type filters, and AI highlight toggle.
- `CodeReferencesPanel`: displays code references for the selected node or AI citation.
- `RightPanel`: `Nexus AI` chat and `Processes` tab; chat output can contain clickable file/node grounding.
- `SettingsPanel` and provider settings: OpenAI, Azure OpenAI, Gemini, Anthropic, Ollama, OpenRouter, MiniMax, GLM, and related provider profiles.
- `StatusBar`: graph size, state, and operation feedback.

Graph rendering uses graphology + Sigma, ForceAtlas2 in a Web Worker, and noverlap cleanup. `graph-adapter.ts` prepositions nodes from structural relationships and communities, then scales size, mass, cluster spread, and edge size based on graph size. Colors express node type and community; selected nodes, neighbors, AI search results, blast radius nodes, and animated nodes are emphasized by Sigma reducers.

UI lessons worth adopting:

- The first screen is a usable workspace, not a marketing landing page. After automatic server detection, users land in repository selection or graph exploration.
- Graph, file tree, code references, and chat grounding share selection state, reducing the gap between an AI answer and inspectable evidence.
- `Processes` sits next to chat, signaling that execution-flow views are first-class objects rather than decorative search output.
- Layout, highlight, filters, focus, re-analyze, and connection-lost states are visible.

Risks to avoid:

- Huge full-graph rendering and long ForceAtlas2 runs must not block the main workflow. `relay-knowledge` Web should support progressive graph loading, scope lenses, server-side layout cache, or sampled views.
- Graph colors and AI highlights cannot replace auditable evidence. Every highlight should resolve to a query, tool call, source span, and graph version.
- Browser-configured provider keys need an explicit security boundary. Production defaults should prefer local server-side provider profiles and redacted diagnostics.

## 4. Comparison with Current relay-knowledge Direction

| Topic | GitNexus approach | relay-knowledge follow-up judgment |
| --- | --- | --- |
| Local-first | CLI, local registry, HTTP bridge, Web auto-connect. | Stay local-first, but keep runtime state under `paths` instead of repository-local defaults. |
| Code graph | Tree-sitter, imports/calls/heritage/MRO, communities, processes. | Continue strengthening tree-sitter code graphs; add route/tool/shape derived views and process read models. |
| Agent tools | MCP tools/resources/prompts/skills/hooks shaped around development workflows. | Keep MCP/ACP as access layers; add freshness, scope, audit, and next action to tool returns. |
| Web UI | React + Sigma graph, AI chat, Processes, Code refs, Repo dropdown. | Evolve Web diagnostics into an interactive graph and operations workspace with scope and budgeted progressive loading. |
| Retrieval | BM25 + semantic + RRF, grouped by process. | Continue BM25/semantic/vector/graph RRF and expose rank contribution, candidate windows, and truncation. |
| Multi-repo | Registry + group.yaml + Contract Registry + group query/impact. | Adopt as source groups, service boundaries, contract edges, and cross-repository impact. |
| Change impact | git diff -> symbol/process impact, rename preview. | Prioritize read-only change impact; any write path requires proposal, audit, and explicit approval. |
| Operations | npm/Docker, signed images, server health/heartbeat. | Continue Rust releases, platform service managers, documented specs, and reversible uninstall. |

## 5. Future Improvement Points

| Priority | Improvement | Acceptance signal |
| --- | --- | --- |
| P0 | Organize code query results by process/module/evidence path instead of node lists only. | `repo query`, MCP query, and Web search show flow groups, key symbols, source spans, freshness, and ranking explanations. |
| P0 | Add a Web code-graph workspace: graph canvas, file tree, code references, process list, and query/chat sharing selection state. | Playwright screenshots cover desktop and mobile viewports and verify nonblank graph, selectable nodes, open references, and non-overlapping panels. |
| P0 | Standardize SSE progress and cancellation for long tasks: repository indexing, semantic/vector refresh, local file indexing, and worker proposals. | Web, CLI, and MCP/HTTP APIs report job id, phase, percent, stale/degraded reason, and cancellation. |
| P1 | Add route/tool/shape derived indexes for API routes, MCP/RPC tools, and consumer property access impact analysis. | Architecture specs, capability docs, unit tests, and fixtures cover `impact` outputs with route/tool consumer risk. |
| P1 | Extend MCP tool contracts with `goal`, `task_context`, `max_symbols`, `include_content`, `freshness_policy`, and `audit_context`. | MCP resources/prompts docs and tests cover validation, defaults, scope limits, and degraded output. |
| P1 | Build source groups and cross-repository contract read models. | Group status, group query, and cross-repo impact are supported; the Contract Registry carries version, provider/consumer, confidence, and stale state. |
| P1 | Bind AI chat grounding to graph selection and context-pack provenance. | Clicking a chat citation locates file, node, source span, tool call, and graph/index version. |
| P2 | Introduce server-side or cached graph layout so large graphs do not start ForceAtlas2 from scratch in the browser every time. | Large repository graph first paint shows a scope lens inside budget; full layout is generated in the background and cancellable. |
| P2 | Add repo-specific skill generation as a `relay-knowledge` skill/export proposal. | Generated content includes graph version, scope, refresh reason, no overwrite of user-authored AGENTS.md, audit, and rollback. |
| P2 | Evaluate read-only rename/refactor planning instead of direct edits. | MCP returns multi-file edit proposals with confidence, references, and test suggestions; writes require user approval. |

## 6. Documentation Impact

Before these lessons move into implementation, update:

- Book 3 unified API and interface architecture: Web graph workspace, selection state, chat grounding, and job stream contracts.
- Book 3 code knowledge graph model: process, route, tool, shape, and contract read-model boundaries.
- Book 3 code retrieval ranking and impact analysis: process-grouped retrieval, change impact, and route/tool consumer risk.
- Book 3 background service, recovery, and self-healing: lease, cancel, timeout, and dead-letter semantics for Web-triggered indexing and embedding jobs.
- Book 2 Web workspace and agent access capabilities: after implementation, convert research conclusions into executable user capability docs.

## 7. Sources

- GitNexus official repository: <https://github.com/abhigyanpatwari/GitNexus>
- GitNexus official website summary: <https://gitnexus.homes/>
- GitNexus `ARCHITECTURE.md`: <https://github.com/abhigyanpatwari/GitNexus/blob/main/ARCHITECTURE.md>
- GitNexus README sections on CLI/MCP, Web UI, MCP tools, resources, prompts, multi-repo, and Docker: <https://github.com/abhigyanpatwari/GitNexus#readme>
- Local source inspection at `7d500390b93068dee43c5e507edf5b9116d1c277`: `gitnexus/src/cli/index.ts`, `gitnexus/src/mcp/tools.ts`, `gitnexus/src/server/api.ts`, `gitnexus/src/server/analyze-job.ts`, `gitnexus/src/storage/repo-manager.ts`, `gitnexus/src/core/search/hybrid-search.ts`, `gitnexus-web/src/App.tsx`, `gitnexus-web/src/components/GraphCanvas.tsx`, `gitnexus-web/src/hooks/useSigma.ts`, `gitnexus-web/src/lib/graph-adapter.ts`, `gitnexus-web/src/core/llm/agent.ts`, `gitnexus-web/src/core/llm/tools.ts`.
