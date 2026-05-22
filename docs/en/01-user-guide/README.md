# relay-knowledge User Guide

[English](../../en/01-user-guide/README.md) | [中文](../../zh/01-user-guide/README.md)

> Document version: 1.3
> Date: 2026-05-17
> Scope: Executable user guidance for local development, CLI operations, knowledge graph retrieval, code repository graphs, the Web workspace, MCP/ACP access, resident service operation, background workers, observability, troubleshooting, and advanced configuration.

This book only covers paths that users can run, verify, and troubleshoot directly. Architecture constraints, interface boundaries, and forward-looking requirements remain in `docs/en/03-architecture-specs/`; this book turns implemented capabilities into commands, configuration, runtime state, and diagnostic steps.

## Current Capability Boundary

`relay-knowledge` is a local-first graph retrieval foundation. It is not a general-purpose agent runtime or final-answer generator. Current user-facing capabilities include:

- Zero-configuration local SQLite graph storage, platform runtime directories, and deterministic semantic/vector read models.
- Evidence ingest, three-layer GraphRAG retrieval, graph-only queries, index refresh, and freshness diagnostics.
- Code repository graphs with tree-sitter indexing, symbol and relationship retrieval, bounded `ripgrep` exact-text fallback, incremental updates, worktree overlays, impact analysis, and reports.
- Static Web workspace and same-origin operation execution for retrieval, ingest, repositories, indexes, providers, workers, proposals, audit, and service snapshots.
- MCP Streamable HTTP, local ACP adapter, QoS, scope policy, cancellation, metrics, and audit.
- Service-manager planning, service definition generation, silent-update operator state, and worker/proposal/audit operation entry points.
- `setup doctor` and `setup profile` for local readiness diagnostics and executable configuration profiles.

## Chapter Index

- Chapter 0: User Guide Overview: this page.
- [Chapter 1: Installation and Runtime Directories](01-install-and-runtime.md): build, local execution, zero-config defaults, and platform directories.
- [Chapter 2: CLI Basics](02-cli-basics.md): command syntax, output formats, freshness, and parser diagnostics.
- [Chapter 3: CLI Command Reference](03-cli-command-reference.md): command overview, status diagnostics, setup profiles, and provider probe.
- [Chapter 4: Knowledge Graph](04-knowledge-graph.md): evidence ingest, context pack query, graph inspection, multimodal evidence, and retrieval backend entry points.
- [Chapter 5: Code Repository Graph Workflow](05-code-repository-graph-workflow.md): repository registration, code graph indexing, symbol and relationship queries, `ripgrep` fallback diagnostics, incremental updates, impact analysis, and reports.
- [Chapter 6: Web Workspace](06-web-workspace.md): static assets, same-origin APIs, operation execution, browser integration tests, and safety boundaries.
- [Chapter 7: MCP Agent Access](07-mcp-agent-access.md): MCP policy, sessions, tools/resources/prompts, and access boundaries.
- [Chapter 8: ACP Local Adapter](08-acp-local-adapter.md): local ACP sessions, progress, cancellation, and context artifacts.
- [Chapter 9: Resident Service](09-resident-service.md): foreground service mode, same-port Web/API/MCP, service manager support, and the silent-update operator.
- [Chapter 10: Workers, Proposals, and Audit](10-workers-proposals-audit.md): worker endpoints, manual review, proposal lifecycle, and audit.
- [Chapter 11: Observability and Telemetry](11-observability-and-telemetry.md): Prometheus metrics, OTLP traces/metrics, and diagnostic state.
- [Chapter 12: Advanced Configuration](12-advanced-configuration.md): runtime directories, retrieval backends, network/QoS, MCP, workers, audit, and setup variables.
- [Chapter 13: Operations and Troubleshooting](13-operations-and-troubleshooting.md): health checks, index freshness, common errors, isolated reproduction, and PR verification.

## Recommended Reading Path

For a first run, complete a zero-config knowledge graph loop:

```bash
cargo build
target/debug/relay-knowledge status --format json
target/debug/relay-knowledge ingest --source docs --content "Rust async services isolate blocking SQLite work" --entity Rust --format json
target/debug/relay-knowledge query SQLite --source docs --freshness wait-until-fresh --format json
target/debug/relay-knowledge setup doctor --format json
```

Read Chapter 5 when using code repositories as retrieval sources. Read Chapter 6 for the browser workspace. Read Chapters 7 and 8 when exposing local graph retrieval to agents. Read Chapters 9 through 11 for resident services, background work, and telemetry. Use Chapters 12 and 13 when connecting external backends, changing budgets, or reproducing environment-specific problems.

## Output and Audit Conventions

The examples use `relay-knowledge` to mean a built or installed binary. If it is not installed on `PATH`, replace it with `target/debug/relay-knowledge` or `target/release/relay-knowledge`. Prefer `--format json` for scripts; use default `text` for terminal checks and `markdown` for report-oriented commands.

CLI, Web, MCP, ACP, workers, proposals, audit, and service operations all enter the core through the shared application service. During troubleshooting, preserve operation, metadata, degraded reason, freshness, audit correlation, and diagnostics fields from JSON responses.
