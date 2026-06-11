# relay-knowledge self-iteration

[中文](README.zh-CN.md) | English

`tools/self_iteration` is the standalone Rust self-iteration harness. It asks Codex to generate candidate patches, then accepts only candidates that improve repository retrieval, semantic/vector retrieval, performance, stability, or research quality against fixed evaluation workloads. It stays outside the product crate `src/` tree and stores runtime state under `.git/relay-knowledge-self-iteration/`. The old tracked Python harness has been removed after feature parity checks; the repository-root `self-iterate.sh` builds and runs the Rust binary directly.

## Quick Path

### Five-Minute Start

Run from the repository root:

```bash
./self-iterate.sh
```

The launcher defaults to:

```bash
cargo build --manifest-path tools/self_iteration/Cargo.toml --bin relay-knowledge-self-iterate
tools/self_iteration/target/debug/relay-knowledge-self-iterate loop --workspace . --yolo --profile fast
```

`self-iterate.sh` is the stable entrypoint. It builds the standalone harness in debug mode by default so local iterations do not start with a release build. Set `RELAY_KNOWLEDGE_SELF_ITERATION_RELEASE=1` when the harness itself should run from `target/release`. Callers do not need to enter `tools/self_iteration` or install the binary on `PATH`.

### Common Tasks

| Goal | Command |
| --- | --- |
| Run one generation and evaluation round | `./self-iterate.sh once --profile fast` |
| Run at most 3 loop iterations | `./self-iterate.sh --max-iterations 3` |
| Score the current working-tree diff without Codex | `./self-iterate.sh evaluate --use-current-candidate --profile fast` |
| Focus semantic/vector work | `./self-iterate.sh once --profile fast --categories semantic_vector` |
| Run coding-agent workflow regressions | `./self-iterate.sh evaluate --use-current-candidate --profile fast --categories agent_workflows` |
| Focus multiple categories | `./self-iterate.sh once --profile fast --categories semantic_vector,competitive` |
| Run the full legacy gates and workload | `./self-iterate.sh once --profile full` |
| Validate launcher and prompt only | `./self-iterate.sh once --profile smoke --dry-run-codex` |
| Run unattended for a longer window | `./self-iterate.sh loop --strategy unattended-layered --max-wall-clock-hours 48 --stop-after-accepted 12` |
| Generate a research plan | `./self-iterate.sh research-plan --research-topic "2026 graph database research" --research-slug graph-database-research --research-date 2026-06-05` |
| Export score charts | `./self-iterate.sh chart` |

### Choosing a Run Level

| Choice | Use it when | Cost and coverage |
| --- | --- | --- |
| `--profile smoke` | You need to check launcher, prompt, or an early candidate | Does not run repository evaluation. |
| `--profile fast` | You want the default local loop or pre-PR check | Runs formatting, debug build, harness check, key product gates, the default repository subset, repo-set guards, and a semantic/vector guardrail. |
| `--profile full` | You need complete product and harness rails | Restores release builds, clippy, tests, local file fixtures, full repository evaluation, semantic/vector fixtures, and the research judge. |
| `--profile exhaustive` | You need long-cycle large-repository and cold-index stress coverage | Adds exhaustive repositories and heavier performance targets. |
| `--categories ...` | You want a round to focus one score family | Keeps explicit `guardrail=true` bottom-line cases. |
| `--strategy unattended-layered` | You want 1-2 days of unattended progress | Combines smoke exploration, fast validation, macro explore escalation, and deep checks. |

Supported categories are `foundational`, `competitive`, `semantic_vector`, `file_fixtures`, `repository_sets`, `agent_workflows`, `research_judge`, `performance`, and `all`. `--exclude-categories` subtracts categories after `all` expansion, for example `--categories all --exclude-categories research_judge`.

### Output Locations

| Artifact | Path | Purpose |
| --- | --- | --- |
| Candidate patches | `.git/relay-knowledge-self-iteration/patches-v2/` | Net patch for each candidate round. |
| Evaluation reports | `.git/relay-knowledge-self-iteration/reports-v2/` | Gate, case, metric, and command-output summaries. |
| Score history | `.git/relay-knowledge-self-iteration/runs-v2.jsonl` | Per-run scores, decisions, and optimization plans. |
| Long-term memory | `.git/relay-knowledge-self-iteration/memory/` | Accepted/rejected patterns, degradations, and patch indexes for later prompts. |
| Unattended state | `.git/relay-knowledge-self-iteration/unattended-state-v2.json` | Category rotation, failure counters, accepted count, and deep-check schedule. |
| Charts | `.git/relay-knowledge-self-iteration/score-v2.csv`, `score-v2.svg` | Scored-run history; green means committed accepted run, amber means manually evaluated pass, red means rejected run. |

### Observability

The harness writes live progress to stderr with the `[self-iterate]` prefix. Each subprocess reports `command start`, a 15-second `command running` heartbeat, and `command done` or `command timeout` with exit status and duration. Evaluation also reports the selected profile, evaluation home, resolved parallelism, quality-gate stage, repository workload size, repository-set workload size, and final gate/case/command counts. Product command stdout and stderr are still captured in the JSON report, so long `fast` runs remain observable.

## Command Reference

### Syntax and Modes

```bash
./self-iterate.sh [mode] [options]
tools/self_iteration/target/debug/relay-knowledge-self-iterate [mode] [options]
```

| Mode | Default | Behavior |
| --- | --- | --- |
| `loop` | yes | Generates candidates until limits stop the loop; accepted candidates are committed by the harness. |
| `once` | no | Runs one generation and evaluation round. |
| `evaluate` | no | Scores the current diff without invoking Codex or creating a commit. |
| `chart` | no | Exports `score-v2.csv` and `score-v2.svg`. |
| `research-plan` | no | Prints a reusable Markdown research self-iteration plan without invoking Codex, running evaluation, or writing history. |

### General Options

| Option | Values / default | Effect |
| --- | --- | --- |
| `--workspace PATH` | launcher sets repository root | Workspace passed to Codex and evaluators. |
| `--strategy VALUE` | `single`; aliases: `unattended-layered`, `unattended_layered`, `layered` | Selects the normal single loop or the long-running layered unattended strategy. |
| `--profile VALUE` | `fast`; values: `smoke`, `fast`, `full`, `exhaustive` | Selects quality gates and evaluation workload. |
| `--categories LIST` | unset | Focuses one or more score families while preserving bottom-line guardrails. |
| `--exclude-categories LIST` | unset | Removes categories after `all` expansion; aliases include `judge`, `semantic-vector`, and `repo_sets`. |
| `--max-iterations N` | unset | Stops after N loop iterations. |
| `--stop-after-accepted N` | unset for normal strategy; `8` in unattended | Stops after N accepted commits. |
| `--sleep-seconds N` | `5` | Sleep between normal loop rounds; also sets unattended cycle sleep unless overridden. |
| `--cycle-sleep-seconds N` | `120` unattended default | Sleep between unattended cycles. |
| `--commit-message TEXT` | generated from score | Overrides accepted candidate commit subject. |
| `--dry-run-codex` | false | Builds the prompt and records a dry generation result without invoking Codex. |
| `--keep-workdirs` | false | Keeps per-run evaluation homes. |
| `--use-current-candidate` | false | Skips Codex and evaluates the current working-tree diff. |
| `--fail-fast` | false | Propagates the first iteration error instead of continuing until limits. |

### Codex, Research, and Parallelism

| Option | Values / default | Effect |
| --- | --- | --- |
| `--research-topic TEXT` | `relay-knowledge research iteration` | Human-readable topic used in the generated research plan. |
| `--research-slug VALUE` | `research-iteration` | Stable slug for archive, issue, or report filenames; lowercase ASCII, digits, `.`, `-`, and `_` only. |
| `--research-date YYYY-MM-DD` | `YYYY-MM-DD` placeholder | Date written into the generated plan. |
| `--yolo` | false; launcher passes it by default | Maps to non-interactive Codex approvals and the `danger-full-access` sandbox. |
| `--model MODEL` | `gpt-5.5` | Codex model for candidate generation. |
| `--codex-reasoning-effort VALUE` | `xhigh`; values: `low`, `medium`, `high`, `xhigh` | Sets `model_reasoning_effort`. |
| `--codex-profile NAME` | unset | Passes `-p NAME` to Codex. |
| `--codex-path PATH` | `codex` | Codex executable path. |
| `--codex-timeout-seconds N` | `3600` | Candidate generation timeout. |
| `--command-timeout-seconds N` | `900` | Timeout for evaluator subprocesses and product CLI commands. |
| `--jobs auto|N` | `auto` | Global command limiter; `auto` uses available CPU count or `RELAY_KNOWLEDGE_SELF_ITERATION_JOBS`. |
| `--repo-jobs auto|N` | `auto` | Repository-level parallelism; `auto` uses half the available CPU count. |
| `--query-jobs auto|N` | `auto` | Query subprocess parallelism; `auto` uses available CPU count. |

### Unattended Options

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

### Environment Variables

| Variable | Effect |
| --- | --- |
| `RELAY_KNOWLEDGE_SELF_ITERATION_RELEASE=1` | Makes `self-iterate.sh` build and run the release harness binary. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_JOBS=N` | Overrides only the global `--jobs auto` default. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS` | Comma-separated fast profile repository subset. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT` | Per-repository fast case limit. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS` | Comma-separated fast repository-set subset. |
| `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT` | Per-repository-set fast case limit. |
| `RELAY_KNOWLEDGE_JUDGE_BACKEND` | `http`, `openai`, `openai_compatible`, `api`, `llm`, `cli`, `opencode`, `agent`, `none`; disable aliases: `off`, `disabled`, `skip`, `false`. |
| `RELAY_KNOWLEDGE_JUDGE_BASE_URL`, `RELAY_KNOWLEDGE_JUDGE_API_KEY`, `RELAY_KNOWLEDGE_JUDGE_MODEL` | OpenAI-compatible HTTP judge settings. |
| `RELAY_KNOWLEDGE_JUDGE_COMMAND` | CLI judge command template; aliases: `RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND`, `RELAY_KNOWLEDGE_JUDGE_CLI_COMMAND`. |
| `RELAY_KNOWLEDGE_JUDGE_TIMEOUT_SECONDS` | Shared judge timeout; default `120`. |

### YOLO and Research Planning

The local Codex CLI does not expose a literal `--yolo` flag. The harness maps `--yolo` to the current non-interactive high-permission Codex invocation:

```bash
codex -a never exec --dangerously-bypass-approvals-and-sandbox -s danger-full-access -C /opt/workspace/relay-knowledge -m gpt-5.5 -c 'model_reasoning_effort="xhigh"' -
```

Use it only in an externally trusted workspace. Candidate generation defaults to `gpt-5.5` with `model_reasoning_effort="xhigh"`; override with `--model` and `--codex-reasoning-effort low|medium|high|xhigh` when a run needs a cheaper or different generation mode.

`research-plan` is read-only: it does not call Codex, run evaluation, or create history records. It turns the graph database, CodeGraph, X.com, Reddit, and arXiv research workflow into a Markdown plan with a source-ledger checklist, synthesis matrix template, competitive issue extraction rules, documentation/archive outputs, validation gates, and completion evidence.

## Runtime Model

### Single-Round Lifecycle

Each iteration:

1. Verifies the worktree is clean unless `--use-current-candidate` is passed.
2. Prompts local Codex to make one focused code retrieval improvement.
3. Saves the candidate patch under `patches-v2/`.
4. Runs profile-specific quality gates and evaluation.
5. Writes a report under `reports-v2/`.
6. Appends score history to `runs-v2.jsonl`.
7. Updates `score-v2.csv` and `score-v2.svg`.
8. Before acceptance, appends the optimization approach, changed files, metric improvements, and known degradations to `docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md`.
9. Squashes the candidate net change and accepted-optimization record into one commit only when the acceptance policy accepts it.
10. Restores the iteration start commit when the candidate is rejected.

If the worktree is dirty at startup, the loop exits immediately instead of retrying the same non-retryable precondition failure. Implementation candidates must update the self-iteration optimization log before evaluation with algorithm, architecture, invariants, expected case/metric impact, and known risks; the `self_iteration_algorithm_documentation` gate rejects code, test, benchmark, or harness-policy changes that do not carry those notes.

### History and Long-Term Memory

The v2 harness keeps `runs-v2.jsonl`, `reports-v2/`, and `patches-v2/` separate from earlier formats. Each scored run also writes `memory/index.jsonl`, `memory/summaries/`, and `memory/details/`; the next prompt receives rejection-recovery memory, a bounded memory index, profile-specific history synthesis, and a bounded historical patch index. Rejected memories include changed paths, score deltas, local improvements, degradations, and repeated rejection clusters so Codex can avoid retrying small edits that already failed the accepted baseline.

The prompt injects only bounded summaries, so long-running iteration does not grow linearly into the LLM context. It also asks Codex to prefer `rg` for repository inspection and to fall back to bounded `grep -RIn` searches that exclude VCS and build directories when `rg` is not installed.

### Default Fast Profile

`fast` is the default profile. It keeps cost low while covering the paths most likely to regress:

| Group | Coverage |
| --- | --- |
| Basic gates | Product and harness `fmt --check`, Linux GNU glibc 2.31 baseline policy gate, product debug build, and harness `cargo check`. |
| Product gates | `skill_metadata_policy_cases`, `code_index_recovery_cases`, `code_index_health_isolation_cases`, `code_index_sqlite_lock_cases`, and CLI contract cases. |
| Default repositories | `index_performance_many_files`, `c_syntax_fixture`, `cpp_syntax_fixture`, `cross_language_syntax_fixture`, `typescript_syntax_fixture`, `nonstandard_layout_fixture`, `software_global_fixture`, `project_alias_fixture`, `relay_teams`, `leveldb_cpp`, `temporal_samples_go`, and `temporal_sdk_go`. |
| Default sampling | First 8 normal query cases per repository, while always preserving explicit `guardrail=true` cases. |
| Repository sets | 2 cross-repository threshold cases from `temporal_go_workspace`. |
| Semantic/vector | 1 guardrail query. |
| Coding-agent workflows | Skipped by default in `fast`; run with `--categories agent_workflows` or by the PR benchmark workflow. |
| Cache reuse | Reuses `.git/relay-knowledge-self-iteration/cache-v2/fast-evaluation-home/` to reduce registration and indexing cost. |

`fast` does not run product release build, full clippy, full tests, local file fixtures, or the research judge by default. `full` and `exhaustive` restore those rails and run complete repository evaluation, repository-set cases, local file fixtures, semantic/vector fixtures, and the research judge.

Key fast guardrail responsibilities:

| Guardrail | Protects |
| --- | --- |
| `skill_metadata_policy_cases` | Rejects Windows commands or asset examples in bash/POSIX code fences so agent-facing instructions stay shell-specific. |
| CLI contract cases | Verify agent-visible help exposes `repo index-worker`, and idle worker plus streaming worker output parseable JSON. |
| `code_index_recovery_cases` | Cover expired task lease recovery, stale worker completion rejection, attempt-budget dead-lettering, and checkpoint-batch lease renewal. |
| `code_index_health_isolation_cases` | Verify health queries stay bounded during no-language-filter repository updates, and `repo query --freshness allow-stale` can read the latest committed scope. |
| `code_index_sqlite_lock_cases` | Protect duplicate-process SQLite lock avoidance, active-task reuse, and concurrent claims for distinct task fingerprints. |
| Syntax and layout fixtures | Protect external-import unresolved metadata, C/C++ recoverable parser errors, non-top-level `src/` layouts, project aliases reusing one indexed scope, and source/text fallback guardrails. |
| `software_global_fixture` | Ensures `repo software` projections come from indexed evidence, not package caches, cloud APIs, SDK directories, or unindexed external source. |
| `agent_workflow_fixture` | Replays coding-agent issue-analysis tasks over generated Rust, TypeScript, Python, YAML, and Markdown evidence, with budgets for tool calls, source reads, output/context size, evidence count, fallback ratio, and total latency. |

Override the default subset with:

```bash
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=index_performance_many_files,c_syntax_fixture,cpp_syntax_fixture,cross_language_syntax_fixture,typescript_syntax_fixture,nonstandard_layout_fixture,software_global_fixture,project_alias_fixture,relay_teams,leveldb_cpp,temporal_samples_go,temporal_sdk_go
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT=12
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS=temporal_go_workspace
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT=2
```

`full` and `exhaustive` also run `index_performance_wide_mixed_files`, which generates 2048 Rust target files and cross-shard bridge queries, then records cold `*_index_ms`, `*_register_index_ms`, and query p50/p95/max metrics to raise the performance bar with a wider workload.

### Coding-Agent Workflow Gate

`--categories agent_workflows` runs deterministic end-to-end coding-agent scenarios from `cases/agent_workflow_targets.json`. The fixture covers definition lookup, cross-language impact tracing, configuration-to-documentation tracing, and freshness policy checks. Each scenario executes bounded `repo query` steps and fails when expected evidence is missing, context/output grows beyond the case budget, too many unique source files must be read, text fallback dominates the evidence pack, or total query latency exceeds the threshold.

The PR benchmark workflow runs this category as `agent-workflow-regression` with the generated fixture isolated through `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=agent_workflow_fixture`. After the evaluation run it checks the generated JSON report and fails when any gate, case, or agent workflow metric budget fails; the score-vs-history adoption decision is not used for this CI gate. This keeps the CI cost bounded while still exercising the agent-facing behavior.

### Category Focus

`--categories` evaluates explicit guardrail cases plus selected category cases; guardrail failures become quality-gate failures and reject the candidate even when the focused score improves. `--categories semantic_vector` runs the full semantic/vector suite while preserving repository and repo-set bottom-line cases. `--categories performance` keeps repository, repo-set, semantic/vector, and file-fixture workloads that emit performance metrics instead of reducing the run to guardrails only. Score history is isolated by profile and category focus, and acceptance also checks the best committed run for the same profile across category focuses so a new category cannot be accepted below the established profile bar.

### Parallelism Boundaries

Parallelism defaults to `--jobs auto`, `--repo-jobs auto`, and `--query-jobs auto`. `auto` uses the available CPU count for the global command limiter and query pool, and half the available CPU count for repository jobs. Repository register/index and repository-set create/add/refresh writer commands are still serialized against the shared evaluation store; query subprocesses run concurrently after writer boundaries.

### Unattended Layered Strategy

`--strategy unattended-layered` is for 1-2 day unattended sessions. Normal `loop` and `once` behavior stays unchanged unless this strategy is explicitly selected. Defaults are tuned for a 36-hour run; see the unattended options table above.

Each cycle runs short `smoke` explore attempts over `competitive -> semantic_vector -> performance -> repository_sets`. Codex runs only in the explore layer. A candidate that passes the smoke screen is validated with `fast` under the same category and only then reaches the existing accept/commit path.

When short attempts stall, the strategy escalates to `macro_explore` for competitive capability. Macro escalation triggers after repeated competitive promotion failures, repeated empty candidates, or a competitive-capability gap against the best accepted focused baseline. The macro prompt includes current capability snapshots plus `research_judge_suite.competitive_feature_targets` and `implementation_guardrails` from `cases.json`, then asks for a larger ranking, indexing, relationship extraction, query-planning, context-construction, or retrieval-evidence improvement. Candidate notes must state the mutation hypothesis, affected subsystem, expected capability jump, and regression containment while still forbidding fixture/query/path/symbol-specific enumeration.

## Scoring and Acceptance

### Weighted Score

When the research judge is disabled or skipped:

```text
foundational_capability * 0.22
+ competitive_capability * 0.22
+ semantic_vector * 0.13
+ performance * 0.18
+ stability * 0.25
```

When the research judge is enabled:

```text
foundational_capability * 0.17
+ competitive_capability * 0.17
+ semantic_vector * 0.10
+ research_judge * 0.22
+ performance * 0.15
+ stability * 0.19
```

These formulas produce `base_score`. The persisted `score` is `min(1.0, base_score + capability_ceiling_bonus)`. The dynamic ceiling bonus is capped at `0.06` and uses only baseline component fields present in the latest matching workload run or best accepted run for the same profile. Missing judge output never creates a research bonus, and the bonus cannot override failed gates, missing diffs, or protected-objective regressions. Missing diffs still reject adoption and no-diff loop records are ignored as future workload baselines, but they do not zero the `stability` component when the selected quality gates pass; manual `evaluate --use-current-candidate` runs therefore keep performance and gate scores readable even when they are only validating the current baseline.

### Research Judge

The research judge evaluates research alignment, competitive advantage, architecture soundness, performance generalization, implementation actionability, fixture-special-casing risk, and judge evidence quality. It must return strict JSON with `passed`, `confidence`, `overall_score`, `scores`, `summary`, `evidence`, `risks`, `recommended_cases`, `capability_delta`, and `research_gaps`; every configured rubric dimension must appear in `scores` and meet `min_dimension_score`.

The judge can run through an OpenAI-compatible HTTP endpoint or through a coding-agent CLI such as `opencode`, `relay-teams`, `codex`, `cc`, or `copilot`. When no judge backend or HTTP settings are provided, the CLI judge defaults to `opencode`. HTTP API keys are read only from the environment and are not persisted in reports. Set `RELAY_KNOWLEDGE_JUDGE_BACKEND=none` to keep the suite selected while recording `judge_skipped`; use `--exclude-categories research_judge` when the suite itself should not run. Explicit misconfiguration, malformed JSON, low confidence, low overall score, low anti-fixture-special-casing score, missing dimension scores, or low required dimension scores rejects the candidate.

### Cases and Performance Targets

Case objectives are continuous quality scores, not pass-rate counters. A passed case at rank 1 starts from `1.0`; a passed case at rank `N > 1` starts from `1.0 / N` even when `N` is within the case's `max_rank` threshold. Cases may also declare `expected_all`, `expected_sequence`, `min_score`, `require_expected_all`, `require_expected_sequence`, `forbidden_rank_penalty`, and `forbidden_rank_penalty_only`. Empty negative cases that pass with `rank=0` still score `1.0`. Missing foundational, competitive, or semantic/vector objectives default to `0.0`; `accuracy` averages only the foundational and competitive objectives that are actually present.

`performance` uses `budget_relative_v2`. If no compatible previous run exists, metrics use their budget-normalized score. Once the previous run also used this strategy, each metric blends budget fit with relative progress against the previous value, so a latency metric that is merely under budget no longer stays at `1.0`; real improvements continue producing bounded scoring signal.

### Acceptance Policy

Acceptance uses an epsilon-Pareto policy with hard constraints and a weighted-score tie-breaker. Build/test gates and candidate diff existence are hard constraints; foundational_capability, competitive_capability, semantic_vector, stability, and latency observations are protected objectives; epsilon thresholds suppress measurement noise; the weighted score breaks ties rather than acting as the only decision rule.

A candidate is accepted when:

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

`bug_fix_priority_improved` means the candidate fixes an observed program failure by turning a previously failed quality gate into a passing gate or a previously failing evaluation case into a passing case. It can override the weighted-score tie-breaker, the profile-level best committed score bar, and raw timing degradation, but it cannot override missing diffs, current gate failures, or protected-objective regressions.

Default epsilons:

| Threshold | Default | Used for |
| --- | --- | --- |
| `score_epsilon` | `0.0005` | Overall score comparison. |
| `ratio_epsilon` | `0.005` | Score components such as foundational, competitive, semantic_vector, performance, and stability. |
| `metric_epsilon` | `max(25ms, previous_metric * 0.03)` | Raw timing metrics. |

Regressions are recorded as degradation feedback for the next Codex prompt; positive improvements are also passed forward so later iterations know what to preserve. Accepted optimization plans are stored in each run record as `optimization_plan` and passed to the next prompt under `Recent adopted optimization plans to build on`.

## Evaluation Data

`cases.json` and its `include_files` define the self-improvement workload. They are not merely a list of capabilities that already work; new cases may represent competitive targets that future candidates must complete. Candidates should improve general parser, graph-edge, candidate-pruning, ranking, service workflow, or observability behavior instead of deleting, weakening, or enumerating cases.

### Generated and Local Fixtures

| Group | Coverage |
| --- | --- |
| Local file-index fixtures | Generate deterministic temporary roots for user documents, Linux `/opt`-style paths, Windows `D:`-style paths, deep directories, and high-noise file sets; run `files index/query`; record `file_index_ms`, `file_query_p50_ms`, and `file_query_p95_ms`. |
| C/C++ syntax fixtures | Generate temporary git repositories and run `repo register/index/query`; cover function pointer typedefs, operation tables, initializers, macros, local includes, callback dispatch, namespaces, templates, overrides, operators, lambdas, aliases, and header/source split. Design notes live in `docs/en/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md`. |
| Cross-language syntax fixture | Covers C calling C++, C++ calling C, Go cgo calling C, and Rust FFI calling C so default fast runs can validate multi-language call graph retrieval without another large checkout. |
| Additional multilingual fixtures | Cover Python, JavaScript, TypeScript/TSX, Go, Java, Rust, Bash, C#, Kotlin, PHP, Ruby, Scala, and Swift; the matrix is documented in `docs/en/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md`. |
| Repository-set targets | Register each member as a full `scope=all` repository, create an explicit `repo-set`, refresh cross-repository overlays, and run `repo-set query`; cases can require member, source scope, path, line, and excerpt evidence. |
| Register-to-index performance targets | `repository_index_performance_targets.json` tightens `index_budget_ms` and adds `register_index_budget_ms`; default fast includes a 1024-file fixture, while `full` and `exhaustive` also include a 2048-file wide fixture. |
| Software global projection targets | `repository_software_global_targets.json` runs `repo software` for dependencies, sdks, files, topics, relationships, build, iac, design, and all projection kinds, with facts derived only from indexed evidence. |
| CLI contract cases | Run product CLI commands without indexing a large repository; default fast covers `repo index-worker` help, idle JSON, and streaming JSON. |
| Semantic/vector suite | Writes a small evidence fixture, refreshes semantic/vector indexes, and verifies `retriever_sources`, `backend_statuses`, and relevant ranking; external providers are inherited only from the runtime environment. |
| Research judge suite | Sends candidate diff, deterministic evaluation summary, documentation excerpts, competitive targets, and implementation guardrails to an LLM or coding-agent judge; it does not replace deterministic gates. |

Multi-language repository retrieval targets are split by language under `cases/repository_*_targets.json` so each language can evolve independently. Language cases cover real `symbol`, `definition`, `references`, `callers`, `callees`, `imports`, and `hybrid` scenarios for functions, methods, classes, exported values, macros, includes/imports, callback or trait relationships, and execution flows. Relationship targets are split into regression and challenge groups; challenge cases use `expected_all` or `expected_sequence` to keep ranking and coverage improvement room even after they pass.

### Real Repository Targets

| Repository | Profile | Target |
| --- | --- | --- |
| `/opt/workspace/relay-teams` | default | Python service, connector, eval checkpoint, and re-export queries. |
| `/opt/workspace/opencode` | default | TypeScript/TSX monorepo queries for symbols, references, overloads, exported constants, TSX components, caller/callee edges, relative imports, `@/` and `~/` aliases, HTTP recorder redaction flow, LLM protocol streaming flow, and negative symbol lookup. |
| `/opt/workspace/leveldb` | default | C/C++ classes, free functions, headers, table cache, recovery, callers, hybrid lookup, and filters. |
| `/opt/workspace/temporal-samples-go`, `/opt/workspace/temporal-sdk-go` | default | Full-scope Go indexing plus repository-set API usage from Temporal samples to the SDK. |
| `/opt/workspace/opentelemetry-collector-contrib`, `/opt/workspace/opentelemetry-collector` | default | Full-scope Go indexing plus contrib-to-core receiver factory and component type usage. |
| `/opt/workspace/linux` | `exhaustive` | C symbols, functions, syscall-style macros, exported symbols, includes, references, callers, callees, mmap flow, epoll/eventfd; `linux_full` repeats full initial-index timing. |
| `/opt/workspace/kubernetes` | `exhaustive` | Go command constructors, kubelet flow, API types, clientset/generic clients, authorizers, informer imports, callers, hybrid lookup, and filters. |
| `/opt/workspace/spring-framework` | `exhaustive` | Java context, bean factory, WebMVC servlet/handler mapping, imports, and filtered lookup. |
| `/opt/workspace/rustfs` | `exhaustive` | Rust trait implementation, function-local imports, authentication caller chains, and startup execution flow. |
| `/opt/workspace/codex` | `exhaustive` | Python exception inheritance, relative imports, retry caller chains, and app-server stdio execution flow. |
| `/opt/workspace/nvm` | `exhaustive` | Bash functions, command references, installer source hooks, and artifact download flows. |
| `/opt/workspace/dotnet-runtime` | `exhaustive` | C# core library classes, methods, using directives, and array-pool buffer flows. |
| `/opt/workspace/okhttp` | `exhaustive` | Kotlin client classes, method definitions, Okio imports, and request dispatch flows. |
| `/opt/workspace/laravel-framework` | `exhaustive` | PHP application classes, constructor calls, namespace uses, and service-provider bootstrapping. |
| `/opt/workspace/rails` | `exhaustive` | Ruby controller classes, singleton methods, require targets, and module composition. |
| `/opt/workspace/scala3` | `exhaustive` | Scala compiler context classes, inline methods, imports, and phase/mode flows. |
| `/opt/workspace/alamofire` | `exhaustive` | Swift session classes, request methods, imports, and queue/delegate flows. |

Prepare the default-profile multi-repository fixtures with:

```bash
git clone --depth 1 https://github.com/temporalio/samples-go.git /opt/workspace/temporal-samples-go
git clone --depth 1 https://github.com/temporalio/sdk-go.git /opt/workspace/temporal-sdk-go
git clone --depth 1 https://github.com/open-telemetry/opentelemetry-collector-contrib.git /opt/workspace/opentelemetry-collector-contrib
git clone --depth 1 https://github.com/open-telemetry/opentelemetry-collector.git /opt/workspace/opentelemetry-collector
```

Prepare the added tree-sitter language repositories with:

```bash
git clone --depth 1 https://github.com/nvm-sh/nvm.git /opt/workspace/nvm
git clone --depth 1 https://github.com/dotnet/runtime.git /opt/workspace/dotnet-runtime
git clone --depth 1 https://github.com/square/okhttp.git /opt/workspace/okhttp
git clone --depth 1 https://github.com/laravel/framework.git /opt/workspace/laravel-framework
git clone --depth 1 https://github.com/rails/rails.git /opt/workspace/rails
git clone --depth 1 https://github.com/scala/scala3.git /opt/workspace/scala3
git clone --depth 1 https://github.com/Alamofire/Alamofire.git /opt/workspace/alamofire
```

All repository targets must use `scope=all`. The evaluator rejects non-full scopes, full-scope registration does not pass path or language filters to `repo register`, and a default guardrail verifies that product registration rejects `--language`; case-level filters remain available to test query filtering. Missing external dependency source is not parser, index, file, scope, or response degradation. It must surface as unresolved edge metadata such as `resolution_state` and `target_hint`, and source/text fallback must not mask authorization gaps, dependency coverage gaps, or parser recovery problems.
