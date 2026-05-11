# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust skeleton for `relay-knowledge`, a graph-database-based knowledge graph project. The root contains Cargo metadata, contributor docs, pre-commit configuration, and GitHub Actions workflow files.

Use the existing Rust layout:

- `Cargo.toml`: package manifest and Rust lint configuration.
- `src/lib.rs`: reusable knowledge graph primitives.
- `src/main.rs`: default CLI entry point.
- `tests/`: integration and smoke tests.
- `docs/specs/engineering-hard-constraints.md`: hard constraints for shallow functions, dead code, documentation completeness, foundational modules, acyclic dependencies, max file length, unit-test coverage, event-driven HTTP, QoS, and Playwright Chromium browser integration-test readiness.
- `docs/specs/installation-and-release.md`: installation, packaging, publishing, service deployment, upgrade, and uninstall requirements.
- `.github/workflows/pr-checks.yml`: CI quality gates.

Keep generated output, build products, and large temporary data out of version control.

## Build, Test, and Development Commands

- `cargo build`: compile the project.
- `cargo test --all-targets --all-features`: run unit and integration tests.
- `cargo fmt --all -- --check`: verify formatting without rewriting files.
- `cargo clippy --all-targets --all-features -- -D warnings`: run lint checks and fail on warnings.
- `cargo run`: run the default binary.
- `cargo package`: verify the crate contents that would be published to crates.io.
- `cargo publish --dry-run`: validate the center-repository publishing path without publishing.
- `./setup.sh` or `setup.bat`: install/check the Rust toolchain, set up hooks, and run quality gates.
- `pre-commit run --all-files`: run the local quality hooks.

Document required services, such as graph databases or local containers, in `README.md` and commit example configuration files.

## Architecture Constraints

- Build the project as event-driven and async-first from the beginning. New I/O, graph database access, indexing, ingestion, and service orchestration should expose async APIs.
- Do not add blocking work to async execution paths. If blocking CPU or filesystem work is unavoidable, isolate it behind explicit worker boundaries.
- Use bounded queues, backpressure, timeouts, and cancellation for event pipelines so ingestion or query spikes cannot grow without control.
- Keep graph storage, event transport, and domain logic separated behind small interfaces. Tests should be able to exercise domain behavior without a live database.
- Prefer observable workflows: important events should carry enough structured context for logging, tracing, retries, and debugging.
- Provide both CLI and Web usage modes. They must share the same core services and domain APIs so behavior does not diverge between interfaces.
- Provide three-layer retrieval from the start: keyword BM25, semantic retrieval, and vector retrieval. Retrieval indexes and answers must stay tied to the latest graph state, with explicit refresh, versioning, or invalidation when graph data changes.
- Treat installed background operation as a first-class runtime. Long-running graph refresh, indexing, maintenance, and diagnostics should be hosted by the platform service manager (systemd, Windows Service, or launchd) rather than an unmanaged CLI loop.
- Silent background updates must be user-configurable, observable, and reversible. They may refresh graph data and derived indexes only within authorized scopes, and must expose freshness, stale, paused, degraded, and failure states.
- Background pipelines must use bounded queues, resource budgets, backpressure, timeouts, cancellation, retry backoff, persistent cursors or leases, and dead-letter handling so spikes cannot consume unbounded CPU, memory, or disk.
- CPU-heavy or disk-heavy work such as embedding, OCR, large-file parsing, full index rebuilds, WAL checkpointing, and compaction must run behind explicit worker or maintenance boundaries and must not block query hot paths or async runtime executors.
- Design ingestion, indexing, and maintenance for crash recovery and hung-task recovery. Startup reconcilers should replay missed index refresh work, recover expired task leases, report index lag, and keep graph facts and derived indexes consistent by version.
- Follow `docs/specs/engineering-hard-constraints.md` as a hard architecture contract, not optional guidance.
- Do not introduce circular dependencies between crates, modules, traits, services, adapters, or configuration objects. Keep the dependency graph acyclic; when two modules need shared types or behavior, extract the contract into the lower layer or a narrowly scoped contract module.
- Provide foundational modules with strict ownership boundaries: `env` owns environment variable loading/parsing/validation, `paths` owns platform paths and runtime directories, and `net` owns all network capabilities including HTTP.
- Do not read environment variables outside `env`, do not construct runtime/config/data/log/cache paths outside `paths`, and do not create sockets, HTTP clients, HTTP servers, listeners, or network loops outside `net`, except for tightly scoped tests or bootstrap code with documented reasons.
- HTTP must live under `net::http` and be implemented through non-blocking operating-system event mechanisms such as epoll, kqueue, or IOCP through a mature async runtime or HTTP library. Do not implement HTTP with blocking sockets, one-thread-per-connection designs, busy polling, or unmanaged background loops.
- Network and HTTP entry points must support high-concurrency operation with bounded memory, connection budgets, request budgets, timeouts, cancellation, backpressure, graceful shutdown, and observability for connection counts, queue depth, drops, rate limits, and timeouts.
- Provide a `net::qos` module for admission control, per-source or per-tenant limits, priorities, resource budgets, overload behavior, and QoS metrics. All inbound and outbound network work must pass through QoS policy before consuming unbounded resources.

## Release & Installation Constraints

- Treat installation and deployment as first-class product surfaces. New user-facing capabilities must consider install, upgrade, rollback, uninstall, service operation, and diagnostics.
- Stable releases must prioritize convenient user installation: publish prebuilt cross-platform binaries, checksums, and release notes through GitHub Releases, and keep `cargo install relay-knowledge` working through crates.io.
- Installation paths must be short, versioned, verifiable, and reversible. Installers should support explicit version selection, install directory selection, service installation, dry-run, and safe rollback after partial failure.
- Keep binary installation separate from runtime state. Configuration, graph databases, indexes, logs, caches, temporary files, and dead-letter data must use documented platform directories, not the repository, current working directory, or release extraction directory unless the user explicitly configures that.
- Service installation must use the platform service manager: systemd on Linux, Windows Service on Windows, and launchd on macOS. Do not implement long-running background operation as an unmanaged CLI loop.
- Package manager manifests such as Homebrew, Scoop, winget, or distro packages should reference artifacts produced from the same release tag rather than rebuilding divergent snapshots.
- Any change that affects packaging, release artifacts, service templates, data directories, configuration, migration, upgrade, or uninstall behavior must update `docs/specs/installation-and-release.md` and any affected README or release-note guidance.

## Coding Style & Naming Conventions

Use idiomatic Rust conventions: four-space indentation, `snake_case` for functions/modules, `PascalCase` for types and traits, and `SCREAMING_SNAKE_CASE` for constants. Keep `unsafe` out of the codebase unless explicitly justified. Run `cargo fmt` before committing Rust code.

Configuration and documentation files should use descriptive names, for example `docs/graph-schema.md` or `examples/load_dataset.rs`.

No tracked file may exceed 1000 lines. Split Rust modules, tests, docs, scripts, and workflow files by responsibility before they cross that limit; do not use dense formatting or unrelated aggregation to bypass the cap.

Do not add shallow functions. A function must enforce an invariant, perform meaningful validation/transformation, isolate an external boundary, manage resource lifecycle, map errors, add observability, or coordinate a real workflow. Prefer constants, typed config, or direct calls over pass-through wrappers that only rename another call.

Do not add or keep dead code. Remove unused modules, functions, types, fields, feature flags, fixtures, commented-out implementations, TODO stubs, and speculative extension points. New public APIs need a production caller or a documented spec-backed extension point with tests. Do not hide dead code with `#[allow(dead_code)]` or similar attributes except for generated/platform/protocol cases with an explicit removal condition.

Documentation completeness is mandatory. Any change that adds or changes public modules, configuration, environment variables, paths, network behavior, HTTP behavior, QoS policy, failure modes, install/runtime directories, diagnostics, or service operation must update the matching docs and README guidance.

## Testing Guidelines

Place unit tests next to the code they exercise using `#[cfg(test)]`. Put cross-module tests in `tests/`. Name tests after observable behavior, such as `creates_node_when_entity_is_new`.

For graph/database behavior, prefer deterministic fixtures and isolate tests from developer-local state. If tests require an external service, provide setup instructions and sensible defaults.

Tests for foundational modules must cover environment parsing errors, platform path resolution, network timeout/backpressure behavior, QoS admission/limit decisions, and HTTP cancellation or graceful shutdown where applicable.

Unit-test line coverage must stay above 90%. Use `cargo llvm-cov` or an equivalent auditable coverage gate for Rust, and add focused unit tests for invariants, error branches, boundary values, and async cancellation/backpressure behavior instead of relying on broad integration tests.

Testing must be layered for the whole project: keep a distinct UT gate and a distinct integration-test gate. The integration-test gate must mirror the `relay-teams` browser pattern by installing Playwright Chromium, for example `uv run --extra dev python -m playwright install --with-deps chromium`, before browser integration tests and failing CI when browser setup or integration tests fail.

## Commit & Pull Request Guidelines

Use short, imperative commit subjects, for example `Add graph schema loader` or `Document local database setup`.

Pull requests should include a summary, reason for the change, test results, and configuration or migration notes. Link related issues when available. Include screenshots only for UI or documentation rendering changes.

## Security & Configuration Tips

Do not commit secrets, database credentials, private datasets, or local environment files. Commit safe templates such as `.env.example` when configuration is required. Prefer explicit environment variables and document them in `README.md`.
