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
