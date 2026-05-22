# Grep Fallback Documentation Refresh Audit 2026-05-22

[English](./07-grep-fallback-documentation-refresh-2026-05-22.md) | [中文](../../zh/06-verification/07-grep-fallback-documentation-refresh-2026-05-22.md)

This audit records the 2026-05-22 documentation refresh for code retrieval `ripgrep` exact-text fallback. The change updates documentation only and does not change Rust, Web, CLI, or test harness behavior.

## Refresh Scope

| Area | Refresh |
| --- | --- |
| User guide | The overview, CLI command reference, code repository workflow, and troubleshooting chapters now state that `repo query` runs `definition`, `references`, and `hybrid` queries through tree-sitter/FTS first, then uses bounded `ripgrep` fallback only when needed. |
| Capabilities | The capability overview, hybrid retrieval, code repository basics, and code graph competitive chapters now document `text_fallback` provenance, missing-`rg` degradation, and the boundary that fallback cannot replace resolved edges. |
| Architecture | The hybrid retrieval, Tree-sitter indexing, and code retrieval ranking chapters now state that fallback inherits scope/path/language/freshness/authorization, runs behind a blocking-worker boundary, and records candidate-file, materialized-byte, line-length, and timeout budgets. |
| Research and benchmarks | The Tree-sitter retrieval research, implementation reference, competitive research, and benchmark target chapters now include grep fallback scenarios, risks, observability fields, and regression principles. |
| Bookshelf indexes | Both documentation bookshelves now link this audit for later traceability. |

## Key Constraints

- `ripgrep` fallback only fills exact source lines from indexed commits; it does not directly scan the current dirty worktree.
- Fallback hits must include `lexical` and `text_fallback`; definition fallback may also include `definition`.
- Fallback hits do not return resolved edge confidence and must not outrank existing exact symbols or resolved edges.
- Missing `rg`, timeouts, candidate-file budget exhaustion, or materialized-byte budget exhaustion degrade only exact-text fallback and surface through `degraded_reason`.

## Verification Commands

```bash
rg -n 'ripgrep|grep 兜底|text_fallback|exact-text fallback' docs/zh docs/en README.md
git diff --check
```
