# relay-knowledge self-iteration

[中文](README.zh-CN.md) | English

This directory contains an independent Codex-driven optimization loop for code repository retrieval quality and graph semantic/vector retrieval quality. It is intentionally outside the Rust crate and stores all runtime state under `.git/relay-knowledge-self-iteration/`.

## Start

From the repository root:

```bash
./self-iterate.sh
```

The launcher defaults to:

```bash
python3 tools/self_iteration/self_iterate.py loop --yolo
```

Useful variants:

```bash
./self-iterate.sh once
./self-iterate.sh --max-iterations 3
./self-iterate.sh chart
./self-iterate.sh once --profile smoke --dry-run-codex
```

## YOLO mode

The local Codex CLI does not expose a literal `--yolo` flag. This framework maps `--yolo` to the current non-interactive high-permission Codex invocation:

```bash
codex -a never exec --dangerously-bypass-approvals-and-sandbox -s danger-full-access -C /opt/workspace/relay-knowledge -
```

Use it only in an externally trusted workspace. The loop is designed to run unattended.

## Loop behavior

Each iteration:

1. Verifies the worktree is clean unless `--use-current-candidate` is passed.
2. Prompts local Codex to make one focused code retrieval improvement.
3. Saves the candidate patch from the iteration start commit under `.git/relay-knowledge-self-iteration/patches/`.
4. Runs build, lint, tests, repository retrieval evaluations, the semantic/vector fixture, the external embedding provider probe when external backends are enabled, the research judge when configured, and the self-iteration documentation gate.
5. Records a report under `.git/relay-knowledge-self-iteration/reports/`.
6. Appends scoring history to `.git/relay-knowledge-self-iteration/runs.jsonl`.
7. Writes progressive memory entries under `.git/relay-knowledge-self-iteration/memory/`.
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
or harness-policy changes that do not carry those notes. The prompt also treats
`.git/relay-knowledge-self-iteration/patches/` as long-term memory: it lists a
bounded patch index and instructs Codex to read only relevant historical patch
files in small ranges when reasoning about the next candidate.

The progressive memory store is the first context entry point for future runs:

- `memory/index.jsonl` is a compact machine-readable index of accepted
  optimizations, rejected attempts, quality-gate failures, and observed
  foundational, competitive, semantic/vector, or performance regressions.
- `memory/summaries/<id>.md` contains the short record Codex should read first.
- `memory/details/<id>.md` contains the full score, gate, case, metric, patch,
  and report references for follow-up inspection.
- `memory/artifacts/<id>/` is reserved for optional extracted artifacts such as
  trimmed report snippets or judge output.

The prompt includes only a bounded memory index. Codex should load matching
summary files first, then open detail or patch files only when the summary is
relevant to the current gate, metric, case, path, or algorithm objective.

## Scoring and acceptance

When the research judge is not configured, the score is:

```text
foundational_capability * 0.22
+ competitive_capability * 0.22
+ semantic_vector * 0.13
+ performance * 0.18
+ stability * 0.25
```

When the research judge is configured, `research_judge` becomes a protected
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

- `RELAY_KNOWLEDGE_JUDGE_BACKEND=http|cli|opencode|none`; `opencode` is a
  CLI alias that uses the default opencode command unless a custom command is
  also set
- HTTP: `RELAY_KNOWLEDGE_JUDGE_BASE_URL`, `RELAY_KNOWLEDGE_JUDGE_API_KEY`,
  `RELAY_KNOWLEDGE_JUDGE_MODEL`
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
at rank 1 scores `1.0`; a passed case at rank `N > 1` scores `1.0 / N` even
when `N` is within the case's `max_rank` acceptance threshold. Empty negative
cases that pass with `rank=0` still score `1.0`. Missing foundational,
competitive, or semantic/vector objectives default to `0.0` instead of silently
appearing complete, and `accuracy` averages only the foundational and
competitive objectives that are actually present. Metric budget misses are
reported in `metric_budget_failures` while the existing budget-normalized
`performance` score remains the weighted latency signal.

`accuracy` is retained as a compatibility roll-up of foundational and competitive case scores. Acceptance uses an `epsilon-Pareto acceptance with hard constraints and weighted-score tie-breaker` policy. In multi-objective optimization terms, build/test gates and candidate diff existence are hard constraints, foundational_capability, competitive_capability, semantic_vector, and stability are protected objectives for basic usability, advanced retrieval quality, semantic/vector source coverage, backend availability, and latency observations are objectives, epsilon thresholds suppress measurement noise, and the weighted score is a tie-breaker rather than the only decision rule.

The candidate is accepted when:

```text
hard_constraints_pass
and no_protected_foundational_competitive_semantic_vector_or_stability_regression
and (
  weighted_score > previous_weighted_score + score_epsilon
  or epsilon_pareto_improved(candidate, previous)
)
```

`epsilon_pareto_improved(candidate, previous)` means at least one tracked objective improves beyond its epsilon threshold and no tracked objective regresses beyond its epsilon threshold. The default thresholds are:

- `score_epsilon = 0.0005`
- `ratio_epsilon = 0.005` for score components such as foundational_capability, competitive_capability, semantic_vector, performance, and stability
- `metric_epsilon = max(25ms, previous_metric * 0.03)` for raw timing metrics

This avoids rejecting a real case/rank improvement because a timing metric moved inside normal noise, and it avoids accepting a candidate that only wins through noise while silently regressing a protected objective. Foundational, competitive, semantic_vector, research_judge, performance, case, gate, and metric regressions are recorded as degradation feedback for the next Codex prompt. Positive score, research_judge, performance, case, gate, and metric improvements are also recorded and passed to the next Codex prompt so later iterations know what to preserve. Accepted optimization plans are also stored in each run record as `optimization_plan` and passed to the next prompt under `Recent adopted optimization plans to build on`.

The `chart` command writes:

- `.git/relay-knowledge-self-iteration/score.csv`
- `.git/relay-knowledge-self-iteration/score.svg`

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
- Multi-language repository retrieval targets cover relay-teams Python and
  JavaScript, LevelDB C++, and Linux C in the default profile; Kubernetes Go and
  Spring Framework Java remain in the exhaustive profile. The JavaScript, Java,
  C, and C++ cases intentionally include nested classes, macros, exported
  functions, caller/callee lookup, hybrid concept queries, and path/language
  filters to drive parser, identity, edge-finalize, FTS/BM25, and ranking-fusion
  improvements.
- Repository register-to-index performance targets in
  `cases/repository_index_performance_targets.json` tighten `index_budget_ms`
  and add combined `register_index_budget_ms` budgets. The evaluator records
  both `*_index_ms` and `*_register_index_ms` so self-iteration prioritizes cold
  indexing wall time after `repo register`, including batching, parser
  throughput, SQLite writes, finalize work, and incremental reuse.
- The built-in `semantic_vector_suite` writes a small evidence fixture into a self-iteration source scope, refreshes semantic/vector indexes, and verifies that query hits expose semantic/vector `retriever_sources`, available `backend_statuses`, and relevant ranking. When `RELAY_KNOWLEDGE_SEMANTIC_BACKEND=external` or `RELAY_KNOWLEDGE_VECTOR_BACKEND=external` is enabled, the evaluator inherits the runtime environment directly and runs `provider probe` first; provider URL, API key, model name, and dimension are not stored in cases or CLI flags.
- `research_judge_suite` runs only when judge environment configuration is present. It sends the candidate diff, deterministic evaluation summary, and selected 02/03/04 documentation excerpts to an LLM or coding-agent judge and emits the `research_judge` objective. This suite does not replace deterministic gates; it covers research-style and open-ended quality judgment.
- `/opt/workspace/relay-teams` full `scope=all` indexing and Python service, connector, eval checkpoint, and re-export queries.
- `/opt/workspace/linux` full `scope=all` indexing in the `exhaustive` profile, covering functions, syscall-style macros, exported symbols, includes, callers, callees, mmap flow, and epoll/eventfd retrieval.
- `/opt/workspace/linux` repeated full-repository initial indexing measurement in the `exhaustive` profile through the `linux_full` target.
- `/opt/workspace/leveldb` full `scope=all` C/C++ indexing and queries for class methods, free functions, headers, table cache, recovery, callers, hybrid lookup, and filters.
- `/opt/workspace/kubernetes` full `scope=all` Go indexing in the `exhaustive` profile for command constructors, kubelet flow, API types, clientset/generic clients, authorizers, informer imports, callers, hybrid lookup, and filters.
- `/opt/workspace/spring-framework` full `scope=all` Java indexing in the `exhaustive` profile for context, bean factory, WebMVC servlet/handler mapping, imports, and filtered lookup.

All repository targets must use `scope=all`. The evaluator rejects non-full scopes, and full-scope registration does not pass path or language filters to `repo register`; case-level filters remain available to test query filtering. Use `--profile smoke` for launcher validation without repository evaluation. Use `--profile exhaustive` when long-cycle Linux, Kubernetes, or Spring Framework full initial indexing gates should be run; these gates are intentionally outside the default profile so single-CPU self-iteration workers do not reject every candidate before actionable retrieval feedback is collected.
