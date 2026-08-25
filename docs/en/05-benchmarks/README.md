# Benchmark and Evaluation Records

[English](README.md) | [中文](../../zh/05-benchmarks/README.md)

This volume stores dated benchmark baselines, evaluation sets, regression
budgets, and accepted self-iteration records. A result applies only to the
documented revision, fixture, configuration, and hardware. It is not a current
performance claim unless a reproducible run confirms the same conditions.

## Record Index

1. [relay-teams Baseline 2026-05-14](01-relay-teams-baseline-2026-05-14.md)
2. [relay-teams Optimization Issues 2026-05-14](02-relay-teams-optimization-issues-2026-05-14.md)
3. [relay-teams Optimization Study 2026-05-14](03-relay-teams-optimization-study-2026-05-14.md)
4. [Self-Iteration Optimization Status Ledger](04-self-iteration-accepted-optimizations.md)
5. [Competitive and High-Performance Benchmark Targets 2026-05-17](05-competitive-performance-benchmark-targets-2026-05-17.md)
6. [C/C++ Syntax Self-Iteration Evaluation Set](06-c-cpp-syntax-self-iteration-evaluation.md)
7. [Multilingual Syntax Self-Iteration Evaluation Set](07-multilingual-syntax-self-iteration-evaluation.md)
11. [Coding-Agent E2E Evaluation Gate](11-coding-agent-e2e-evaluation.md)
12. [Elastic Long Budgets for Large Repository Indexing](12-elastic-index-budgets.md)

The following addenda are currently Chinese-only:
[8. Code-Index Fact Versioning](../../zh/05-benchmarks/08-code-index-fact-versioning.md),
[9. Foundational Code-Query Ranking](../../zh/05-benchmarks/09-code-query-ranking-foundational.md),
and [10. Profile-All Performance Source Surface](../../zh/05-benchmarks/10-profile-all-performance-source-surface-2026-06-04.md).
The dated detailed runs split from record 4 are likewise Chinese-only and remain
discoverable through the [Chinese archive index](../../zh/05-benchmarks/archive/README.md).

## Reading Results Safely

- Compare results only when the commit, fixture revision, profile, backend,
  freshness policy, and resource budget match.
- Treat a timeout, stale scope, degraded parser, skipped stage, or incomplete
  checkpoint as a failed or incomplete measurement—not as success.
- Preserve durable task leases, bounded work, single-writer publication, and
  all indexing stages when optimizing performance.
- Require a case or metric that fails when the same regression returns.

---

Navigation: [Documentation bookshelf](../README.md) | Next: [1. relay-teams Baseline](01-relay-teams-baseline-2026-05-14.md)
