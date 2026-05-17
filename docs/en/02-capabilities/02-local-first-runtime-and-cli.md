# Local-first Runtime and CLI

[English](./02-local-first-runtime-and-cli.md) | [中文](../../zh/02-capabilities/02-local-first-runtime-and-cli.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

The local-first runtime is the starting point for every capability. Users can build, run, write evidence, query, and inspect health without deploying a database, vector service, or background system first.

## User-visible Behavior

- `relay-knowledge status` returns project identity, runtime directories, storage state, and capability overview.
- `relay-knowledge help --format json` exposes a machine-readable command contract for scripts and LLM tools.
- `relay-knowledge health --format json` and `service doctor` return unified diagnostics instead of separate hand-built status.
- The default profile uses local SQLite and deterministic semantic/vector read models.

## Competitive Features

Local-first mode is not a reduced mode. It preserves graph versions, index freshness, scopes, QoS, worker queues, and agent audit semantics, so switching from CLI to Web or service mode does not fork behavior.

## Command/API Entry Points

```bash
relay-knowledge status --format json
relay-knowledge help --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

CLI command paths, read/write effects, required parameters, defaults, allowed values, examples, and notes are part of the public contract.

## Degradation and Diagnostics

Configuration errors are explained through status, health, or doctor output. Missing external providers do not block default local queries; an uninstalled service does not block local CLI use.

## Related Architecture Chapters

- [Foundational Runtime](../03-architecture-specs/03-foundational-runtime.md)
- [Unified API and Interface Architecture](../03-architecture-specs/16-unified-api-and-interface-architecture.md)

---

Navigation: Previous: [1. Capability Overview](01-capability-overview.md) | Next: [3. Evidence and Graph Facts](03-evidence-and-graph-facts.md)
