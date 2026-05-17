# Open Agent Runtime Adapter Architecture

[English](../../en/03-architecture-specs/14-open-agent-runtime-adapter-architecture.md) | [中文](../../zh/03-architecture-specs/14-open-agent-runtime-adapter-architecture.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

`relay-knowledge` supports agents but does not implement an agent runtime. External runtimes own planning, tool calls, approvals, handoffs, long-running task state, and final LLM orchestration. This system owns knowledge facts, retrieval, audit, freshness, and context packs.

## 2. Adapter Boundary

```text
External Agent Runtime / Host
        |
        v
Protocol Adapter: MCP / ACP / future A2A / local SDK
        |
        v
Unified API
        |
        v
Application Services
```

Adapters perform protocol translation, identity injection, preflight authorization, stream/cancel mapping, and audit metadata. They do not access storage, index writers, Git, tree-sitter parsers, or embedding clients.

## 3. Runtime Independence

Domain, application, retrieval, storage, and indexing types do not contain MCP, A2A, OpenAI, LangGraph, CrewAI, AutoGen, or other runtime-specific types. Protocol objects terminate at the adapter layer and become stable API requests inside the system.

## 4. Agent Action Audit

Every agent access records runtime identity, scope, tool/action, freshness policy, QoS decision, trace id, result count, truncation, degraded state, and error class. Audit events can be persisted with redaction.

## 5. Candidate Fact Policy

Entities, relations, summaries, conflict judgments, and rewrites from agents or LLMs are proposals by default. They enter the accepted graph only through validation, approval, and the mutation contract.

## 6. Acceptance Criteria

- A new protocol adapter does not require domain fact-model changes.
- Adapters are forbidden from direct database or index-table access.
- Agent output cannot bypass proposal and mutation contracts.

---

Navigation: Previous: [13. Code Retrieval Ranking and Impact Analysis](13-code-retrieval-ranking-and-impact-analysis.md) | Next: [15. Resident Agent Graph Access Protocol](15-resident-agent-graph-access-protocol.md)
