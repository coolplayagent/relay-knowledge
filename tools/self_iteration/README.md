# relay-knowledge self-iteration

[中文](README.zh-CN.md) | English

This directory contains an independent Codex-driven optimization loop for code repository retrieval quality and graph semantic/vector retrieval quality. It also provides a reusable research-planning mode for source-backed roadmap research. It now evolves as a standalone Rust harness under `tools/self_iteration`, outside the product crate `src/` tree, and stores runtime state under `.git/relay-knowledge-self-iteration/`. The old tracked Python harness has been removed after feature parity checks; `self-iterate.sh` builds and runs the Rust binary directly.

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
./self-iterate.sh research-plan --research-topic "2026 graph database research" --research-slug graph-database-research --research-date 2026-06-05
./self-iterate.sh loop --strategy unattended-layered
./self-iterate.sh loop --strategy unattended-layered --max-wall-clock-hours 48 --stop-after-accepted 12
```

## Command Line Reference

Syntax:

```bash
./self-iterate.sh [mode] [options]
tools/self_iteration/target/debug/relay-knowledge-self-iterate [mode] [options]
```

Modes:

| Mode | Default | Behavior |
| --- | --- | --- |
| `loop` | yes | Generates candidates until limits stop the loop; accepted candidates are committed by the harness. |
| `once` | no | Runs one generation/evaluation iteration. |
| `evaluate` | no | Scores the current diff without invoking Codex or creating a commit. |
| `chart` | no | Exports `.git/relay-knowledge-self-iteration/score-v2.csv` and `score-v2.svg`. |
| `research-plan` | no | Prints a reusable Markdown research self-iteration plan without invoking Codex, running evaluation, or writing history. |

General options:

| Option | Values / default | Effect |
| --- | --- | --- |
| `--workspace PATH` | launcher sets repository root | Workspace passed to Codex and evaluators. |
| `--strategy VALUE` | `single`; aliases: `unattended-layered`, `unattended_layered`, `layered` | Selects the normal single loop or the long-running layered unattended strategy. |
| `--profile VALUE` | `fast`; values: `smoke`, `fast`, `full`, `exhaustive` | Selects quality gates and evaluation workload. |
| `--categories LIST` | unset; values: `foundational`, `competitive`, `semantic_vector`, `file_fixtures`, `repository_sets`, `research_judge`, `performance`, `all` | Focuses scoring/evaluation on selected objective families while preserving guardrails. |
| `--exclude-categories LIST` | unset; same values as `--categories`; aliases include `judge`, `semantic-vector`, `repo_sets` | Removes categories after `all` expansion. Fails if nothing remains. |
| `--max-iterations N` | unset | Stops after N loop iterations. |
| `--stop-after-accepted N` | unset for `single`; `8` default in unattended | Stops after N accepted commits. |
| `--sleep-seconds N` | `5` | Sleep between normal loop iterations; also sets unattended cycle sleep unless overridden. |
| `--cycle-sleep-seconds N` | `120` unattended default | Sleep between unattended cycles. |
| `--commit-message TEXT` | generated from score | Overrides accepted candidate commit subject. |
| `--dry-run-codex` | false | Builds the prompt and records a dry generation result without invoking Codex. |
| `--keep-workdirs` | false | Keeps per-run evaluation homes instead of deleting transient homes. |
| `--use-current-candidate` | false | Skips Codex and evaluates the current working tree diff. |
| `--fail-fast` | false | Propagates the first iteration error instead of continuing until limits. |

Research planning options:

| Option | Values / default | Effect |
| --- | --- | --- |
| `--research-topic TEXT` | `relay-knowledge research iteration` | Human-readable topic used in the generated research plan. |
| `--research-slug VALUE` | `research-iteration`; lowercase ASCII, digits, `.`, `-`, `_` | Stable slug for generated archive, issue, or report filenames. |
| `--research-date YYYY-MM-DD` | `YYYY-MM-DD` placeholder | Date written into the generated research plan. |

Codex generation options:

| Option | Values / default | Effect |
| --- | --- | --- |
| `--yolo` | false; launcher passes it by default | Maps to non-interactive Codex approvals and `danger-full-access` sandbox. |
| `--model MODEL` | `gpt-5.5` | Codex model for candidate generation. |
| `--codex-reasoning-effort VALUE` | `xhigh`; values: `low`, `medium`, `high`, `xhigh` | Sets `model_reasoning_effort`. |
| `--codex-profile NAME` | unset | Passes `-p NAME` to Codex. |
| `--codex-path PATH` | `codex` | Codex executable path. |
| `--codex-timeout-seconds N` | `3600` | Candidate generation timeout. |

Evaluation and concurrency options:

| Option | Values / default | Effect |
| --- | --- | --- |
| `--command-timeout-seconds N` | `900` | Timeout for evaluator subprocesses and product CLI commands. |
| `--jobs auto|N` | `auto` | Global command limiter; `auto` uses available CPU count or `RELAY_KNOWLEDGE_SELF_ITERATION_JOBS`. |
| `--repo-jobs auto|N` | `auto` | Repository-level parallelism; `auto` uses half the available CPU count. |
| `--query-jobs auto|N` | `auto` | Query subprocess parallelism; `auto` uses available CPU count. |

Unattended layered options:

| Option | Default | Effect |
| --- | --- | --- |
| `--max-wall-clock-hours N` | `36` | Overall unattended runtime cap. |
| `--explore-timeout-seconds N` | `900` | Timeout for short explore Codex attempts. |
| `--macro-explore-timeout-seconds N` | `2700` | Timeout for macro mutation attempts. |
| `--max-explore-attempts-per-cycle N` | `3` | Short explore retries before a cycle ends. |
| `--max-consecutive-empty-candidates N` | `8` | Stops after repeated no-diff generations. |
| `--max-consecutive-promotion-failures N` | `10` | Stops after repeated screen/validate failures. |
| `--macro-after-competitive-failures N` | `4` | Triggers macro mutation after repeated competitive failures. |
| `--macro-after-empty-candidates N` | `6` | Triggers macro mutation after repeated empty candidates. |
| `--cooldown-after-accept-seconds N` | `300` | Sleep after accepted unattended commits. |
| `--cooldown-after-timeout-seconds N` | `900` | Sleep after Codex timeout. |
| `--deep-check-interval-accepts N` | `6` | Runs deeper validation after this many accepts. |
| `--deep-check-interval-hours N` | `12` | Runs deeper validation after this many hours. |

Environment variables:

| Variable | Effect |
| --- | --- |
| `RELAY_KNOWLEDGE_SELF_ITERATION_RELEASE=1` | Makes `self-iterate.sh` build and run the release harness binary. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_JOBS=N` | Overrides the global `--jobs auto` default only. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS` | Comma-separated fast profile repository subset. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT` | Per-repository fast case limit. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS` | Comma-separated fast repository-set subset. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT` | Per-repository-set fast case limit. |
| `RELAY_KNOWLEDGE_JUDGE_BACKEND` | `http`, `openai`, `openai_compatible`, `api`, `llm`, `cli`, `opencode`, `agent`, `none`; disable aliases: `off`, `disabled`, `skip`, `false`. |
| `RELAY_KNOWLEDGE_JUDGE_BASE_URL`, `RELAY_KNOWLEDGE_JUDGE_API_KEY`, `RELAY_KNOWLEDGE_JUDGE_MODEL` | OpenAI-compatible HTTP judge settings. |
| `RELAY_KNOWLEDGE_JUDGE_COMMAND` | CLI judge command template; aliases: `RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND`, `RELAY_KNOWLEDGE_JUDGE_CLI_COMMAND`. |
| `RELAY_KNOWLEDGE_JUDGE_TIMEOUT_SECONDS` | Shared judge timeout; default `120`. |

Copyable examples:

```bash
./self-iterate.sh once --profile fast
./self-iterate.sh evaluate --use-current-candidate --profile fast
./self-iterate.sh once --profile fast --categories semantic_vector
./self-iterate.sh once --profile full --categories all --exclude-categories research_judge
./self-iterate.sh loop --strategy unattended-layered --max-wall-clock-hours 48 --stop-after-accepted 12
./self-iterate.sh research-plan --research-topic "2026 graph database research" --research-slug graph-database-research --research-date 2026-06-05 > .git/relay-knowledge-self-iteration/research-plan.md
RELAY_KNOWLEDGE_JUDGE_BACKEND=none ./self-iterate.sh once --profile full --categories research_judge
RELAY_KNOWLEDGE_JUDGE_BACKEND=http RELAY_KNOWLEDGE_JUDGE_BASE_URL=http://localhost:11434/v1 RELAY_KNOWLEDGE_JUDGE_API_KEY=local RELAY_KNOWLEDGE_JUDGE_MODEL=judge-model ./self-iterate.sh once --profile full --categories research_judge
RELAY_KNOWLEDGE_JUDGE_COMMAND='opencode run "Read the attached relay-knowledge judge prompt and return only the strict JSON object it requests." --file {prompt_file}' ./self-iterate.sh once --profile full --categories research_judge
./self-iterate.sh chart
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

## Research Planning Mode

`research-plan` extracts the repeatable method from the 2026 graph database,
CodeGraph, X.com, Reddit, and arXiv research pass. It prints a Markdown plan
that can be used as the starting artifact for future research iterations. The
plan includes a reference action summary, source-ledger checklist, synthesis
matrix template, competitive issue extraction rules, documentation/archive
outputs, validation gates, and completion evidence.

The mode is intentionally read-only: it does not call Codex, does not run
evaluation, and does not create `.git/relay-knowledge-self-iteration/` history
records. Use it before a research iteration to keep source credibility,
bilingual documentation, issue creation, archive records, and remote-main
publication checks explicit.

## Loop behavior

Each iteration:

1. Verifies the worktree is clean unless `--use-current-candidate` is passed.
2. Prompts local Codex to make one focused code retrieval improvement.
3. Saves the candidate patch from the iteration start commit under `.git/relay-knowledge-self-iteration/patches-v2/`.
4. Runs profile-specific gates and evaluation. The default `fast` profile runs formatting checks, the Linux glibc compatibility policy gate, a product debug build, harness `cargo check`, agent-facing CLI contract cases, an expanded normal-repository subset, repository-set guards, and a semantic/vector guardrail query. `full` and `exhaustive` restore both release builds, product `clippy -> test` and harness `clippy -> test` rails, plus the full repository evaluation, repository-set cases, local-file fixtures, semantic/vector fixtures, and research judge.
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

The default profile is `fast`. It runs product and harness `fmt --check`, checks
that the release workflow still enforces the glibc 2.31 Linux GNU baseline, then
runs a product debug build, harness `cargo check`, and the targeted
`skill_metadata_policy_cases`, `code_index_recovery_cases`, and
`code_index_sqlite_lock_cases` gates before evaluating with
`target/debug/relay-knowledge`. It does not run the product release build, full
clippy, full test suite, local-file fixtures, or research judge by default. The
skill metadata policy gate rejects CLI skill command examples that put Windows
`.exe` assets in bash/POSIX code fences, so agent-facing instructions stay
shell-specific.
The CLI contract cases run after the debug product build and before repository
evaluation. They verify that agent-visible help exposes `repo index-worker` and
that idle worker attempts return parseable JSON plus `started`/`item`/`completed`
streaming JSON events instead of empty stdout.
The code-index recovery gate covers expired task lease recovery, stale worker
completion rejection, attempt-budget dead-lettering, and checkpoint-batch lease
renewal without indexing exhaustive large repositories.
The SQLite lock gate opens independent file-backed stores against the same
database and verifies duplicate full-index starts reuse the running task while
distinct task fingerprints can claim independent leases without waiting behind
another running task.
`fast` also runs a registration guardrail proving `repo register --language`
is rejected so mixed C/C++ repositories cannot be narrowed at registration.
`fast` evaluates `index_performance_many_files`, `c_syntax_fixture`, `cpp_syntax_fixture`,
`cross_language_syntax_fixture`, `typescript_syntax_fixture`,
`nonstandard_layout_fixture`, `software_global_fixture`, `project_alias_fixture`, `relay_teams`, `leveldb_cpp`,
`temporal_samples_go`, and `temporal_sdk_go`, takes the first 8 normal query
cases per repository while always preserving explicit guardrail cases, keeps 2
cross-repository threshold cases from the `temporal_go_workspace` repo-set, and
runs the semantic/vector guardrail query. The generated index-performance
fixture creates 1024 small Rust files and guards cold register-to-index
throughput without depending on an external checkout. The project alias fixture registers a
generated repository without `--alias`, adds a session-style explicit alias to
the same root before indexing, and verifies both aliases reuse the same indexed
scope. The TypeScript, C, and C++ syntax
fixtures keep external-import source fallback guardrails in the default fast
loop and require missing dependency source to stay as unresolved edge metadata
instead of `degraded_reason`. The C and C++ fixtures also cover macro-generated
handlers and export-macro-decorated classes so recoverable parser errors do not
force query-time source fallback. The
nonstandard layout fixture keeps Python, TypeScript, Go, Java, C++, and Swift
sources outside a top-level `src/` covered by fast guardrails. The same
nonstandard layout fixture also carries fast guardrails for
`repo query --kind sbom` over Cargo, npm, Go, Python, Maven BOM, Gradle, and
Conan manifest or lock files, so dependency-inventory regressions are rejected
by the default loop. The software global fixture runs `repo software` over a
generated repository and guards dependency, SDK, file, topic, relationship,
build, IaC, design, and all-slice projection kinds without scanning package
caches, cloud APIs, SDK directories, or unindexed external source. The C fixture also includes explicit source/text-fallback
cases early in its fast case window, so exact source-text recovery stays covered
without indexing another large repository or depending on an external `rg`
binary. It reuses
`.git/relay-knowledge-self-iteration/cache-v2/fast-evaluation-home/` to reduce
repeated registration and indexing cost. Score history is isolated by profile
and category focus, so `fast --categories semantic_vector` compares only
against matching semantic/vector-focused fast runs and does not treat
full/exhaustive judge scores as fast regressions. Acceptance also checks the
best accepted run for the same profile across category focuses, so a first run
for a new category cannot be committed below the established profile-level bar.
Override the subset with
`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=index_performance_many_files,c_syntax_fixture,cpp_syntax_fixture,cross_language_syntax_fixture,typescript_syntax_fixture,nonstandard_layout_fixture,software_global_fixture,project_alias_fixture,relay_teams,leveldb_cpp,temporal_samples_go,temporal_sdk_go`,
`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT=12`,
`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS=temporal_go_workspace`, and
`RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT=2`. Pass
`--profile full` for the previous full gates and workload; long-running
large-repository checks still use `--profile exhaustive`.

The `full` and `exhaustive` profiles add the generated
`index_performance_wide_mixed_files` repository to the performance workload.
It creates a wider Rust workspace with 2048 indexed target files and cross-shard
bridge queries, then records the same cold `*_index_ms`,
`*_register_index_ms`, query percentile, and query max metrics under stricter
budgets. This raises the performance bar for deeper validation without adding
cost to the default `fast` profile.

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
collapsing to guardrails only. Use `--exclude-categories` to subtract categories
after `all` expansion; for example,
`--categories all --exclude-categories research_judge` keeps the full focused
workload but does not run the research judge suite.

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
against the best accepted focused baseline. The macro prompt uses a bounded
biological-mutation profile: it includes current capability snapshots,
`research_judge_suite.competitive_feature_targets`, and
`implementation_guardrails` from `cases.json`, then asks for a larger ranking,
indexing, relationship extraction, query-planning, context-construction, or
retrieval-evidence improvement. The final candidate notes must state the
mutation hypothesis, affected subsystem, expected capability jump, and
regression containment while still forbidding fixture-specific enumeration.

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

These formulas produce `base_score`. The persisted `score` is
`min(1.0, base_score + capability_ceiling_bonus)`. The dynamic ceiling bonus is
bounded to `0.06` and is computed only from components that have real baseline
fields in the latest matching workload run or the best accepted run for the
same profile. It rewards progress against the remaining headroom toward `1.0`
for competitive capability, semantic/vector quality, performance when the
current run emits key performance metrics, and research_judge when a current
judge score exists. Missing judge output never creates a research bonus, and
the bonus cannot override failed gates, missing diffs, or protected objective
regressions.

This policy intentionally gives research quality, competitive capability, and
performance more room to compound after the current baseline gets strong while
keeping the other objectives protected by regression checks.

The research judge evaluates research alignment, competitive advantage,
architecture soundness, performance generalization, implementation
actionability, fixture-special-casing risk, and judge evidence quality. It must
return strict JSON with `passed`, `confidence`, `overall_score`, `scores`,
`summary`, `evidence`, `risks`, `recommended_cases`, `capability_delta`, and
`research_gaps`; every configured rubric dimension must appear in `scores` and
meet `min_dimension_score`. It can run through an
OpenAI-compatible HTTP endpoint or through an open coding-agent CLI such as
`opencode`, `relay-teams`, `codex`, `cc`, or `copilot`. When no judge backend or
HTTP settings are provided, the CLI judge defaults to `opencode`. All judge
overrides come from runtime environment variables:

`cases.json` can also configure the judge workload. `documents` selects bounded
02/03/04 excerpts, `competitive_feature_targets` lists the research-derived
capabilities candidates should advance, `rubric_dimensions` and
`min_dimension_score` define the strict scoring dimensions, and
`implementation_guardrails` lists non-negotiable constraints such as
anti-fixture behavior, async boundaries, freshness/version evidence, and
same-change documentation updates.

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
JSON. Set `RELAY_KNOWLEDGE_JUDGE_BACKEND=none` to keep the suite selected while
recording `judge_skipped`; `off`, `disabled`, `skip`, and `false` are accepted
as disable aliases. Use `--exclude-categories research_judge` when the suite
itself should not run. Explicit misconfiguration, malformed JSON, low
confidence, low overall score, low anti-fixture-special-casing score, missing
dimension scores, or low required dimension scores rejects the candidate.

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
  The default profile covers generated C/C++ syntax fixtures, a generated
  C/C++/Go/Rust cross-language call fixture, relay-teams Python/JavaScript,
  opencode TypeScript/TSX, and LevelDB C++; Linux C,
  Kubernetes Go,
  Spring Framework Java, RustFS Rust, Codex Python, nvm Bash, dotnet/runtime C#,
  OkHttp Kotlin, Laravel PHP, Rails Ruby, Scala 3, and Alamofire Swift remain behind
  repository-level `profile=exhaustive`. The language files define real
  `symbol`, `definition`, `references`, `callers`, `callees`, `imports`, and `hybrid` scenarios for
  functions, methods, classes, exported values, macros, includes/imports,
  callback or trait relationships, and execution flows. Import cases may require
  the external-dependency source fallback diagnostic: when an import target is
  unresolved because the dependency library is not indexed, the product searches
  only the current indexed repository source and returns `text_fallback`
  evidence for LLM reasoning. Fast C fixture guardrails also exercise exact
  source fallback for comment-only references and hybrid source-text hits in
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
- The cross-language syntax fixture is also generated locally and stays in the
  default fast profile. It covers C calling C++, C++ calling C, Go cgo calling C,
  and Rust FFI calling C with caller and callee queries so the fast loop keeps
  pressure on multi-language call graph retrieval without cloning another
  external repository.
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
  and add combined `register_index_budget_ms` budgets. The default fast profile
  includes the generated `index_performance_many_files` repository so cold
  throughput regressions are visible even when external large repositories are
  absent. Full and exhaustive profiles also include
  `index_performance_wide_mixed_files`, a 2048-file Rust workspace with
  cross-shard bridge queries. The evaluator records both `*_index_ms` and
  `*_register_index_ms` so self-iteration prioritizes cold indexing wall time
  after `repo register`, including batching, parser throughput, SQLite writes,
  finalize work, query p50/p95/max, and incremental reuse.
- Software global projection targets in
  `cases/repository_software_global_targets.json` run `repo software` against a
  generated full-scope repository after normal registration and indexing. They
  cover `dependencies`, `sdks`, `files`, `topics`, `relationships`, `build`,
  `iac`, `design`, and `all` projection kinds, including unresolved external
  SDK metadata, documentation/config relationships, build targets, IaC
  resources, and design elements derived from indexed evidence only.
- CLI contract cases in `cases.json` run product CLI commands without indexing
  a large repository. Default fast guardrails cover `repo index-worker`
  machine-readable help and idle JSON/streaming JSON output so
  non-interactive agents can explicitly drain durable code-index tasks when a
  long-running service is unavailable.
- The fast profile runs `code_index_health_isolation_cases` as a product gate.
  The case indexes a no-language-filter repository update while checking that
  health remains bounded and `repo query --freshness allow-stale` can read the
  latest completed committed scope instead of hanging behind the index writer.
- The fast profile runs `code_index_sqlite_lock_cases` as a product and storage
  gate. The cases protect duplicate-process SQLite lock avoidance, active-task
  reuse, and independent task claim concurrency.
- The built-in `semantic_vector_suite` writes a small evidence fixture into a self-iteration source scope, refreshes semantic/vector indexes, and verifies that query hits expose semantic/vector `retriever_sources`, available `backend_statuses`, and relevant ranking. When `RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external` or `RELAY_KNOWLEDGE_VECTOR_BACKEND=external` is enabled, the evaluator inherits the runtime environment directly and runs `provider probe` first; provider URL, API key, model name, and dimension are not stored in cases or CLI flags.
- `research_judge_suite` sends the candidate diff, deterministic evaluation summary, selected 02/03/04 documentation excerpts, configured competitive feature targets, and implementation guardrails to an LLM or coding-agent judge and emits the `research_judge` objective. It defaults to an `opencode` CLI judge, can be pointed at OpenAI-compatible HTTP, and can keep the suite selected while disabling the backend with `RELAY_KNOWLEDGE_JUDGE_BACKEND=none`; use `--exclude-categories research_judge` when the suite itself should not run. Unsupported backend names fail the judge gate, and an explicit CLI judge command selects the CLI backend unless `RELAY_KNOWLEDGE_JUDGE_BACKEND` explicitly requests HTTP. This suite does not replace deterministic gates; it covers research-style and open-ended quality judgment.
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

All repository targets must use `scope=all`. The evaluator rejects non-full scopes, full-scope registration does not pass path or language filters to `repo register`, and a default guardrail verifies that product registration rejects `--language`; case-level filters remain available to test query filtering. Use `--profile smoke` for launcher validation without repository evaluation. Use `--profile exhaustive` when long-cycle full initial indexing gates should be run; these gates are intentionally outside the default profile so single-CPU self-iteration workers do not reject every candidate before actionable retrieval feedback is collected.
