# Profile Full Performance And Source Surface Notes - 2026-06-04

## Scope

This note records the profile=full performance change set that raised the
self-iteration score above the 0.95 acceptance target without weakening parser,
indexing, freshness, or source-surface requirements.

The change is intentionally general-purpose. Product code must not enumerate
repository names, fixture paths, known query text, benchmark ids, symbols, or
SDK names to satisfy these cases.

## Implemented Behavior

- SQLite schema startup now records a current schema marker after successful
  initialization and skips redundant schema work when the marker is current.
  Foreign-key enforcement is still enabled for each connection before marker
  checks, so the fast path does not bypass consistency rules.
- Code query planning can defer graph expansion for exact-path hybrid queries
  when structured or lexical evidence already covers the requested source
  surface. Dense API and workflow-like queries still keep the layered chunk
  plan where graph/context expansion is needed.
- Hybrid FTS and symbol lookup use bounded, high-signal identifier windows
  instead of broad query expansion. This keeps query-time work bounded while
  preserving recall for multi-identifier APIs, procedural surfaces, and
  workflow sequences.
- Exact-path source fallback no longer runs ripgrep over the full natural
  language hybrid query. It selects one primary source token from existing
  structured evidence and may add one supporting identity token when a canonical
  symbol exposes an incomplete aggregate/type surface. This keeps fallback
  bounded while allowing C designated initializer tables to refresh both the
  table/member source and the surrounding struct surface.
- The internal source scanner carries compound-initializer context across
  adjacent dotted fields, so a later `.read = ...` match can include the
  enclosing `[STAGE] = {` header after intermediate fields such as `.name`.
- Source fallback can preserve nested source matches inside a structured hit
  range and rank assignment-like initializer lines above lower-confidence text
  fallback when the original hybrid query terms support the match.
- C and C++ query filters treat `.h` headers as eligible C/C++ source surfaces.
  Document-like paths remain eligible for bounded unknown-language source
  fallback without treating missing external dependency source as degradation.

## Guardrails

- Source fallback remains bounded: exact-path hybrid source refresh uses at most
  two selected terms for the targeted fallback plan instead of every query term.
- Parser and dependency coverage gaps remain structured metadata problems. The
  changes do not convert missing external headers, SDK types, generated modules,
  or unauthorized cross-repository targets into `degraded_reason`.
- Large-repository performance is addressed through schema startup, query
  planning, ranking, and bounded fallback surfaces. The implementation does not
  hard-code fixture repositories, paths, query strings, benchmark ids, or known
  symbols.
- Indexing durability constraints are unchanged: task leases, checkpoint
  replay, at-most-one active writer per repository, bounded retry/backoff, and
  observable status remain required for code-index work.

## Validation

Fast C syntax fixture validation:

```sh
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=c_syntax_fixture \
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT=99 \
./self-iterate.sh evaluate --profile fast --categories competitive \
  --jobs 8 --repo-jobs 1 --query-jobs 8 --command-timeout-seconds 900
```

Result:

- Run: `manual-evaluate-1780531320632480076`
- Score: `0.995947`
- Cases: `30/30`
- C syntax fixture: `26/26`

Full performance validation:

```sh
./self-iterate.sh evaluate --profile full --categories performance \
  --jobs 16 --repo-jobs 8 --query-jobs 16 --command-timeout-seconds 900
```

Result:

- Run: `manual-evaluate-1780531379759132664`
- Score: `0.950637`
- Performance score: `0.924121`
- Cases: `290/315`
- Gates: `578/578`
- C syntax fixture: `26/26`

Additional focused unit validation:

```sh
cargo test --lib hybrid_grep_fallback_fills_after_structured_hits
cargo test --lib hybrid_exact_path_fallback_uses_leading_identity_before_member_surface
cargo test --lib reference_grep_fallback_ranks_declaration_first_for_typedef_intent
```

## 2026-08-14 Bounded Recall Follow-up

A later full-profile diagnosis found two general recall gaps behind otherwise
bounded query plans. Layered Hybrid search reused its 40-to-120 narrow-probe
cap for the final OR-FTS pass, and exact-file queries could defer contextual
queries after symbol coverage alone. The correction keeps every narrow probe
unchanged, restores the existing 300-to-900 Chunk cap only for the final broad
pass, and keeps merge/dedupe within that bound. Exact-file symbol-only return
and its targeted source refresh now require a true single-symbol identity;
multi-term queries continue through bounded chunk retrieval instead of replacing
missing context with a source fallback identity. Bash import coverage also treats
the exact escaped dot builtin `\.` as `.`, with whitespace/non-empty-operand
boundaries and the same local target resolution.

The direct synthetic regression filters are:

```sh
cargo test --lib broad_hybrid_fallback_recalls_beyond_strict_probe_cap
cargo test --lib exact_path_contextual_hybrid_query_keeps_chunk_body_evidence
cargo test --lib escaped_dot_builtin_is_a_bounded_source_import
cargo test --lib bash_escaped_dot_import_resolves_like_plain_dot_builtin
cargo test --lib hybrid_exact_path_fallback_does_not_replace_missing_context_terms
```

These fixtures exceed the strict probe cap with generated distractors, verify
body context under an exact path, and exercise the Bash token boundary. They do
not encode external repository names, paths, challenge ids, or known production
symbols. This follow-up records the algorithm and targeted gates; it does not
claim a new full-profile score until the complete evaluation is rerun.

## 2026-08-30 Partitioned Catalog Read-Path Follow-up

The fast performance profile exposed a general concurrent-startup convoy. Every
CLI query opened `PartitionedSqliteKnowledgeStore`, and catalog initialization
unconditionally entered a `BEGIN IMMEDIATE` transaction before running
idempotent DDL. Parallel read-only queries therefore serialized on the control
database writer lock.

Catalog startup now probes both catalog tables and their complete required
column sets through the existing bounded read-only connection. A current shape
returns without a write transaction; only first creation or legacy migration
uses the serialized immediate transaction. The change does not alter durable
task leases, checkpoint replay, publication fences, bounded retry/backoff, or
the at-most-one-writer-per-repository rule.

The pre-change current-candidate baseline used:

```bash
./self-iterate.sh evaluate --use-current-candidate --profile fast --categories performance
```

Baseline report `manual-evaluate-1788054780161505913-0-911521` executed all
124 selected cases and 352 gates. It rejected the unchanged candidate, with the
key `cpp_syntax_fixture_query_p95_ms` metric at 182 ms against a 180 ms budget;
the C++ p50 was 89 ms.

The same command on the post-change warm worktree produced report
`manual-evaluate-1788056804251829828-0-984166`:

- Score and performance score: `1.0`.
- Cases: `124/124`; gates: `352/352`.
- Commands: `299`; metrics: `80`; metric budget failures: `0`.
- C++ query p50/p95: `64/110 ms` (budgets `80/180 ms`).
- C query p50/p95: `75/149 ms` (budgets `80/180 ms`).
- TypeScript query p50/p95: `63/121 ms` (budgets `90/200 ms`).
- Cross-language query p50/p95: `59/64 ms` (budgets `100/220 ms`).

The first post-change run performed the initial release rebuild and is not used
as a warm latency comparison; it still completed all selected functional gates.
The recorded warm rerun is the acceptance result. The checked-in p95 budgets are
the end-to-end regression rail: the unchanged baseline failed them, while the
corrected startup path passes without fixture-specific product behavior.

Focused storage regression commands:

```bash
cargo test current_catalog_schema_revalidation_does_not_request_a_write_lock --all-features
cargo test legacy_scope_publication_columns_migrate_idempotently_and_default_active --all-features
```
