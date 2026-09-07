# Map and Graph Storage Acceptance 2026-09-07

[English](../../en/06-verification/16-map-graph-storage-acceptance-2026-09-07.md) | [中文](../../zh/06-verification/16-map-graph-storage-acceptance-2026-09-07.md)

> Date: 2026-09-07
> Baseline: `a93406aeaa79b382d6a8357fb3ad986df472bbe5`
> Scope: Repository Map governance, software relationships, durable indexing,
> cross-dimensional provenance, storage and indexing performance.

## 1. Storage Decision

Compatibility relationships have no independent facts: file-to-topic,
dependency, SDK and configuration edges can be reconstructed from their
snapshot-bound source rows. Schema 8 removes their per-edge writes and repeated
OFFSET materialization scans. Reads apply scope, path and language filters and
the result budget to one SQL projection; the durable relationships phase stores
its exact count after streaming domain validation of every joined fact, including
losing configuration duplicates. Invalid facts cannot publish a fresh checkpoint.
Stable identity, evidence, unresolved targets and graph version
remain in the response. Duplicate configuration identities select the highest
confidence, widest end line and then smallest usage ID; different relationship
kinds remain separate.

Typed ontology statements retain their stored provenance, fact state and
reconciliation semantics. Code calls, references, imports and search indexes
remain intact. The legacy compatibility table is retained for old import and
retention cursors. Successful scope refresh reclaims its old rows; opening a
database does not bulk-delete data or compact SQLite. See the
[upgrade and rollback contract](../03-architecture-specs/19-installation-release-and-upgrade.md).

## 2. Acceptance Cases

| Requirement | Evidence |
| --- | --- |
| Both maps remain compatible | Current and published 1.1.17 CLIs validate CodeSpec and Knowledge Map v4; eight CLI governance contracts cover directory visibility, source ordering, reserved routes and retained history. |
| Large topic projections avoid duplicate writes | Owner tests count 4,101 relationships over 4,096 additional topics, return a bounded 1,000-edge window, and require unchanged SQLite write/page counters and zero stored compatibility rows. A separate 513-topic refresh crosses the former 512-row page boundary. |
| Durable recovery retains the optimization | The 12,000-file performance case resumes fenced software publication, reports 12,000 SDK relationships and requires zero stored compatibility rows before the completed checkpoint. |
| Migration preserves recovery | A failing ontology insert rolls back legacy-row cleanup; a successful refresh reclaims only its own scope. Schema initialization marks old state stale without deleting old payloads. |
| High-dimensional map indexing | Four repository-map cases cover eight authorized topics, orphan-shard exclusion, combined software slices and typed `documents`, `derived_from`, `depends_on`, `deploys`, and `runs_as` provenance. |
| Incremental map changes preserve immutable history | The CLI integration case removes a source, verifies its empty route and replacement shard, excludes retired-shard edges, checks graph endpoints and replays identical base-commit topics and relationships. |
| Business and architecture stay on one snapshot | The bootstrap integration case checks authored business mappings, software/context and both architecture/business-domain views against one immutable commit and source scope. |

The storage cases are mandatory fast self-iteration gates. Configuration
deduplication, invalid public feature-flag publication, Unicode-whitespace fields,
unresolved SDK hints, filtering before limiting, cross-scope
exclusion, invalid facts and SQL errors have focused owner tests.

## 3. Real Repository Snapshot

An isolated runtime indexed the baseline repository through one durable task
and five batches. Its completed checkpoint contains 2,471 files, 44,292 symbols,
237,775 references and 26,554 chunks. Software, business, architecture,
business-domain and context reads used the same pinned commit.

The software projection contains 2,409 topics, 12,832 entities, 15,240 typed
statements and 9,982 reported compatibility relationships. Direct read-only
storage inspection confirms zero persisted compatibility rows and all six
root-authorized map topics: architecture, benchmarks, business-knowledge, cli,
release-documentation and software-model. Statement provenance completeness is
10,000 basis points and ontology diagnostic count is zero.

This snapshot has twenty `text_only` files, including CSS, lockfiles, ignore
rules and previous map roots. Its projection therefore explicitly reports
degraded freshness despite a completed, non-stale snapshot. The authored
business glossary has no domains or terms, and there are no indexed IaC
resources; positive business/deployment evidence comes from the deterministic
fixtures. Context is bounded and reports truncation. Ordinary architecture
Markdown is not an OKF concept bundle, so it cannot serve as a successful
`repo graph` neighborhood fixture. These are explicit evidence boundaries,
not missing facts silently classified as complete.

## 4. Reproduction

```bash
cargo test --lib software_relationship_storage -- --nocapture
cargo test --lib code_index_persistence_performance_suite -- --nocapture
cargo test --test relay_knowledge knowledge_development_loop
cargo test --manifest-path tools/self_iteration/Cargo.toml
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

Storage byte measurements must use separate fresh runtime homes, the same Git
fixture and different verified baseline/candidate binaries. Require completed
checkpoints, projection schema versions 7/8, unchanged non-compatibility fact
counts and equal bounded compatibility payloads before interpreting timing.
Measure occupied pages with SQLite `dbstat`, including the legacy table's
indexes; an empty table still occupies root pages. Alternate baseline/candidate
order and measure cold indexing separately from repeated bounded queries.

## 5. Measured Storage and Indexing

The release comparison extends the generated `repository_map_graph_v4` fixture
with 256 Markdown files containing sixteen headings each and 256 Rust files
containing four functions each. Both binaries indexed commit
`c53e0fa0c0a28e63d3924a84186a44d98e9b7a19` in separate fresh runtime homes,
using local semantic/vector backends. Five alternating runs per build each
included seven relationship queries with limit 100. The table reports medians;
timings are observations for this workload, not a universal speed guarantee.

| Metric | Baseline | Schema 8 | Reduction |
| --- | ---: | ---: | ---: |
| Stored compatibility rows | 4,120 | 0 | 100% |
| Compatibility table and index bytes | 1,507,328 | 12,288 | 99.18% |
| Database allocated bytes | 35,438,592 | 33,943,552 | 4.22% |
| Cold indexing | 1,355.2 ms | 1,332.5 ms | 1.67% |
| Bounded relationship query | 72.8 ms | 70.5 ms | 3.08% |

Both builds retain 546 files, 5,860 code symbols, 2,048 references, 2,048 calls,
1,314 chunks, 4,118 topics, 8,776 ontology entities and 12,893 typed statements.
All twelve checked non-compatibility table counts and the bounded relationship
payloads match. Both report 4,120 compatibility relationships and completed,
non-stale snapshots.

Baseline binary SHA-256:
`f2b2a3fe38bc77c620ca0c1e536a80bdce607475221e2028b9234d3d97de3904`.
Candidate binary SHA-256:
`e913ac3ee26f7a1cddec2fefc6f89770fd3ca0eb099c70c57ff74a4cb3c778ed`.
The baseline source was compared byte-for-byte against all 1,825 tracked
Cargo/source files at the baseline revision. An earlier pair of identically
hashed binaries was rejected as invalid comparison evidence. A pilot run
overlapping test compilation is excluded from the reported timings.

## 6. Local Quality Gates

| Gate | Result |
| --- | --- |
| Cargo check and Clippy, all targets/features | Passed, warnings denied |
| Rust formatting and documentation checker | Passed; 216 Markdown files |
| Rust unit tests | 3,908 passed; one subprocess fixture intentionally ignored and invoked by its parent tests |
| Rust integration tests | 157 passed |
| Deterministic benchmark target | 1 passed |
| Self-iteration harness unit tests | 240 passed |
| Current/stable map compatibility and CLI contracts | Both readers compatible; 8 contracts passed |
| Release map graph matrix | 4 cases passed |
| Initial local LLVM coverage, all targets/features | 90.09% line coverage at `9d2363394b`; 90% gate passed; final review fixes also require the PR coverage gate |
| Playwright Chromium browser test | 1 passed |
| Fast/performance evaluation | `would_accept`; 392/392 gates, 139/139 cases, 327 command contracts, 86 metrics; performance and stability scores both 1.0 |

The complete evaluation used `--jobs 2 --repo-jobs 1 --query-jobs 2` and took
134,542 ms. Report:
`manual-evaluate-1788784416972397547-0-518296.json`, SHA-256
`06df94a4a92af9051198d1c8c77fa3d91193fa448ae41b9f0a9278dc035c19a8`.
Its generated map fixture passed all four cases through the harness's actual
scoring path. Evaluation mode created no commit.

Chromium was installed through Playwright and ran against the existing Linux
libraries. The `--with-deps` installation attempt required unavailable sudo
credentials; the browser itself installed and executed successfully. The
browser test uses API fixtures and does not prove SQLite storage behavior.
Miri and AddressSanitizer remain separate required PR jobs; these local results
do not claim either nightly gate or cross-platform release certification.
