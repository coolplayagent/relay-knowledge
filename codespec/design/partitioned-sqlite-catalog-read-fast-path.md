# Partitioned SQLite Catalog Startup Read Fast Path

## Decision

Opening `PartitionedSqliteKnowledgeStore` must not request the control database
writer lock when the catalog already has every required column. The startup
path validates the two catalog tables through a read-only connection and
returns immediately when their required-column contract is current. Only a
missing database, missing table, or legacy column set enters the existing
`BEGIN IMMEDIATE` migration transaction.

## Invariants

- The read probe covers every required column in
  `storage_repository_shards` and `storage_repository_shard_scopes`.
- A current catalog executes no DDL, `ALTER TABLE`, or write transaction during
  catalog revalidation.
- A missing or legacy catalog still serializes creation and migration with one
  immediate transaction, and migration remains idempotent.
- Corrupt or unreadable catalog state fails closed; it is not treated as an
  empty database.
- The read probe uses the catalog's bounded read busy timeout. It does not add
  retry loops or weaken publication fences, task leases, or shard writer
  authority.
- Any future catalog migration must extend the required-column contract and
  its migration tests in the same change.

## Concurrency rationale

Every CLI process opens the partitioned store before repository queries. An
unconditional immediate schema transaction therefore serialized otherwise
read-only concurrent queries on the control database. Separating the
compatibility probe from the migration boundary removes that avoidable writer
lock while preserving the existing migration path.

## Verification contract

`current_catalog_schema_revalidation_does_not_request_a_write_lock` holds an
independent immediate writer transaction and requires current-schema
revalidation to succeed concurrently. The legacy migration test continues to
cover missing publication columns and a second idempotent initialization.

The fast performance self-iteration rail must also remain within the C, C++,
TypeScript, and cross-language query p95 budgets. This end-to-end guard detects
a return of serialized CLI startup even when individual catalog operations
remain functionally correct.
