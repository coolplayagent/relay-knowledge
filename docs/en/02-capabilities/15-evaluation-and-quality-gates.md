# Evaluation and Quality Gates

[English](./15-evaluation-and-quality-gates.md) | [中文](../../zh/02-capabilities/15-evaluation-and-quality-gates.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Evaluation capability ensures foundational features and competitive capabilities work beyond demos. It covers GraphRAG fixtures, code retrieval E2E, browser integration, and documentation freshness.

## User-visible Behavior

- The Rust evaluation harness covers exact facts, multi-hop retrieval, temporal facts, negative rejection, stale indexes, ambiguous entities, and code impact.
- relay-teams and Linux code graph retrieval accuracy records stay in the verification volume.
- Browser integration tests validate Web diagnostics, GraphRAG readiness, operation composer, index tables, runtime panels, and mobile layout.

## Competitive Features

Quality gates keep retrieval accuracy, code graph structure, Web operations, and documentation links under one engineering contract, avoiding unverified features.

## Command/API Entry Points

```bash
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

## Degradation and Diagnostics

Failing tests are not fixed by enumerating known queries, paths, symbols, or fixture cases. Improvements come from general ranking signals, indexing strategy, data structures, query planning, or concurrency boundaries.

## File Watcher (fs.watch) Acceptance Criteria

The file watcher feature must satisfy:

- **Cross-platform support**: `notify` crate integration covering Linux (inotify), macOS (FSEvents), Windows (ReadDirectoryChangesW)
- **Event debounce**: Configurable debounce window (default 3s) merges high-frequency file change events
- **Content hash filtering**: `ContentHashCache` (FNV-1a) filters save operations with no content change
- **Path filtering**: Automatically ignores `.git/`, `target/`, `node_modules/` directories and binary files
- **Bounded resources**: `max_watch_dirs` caps maximum watched directories, preventing fd/inotify exhaustion
- **Graceful degradation**: Watch failures auto-degrade to `Degraded` state without affecting query hot paths
- **Diagnostic exposure**: Watcher state exposed via `service status` API (state, event counts, degradation reason)
- **Durable tasks**: Incremental index tasks enter the durable queue via `CodeIndexTaskSeed` (WorktreeOverlay mode)
- **Unit test coverage**: config parsing, path filtering, content hashing, state management, task generation, diagnostics serialization

## Related Verification Records

- [Documentation Book Refresh Audit](../06-verification/01-documentation-book-refresh-2026-05-17.md)
- [relay-teams E2E Verification](../06-verification/04-relay-teams-e2e-2026-05-14.md)
- [Linux Code Graph Retrieval Accuracy](../../zh/06-verification/06-code-graph-retrieval-accuracy-linux-2026-05-15.md)

---

Navigation: Previous: [14. Operations and Worker Capabilities](14-operations-and-worker-capabilities.md)
