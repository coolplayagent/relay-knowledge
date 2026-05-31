# Software Global Modeling, CodeGraph, and Search Everything Comparison 2026

[English](../../en/04-research/11-software-global-codegraph-search-everything-comparison-2026.md) | [中文](../../zh/04-research/11-software-global-codegraph-search-everything-comparison-2026.md)

> Document version: 1.0
> Prepared: 2026-05-31
> Scope: comparison of 2026 open-source systems, papers, and systems-engineering references across software global modeling, code graphs, and search-everything retrieval.

## 1. Research Conclusions

In 2026, software-understanding tools are moving from "read files and grep" toward three complementary foundations:

- **Software global modeling**: versioned facts for source, builds, dependencies, SDKs, configuration, tests, documentation, releases, runtime state, vulnerabilities, licenses, and operational events.
- **CodeGraph**: structured code navigation for agents through Tree-sitter, LSP, CPG, call/reference/import edges, and community discovery.
- **Search Everything**: one retrieval experience across file paths, metadata, full text, BM25, vectors, symbols, graph paths, and authorization filters.

The strongest systems are not defined by having a vector database or an MCP tool. They separate structural facts, derived indexes, refresh state, and query budgets. `relay-knowledge` should keep graph facts as the source of truth, while BM25, semantic indexes, vector indexes, local file paths, code symbols, and community summaries remain freshness-aware read models.

## 2. Comparison Matrix

| Direction | Representative sources | Strengths | Gaps | What relay-knowledge should absorb |
| --- | --- | --- | --- | --- |
| Software global modeling | RepoDoc, RPG, SemanticForge, SBOM/SPDX/CycloneDX | Brings repositories, modules, dependencies, generation, documentation, and supply chain into one lifecycle | Most papers evaluate generation or documentation tasks, not local service operations | Treat `SoftwareSystem`, `BuildTarget`, `PackageComponent`, `Sdk`, `ReleaseArtifact`, and runtime state as first-class facts |
| CodeGraph | Codebase-Memory, jcodemunch-mcp, code-graph-mcp, GitNexus, Tree-sitter, Code Property Graph | Answers call-chain, reference, entrypoint, impact, dead-code, and cross-file navigation questions | Early open-source tools often lack durable tasks, authorization scope, index versions, and recovery semantics | Serve agents through a persistent graph, but require source scope, versions, and diagnostics for extraction, refresh, and query paths |
| Search Everything | Everything, plocate, Zoekt, Sourcegraph Code Search, ripgrep, Vespa/OpenSearch RRF | Mature path/name/full-text/regex/BM25/hybrid ranking mechanisms | Alone, these systems do not model software lifecycle or graph facts | Split path, metadata, content, code-symbol, vector, and graph-path read models, then fuse them through query routing and RRF/phased ranking |
| Deterministic Code RAG | RAGdeterm, Repository-Level Code Generation with KG, KCoEvo | Uses structural stores, dependency edges, and repeatable retrieval to reduce embedding randomness | Language coverage and runtime facts may be limited | Prefer structural facts for code generation, impact analysis, and audit queries; use vectors only for supplemental recall and reranking |
| Vulnerability and quality analysis | KG-HiAttention, Code Property Graph, Graph4Code | AST, CFG, DFG, expert features, and graph attention support explainable risk paths | Model output cannot replace auditable evidence | Make vulnerabilities, test failures, complexity, churn, and runtime events graph facts or traceable derived signals |

## 3. Open-Source Observations

### 3.1 MCP-Native CodeGraph

Codebase-Memory is one of the clearest 2026 signals: it combines Tree-sitter parsing, multilingual extraction, a persistent knowledge graph, call-graph traversal, impact analysis, and MCP exposure to reduce the cost of agents repeatedly reading files and running grep. Its paper evaluates on real repositories and agent queries, showing that code graphs are shifting from IDE assistance into core agent runtime context.

jcodemunch-mcp, code-graph-mcp, CodeGraphContext, Octocode, and similar projects show rapid convergence around "local SQLite or embedded storage + Tree-sitter + FTS/BM25 + optional vectors + MCP tools." This direction fits local-first products, but it exposes two risks:

- Tools can confuse "a relationship is queryable" with "the relationship is fresh and authorized," without source scopes, permission summaries, index cursors, or stale/degraded states.
- More MCP tools increase the need for query routing, task intent classification, token budgets, traces, and negative evidence, otherwise agents still waste cost on tool calls.

### 3.2 Code Search and Symbol Retrieval

Zoekt, Sourcegraph Code Search, GitHub Code Search, and ripgrep define the engineering baseline for mature code search: trigrams, regex literal extraction, shard indexes, concurrent walking, path filters, and symbol-first retrieval must be fast. For `relay-knowledge`, structural graphs should not replace those lower-level recall systems. The correct route is to shrink candidates through filters and bounded windows first, then treat symbol edges, BM25, exact text, vectors, and graph paths as explainable ranking signals.

### 3.3 Search Everything and Local File Indexes

Everything and plocate show that the "search everything" experience starts with extremely low-latency indexes over filenames, paths, and metadata, not by embedding all content. Platform watchers, USN/FSEvents/inotify/fanotify, and bounded rescans are useful models for incremental file read models. `relay-knowledge` should not depend on external desktop-search daemons, but it should adopt the mechanisms: separate path and content indexes, diagnosable change cursors, bounded rescans after cursor overflow, and authorization filtering before candidate windows.

## 4. Paper Route Observations

RepoDoc uses a repository knowledge graph for documentation generation and incremental updates. Its key lesson is not only generation quality; it connects module clustering, cross references, Mermaid diagrams, and change-impact propagation into a documentation lifecycle. For `relay-knowledge`, documentation should be a derived view of the software graph, not content detached from code, dependencies, and version changes.

RPG and Repository-Level Code Generation with KG lift code generation into repository-level planning, emphasizing file structure, capabilities, data flows, functions, and dependency constraints. SemanticForge further combines static/dynamic knowledge graphs, constraint solving, and incremental maintenance. Together, they show that generation context needs structural constraints, not only similar snippets or long-context concatenation.

RAGdeterm highlights the deterministic requirements of code retrieval. For audit, generation, repair, and impact analysis, the same repository version, query policy, and authorization scope should return reproducible evidence. Vector search can improve conceptual recall, but it cannot be the only entrypoint for code and dependency facts.

KCoEvo and KG-HiAttention represent graph-assisted code evolution and vulnerability analysis. They show that KGs are useful beyond question answering: they can carry evolution traces, candidate changes, risk paths, and explainability signals. Product implementation should first make graph facts, evidence, tests, and runtime events traceable, then put learned models in ranking or risk-assessment layers.

## 5. `relay-knowledge` Implementation Principles

1. **Graph facts are the source of truth**: source symbols, dependencies, SDKs, build targets, configuration, tests, documentation, releases, and runtime events enter versioned graph facts; derived indexes are rebuildable views.
2. **Multiple read models run in parallel**: `code_symbol`, `code_text`, `local_file_path`, `local_file_metadata`, `local_file_content`, `semantic`, `vector`, and `graph_path` refresh, observe, and degrade independently.
3. **Route before fusion**: exact-symbol, file-path, conceptual, impact, dependency, documentation, vulnerability, and temporal queries use different retriever combinations, then merge through RRF or phased ranking.
4. **Refresh must be recoverable**: extraction, embedding, FTS, edge finalization, community discovery, and documentation derivation go through durable tasks, attempt leases, bounded queues, retry backoff, checkpoints, dead letters, and index-lag metrics.
5. **Missing external scope must stay explicit**: unauthorized repositories, missing SDKs/headers, external packages, and generated SDKs become unresolved metadata, not `degraded_reason` or guessed accepted edges.
6. **Agent interfaces need diagnostics**: MCP/ACP responses should include source scope, graph version, index cursor, candidate window, ranking contribution, truncation reason, and stale/degraded state.

## 6. Product Route

| Priority | Recommendation | Acceptance signal |
| --- | --- | --- |
| P0 | Define the minimal schema for global code, file, dependency, SDK, build, test, documentation, and release facts. | Architecture docs explain each fact type's owner, version, and derived indexes. |
| P0 | Standardize code graph retrieval traces across exact symbol, BM25, source fallback, graph path, semantic/vector, and RRF contribution. | Context packs explain which retriever and graph version produced each evidence item. |
| P1 | Add local-file Search Everything read models, with path/metadata first and content/semantic/vector second. | Filename queries are not delayed by content extraction, and cursor overflow reports stale/degraded diagnostics. |
| P1 | Treat RepoKG documentation lifecycle as a derived-view design. | Documentation updates use affected scopes instead of regenerating an entire repository. |
| P1 | Add an agent golden set for structural CodeGraph. | Evaluation covers call chains, references, entrypoints, dead code, impact range, dependency drift, and cross-language search. |

## 7. References

- Codebase-Memory: Tree-Sitter-Based Knowledge Graphs for LLM Code Exploration via MCP. <https://arxiv.org/abs/2603.27277>
- RepoDoc: A Knowledge Graph-Based Framework to Automatic Documentation Generation and Incremental Updates. <https://arxiv.org/abs/2604.26523>
- RAGdeterm: Deterministic retrieval-augmented generation for code generation. <https://www.sciencedirect.com/science/article/pii/S2352711026001299>
- KCoEvo: A Knowledge Graph Augmented Framework for Evolutionary Code Generation. <https://arxiv.org/abs/2603.07581>
- KG-HiAttention: synergizing AI-based knowledge graphs and deep learning for explainable software vulnerability analysis. <https://www.frontiersin.org/journals/artificial-intelligence/articles/10.3389/frai.2026.1794125/full>
- RPG: A Repository Planning Graph for Unified and Scalable Codebase Generation. <https://arxiv.org/abs/2509.16198>
- Knowledge Graph Based Repository-Level Code Generation. <https://arxiv.org/abs/2505.14394>
- SemanticForge: Repository-Level Code Generation through Semantic Knowledge Graphs and Constraint Satisfaction. <https://arxiv.org/abs/2511.07584>
- Code Property Graphs. <https://www.ieee-security.org/TC/SP2014/papers/ModelingandDiscoveringVulnerabilitieswithCodePropertyGraphs.pdf>
- Graph4Code. <https://www.semantic-web-journal.net/system/files/swj2575.pdf>
- Tree-sitter. <https://github.com/tree-sitter/tree-sitter>
- Zoekt. <https://github.com/sourcegraph/zoekt>
- Sourcegraph Code Search. <https://sourcegraph.com/docs/code-search/features>
- Everything indexes. <https://www.voidtools.com/support/everything/indexes>
- plocate. <https://plocate.sesse.net/>
- ripgrep performance notes. <https://burntsushi.net/ripgrep/>

---

Navigation: Previous: [10. Software Global Domain Modeling Research 2026](10-software-global-domain-modeling-research-2026.md)
