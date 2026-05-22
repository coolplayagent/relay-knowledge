# Capability Overview

[English](./01-capability-overview.md) | [中文](../../zh/02-capabilities/01-capability-overview.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Book 2 describes capabilities that are implemented, understandable, and verifiable by users. It does not replace the architecture whitepaper in Book 3 and does not store research process or benchmark logs.

The foundation is local graph storage, evidence writes, query, CLI, Web, and diagnostics. Competitive capabilities come from hybrid retrieval, versioned freshness, code knowledge graphs, agent access, background recovery, multimodal evidence, and observable operations.

## User-visible Behavior

- The local SQLite profile and deterministic semantic/vector read models work without configuration.
- Users can write evidence and entity labels, query context packs, and integrate scripts through JSON output.
- Users can register Git repositories, index clean snapshots, query symbols/references/chunks, and analyze changeset impact.
- Resident service mode can expose Web, HTTP API, MCP Streamable HTTP, and local ACP session access.
- Health, service doctor, index status, repo status, and Web readiness share the same diagnostic semantics.

## Competitive Features

| Capability domain | Ordinary implementation | relay-knowledge difference |
| --- | --- | --- |
| GraphRAG | Returns text snippets only | Returns context packs with source spans, structured facts, graph paths, freshness, and ranking explanation |
| Retrieval | BM25 or vector only | BM25, semantic, vector, graph paths, code structure, and RRF fusion |
| Freshness | Opaque index state | Each cursor binds scope, graph version, backend, and stale reason |
| Code retrieval | Grep or full-text search | Git snapshots, tree-sitter, symbols, references, calls, imports, chunks, impact analysis, and bounded exact-text fallback |
| Agent access | Raw tool calls | MCP/ACP over shared services with scope policy, QoS, cancellation, and audit |
| Operations | Command pile-up | Workers, proposals, silent updates, service definitions, OTLP, and Prometheus-ready diagnostics |

## Command/API Entry Points

Common entry points include `status`, `ingest`, `query`, `repo register`, `repo index`, `repo query`, `repo impact`, `health`, `service doctor`, `service run --web --mcp streamable-http`, and the Web operation composer.

## Degradation and Diagnostics

Capabilities degrade explicitly: semantic/vector can be disabled, code parse failures become text-only, missing `ripgrep` only affects exact-text fallback, stale indexes are explainable, and unavailable external workers can leave BM25 and graph paths usable. Degradation enters response metadata instead of silently dropping capability.

## Related Architecture Chapters

- [Architecture Vision and Algorithm Map](../03-architecture-specs/01-architecture-vision-and-algorithm-map.md)
- [Hybrid Retrieval and Context Packing](../03-architecture-specs/09-hybrid-retrieval-and-context-packing.md)
- [Background Service, Recovery, and Self-Healing](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)

---

Navigation: Next: [2. Local-first Runtime and CLI](02-local-first-runtime-and-cli.md)
