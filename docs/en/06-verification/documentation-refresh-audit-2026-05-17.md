# Documentation Refresh Audit 2026-05-17

[English](./documentation-refresh-audit-2026-05-17.md) | [中文](../../zh/06-verification/documentation-refresh-audit-2026-05-17.md)

This audit records the 2026-05-17 documentation pass for the code repository
retrieval self-iteration commits. The related implementation changes focus on
SQLite code retrieval candidate selection, ranking, call excerpt query plans,
set-based call FTS finalization, and HTTP graceful-shutdown test stability.

## Refreshed In This Pass

| Area | Refresh |
| --- | --- |
| README | Added the current code-repository FTS candidate window, path-filter pushdown, BM25 ordering, identifier-aware scoring, call excerpt line containment, and declaration-chunk ranking summary. |
| Code repository capability docs | Documented symbol/reference/call/import/chunk FTS behavior, fuzzy symbol OR recall, graph-edge all-term recall, declaration chunk bonuses, the call chunk lookup index, and set-based call FTS finalization. |
| Code repository architecture spec | Added lexical candidate-window constraints: effective path filters must apply before the FTS LIMIT, graph edges must not loosen to OR recall without a contract, and ranking bonuses may only affect recalled candidates. |
| Self-iteration benchmark notes | Preserved the 2026-05-16 accepted-candidate records, scores, case changes, latency metrics, known degradations, and optimization notes as long-term memory for later Codex iterations. |
| HTTP test notes | No user-facing or operator-facing behavior changed; the update only stabilizes the synchronization boundary in `serve_router_enforces_graceful_shutdown_timeout`. |

## Synced Implementation Behavior

- FTS candidate windows apply both indexed-scope path filters and request path
  filters, then use BM25 ordering before bounded Rust scoring.
- Symbol queries can use OR-term fuzzy recall. Reference, call, and import
  queries keep narrower recall so graph-edge fan-out is not broadened.
- Scoring supports snake_case/CamelCase identifier parts, multi-part symbol
  names, directional call context, related callee names, declaration-shaped
  prototype chunks, and pure-virtual interface chunks through limited bonuses.
- Call excerpt queries use the
  `code_repository_chunks(source_scope, symbol_snapshot_id)` index and call-line
  containment to select the actual call-site chunk.
- Call FTS documents are rebuilt from `code_repository_calls` with set-based SQL
  after finalization, and schema backfill uses the same content fields.

## Verification Commands

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

For PR delivery, also wait for GitHub Actions `pr-checks` and handle
Codex/GitHub review comments.
