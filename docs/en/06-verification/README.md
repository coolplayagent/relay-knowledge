# Verification Records

[English](README.md) | [中文](../../zh/06-verification/README.md)

This volume contains dated audits and verification records. Each record states
what was checked at a particular revision; it does not certify later changes.
For current release readiness, rerun the repository's active quality gates and
record the exact commands, revision, environment, and any skipped checks.

The current entry point is
[Documentation and Self-Iteration Readiness Verification 2026-08-18](13-documentation-self-iteration-readiness-2026-08-18.md).
The 2026-06-05 documentation audit remains a historical snapshot.

## Record Index

1. [Documentation Book Refresh Audit 2026-05-17](01-documentation-book-refresh-2026-05-17.md)
2. [Documentation Refresh Audit 2026-05-17](02-documentation-refresh-audit-2026-05-17.md)
3. [Documentation Refresh Audit 2026-05-14](03-documentation-refresh-audit-2026-05-14.md)
4. [relay-teams E2E Verification 2026-05-14](04-relay-teams-e2e-2026-05-14.md)
7. [Grep Fallback Documentation Refresh Audit 2026-05-22](07-grep-fallback-documentation-refresh-2026-05-22.md)
8. [Software Global Modeling Documentation Refresh Audit 2026-05-28](08-software-global-modeling-documentation-refresh-2026-05-28.md)
9. [Software Global, CodeGraph, and Search Everything Research Audit 2026-05-31](09-software-global-codegraph-search-everything-research-2026-05-31.md)
10. [Service Deployment, Control Plane, and Data Plane Audit 2026-06-04](10-service-deployment-control-data-plane-2026-06-04.md)
11. [Documentation Release Readiness Audit 2026-06-05](11-documentation-release-readiness-2026-06-05.md)
12. [Graph Database, Knowledge Graph, and CodeGraph Research Archive 2026-06-05](12-graph-database-codegraph-deep-research-archive-2026-06-05.md)
13. [Documentation and Self-Iteration Readiness Verification 2026-08-18](13-documentation-self-iteration-readiness-2026-08-18.md)

Two retrieval-accuracy records are currently Chinese-only:
[5. relay-teams Retrieval Accuracy](../../zh/06-verification/05-code-graph-retrieval-accuracy-relay-teams-2026-05-15.md)
and [6. Linux Retrieval Accuracy](../../zh/06-verification/06-code-graph-retrieval-accuracy-linux-2026-05-15.md).

## Evidence Rules

- Record command output as evidence only after confirming that the command
  covers the stated requirement.
- Distinguish unit, integration, browser, coverage, packaging, and benchmark
  gates; one green layer does not imply the others passed.
- State missing, skipped, timed-out, stale, or environment-dependent checks
  explicitly.
- Keep historical records intact; add a new dated record when current evidence
  supersedes them.

---

Navigation: [Documentation bookshelf](../README.md) | Next: [1. Documentation Book Refresh Audit](01-documentation-book-refresh-2026-05-17.md)
