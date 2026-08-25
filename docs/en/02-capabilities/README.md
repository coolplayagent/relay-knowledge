# Capabilities

[English](README.md) | [中文](../../zh/02-capabilities/README.md)

This volume describes behavior that users can exercise in the current
`relay-knowledge` implementation. It explains what each capability does, which
surface exposes it, how it degrades, and where to find its architecture and
evaluation contracts. Forward-looking requirements belong in
[Architecture Specifications](../03-architecture-specs/README.md); dated results
belong in [Benchmark and Evaluation Records](../05-benchmarks/README.md).

## Reading Guide

- Chapters 1–7 introduce the local runtime, graph facts, context packs, hybrid
  retrieval, freshness, and multimodal evidence.
- Chapters 8–10 cover repository indexing, code-graph retrieval, impact, and
  reporting.
- Chapters 11–14 cover provider, Web, agent-access, and worker surfaces.
- Chapter 15 defines evaluation and quality-gate expectations.

## Chapter Index

1. [Capability Overview](01-capability-overview.md)
2. [Local-first Runtime and CLI](02-local-first-runtime-and-cli.md)
3. [Evidence and Graph Facts](03-evidence-and-graph-facts.md)
4. [Query and Context Pack Basics](04-query-and-context-pack-basics.md)
5. [Hybrid Retrieval Advantage](05-hybrid-retrieval-advantage.md)
6. [Freshness and Index Recovery](06-freshness-and-index-recovery.md)
7. [Multimodal Evidence Capability](07-multimodal-evidence-capability.md)
8. [Code Repository Basics](08-code-repository-basics.md)
9. [Code Graph Competitive Features](09-code-graph-competitive-features.md)
10. [Code Impact and Reporting](10-code-impact-and-reporting.md)
11. [Semantic/Vector Provider Backend](11-semantic-vector-provider-backend.md)
12. [Web Workspace Capabilities](12-web-workspace-capabilities.md)
13. [Agent Access Capabilities](13-agent-access-capabilities.md)
14. [Operations and Worker Capabilities](14-operations-and-worker-capabilities.md)
15. [Evaluation and Quality Gates](15-evaluation-and-quality-gates.md)

## Status Language

“Implemented” means the documented path exists and has a production caller.
“Degraded” means the response remains usable within the stated boundary and
reports the affected layer. Research targets and architecture requirements are
not current capability claims unless this volume links them to executable
evidence.

---

Navigation: [Documentation bookshelf](../README.md) | Next: [1. Capability Overview](01-capability-overview.md)
