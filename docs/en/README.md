# relay-knowledge English Documentation

[English](../en/README.md) | [中文](../zh/README.md)

This edition follows the same numbered book structure as the Chinese edition:
user workflows first, feature behavior second, then architecture specifications,
research notes, benchmark records, and verification records.

> Translation status: several English pages still preserve Chinese source prose
> while the full translation is maintained incrementally. Commands, API paths,
> environment variables, filenames, and configuration contracts are authoritative.

## Book 1: User Guide

- [User Guide Overview](01-user-guide/README.md): install, CLI, GraphRAG, code repositories, Web, agent services, troubleshooting, and advanced configuration.
- [Chapter 1: Installation and Runtime Directories](01-user-guide/01-install-and-runtime.md)
- [Chapter 2: CLI Basics](01-user-guide/02-cli-basics.md)
- [Chapter 3: Knowledge Graph Workflow](01-user-guide/03-knowledge-graph-workflow.md)
- [Chapter 4: Code Repository Workflow](01-user-guide/04-code-repository-workflow.md)
- [Chapter 5: Web Workspace](01-user-guide/05-web-workspace.md)
- [Chapter 6: Agent and Resident Service](01-user-guide/06-agent-and-service.md)
- [Chapter 7: Operations and Troubleshooting](01-user-guide/07-operations-and-troubleshooting.md)
- [Chapter 8: Advanced Configuration](01-user-guide/08-advanced-configuration.md)

## Book 2: Capabilities

- [GraphRAG Capability Guide](02-capabilities/graphrag-capability-guide.md): context packs, freshness, backends, multimodal evidence, code graph, recovery, Web, MCP, and ACP behavior.
- [Hybrid Retrieval Context Pack](02-capabilities/hybrid-retrieval-context-pack.md): retriever sources, RRF fusion, structured graph facts, graph paths, and backend status.
- [Semantic/Vector Provider Backend](02-capabilities/semantic-vector-provider-backend.md): external embedding provider setup, redacted diagnostics, Web provider panels, and degradation behavior.
- [Code Repository Tree-sitter Retrieval](02-capabilities/code-repository-tree-sitter-retrieval.md): repository indexing, retrieval, reports, and impact analysis.
- [Documentation Refresh Audit 2026-05-17](02-capabilities/documentation-refresh-audit-2026-05-17.md): documentation sync record for the code retrieval self-iteration commits.
- [Documentation Refresh Audit 2026-05-14](02-capabilities/documentation-refresh-audit-2026-05-14.md): current documentation status, refreshed gaps, and remaining implementation work.

## Book 3: Architecture Specifications

- [Engineering Hard Constraints](03-architecture-specs/engineering-hard-constraints.md)
- [Foundational Runtime](03-architecture-specs/foundational-runtime.md)
- [Storage Layer Design](03-architecture-specs/storage-layer-design.md)
- [Unified API and Interface Architecture](03-architecture-specs/unified-api-and-interface-architecture.md)
- [GraphRAG Product and Implementation Roadmap](03-architecture-specs/graphrag-product-and-implementation-roadmap.md)
- [Source Scope and Multimodal Ingestion](03-architecture-specs/source-scope-and-multimodal-ingestion.md)
- [Code Repository Tree-sitter Retrieval Specification](03-architecture-specs/code-repository-tree-sitter-retrieval.md)
- [Code Repository Retrieval v2 Optimization](03-architecture-specs/code-repository-retrieval-v2-optimization.md)
- [Knowledge Graph Capability Reference](03-architecture-specs/knowledge-graph-capability-reference.md)
- [Semantic/Vector Provider Backend Specification](03-architecture-specs/semantic-vector-provider-backend.md)
- [Open Agent Runtime and Hybrid Retrieval Architecture](03-architecture-specs/open-agent-runtime-and-hybrid-retrieval-architecture.md)
- [Resident Agent Graph Retrieval Access](03-architecture-specs/resident-agent-graph-retrieval-access.md)
- [Background Service, Silent Updates, and Self-Healing](03-architecture-specs/background-service-and-self-healing.md)
- [Advanced Architecture and Observability](03-architecture-specs/advanced-architecture-observability.md)
- [Installation and Release](03-architecture-specs/installation-and-release.md)

## Book 4: Research

- [Knowledge Graph Research Summary](04-research/knowledge-graph-research.md)
- [arXiv Knowledge Graph Paper Insights](04-research/arxiv-knowledge-graph-paper-insights.md)
- [Code Repository Tree-sitter Retrieval Research](04-research/code-repository-tree-sitter-retrieval-research.md)
- [Agent Protocol Graph Retrieval Research](04-research/agent-protocol-graph-retrieval-research.md)
- [relay-knowledge Implementation Reference](04-research/relay-knowledge-implementation-reference.md)
- [ai-knowledge-graph Reference Analysis](04-research/ai-knowledge-graph-reference-analysis.md)
- [Industry Capability Snapshot 2026](04-research/industry-capability-snapshot-2026.md)

## Appendix A: Benchmarks

- [relay-teams Baseline 2026-05-14](05-benchmarks/relay-teams-baseline-2026-05-14.md)
- [relay-teams Optimization Study 2026-05-14](05-benchmarks/relay-teams-optimization-study-2026-05-14.md)
- [relay-teams Optimization Issues 2026-05-14](05-benchmarks/relay-teams-optimization-issues-2026-05-14.md)

## Appendix B: Verification

- [relay-teams E2E Verification 2026-05-14](06-verification/relay-teams-e2e-2026-05-14.md)
