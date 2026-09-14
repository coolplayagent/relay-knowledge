# Keep Schema Compatibility Probes Read-Only

## Practice

Separate an existing database's compatibility probe from its migration
transaction. When startup only needs to decide whether migration is required,
open the database read-only, inspect a bounded schema shape, and return without
requesting writer authority when that shape is current.

For the partitioned SQLite catalog, the compatibility contract is the required
column set of `storage_repository_shards` and
`storage_repository_shard_scopes`. Missing files, tables, or legacy columns
route to the serialized immediate migration transaction. Invalid or unreadable
state remains an error.

## Why it matters

Read-mostly commands often construct the full storage adapter. If construction
unconditionally performs idempotent DDL, concurrent command processes contend
on the same control-plane writer lock even though their business operation is
read-only. A read-only probe preserves migration safety while removing that
startup convoy.

## Guardrails

- Keep the probe bounded by the normal read timeout; do not hide contention
  behind an unbounded busy timeout or retry loop.
- Validate the complete required-column contract, not one convenient sentinel.
- Keep first creation and legacy repair in one serialized transaction.
- Extend the probe, migration, unit tests, and architecture contract together
  whenever the catalog schema changes.
- Retain end-to-end concurrent CLI latency metrics because a unit test alone
  cannot detect every future startup write boundary.

## Evidence

The 2026-08-30 post-change warm performance evaluation reduced the C++
fixture query p95 from 182 ms to 110 ms and passed all 124 selected cases and
352 gates. The detailed command, budgets, and report identities are recorded in
`docs/zh/05-benchmarks/10-profile-all-performance-source-surface-2026-06-04.md`.
