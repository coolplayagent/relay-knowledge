# relay-knowledge self-iteration

[中文](README.zh-CN.md) | English

This directory contains an independent Codex-driven optimization loop for code repository retrieval quality and graph semantic/vector retrieval quality. It now evolves as a standalone Rust harness under `tools/self_iteration`, outside the product crate `src/` tree, and stores runtime state under `.git/relay-knowledge-self-iteration/`. The old tracked Python harness has been removed after feature parity checks; `self-iterate.sh` builds and runs the Rust binary directly.

## Start

From the repository root:

```bash
./self-iterate.sh
```

The launcher defaults to:

```bash
cargo build --manifest-path tools/self_iteration/Cargo.toml --bin relay-knowledge-self-iterate
tools/self_iteration/target/debug/relay-knowledge-self-iterate loop --workspace . --yolo --profile fast
```

`self-iterate.sh` remains the stable entrypoint. It builds the standalone Rust harness in debug mode by default so local iterations do not begin with a release build. Set `RELAY_KNOWLEDGE_SELF_ITERATION_RELEASE=1` when the harness itself should run from `target/release`.

Useful variants:

```bash
./self-iterate.sh once
./self-iterate.sh --max-iterations 3
./self-iterate.sh chart
./self-iterate.sh once --profile full
./self-iterate.sh once --profile fast --categories semantic_vector
./self-iterate.sh once --profile fast --categories semantic_vector,competitive
./self-iterate.sh once --profile smoke --dry-run-codex
./self-iterate.sh loop --strategy unattended-layered
./self-iterate.sh loop --strategy unattended-layered --max-wall-clock-hours 48 --stop-after-accepted 12
```

## Progress logs

The harness writes live progress to stderr with the `[self-iterate]` prefix. Each
subprocess reports `command start`, a 15-second `command running` heartbeat, and
`command done` or `command timeout` with exit status and duration. Evaluation
also reports the selected profile, evaluation home, resolved parallelism, quality
gate stages, repository workload size, repository-set workload size, and final
gate/case/command counts. This keeps long `fast` runs observable even though
product command stdout and stderr are still captured for the JSON report.

## YOLO mode

The local Codex CLI does not expose a literal `--yolo` flag. This framework maps `--yolo` to the current non-interactive high-permission Codex invocation:

```bash
codex -a never exec --dangerously-bypass-approvals-and-sandbox -s danger-full-access -C /opt/workspace/relay-knowledge -m gpt-5.5 -c 'model_reasoning_effort="xhigh"' -
```

Use it only in an externally trusted workspace. The loop is designed to run unattended.

Self-iteration generation defaults to `gpt-5.5` with Codex
`model_reasoning_effort="xhigh"` so unattended candidates use the strongest
local reasoning profile by default. Override the model with `--model` and the
reasoning effort with `--codex-reasoning-effort low|medium|high|xhigh` when a
run intentionally needs a cheaper or different generation mode.

## Loop behavior

Each iteration:

1. Verifies the worktree is clean unless `--use-current-candidate` is passed.
2. Prompts local Codex to make one focused code retrieval improvement.
3. Saves the candidate patch from the iteration start commit under `.git/relay-knowledge-self-iteration/patches-v2/`.
4. Runs profile-specific gates and evaluation. The default `fast` profile runs formatting checks, a product debug build, harness `cargo check`, an expanded normal-repository subset, repository-set guards, and a semantic/vector guardrail query. `full` and `exhaustive` restore both release builds, product `clippy -> test` and harness `clippy -> test` rails, plus the full repository evaluation, repository-set cases, local-file fixtures, semantic/vector fixtures, and research judge.
5. Records a report under `.git/relay-knowledge-self-iteration/reports-v2/`.
6. Appends scoring history to `.git/relay-knowledge-self-iteration/runs-v2.jsonl`.
7. Writes charts to `.git/relay-knowledge-self-iteration/score-v2.csv` and `.git/relay-knowledge-self-iteration/score-v2.svg`; `accepted` means a git commit was created.
8. Appends the accepted optimization approach, changed files, metric improvements, and known degradations to `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md` before committing.
9. Commits the candidate net change and accepted-optimization record as one squash commit only when the previous-run improvement policy accepts it.
10. Restores the iteration start commit when the candidate is rejected.

If the worktree is dirty at startup, the loop exits immediately instead of
retrying the same non-retryable precondition failure.

Implementation candidates must update
`docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md` with the
algorithm, architecture, invariants, expected case/metric impact, and known
risks before evaluation. The harness adds a
`self_iteration_algorithm_documentation` gate to reject code, test, benchmark,
or harness-policy changes that do not carry those notes. The prompt uses the
v2 run history, a synthesized history digest, and patch paths as bounded context
when reasoning about the next candidate.

The v2 Rust harness keeps `runs-v2.jsonl`, `reports-v2/`, and `patches-v2/`
separate from earlier run/report/patch formats, which may remain in existing
worktrees as historical artifacts. Progressive long-term memory is preserved under the shared
`.git/relay-knowledge-self-iteration/memory/` tree: each scored run writes
`memory/index.jsonl`, `memory/summaries/`, and `memory/details/`, and the next
generation prompt receives a rejection-recovery memory review, a bounded memory
index, a synthesized profile-specific history digest, and a bounded historical
patch index. Rejected memories include changed paths, score deltas, local
improvements, degradations, and repeated rejection clusters so Codex can avoid
retrying small local edits that already failed the acceptance baseline. Codex
should open the referenced summary, detail, or patch files only when they match
the current gate, metric, case, path, or algorithm objective. The direct history
synthesis has a hard prompt budget cap, so long-running iteration does not
expand linearly into the LLM context.

The default profile is `fast`. It runs product and harness `fmt --check`, then a
product debug build plus harness `cargo check`, and evaluates with
`target/debug/relay-knowledge`. It does not run the product release build, full
clippy, full test suite, local-file fixtures, or research judge by default.
`fast` evaluates `c_syntax_fixture`, `cpp_syntax_fixture`,
`typescript_syntax_fixture`, `relay_teams`, `leveldb_cpp`,
`temporal_samples_go`, and `temporal_sdk_go`, takes the first 8 normal query
cases per repository while always preserving explicit guardrail cases, keeps 2
cross-repository threshold cases from the `temporal_go_workspace` repo-set, and
runs the semantic/vector guardrail query. The TypeScript fixture keeps the
external-import grep fallback guardrail in the default fast loop. The C fixture
also includes explicit grep/text-fallback cases early in its fast case window,
so exact source-text recovery stays covered without indexing another large
repository or making missing `rg` a hard quality-gate failure. It reuses
`.git/relay-knowledge-self-iteration/cache-v2/fast-evaluation-home/` to reduce
repeated registration and indexing cost. Score history is isolated by profile
and category focus, so `fast --categories semantic_vector` compares only
against matching semantic/vector-focused fast runs and does not treat
full/exhaustive judge scores as fast regressions. Acceptance also checks the
best accepted run for the same profile across category focuses, so a first run
for a new category cannot be committed below the established profile-level bar.
Override the subset with
`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=c_syntax_fixture,cpp_syntax_fixture,typescript_syntax_fixture,relay_teams,leveldb_cpp,temporal_samples_go,temporal_sdk_go`,
`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT=12`,
`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS=temporal_go_workspace`, and
`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT=2`. Pass
`--profile full` for the previous full gates and workload; long-running
large-repository checks still use `--profile exhaustive`.

Use `--categories` to focus an iteration on a specific score family without
dropping bottom-line protection. Supported categories are `foundational`,
`competitive`, `semantic_vector`, `file_fixtures`, `repository_sets`,
`research_judge`, `performance`, and `all`. A focused run evaluates explicit
`guardrail=true` cases plus the selected category cases; guardrail case failures
are emitted as quality gates and reject the candidate even when the focused
score improves. For example, `--categories semantic_vector` runs the full
semantic/vector suite plus repository and repo-set guardrails, while
`--categories competitive` runs competitive repository cases plus the same
guardrails. `--categories performance` keeps the performance-bearing repository,
repo-set, semantic/vector, and file-fixture measurement workloads instead of
collapsing to guardrails only.

Concurrency defaults to `--jobs auto`, `--repo-jobs auto`, and `--query-jobs
auto`. `auto` uses the local machine aggressively: the global command limiter
and query pool default to the available CPU count, and repository jobs default
to half of that count. Repository register/index and repository-set
create/add/refresh writer commands are still serialized against the shared
evaluation store; query subprocesses run concurrently after writer boundaries.
Set `--jobs N` or `RELAY_KNOWLEDGE_SELF_ITERATION_JOBS=N` to override the global
limit.

The prompt includes bounded run history, a direct synthesis of accepted and
rejected patterns, progressive memory, and patch indexes so repeated rejections
stay tied to recent evidence instead of retrying the same shape of patch. It
also tells Codex to prefer `rg` for repository inspection and to fall back to
bounded `grep -RIn` searches that exclude VCS and build directories when `rg`
is not installed on the local machine.

## Unattended layered strategy

`--strategy unattended-layered` is the default long-run mode for 1-2 day
unattended sessions. It keeps the normal single-iteration behavior untouched
unless the strategy is explicitly selected.

Defaults are tuned for a 36-hour run: `--max-wall-clock-hours 36`,
`--stop-after-accepted 8`, `--explore-timeout-seconds 900`,
`--macro-explore-timeout-seconds 2700`,
`--max-explore-attempts-per-cycle 3`,
`--max-consecutive-empty-candidates 8`,
`--max-consecutive-promotion-failures 10`,
`--macro-after-competitive-failures 4`,
`--macro-after-empty-candidates 6`, `--cycle-sleep-seconds 120`,
`--cooldown-after-accept-seconds 300`, and
`--cooldown-after-timeout-seconds 900`.

Each cycle runs short `smoke` explore attempts over the rotation
`competitive -> semantic_vector -> performance -> repository_sets`. Codex runs
only in the explore layer. A candidate that passes the smoke screen is validated
with `fast` under the same category and only then reaches the existing
accept/commit path. The generated patch is reused across screen and validation
so the same candidate is not regenerated.

When short attempts stall, the strategy escalates to `macro_explore` for
competitive capability. Macro escalation triggers after repeated competitive
promotion failures, repeated empty candidates, or a competitive-capability gap
against the best accepted focused baseline. The macro prompt includes
`research_judge_suite.competitive_feature_targets` and
`implementation_guardrails` from `cases.json`, asks for a larger ranking,
indexing, relationship extraction, query-planning, context-construction, or
retrieval-evidence improvement, and still forbids fixture-specific
enumeration.

Run state is persisted in
`.git/relay-knowledge-self-iteration/unattended-state-v2.json` so a crashed
session can resume its category rotation, failure counters, accepted count, and
deep-check schedule. Layered history records add `strategy`, `layer`,
`parent_run_id`, `promoted_from_run_id`, `macro_trigger`,
`promotion_decision`, and wall-clock fields. Passing `--use-current-candidate`
skips Codex and starts the layered path from the current diff's smoke screen and
fast validation.

## Scoring and acceptance

When the research judge is disabled or skipped, the score is:

```text
foundational_capability * 0.22
+ competitive_capability * 0.22
+ semantic_vector * 0.13
+ performance * 0.18
+ stability * 0.25
```

When the research judge is enabled, `research_judge` becomes a protected
objective and the weights switch to:

```text
foundational_capability * 0.17
+ competitive_capability * 0.17
+ semantic_vector * 0.10
+ research_judge * 0.22
+ performance * 0.15
+ stability * 0.19
```

This policy intentionally gives research quality and performance more influence
than earlier self-iteration runs while keeping the other objectives protected by
regression checks.

The research judge evaluates research alignment, architecture soundness,
reliability reasoning, performance generalization, implementation
actionability, and fixture-special-casing risk. It can run through an
OpenAI-compatible HTTP endpoint or through an open coding-agent CLI such as
`opencode`, `relay-teams`, `codex`, `cc`, or `copilot`. When no judge backend or
HTTP settings are provided, the CLI judge defaults to `opencode`. All judge
overrides come from runtime environment variables:

`cases.json` can also configure the judge workload. `documents` selects bounded
02/03/04 excerpts, `competitive_feature_targets` lists the research-derived
capabilities candidates should advance, and `implementation_guardrails` lists
non-negotiable constraints such as anti-fixture behavior, async boundaries,
freshness/version evidence, and same-change documentation updates.

- `RELAY_KNOWLEDGE_JUDGE_BACKEND=http|cli|opencode|none`; `opencode` is a
  CLI alias that uses the default opencode command unless a custom command is
  also set
- HTTP: `RELAY_KNOWLEDGE_JUDGE_BASE_URL`, `RELAY_KNOWLEDGE_JUDGE_API_KEY`,
  `RELAY_KNOWLEDGE_JUDGE_MODEL`; the standalone harness posts the request with
  `curl`, and the API key is read from the environment rather than persisted in
  reports
- CLI: `RELAY_KNOWLEDGE_JUDGE_COMMAND`, with aliases
  `RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND` and
  `RELAY_KNOWLEDGE_JUDGE_CLI_COMMAND`; when unset, the default is
  `opencode run "Read the attached relay-knowledge judge prompt and return only the strict JSON object it requests." --file {prompt_file}`
- Shared timeout: `RELAY_KNOWLEDGE_JUDGE_TIMEOUT_SECONDS`

Custom CLI commands receive the judge prompt on stdin by default. Command
templates may also use `{workspace}`, `{prompt_file}`, or `{prompt}`
placeholders. The harness requires either HTTP or CLI judges to return strict
JSON. Set `RELAY_KNOWLEDGE_JUDGE_BACKEND=none` to record `judge_skipped`; `off`,
`disabled`, `skip`, and `false` are accepted as disable aliases. Explicit
misconfiguration, malformed JSON, low confidence, low overall score, or low
anti-fixture-special-casing score rejects the candidate.

Case objectives are continuous quality scores, not pass-rate counters. A passed case
at rank 1 starts from `1.0`; a passed case at rank `N > 1` starts from
`1.0 / N` even when `N` is within the case's `max_rank` acceptance threshold.
Cases may also declare `expected_all`, `expected_sequence`, `min_score`,
`require_expected_all`, `require_expected_sequence`,
`forbidden_rank_penalty`, and `forbidden_rank_penalty_only`. These fields let a
case pass while still scoring below `1.0` when it finds only part of a
relationship set, misses execution-flow steps, or ranks a forbidden result too
high. Empty negative cases that pass with `rank=0` still score `1.0`. Missing
foundational, competitive, or semantic/vector objectives default to `0.0`
instead of silently appearing complete, and `accuracy` averages only the
foundational and competitive objectives that are actually present. Metric
budget misses are reported in `metric_budget_failures`.

`performance` uses `budget_relative_v2`. If no compatible previous run exists,
metrics use their budget-normalized score. Once the previous run also used this
strategy, each metric blends budget fit with relative progress against the
previous value, so a latency metric that is merely under budget no longer stays
at `1.0`; real improvements keep producing bounded scoring signal while normal
metric noise is still filtered by the epsilon policy.

`accuracy` is retained as a compatibility roll-up of foundational and competitive case scores. Acceptance uses an `epsilon-Pareto acceptance with hard constraints and weighted-score tie-breaker` policy. In multi-objective optimization terms, build/test gates and candidate diff existence are hard constraints, foundational_capability, competitive_capability, semantic_vector, and stability are protected objectives for basic usability, advanced retrieval quality, semantic/vector source coverage, backend availability, and latency observations are objectives, epsilon thresholds suppress measurement noise, and the weighted score is a tie-breaker rather than the only decision rule.

The candidate is accepted when:

```text
hard_constraints_pass
and no_protected_foundational_competitive_semantic_vector_or_stability_regression
and (
  no_profile_best_accepted
  or weighted_score > profile_best_accepted_weighted_score + score_epsilon
  or bug_fix_priority_improved(candidate, previous)
)
and (
  bug_fix_priority_improved(candidate, previous)
  or
  weighted_score > previous_weighted_score + score_epsilon
  or epsilon_pareto_improved(candidate, previous)
)
```

`bug_fix_priority_improved(candidate, previous)` is true when the candidate
fixes an observed program failure by turning a previously failed quality gate
into a passing gate or a previously failing evaluation case into a passing
case. This priority can override the weighted-score tie-breaker and raw timing
degradations, but it does not override missing diffs, current quality-gate
failures, or protected objective regressions. The profile-level best accepted
bar uses committed runs with the same profile, regardless of category focus,
to prevent a newly focused category from accepting a lower-scoring candidate
only because its same-category baseline is empty.

`epsilon_pareto_improved(candidate, previous)` means at least one tracked objective improves beyond its epsilon threshold and no tracked objective regresses beyond its epsilon threshold. The default thresholds are:

- `score_epsilon = 0.0005`
- `ratio_epsilon = 0.005` for score components such as foundational_capability, competitive_capability, semantic_vector, performance, and stability
- `metric_epsilon = max(25ms, previous_metric * 0.03)` for raw timing metrics

This avoids rejecting a real case/rank improvement because a timing metric moved inside normal noise, and it avoids accepting a candidate that only wins through noise while silently regressing a protected objective. When local metric improvements do not beat the latest profile baseline, the reject reasons now include that diagnostic and the score delta. Foundational, competitive, semantic_vector, research_judge, performance, case, gate, and metric regressions are recorded as degradation feedback for the next Codex prompt. Positive score, research_judge, performance, case, gate, and metric improvements are also recorded and passed to the next Codex prompt so later iterations know what to preserve. Accepted optimization plans are also stored in each run record as `optimization_plan` and passed to the next prompt under `Recent adopted optimization plans to build on`.

The `chart` command writes:

- `.git/relay-knowledge-self-iteration/score-v2.csv`
- `.git/relay-knowledge-self-iteration/score-v2.svg`

The CSV is a scored-run history, not a patch-directory inventory. It includes
the run mode, patch path, `score_accepted`, and `committed` fields so manual
evaluations and loop iterations can be separated. Manual `evaluate` runs use
unique `manual-evaluate-*` patch/report names and may be marked
`score_accepted=true`, but they are never `accepted=true` unless a git commit
was created. In the SVG, green points are accepted commits, amber points are
manual evaluations that would pass scoring, and red points are rejected runs.

## Evaluation data

`cases.json` and its `include_files` define the self-improvement target
workload. It is not just a list of behavior that already works; newly added cases may represent
competitive targets that future candidates must complete. Candidates should
improve general parser, graph-edge, candidate-pruning, ranking, service
workflow, or observability behavior instead of deleting, weakening, or
enumerating cases.

- Local file-index fixtures create deterministic temporary roots for user
  documents, Linux `/opt`-style paths, Windows `D:`-style paths, deep
  directories, and high-noise file sets. The evaluator runs
  `relay-knowledge files index/query`, records `file_index_ms`,
  `file_query_p50_ms`, and `file_query_p95_ms`. File cases can declare
  `objective`, `max_results`, `truncated`, `degraded_reason`, and more precise
  hit fields to express path/content separation, scope-first filtering,
  candidate pruning, background indexing, and diagnostics targets. A subprocess
  timeout is applied to each file query so a candidate cannot hang the
  evaluator.
- Multi-language repository retrieval targets are split by language under
  `cases/repository_*_targets.json` so each language can evolve independently.
  The default profile covers generated C/C++ syntax fixtures, relay-teams
  Python/JavaScript, opencode TypeScript/TSX, and LevelDB C++; Linux C,
  Kubernetes Go,
  Spring Framework Java, RustFS Rust, Codex Python, nvm Bash, dotnet/runtime C#,
  OkHttp Kotlin, Laravel PHP, Rails Ruby, Scala 3, and Alamofire Swift remain behind
  repository-level `profile=exhaustive`. The language files define real
  `symbol`, `definition`, `references`, `callers`, `callees`, `imports`, and `hybrid` scenarios for
  functions, methods, classes, exported values, macros, includes/imports,
  callback or trait relationships, and execution flows. Import cases may require
  the external-dependency grep fallback diagnostic: when an import target is
  unresolved because the dependency library is not indexed, the product searches
  only the current indexed repository source and returns `text_fallback`
  evidence for LLM reasoning. Fast C fixture guardrails also exercise exact
  grep fallback for comment-only references and hybrid source-text hits in
  headers, implementation files, and generated-table source. Relationship targets
  stay split into regression and challenge groups, with extended relationship
  files adding explicit implementation, alias, and inline callback/closure
  scenarios for Rust, Go, C, C++, Java, Python, JavaScript, and TypeScript.
  Regression cases keep path filters and broader rank thresholds as stable
  guardrails. Challenge cases remove path filters, lower limits and max ranks,
  and add `expected_all` or `expected_sequence` scoring so passed inheritance,
  implementation, dependency, alias, inline, caller-chain, and execution-flow
  cases still leave ranking and coverage improvement room.
- C/C++ syntax fixtures are generated as temporary git repositories and then
  evaluated through the normal `repo register/index/query` path. The C fixture
  covers function pointer typedefs, operation tables, designated and compound
  initializers, function-like macros, local includes, and callback dispatch. The
  C++ fixture covers namespaces, template classes, out-of-line template methods,
  virtual overrides, overloaded operators, lambda captures, namespace aliases,
  using aliases, and header/source split. Their design and external repository
  commit pins are recorded in
  `docs/en/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md`.
- Additional generated syntax fixtures cover Python, JavaScript, TypeScript/TSX,
  Go, Java, Rust, Bash, C#, Kotlin, PHP, Ruby, Scala, and Swift. They keep
  language-specific cases compact and reproducible while real pinned
  repositories continue to provide scale, noise, and performance pressure. The
  fixture matrix is documented in
  `docs/en/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md`.
- Multi-repository repository-set targets in
  `cases/repository_multi_repository_targets.json` register each member as a
  normal full-scope repository first, create an explicit `repo-set`, refresh the
  cross-repository overlay, and run `repo-set query`. The scorer flattens
  `results[*].member` and `results[*].hit` so cases can require specific
  `repository_alias`, `source_scope`, path, line, and excerpt evidence without
  pretending that repository-set hits are single-repository facts. The default
  profile covers Temporal `samples-go` to `sdk-go` usage and OpenTelemetry
  `opentelemetry-collector-contrib` to `opentelemetry-collector` usage.
- Repository register-to-index performance targets in
  `cases/repository_index_performance_targets.json` tighten `index_budget_ms`
  and add combined `register_index_budget_ms` budgets. The evaluator records
  both `*_index_ms` and `*_register_index_ms` so self-iteration prioritizes cold
  indexing wall time after `repo register`, including batching, parser
  throughput, SQLite writes, finalize work, and incremental reuse.
- The built-in `semantic_vector_suite` writes a small evidence fixture into a self-iteration source scope, refreshes semantic/vector indexes, and verifies that query hits expose semantic/vector `retriever_sources`, available `backend_statuses`, and relevant ranking. When `RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external` or `RELAY_KNOWLEDGE_VECTOR_BACKEND=external` is enabled, the evaluator inherits the runtime environment directly and runs `provider probe` first; provider URL, API key, model name, and dimension are not stored in cases or CLI flags.
- `research_judge_suite` sends the candidate diff, deterministic evaluation summary, selected 02/03/04 documentation excerpts, configured competitive feature targets, and implementation guardrails to an LLM or coding-agent judge and emits the `research_judge` objective. It defaults to an `opencode` CLI judge, can be pointed at OpenAI-compatible HTTP, and can be disabled with `RELAY_KNOWLEDGE_JUDGE_BACKEND=none`. Unsupported backend names fail the judge gate, and an explicit CLI judge command selects the CLI backend unless `RELAY_KNOWLEDGE_JUDGE_BACKEND` explicitly requests HTTP. This suite does not replace deterministic gates; it covers research-style and open-ended quality judgment.
- `/opt/workspace/relay-teams` full `scope=all` indexing and Python service, connector, eval checkpoint, and re-export queries.
- `/opt/workspace/opencode` full `scope=all` indexing and TypeScript/TSX monorepo queries for symbols, references, overloaded functions, exported constants, TSX components, caller/callee edges, relative imports, `@/` and `~/` alias imports, HTTP recorder redaction flows, LLM protocol streaming flows, and negative symbol lookup. This target is intentionally import-heavy so the loop can evolve stable TypeScript import identities and duplicate-edge handling instead of only optimizing small fixtures.
- `/opt/workspace/linux` full `scope=all` indexing in the `exhaustive` profile, covering symbols, functions, syscall-style macros, exported symbols, includes, references, callers, callees, mmap flow, and epoll/eventfd retrieval.
- `/opt/workspace/linux` repeated full-repository initial indexing measurement in the `exhaustive` profile through the `linux_full` target.
- `/opt/workspace/leveldb` full `scope=all` C/C++ indexing and queries for class methods, free functions, headers, table cache, recovery, callers, hybrid lookup, and filters.
- `/opt/workspace/temporal-samples-go` and `/opt/workspace/temporal-sdk-go`
  full `scope=all` Go indexing plus a default-profile repository-set workload
  for Temporal worker/client API usage across sample and SDK repositories.
- `/opt/workspace/opentelemetry-collector-contrib` and
  `/opt/workspace/opentelemetry-collector` full `scope=all` Go indexing plus a
  default-profile repository-set workload for receiver factory and component
  type usage across contrib and core repositories.
- `/opt/workspace/kubernetes` full `scope=all` Go indexing in the `exhaustive` profile for command constructors, kubelet flow, API types, clientset/generic clients, authorizers, informer imports, callers, hybrid lookup, and filters.
- `/opt/workspace/spring-framework` full `scope=all` Java indexing in the `exhaustive` profile for context, bean factory, WebMVC servlet/handler mapping, imports, and filtered lookup.
- `/opt/workspace/rustfs` full `scope=all` Rust indexing in the `exhaustive` profile for trait implementation, function-local imports, authentication caller chains, and startup execution flow.
- `/opt/workspace/codex` full `scope=all` Python indexing in the `exhaustive` profile for exception inheritance, relative imports, retry caller chains, and app-server stdio execution flow.
- `/opt/workspace/nvm` full `scope=all` Bash indexing in the `exhaustive` profile for shell functions, command references, installer source hooks, and artifact download flows.
- `/opt/workspace/dotnet-runtime` full `scope=all` C# indexing in the `exhaustive` profile for core library classes, methods, using directives, and array-pool buffer flows.
- `/opt/workspace/okhttp` full `scope=all` Kotlin indexing in the `exhaustive` profile for client classes, method definitions, Okio imports, and request dispatch flows.
- `/opt/workspace/laravel-framework` full `scope=all` PHP indexing in the `exhaustive` profile for application classes, constructor calls, namespace uses, and service-provider bootstrapping.
- `/opt/workspace/rails` full `scope=all` Ruby indexing in the `exhaustive` profile for controller classes, singleton methods, require targets, and module composition.
- `/opt/workspace/scala3` full `scope=all` Scala indexing in the `exhaustive` profile for compiler context classes, inline methods, imports, and phase/mode flows.
- `/opt/workspace/alamofire` full `scope=all` Swift indexing in the `exhaustive` profile for session classes, request methods, imports, and queue/delegate flows.

Prepare the default-profile multi-repository fixtures with:

```bash
git clone --depth 1 https://github.com/temporalio/samples-go.git /opt/workspace/temporal-samples-go
git clone --depth 1 https://github.com/temporalio/sdk-go.git /opt/workspace/temporal-sdk-go
git clone --depth 1 https://github.com/open-telemetry/opentelemetry-collector-contrib.git /opt/workspace/opentelemetry-collector-contrib
git clone --depth 1 https://github.com/open-telemetry/opentelemetry-collector.git /opt/workspace/opentelemetry-collector
```

Prepare the added tree-sitter language fixtures with:

```bash
git clone --depth 1 https://github.com/nvm-sh/nvm.git /opt/workspace/nvm
git clone --depth 1 https://github.com/dotnet/runtime.git /opt/workspace/dotnet-runtime
git clone --depth 1 https://github.com/square/okhttp.git /opt/workspace/okhttp
git clone --depth 1 https://github.com/laravel/framework.git /opt/workspace/laravel-framework
git clone --depth 1 https://github.com/rails/rails.git /opt/workspace/rails
git clone --depth 1 https://github.com/scala/scala3.git /opt/workspace/scala3
git clone --depth 1 https://github.com/Alamofire/Alamofire.git /opt/workspace/alamofire
```

All repository targets must use `scope=all`. The evaluator rejects non-full scopes, and full-scope registration does not pass path or language filters to `repo register`; case-level filters remain available to test query filtering. Use `--profile smoke` for launcher validation without repository evaluation. Use `--profile exhaustive` when long-cycle full initial indexing gates should be run; these gates are intentionally outside the default profile so single-CPU self-iteration workers do not reject every candidate before actionable retrieval feedback is collected.
