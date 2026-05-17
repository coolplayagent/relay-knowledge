# Code Impact and Reporting

[English](./10-code-impact-and-reporting.md) | [中文](../../zh/02-capabilities/10-code-impact-and-reporting.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Impact analysis turns changesets into explainable risk signals. It starts from changed paths, deleted symbol names, callee identity, and import/module seeds to avoid unbounded scans over the full scope table.

## User-visible Behavior

```bash
relay-knowledge repo impact core --base main --head HEAD --format json
```

Impact output includes changed paths, affected symbols, reference/call/import signals, freshness, score, excerpts, and edge metadata. Regular `repo query` does not accept `impact` as a kind, keeping changeset results separate from query results.

## Competitive Features

Compared with ordinary diff tools, impact analysis combines the code graph and retrieval context to explain why a file or symbol may be affected. Compared with coverage reports, it can surface risk propagated through calls, references, or imports.

## Verified Findings

relay-teams E2E validated definition, reference, import, caller, and hybrid queries over a Python production source scope. Results carried resolved commit, tree hash, path, line range, retrieval layer, index version, freshness, score, and excerpt metadata.

Detailed records stay in [relay-teams E2E Verification](../06-verification/04-relay-teams-e2e-2026-05-14.md) and the Chinese [relay-teams Code Graph Retrieval Accuracy](../../zh/06-verification/05-code-graph-retrieval-accuracy-relay-teams-2026-05-15.md) record.

## Degradation and Diagnostics

Path reports distinguish changes inside and outside scope. Large indexing or reporting flows explain cost through scope preview, progress, degradation summary, and freshness state.

## Related Architecture Chapters

- [Code Retrieval Ranking and Impact Analysis](../03-architecture-specs/13-code-retrieval-ranking-and-impact-analysis.md)

---

Navigation: Previous: [9. Code Graph Competitive Features](09-code-graph-competitive-features.md) | Next: [11. Semantic/Vector Provider Backend](11-semantic-vector-provider-backend.md)
