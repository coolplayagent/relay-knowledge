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

- No tracked file may exceed 1000 lines.
- Do not add shallow functions; functions must validate, transform, isolate boundaries, manage resources, map errors, add observability, or coordinate real workflows.
- Do not keep dead code, TODO stubs, unused public APIs, untested speculative extension points, or commented-out implementations.
- Project identity constants live in the `project` module; module-local operational defaults stay with the owning module.
- `unsafe` is prohibited by default unless the boundary, reason, and tests are explicit.

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
