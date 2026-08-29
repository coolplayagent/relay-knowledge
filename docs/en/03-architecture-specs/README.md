# Architecture Specifications

[English](README.md) | [中文](../../zh/03-architecture-specs/README.md)

This volume is the normative architecture contract for `relay-knowledge`.
Capability pages describe what is available; these specifications define the
ownership boundaries, invariants, resource limits, recovery semantics, and
acceptance evidence that implementation changes must preserve.

## Reading Guide

- Chapters 1–4 establish the system vision, engineering constraints, runtime
  foundations, and source-scope model.
- Chapters 5–13 define evidence, graph facts, storage, derived indexes,
  retrieval, providers, code graphs, indexing, ranking, and impact analysis.
- Chapters 14–18 define agent adapters, unified interfaces, background
  recovery, observability, and SLOs.
- Chapters 19–22 cover installation, multi-repository overlays, software-global
  modeling, and service deployment.
- Chapters 24–27 cover the executable knowledge-development contract, code
  index retention, the Git-commit mental model, and business-to-technical mapping.

## Chapter Index

1. [Architecture Vision and Algorithm Map](01-architecture-vision-and-algorithm-map.md)
2. [Engineering Hard Constraints](02-engineering-hard-constraints.md)
3. [Foundational Runtime](03-foundational-runtime.md)
4. [Source Scope Model](04-source-scope-model.md)
5. [Multimodal Evidence Ingestion](05-multimodal-evidence-ingestion.md)
6. [Graph Fact Model and Versioning](06-graph-fact-model-and-versioning.md)
7. [Storage Engine and Mutation Log](07-storage-engine-and-mutation-log.md)
8. [Derived Indexes and Freshness](08-derived-indexes-and-freshness.md)
9. [Hybrid Retrieval and Context Packing](09-hybrid-retrieval-and-context-packing.md)
10. [Semantic/Vector Provider Architecture](10-semantic-vector-provider-architecture.md)
11. [Code Knowledge Graph Model](11-code-knowledge-graph-model.md)
12. [Tree-sitter Extraction and Incremental Indexing](12-tree-sitter-extraction-and-incremental-indexing.md)
13. [Code Retrieval Ranking and Impact Analysis](13-code-retrieval-ranking-and-impact-analysis.md)
14. [Open Agent Runtime Adapter Architecture](14-open-agent-runtime-adapter-architecture.md)
15. [Resident Agent Graph Access Protocol](15-resident-agent-graph-access-protocol.md)
16. [Unified API and Interface Architecture](16-unified-api-and-interface-architecture.md)
17. [Background Service, Recovery, and Self-Healing](17-background-service-recovery-and-self-healing.md)
18. [Observability, Diagnostics, and SLO](18-observability-diagnostics-and-slo.md)
19. [Installation, Release, and Upgrade](19-installation-release-and-upgrade.md)
20. [Multi-Repository Code Graph Overlay](20-multi-repository-code-graph-overlay.md)
21. [Software Global Domain Modeling Architecture](21-software-global-domain-modeling.md)
22. [Service Deployment, Control Plane, and Data Plane](22-service-deployment-control-data-plane.md)
24. [Code-Map-Backed Knowledge Development Loop](24-code-map-backed-knowledge-development-loop.md)
25. [Code Index Retention](25-code-index-retention.md)
26. [Git Commit + Knowledge: Development Philosophy and Iteration Loop](26-git-commit-knowledge-development-loop.md)
27. [Business Knowledge to Technical Graph Mapping](27-business-knowledge-technical-mapping.md)

Chapter 23 currently has a Chinese-only
[API Reference](../../zh/03-architecture-specs/23-api-reference.md), with
an [API topic index](../../zh/03-architecture-specs/reference/README.md),
[API Codebase Views](../../zh/03-architecture-specs/reference/04-codebase-view-api.md),
and an [endpoint quick reference](../../zh/03-architecture-specs/reference/01-http-endpoints.md)
kept as supporting reference material. The topic index links the split control/Web,
code-repository, MCP Streamable HTTP, and model-configuration details.

## Contract Interpretation

Normative words such as “must,” “must not,” and “required” identify acceptance
conditions. A dated benchmark can support one condition but cannot waive an
architecture invariant. When implementation and documentation disagree, the
change must either restore the contract or update the specification and its
acceptance evidence together.

---

Navigation: [Documentation bookshelf](../README.md) | Next: [1. Architecture Vision and Algorithm Map](01-architecture-vision-and-algorithm-map.md)
