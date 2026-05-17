# Background Service, Recovery, and Self-Healing

[English](../../en/03-architecture-specs/17-background-service-recovery-and-self-healing.md) | [中文](../../zh/03-architecture-specs/17-background-service-recovery-and-self-healing.md)

> Document version: 2.0
> Date: 2026-05-17
> Scope: Book 3 architecture and algorithm whitepaper

## 1. Design Conclusion

Background service operation is not an unmanaged CLI loop. Long-running refresh, indexing, maintenance, diagnostics, and silent updates run under platform service managers with bounded resources, persistent leases, startup reconciliation, and dead-letter recovery.

## 2. Runtime Modes

| Platform | Manager |
| --- | --- |
| Linux | systemd |
| macOS | launchd |
| Windows | Windows Service |

CLI may generate service definitions and run doctor checks, but it does not pretend to be the resident service manager.

## 3. Work Queues

Every background task has kind, scope, priority, budget, attempt, lease owner, lease expiry, target graph version, payload hash, and last error. Queue capacity is a hard ceiling; enqueue failure returns overload or retryable errors.

## 4. Reconciler

The startup reconciler:

- Replays mutation-log refresh work that was missed.
- Recovers expired leases.
- Preserves dead-letter isolation.
- Reports index lag, queue depth, stale scopes, and failed cursors.
- Corrects cursor state when graph version advances while a task is running.

## 5. Silent Updates

Silent updates are user-configurable, pausable, observable, and reversible. They refresh graph data and derived indexes only within authorized scopes and expose fresh, stale, paused, degraded, and failed states.
Resident local file indexing follows the same rule: scanners run only on
configured absolute roots, reject relative root configuration before scanning,
persist cursors and diagnostics, enforce scan/query timeout budgets, and report
truncated roots, scan errors, freshness, and lag instead of blocking query paths
or silently expanding to unapproved disks.

## 6. Acceptance Criteria

- Crashes and restarts do not lose required refresh work.
- Diagnostic paths do not automatically revive dead-letter tasks.
- CPU/IO-heavy background work does not block query hot paths.

---

Navigation: Previous: [16. Unified API and Interface Architecture](16-unified-api-and-interface-architecture.md) | Next: [18. Observability, Diagnostics, and SLO](18-observability-diagnostics-and-slo.md)
