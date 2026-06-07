# Engineering Hard Constraints

[English](../../en/03-architecture-specs/02-engineering-hard-constraints.md) | [中文](../../zh/03-architecture-specs/02-engineering-hard-constraints.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

This chapter is the hard contract for Book 3. Implementation, documentation, tests, release, and operations changes must satisfy it; these rules are not optional guidance and cannot be postponed as follow-up work.

Advanced architecture is earned through clear boundaries, acyclic dependencies, recoverable state, bounded resources, and verifiable behavior.

## 2. Architecture Constraints

- **Async first**: I/O, graph database access, index refresh, ingestion, and service orchestration expose async APIs.
- **No blocking hot paths**: CPU-heavy, disk-heavy, or blocking work runs behind explicit workers, maintenance tasks, or blocking boundaries.
- **Bounded resources**: event pipelines, network entry points, index refresh, and background tasks have queue depth, budgets, timeouts, cancellation, backpressure, and overload behavior.
- **Facts separated from read models**: GraphStore is the source of truth; BM25, semantic, vector, summary, community, and code indexes are derived read models.
- **Acyclic dependencies**: crates, modules, traits, services, adapters, and configuration objects do not form cycles.
- **Clear code-source directory authority**: Git-managed code repositories use the tracked tree as the indexing directory authority, so tracked source must not be skipped only because it lives under names such as `build/`, `dist/`, `vendor/`, or `third_party/`; non-Git source directories default to source/config/documentation whitelist scanning so build products, caches, and dependency copies do not enter the index unless an explicit path opts into that broad directory. A narrow non-Git path such as `src` must not opt into sibling broad directories or walk unrelated filtered siblings before selection; an unfiltered non-Git scan must not walk directories that cannot contribute to the default whitelist; `--path .` is the explicit whole-root opt-in for broad directories. Git probe failures on real Git metadata must not silently fall back to filesystem indexing, and source fallback must not read live files for a stale scoped `filesystem:` commit. Non-Git synthetic hashes must be derived from the effective indexed scope after source-layout discovery, non-Git pre-scope hashing must not read files excluded by the file preset unless an explicit path filter opts into that file, non-Git ref resolution, source fallback verification, and impact path collection must include effective path and language filters, queued synthetic refs, synchronous full-snapshot reads, and full-index or delta live-byte reads must be verified before accepting bytes, non-Git file byte/hash/metadata materialization must reject final-path and ancestor-directory symlink replacements, explicit stored `filesystem:` refs plus source fallback verification, impact collection, impact partitioning, and deleted-symbol extraction must resolve through filesystem scope identity before dynamic source-kind or Git probes, repository-set members and freshness checks with narrower filters must reuse compatible broader non-Git scopes, incremental deletion must account for previous discovered roots, explicit non-Git incremental `base_ref` values must load that stored base scope, active non-Git task matching must compare with the task's effective filters for narrower stale reads, non-Git impact paths must return no changes when scoped base/head refs match, and Git ref normalization and fresh full-index checks must not perform full tree walks.
- **Performance must generalize**: improvements come from data structures, ranking signals, indexing strategy, query planning, batching, concurrency boundaries, or storage layout, not enumerated fixture cases.

## 3. Foundational Ownership

| Module | Sole responsibility | Forbidden |
| --- | --- | --- |
| `env` | Environment loading, parsing, validation, redacted diagnostics | Direct environment reads elsewhere |
| `paths` | Platform paths and runtime/data/log/cache directories | Runtime path construction elsewhere |
| `net` | Sockets, HTTP clients/servers, listeners, network loops | Network capability creation elsewhere |
| `net::http` | HTTP over a mature async runtime/library | Blocking sockets, thread-per-connection, busy polling |
| `net::qos` | Admission control, source/tenant limits, priority, budgets, overload metrics | Resource consumption before QoS |

## 4. HTTP and QoS

HTTP is implemented over non-blocking operating-system event mechanisms, such as epoll, kqueue, or IOCP through a mature async runtime. All inbound and outbound network work passes through QoS policy before consuming resources.

Network entry points support connection budgets, request budgets, body limits, timeouts, cancellation, graceful shutdown, rate limits, queue-depth metrics, drop metrics, and overload responses.

## 5. Code Quality Constraints

- No tracked source, test, documentation, script, or workflow file may exceed 1000 lines. Generated release lockfiles required by locked builds, currently `Cargo.lock`, are exempt and must stay machine-generated.
- Do not add shallow functions; functions must validate, transform, isolate boundaries, manage resources, map errors, add observability, or coordinate real workflows.
- Do not keep dead code, TODO stubs, unused public APIs, untested speculative extension points, or commented-out implementations.
- Project identity constants live in the `project` module; module-local operational defaults stay with the owning module.
- `unsafe` is prohibited by default unless the boundary, reason, and tests are explicit.

## 5.1 File Watcher (fs.watch) Constraints

- File watching uses the `notify` crate for cross-platform support (Linux inotify, macOS FSEvents, Windows ReadDirectoryChangesW).
- Watch events must be debounced within a configurable window to prevent unbounded task generation from high-frequency file changes.
- Content hash filtering (`ContentHashCache`) must skip save operations with no actual content change.
- `max_watch_dirs` must cap the maximum watched directory count to prevent fd/inotify watch resource exhaustion.
- Watch failures must auto-degrade (Degraded state) and must not affect query hot paths or the async runtime.
- Watcher configuration must load through the `env` module environment variable override mechanism; no other module may read watcher environment variables directly.
- Watcher state and diagnostics must be exposed through the `service status` API.
- Incremental index tasks (`CodeIndexTaskSeed`) must enter the durable task queue; durable task leases, checkpoints, and bounded retry must not be skipped.

## 6. Documentation and Test Constraints

- Code, configuration, behavior, tests, workflows, benchmarks, installation, and operations changes include matching documentation refreshes.
- Unit-test and integration-test gates remain distinct.
- Rust line coverage stays above 90%, including invariants, error branches, boundaries, async cancellation, and backpressure.
- Browser integration gates install Playwright Chromium, for example `uv run --extra dev python -m playwright install --with-deps chromium`.
- Documentation changes check links, numbering, line limits, and stale state.

## 7. Acceptance Criteria

- A new module can name its ownership boundary and show why it does not create a dependency cycle.
- New background or network behavior states budgets, failure modes, cancellation, and observability metrics.
- New retrieval or performance work explains a general mechanism, not only why one example passes.

---

Navigation: Previous: [1. Architecture Vision and Algorithm Map](01-architecture-vision-and-algorithm-map.md) | Next: [3. Foundational Runtime](03-foundational-runtime.md)
