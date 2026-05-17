# Operations and Worker Capabilities

[English](./14-operations-and-worker-capabilities.md) | [中文](../../zh/02-capabilities/14-operations-and-worker-capabilities.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 2 capability guide

## Capability Positioning

Operations capability brings background tasks, human proposals, audit, silent updates, and service definitions into the same application API. It keeps state consistent between local development and resident service mode.

## User-visible Behavior

```bash
relay-knowledge worker status --format json
relay-knowledge worker run-once --kind ocr --format json
relay-knowledge proposal list --state proposed --format json
relay-knowledge proposal accept <proposal-id> --by reviewer --reason reviewed
relay-knowledge audit query --limit 50 --format json
relay-knowledge service definition write --format json
relay-knowledge service operator pause
```

## Competitive Features

Workers can call an external HTTP worker contract or generate deterministic fallback proposals. Proposal accept/reject/supersede follows the same graph mutation path. Service-manager commands generate platform service definitions and do not perform privileged installation.

## Command/API Entry Points

Silent update operator status, pause, resume, and service definition paths appear in service doctor and Web diagnostics. Agent audit can mirror to a JSONL sink managed by `paths`.

## Degradation and Diagnostics

Worker queues are bounded; failure retries or dead-letters. Audit sink is disabled by default; when enabled, it uses a bounded asynchronous queue and cannot block agent request hot paths.

## Related Architecture Chapters

- [Background Service, Recovery, and Self-Healing](../03-architecture-specs/17-background-service-recovery-and-self-healing.md)
- [Installation, Release, and Upgrade](../03-architecture-specs/19-installation-release-and-upgrade.md)

---

Navigation: Previous: [13. Agent Access Capabilities](13-agent-access-capabilities.md) | Next: [15. Evaluation and Quality Gates](15-evaluation-and-quality-gates.md)
