# Research Overview

[English](../../en/04-research/README.md) | [中文](../../zh/04-research/README.md)

> Document version: 1.0
> Prepared: 2026-05-17
> Scope: research sources, research goals, competitive positioning, scenario fit, and forward roadmap.

`04-research` is the forward-looking decision layer for `relay-knowledge`, not a passive archive. Each research note should make four things explicit: why the sources are credible, which product scenario the research serves, what should be adopted or avoided, and which capabilities can become durable competitive strengths.

## Research Principles

- **Clear source hierarchy**: official specifications and product docs define protocol and ecosystem facts; papers expose algorithmic boundaries and long-term trends; open-source projects validate engineering feasibility; internal benchmarks measure the actual relay-knowledge gap.
- **Future-oriented goals**: research should explain current implementation choices and anticipate pressure from GraphRAG, agent protocols, code graphs, multimodal evidence, resident services, and evaluation systems.
- **Selective adoption**: borrow mature product abstractions, algorithm combinations, and operational patterns without copying designs that weaken local-first operation, auditability, version consistency, or authorization boundaries.
- **Scenario-driven conclusions**: every conclusion should map to onboarding, repository understanding, agent retrieval, service operation, index recovery, or quality evaluation.

## Source Layers

| Source layer | Typical sources | Purpose | Adoption rule |
| --- | --- | --- | --- |
| Official specifications and product docs | Microsoft GraphRAG, MCP, A2A, OpenAI File Search, Neo4j GraphRAG | Confirm ecosystem interfaces, protocol constraints, and UX defaults | Treat as primary facts rather than replacing them with commentary |
| Papers and surveys | KG construction, KG refinement, RAG, GraphRAG, KGE, HybridRAG | Identify algorithm boundaries, quality risks, and long-term direction | Convert only into testable architecture principles |
| Search, database, and systems engineering | Lucene/BM25, Vespa/OpenSearch, Everything, plocate, Zoekt, RocksDB, Zanzibar, TAO, SRE | Validate high-performance indexing, permission filtering, background recovery, overload protection, and observability practices | Adopt general mechanisms without taking external desktop-search or cloud-service dependencies |
| Open-source implementations and engineering cases | ai-knowledge-graph, Tree-sitter, Codebase-Memory, GitHub code navigation | Validate pipeline, parsing, indexing, and agent-access feasibility | Adopt capability semantics, not script boundaries |
| Project-internal material | architecture specs, capability docs, relay-teams benchmarks, self-iteration records | Compare current implementation with gaps and priorities | Use as implementation constraints and acceptance baselines |

Core primary-source entry points:

- Microsoft GraphRAG query engine: <https://microsoft.github.io/graphrag/query/overview/>
- Microsoft Research DRIFT Search: <https://www.microsoft.com/en-us/research/blog/introducing-drift-search-combining-global-and-local-search-methods-to-improve-quality-and-efficiency/>
- MCP Streamable HTTP transport specification: <https://modelcontextprotocol.io/specification/2025-11-25/basic/transports>
- A2A protocol specification: <https://a2a-protocol.org/dev/specification/>
- OpenAI File Search: <https://platform.openai.com/docs/guides/tools-file-search/>
- Neo4j GraphRAG: <https://neo4j.com/labs/genai-ecosystem/graphrag/>
- Everything indexes: <https://www.voidtools.com/support/everything/indexes>
- plocate: <https://plocate.sesse.net/>
- Sourcegraph/Zoekt: <https://github.com/sourcegraph/zoekt>

## Competitive Focus

- **Local-first and governable**: default operation stays within authorized user scopes, with diagnosable configuration, indexes, logs, and service states.
- **Graph versions and index freshness**: graph facts, BM25, semantic indexes, vector indexes, and community summaries must report their version alignment before they enter answers.
- **Three-layer retrieval and explainable context packs**: keyword, semantic, and vector retrieval each cover different failure modes, then graph paths, evidence, and ranking explanations organize the final context.
- **Code knowledge graph**: Git snapshots, Tree-sitter, symbol/reference graphs, and impact analysis move repository understanding beyond text search.
- **Fast local file retrieval**: Separate filename/path, metadata, content, and change-cursor read models connect desktop file location, document content retrieval, and graph context while preserving explainable authorization and freshness.
- **Agent interoperability**: MCP and ACP are access layers, while the core service keeps stable APIs and authorization boundaries; A2A can become a later gateway.

## Chapter Guide

- [Chapter 1: Industry Capability Snapshot 2026](01-industry-capability-snapshot-2026.md): extracts product gaps and forward direction from industry signals.
- [Chapter 2: Knowledge Graph Research Summary](02-knowledge-graph-research.md): turns papers and engineering research into graph-model, indexing, and quality principles.
- [Chapter 3: arXiv Knowledge Graph Paper Insights](03-arxiv-knowledge-graph-paper-insights.md): converts frontier research into the relay-knowledge algorithm radar.
- [Chapter 4: ai-knowledge-graph Reference Analysis](04-ai-knowledge-graph-reference-analysis.md): selectively adopts productizable lessons from an open-source pipeline.
- [Chapter 5: Code Repository Tree-sitter Retrieval Research](05-code-repository-tree-sitter-retrieval-research.md): defines the engineering route for code graphs, incremental indexing, and hybrid retrieval.
- [Chapter 6: Agent Protocol Graph Retrieval Research](06-agent-protocol-graph-retrieval-research.md): plans graph-retrieval interoperability across MCP, ACP, and future A2A gateways.
- [Chapter 7: relay-knowledge Implementation Reference](07-relay-knowledge-implementation-reference.md): turns research conclusions into implementation priorities and gap closure.
- [Chapter 8: Competitive, High-Performance, and Local File Retrieval Research 2026](08-competitive-performance-research-2026.md): extracts optimization guidance from GraphRAG, hybrid search, vector indexing, code search, local file retrieval, graph storage, and SRE practice.
- [Chapter 9: GitNexus Feature and UI Implementation Research 2026](09-gitnexus-reference-analysis-2026.md): analyzes GitNexus CLI/MCP/HTTP backend, code graph, Web graph UI, agent workflows, and future improvement points.
- [Chapter 10: Software Global Domain Modeling Research 2026](10-software-global-domain-modeling-research-2026.md): extracts a global modeling route from software engineering KGs, code KGs, SBOMs, SDK/dependency versions, dynamic graphs, and code generation research.
- [Chapter 11: Software Global Modeling, CodeGraph, and Search Everything Comparison 2026](11-software-global-codegraph-search-everything-comparison-2026.md): compares 2026 open-source systems and papers across global modeling, code graphs, deterministic Code RAG, and search-everything retrieval.
