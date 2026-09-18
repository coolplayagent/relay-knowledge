# Self-Iteration Failed-Case Verification 2026-09-18

[English](18-self-iteration-failed-cases-2026-09-18.md) | [中文](../../zh/06-verification/18-self-iteration-failed-cases-2026-09-18.md)

## Scope and baseline

This change fixes four fast self-iteration cases on `main fcedd99e` after syncing
the remote. The baseline used a release product binary with `--profile fast
--jobs 8 --repo-jobs 4 --query-jobs 8` and passed 396/399 gates and 128/132
cases. The failures were `software_global_statements_keep_complete_provenance`,
`repository_map_v4_keeps_typed_cross_dimension_provenance`,
`java_class_member_callees`, and `c_syntax_callers_function_pointer_read`.
Project-alias query p95 was 353 ms against the unchanged 150 ms budget.
The machine-local baseline report is
`manual-evaluate-1789692382483866536-0-2568916.json`; it applies only to that
environment and those concurrency settings.

## Change and invariants

Snapshot materialization and SQLite durable finalization share one call-name
rule: a resolved call uses the target symbol name; an unresolved call uses the
parser member name, except when a qualified lookup name contains a shorter
receiver hint written in source. A longer field receiver or subscript expression
stays in `target_hint` and appears in query excerpts. Existing call-edge indexes
can therefore find the C function-pointer field `read` and Java member
`println` while Java unknown-type and cross-language call displays remain
source-facing. Two software-projection cases now expect the product's schema
version 9; provenance, ontology, and completeness assertions remain intact.

The first repair evaluation also exposed two evaluator-contract mismatches.
The Java callee case expected `processItem calls println`, whereas an existing
integration test requires the source expression `processItem calls
System.out.println`. The case now uses the latter and still requires first
rank, exact path, and the call-graph layer. The software dependency projection
pages by an opaque component ID that includes scope identity; the target fact's
position among ten results varies between scopes. The former top-six assertion
did not match that pagination contract. The case now checks the full ten-result
range while retaining declared, locked-version, and usage facts plus ecosystem,
evidence-path, and state assertions.

Candidate windows, queues, batches, leases, and retry budgets did not grow.
The existing paths still perform reference, call, and FTS writes, durable task
checkpoints, single-writer ownership, and publication barriers. This change
does not alter installation, configuration, service behavior, or data migration.

## Verification results

The environment was Ubuntu 24.04.4, Linux x86_64, 16 logical CPUs, and Rust
1.97.1. At verification time, local `main` and `origin/main` both pointed to
`fcedd99ef1db79f3e56ef95854b74403a759d395`. The fix was then in the
worktree; the patch digest identifies the candidate changes against that base. The
release product binary was `target/release/relay-knowledge`, SHA-256
`4afd450d9bcc91024eb671ff43f8d4bdb207f832a218a7403c32ae78503a0601`.

```bash
cargo build --release --bin relay-knowledge
./self-iterate.sh evaluate --use-current-candidate --profile fast --jobs 4 --repo-jobs 1 --query-jobs 2
```

These concurrency settings match the repository's fast benchmark CI job.
Prebuilding the release binary made the evaluator's `cargo_build_release_ms`
517/180,000 ms; this cached build time does not estimate a clean build. The
final report `manual-evaluate-1789698241433448978-0-3022855.json` has SHA-256
`5ed58b19c72e4a0a6276f856211895e7c6b1ce8578aee3a51633cb4885472456`.
The candidate patch SHA-256 is
`056c2ed30a4e1e846d12edddce726b3a65d45df8297a9ddb9e4b803b2be9c199`.
The original report and patch are in the ignored, machine-local
`.git/relay-knowledge-self-iteration/` directory; these digests and this
tracked record are the portable evidence index. Verification prose added
afterward is outside the captured candidate patch.

The final report is `would_accept`, score `0.9872394591255969`: 399/399 gates,
132/132 selected cases, 332 command contracts, 86 metrics, and 18 selected
repository workloads. The one nonzero command intentionally rejected an
invalid C++ language filter in a registration negative case with exit code 1;
it was not a failed gate. The fast profile skipped `file_fixtures`,
`agent_workflows`, and `research_judge`. All four baseline failed cases passed:
the Java callee and C function-pointer caller ranked first, and the two
software-projection cases retained full provenance and typed cross-dimension
assertions.

| Fixed metric | Measured / budget |
| --- | ---: |
| Project-alias query p95 | 56 / 150 ms |
| Nonstandard-layout query p95 | 113 / 200 ms |
| Java class-calls query p95 | 77 / 2,000 ms |
| C syntax query p95 | 126 / 180 ms |
| `relay-teams` cold index | 40,682 / 45,000 ms |

The full `cargo test --all-targets --all-features` run passed 4,502 unit tests,
171 integration tests, and one additional test, with zero failures and one
pre-existing ignored fixture, before the final empty-`target_hint` guard was
added. A focused unit test then passed for that new branch. The final code
also passed 171/171 integration tests with
`cargo test --test relay_knowledge --all-features`. It passed
`cargo check --all-targets --all-features`,
`cargo clippy --all-targets --all-features -- -D warnings`,
`cargo fmt --all -- --check`, and the 220-file Markdown documentation check.
The final-code `cargo llvm-cov --lib --bins --all-features --fail-under-lines 90`
run passed 4,502 unit tests with one pre-existing ignored fixture and achieved
**90.05443%** line coverage (156,674/173,977 lines), above the 90% gate.

This record covers the local fast profile and the named local quality gates.
Full/exhaustive evaluation, browser integration, Miri, AddressSanitizer,
cross-platform packaging, and release certification were not run. An earlier
exploratory 8/4/8-concurrency run exceeded a query p95 budget; the successful
4/1/2 run does not certify that higher-concurrency setting.

---

Navigation: [Verification records](README.md) | Previous: [17. Lossless Cold-Index Verification](17-lossless-cold-index-2026-09-17.md)
