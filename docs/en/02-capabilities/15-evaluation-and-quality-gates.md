# Evaluation and Quality Gates

[English](./15-evaluation-and-quality-gates.md) | [中文](../../zh/02-capabilities/15-evaluation-and-quality-gates.md)

> Document version: 2.2
> Date: 2026-09-01
> Scope: Book 2 capability guide

## Capability Positioning

Evaluation capability ensures foundational features and competitive capabilities work beyond demos. It covers GraphRAG fixtures, code retrieval E2E, browser integration, and documentation freshness.

## User-visible Behavior

- The Rust evaluation harness covers exact facts, multi-hop retrieval, temporal facts, negative rejection, stale indexes, ambiguous entities, and code impact.
- relay-teams and Linux code graph retrieval accuracy records stay in the verification volume.
- Browser integration tests validate Web diagnostics, GraphRAG readiness, knowledge/code graph canvases, the software ontology graph, conflicts and shape diagnostics, operation composer, index tables, runtime panels, and mobile layout.

## Competitive Features

Quality gates keep retrieval accuracy, code graph structure, Web operations, and documentation links under one engineering contract, avoiding unverified features.

## Command/API Entry Points

```bash
cargo test --all-targets --all-features
cargo test --test relay_knowledge graphrag_fixture_dataset_scores_phase4_cases
uv run --extra dev python -m playwright install --with-deps chromium
uv run --extra dev pytest tests/browser
```

## Commit and Deep Rust Gates

Issue #358 is implemented as a layered contract rather than making every Git
commit rebuild nightly-instrumented artifacts:

| Gate | Routine commit evidence | Deep/PR evidence |
| --- | --- | --- |
| Cargo check | `cargo check --all-targets --all-features` in pre-commit and PR CI | Repeated by `./check.sh --deep` before instrumentation |
| Clippy | All targets/features with warnings denied in pre-commit and PR CI | Repeated by the deep profile |
| Tests | All targets/features in pre-commit; split unit/integration jobs in PR CI | Library and binary tests execute again under AddressSanitizer |
| Miri | Not run by the stable commit hook | Nightly `domain::core::` tests with strict provenance, symbolic alignment, and deterministic concurrency |
| Sanitizer | Not run by the stable commit hook | Nightly AddressSanitizer with an instrumented standard library on Linux x86_64 CI |
| Benchmark | Included in pre-commit through `--all-targets` | Explicit deterministic benchmark jobs and `--deep` diagnostics |

The ordinary commit hook remains deterministic on the repository's stable
toolchain. Miri and AddressSanitizer require nightly, have substantial compile
or interpretation cost, and are therefore mandatory pull-request jobs plus an
explicit local deep profile:

```bash
rustup toolchain install nightly --profile minimal --component miri,rust-src
./check.sh --deep
```

Miri runs only the core domain surface because the product's SQLite and network
boundaries use FFI or host APIs that Miri does not support. This is an explicit
coverage boundary, not a skipped failure: normal tests continue to cover those
paths, while AddressSanitizer executes library and binary tests on a supported
native target. See the [Miri support and CI guidance](https://github.com/rust-lang/miri#using-miri)
and the [Rust sanitizer target and instrumentation contract](https://doc.rust-lang.org/stable/unstable-book/compiler-flags/sanitizer.html).

## Degradation and Diagnostics

Failing tests are not fixed by enumerating known queries, paths, symbols, or fixture cases. Improvements come from general ranking signals, indexing strategy, data structures, query planning, or concurrency boundaries.

## GitHub Automation Policy

The repository keeps deterministic documentation, formatting, Cargo check,
Clippy, unit, integration, benchmark, Miri, AddressSanitizer, architecture,
compatibility, coverage, build, runtime, and browser checks on pull requests.
Qodana is an optional cloud diagnostic and
is available only through manual `workflow_dispatch`; pull requests and pushes
do not start it. External service quota or availability must not become a merge
gate for product correctness.

The pull-request index-performance job compiles the release product in a
separate prerequisite step before starting the timed self-iteration workload.
The report still requires `target/release/relay-knowledge`, a passing
incremental build gate, completed cold/incremental tasks, and every declared
index latency budget. This keeps cold runner compiler variance out of the index
runtime signal without weakening compilation or product-performance checks.

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
- **Worker compatibility**: Watcher-generated payloads deserialize as `CodeIndexRequest`, and queued `WorktreeOverlay` tasks preserve the payload ref selector when workers claim them
- **Unit test coverage**: config parsing, path filtering, deterministic content-hash eviction, state management, dynamic watch/unwatch, task generation, dropped-event diagnostics, worker overlay task execution, diagnostics serialization

## Related Verification Records

- [Documentation Book Structure Audit](../06-verification/05-documentation-book-structure-audit-2026-05-17.md)
- [relay-teams E2E Verification](../06-verification/01-relay-teams-e2e-2026-05-14.md)
- [Linux Code Graph Retrieval Accuracy](../../zh/06-verification/04-code-graph-retrieval-accuracy-linux-2026-05-15.md)

---

Navigation: Previous: [14. Operations and Worker Capabilities](14-operations-and-worker-capabilities.md)
