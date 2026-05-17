# relay-knowledge English Documentation

[English](../en/README.md) | [中文](../zh/README.md)

This edition follows the same numbered book structure as the Chinese edition:
user workflows first, feature behavior second, then architecture specifications,
research notes, benchmark records, and verification records.

> Translation status: several English pages still preserve Chinese source prose
> while the full translation is maintained incrementally. Commands, API paths,
> environment variables, filenames, and configuration contracts are authoritative.

Directory responsibilities are fixed: `01-user-guide` is for executable user
workflows; `02-capabilities` describes implemented behavior only;
`03-architecture-specs` keeps hard contracts, interface boundaries, and forward
product requirements; `04-research` keeps dated research and gap analysis;
`05-benchmarks` stores benchmark and optimization records; `06-verification`
stores validation, audit, and documentation freshness records. Content files
inside numbered volumes use a two-digit chapter prefix; `README.md` files are
volume covers and count as chapter 0 when they appear in a reading path.

## Book 1: User Guide

- [Chapter 0: User Guide Overview](01-user-guide/README.md): install, CLI, knowledge graphs, code repository graphs, Web, agent access, resident service operation, background tasks, observability, troubleshooting, and advanced configuration.
- [Chapter 1: Installation and Runtime Directories](01-user-guide/01-install-and-runtime.md)
- [Chapter 2: CLI Basics](01-user-guide/02-cli-basics.md)
- [Chapter 3: CLI Command Reference](01-user-guide/03-cli-command-reference.md)
- [Chapter 4: Knowledge Graph](01-user-guide/04-knowledge-graph.md)
- [Chapter 5: Code Repository Graph](01-user-guide/05-code-repository-graph.md)
- [Chapter 6: Web Workspace](01-user-guide/06-web-workspace.md)
- [Chapter 7: MCP Agent Access](01-user-guide/07-mcp-agent-access.md)
- [Chapter 8: ACP Local Adapter](01-user-guide/08-acp-local-adapter.md)
- [Chapter 9: Resident Service](01-user-guide/09-resident-service.md)
- [Chapter 10: Workers, Proposals, and Audit](01-user-guide/10-workers-proposals-audit.md)
- [Chapter 11: Observability and Telemetry](01-user-guide/11-observability-and-telemetry.md)
- [Chapter 12: Advanced Configuration](01-user-guide/12-advanced-configuration.md)
- [Chapter 13: Operations and Troubleshooting](01-user-guide/13-operations-and-troubleshooting.md)

## Book 2: Capabilities

- [Chapter 1: GraphRAG Capability Guide](02-capabilities/01-graphrag-capability-guide.md): context packs, freshness, backends, multimodal evidence, code graph, recovery, Web, MCP, and ACP behavior.
- [Chapter 2: Hybrid Retrieval Context Pack](02-capabilities/02-hybrid-retrieval-context-pack.md): retriever sources, RRF fusion, structured graph facts, graph paths, and backend status.
- [Chapter 3: Code Repository Tree-sitter Retrieval](02-capabilities/03-code-repository-tree-sitter-retrieval.md): repository indexing, retrieval, reports, and impact analysis.
- [Chapter 4: Semantic/Vector Provider Backend](02-capabilities/04-semantic-vector-provider-backend.md): external embedding provider setup, redacted diagnostics, Web provider panels, and degradation behavior.

## Book 3: Architecture Specifications

- [Chapter 1: Architecture Vision and Algorithm Map](03-architecture-specs/01-architecture-vision-and-algorithm-map.md)
- [Chapter 2: Engineering Hard Constraints](03-architecture-specs/02-engineering-hard-constraints.md)
- [Chapter 3: Foundational Runtime](03-architecture-specs/03-foundational-runtime.md)
- [Chapter 4: Source Scope Model](03-architecture-specs/04-source-scope-model.md)
- [Chapter 5: Multimodal Evidence Ingestion](03-architecture-specs/05-multimodal-evidence-ingestion.md)
- [Chapter 6: Graph Fact Model and Versioning](03-architecture-specs/06-graph-fact-model-and-versioning.md)
- [Chapter 7: Storage Engine and Mutation Log](03-architecture-specs/07-storage-engine-and-mutation-log.md)
- [Chapter 8: Derived Indexes and Freshness](03-architecture-specs/08-derived-indexes-and-freshness.md)
- [Chapter 9: Hybrid Retrieval and Context Packing](03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Chapter 10: Semantic/Vector Provider Architecture](03-architecture-specs/10-semantic-vector-provider-architecture.md)
- [Chapter 11: Code Knowledge Graph Model](03-architecture-specs/11-code-knowledge-graph-model.md)
- [Chapter 12: Tree-sitter Extraction and Incremental Indexing](03-architecture-specs/12-tree-sitter-extraction-and-incremental-indexing.md)
- [Chapter 13: Code Retrieval Ranking and Impact Analysis](03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)
- [Chapter 14: Open Agent Runtime Adapter Architecture](03-architecture-specs/14-open-agent-runtime-adapter-architecture.md)
- [Chapter 15: Resident Agent Graph Access Protocol](03-architecture-specs/15-resident-agent-graph-access-protocol.md)
- [Chapter 16: Unified API and Interface Architecture](03-architecture-specs/16-unified-api-and-interface-architecture.md)
- [Chapter 17: Background Service, Recovery, and Self-Healing](03-architecture-specs/17-background-service-recovery-and-self-healing.md)
- [Chapter 18: Observability, Diagnostics, and SLO](03-architecture-specs/18-observability-diagnostics-and-slo.md)
- [Chapter 19: Installation, Release, and Upgrade](03-architecture-specs/19-installation-release-and-upgrade.md)

## Book 4: Research

- [Chapter 1: Industry Capability Snapshot 2026](04-research/01-industry-capability-snapshot-2026.md)
- [Chapter 2: Knowledge Graph Research Summary](04-research/02-knowledge-graph-research.md)
- [Chapter 3: arXiv Knowledge Graph Paper Insights](04-research/03-arxiv-knowledge-graph-paper-insights.md)
- [Chapter 4: ai-knowledge-graph Reference Analysis](04-research/04-ai-knowledge-graph-reference-analysis.md)
- [Chapter 5: Code Repository Tree-sitter Retrieval Research](04-research/05-code-repository-tree-sitter-retrieval-research.md)
- [Chapter 6: Agent Protocol Graph Retrieval Research](04-research/06-agent-protocol-graph-retrieval-research.md)
- [Chapter 7: relay-knowledge Implementation Reference](04-research/07-relay-knowledge-implementation-reference.md)

## Appendix A: Benchmarks

- [Appendix A.1: relay-teams Baseline 2026-05-14](05-benchmarks/01-relay-teams-baseline-2026-05-14.md)
- [Appendix A.2: relay-teams Optimization Issues 2026-05-14](05-benchmarks/02-relay-teams-optimization-issues-2026-05-14.md)
- [Appendix A.3: relay-teams Optimization Study 2026-05-14](05-benchmarks/03-relay-teams-optimization-study-2026-05-14.md)

## Appendix B: Verification

- [Appendix B.1: Documentation Book Refresh Audit 2026-05-17](06-verification/01-documentation-book-refresh-2026-05-17.md): directory responsibilities, closed capability status, and bookshelf index refresh.
- [Appendix B.2: Documentation Refresh Audit 2026-05-17](06-verification/02-documentation-refresh-audit-2026-05-17.md): documentation sync record for the code retrieval self-iteration commits.
- [Appendix B.3: Documentation Refresh Audit 2026-05-14](06-verification/03-documentation-refresh-audit-2026-05-14.md): current documentation status, refreshed gaps, and open productization work.
- [Appendix B.4: relay-teams E2E Verification 2026-05-14](06-verification/04-relay-teams-e2e-2026-05-14.md)
