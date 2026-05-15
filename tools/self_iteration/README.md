# relay-knowledge self-iteration

[中文](README.zh-CN.md) | English

This directory contains an independent Codex-driven optimization loop for code repository retrieval quality. It is intentionally outside the Rust crate and stores all runtime state under `.git/relay-knowledge-self-iteration/`.

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
4. Runs build, lint, tests, and repository retrieval evaluations.
5. Records a report under `.git/relay-knowledge-self-iteration/reports/`.
6. Appends scoring history to `.git/relay-knowledge-self-iteration/runs.jsonl`.
7. Appends the accepted optimization approach, changed files, metric improvements, and known degradations to `docs/zh/05-benchmarks/self-iteration-accepted-optimizations.md` before committing.
8. Commits the candidate net change and accepted-optimization record as one squash commit only when the previous-run improvement policy accepts it.
9. Restores the iteration start commit when the candidate is rejected.

If the worktree is dirty at startup, the loop exits immediately instead of
retrying the same non-retryable precondition failure.

## Scoring and acceptance

The score is:

```text
accuracy * 0.55 + performance * 0.30 + stability * 0.15
```

Acceptance uses an `epsilon-Pareto acceptance with hard constraints and weighted-score tie-breaker` policy. In multi-objective optimization terms, build/test gates and candidate diff existence are hard constraints, retrieval quality and latency observations are objectives, epsilon thresholds suppress measurement noise, and the weighted score is a tie-breaker rather than the only decision rule.

The candidate is accepted when:

```text
hard_constraints_pass
and (
  weighted_score > previous_weighted_score + score_epsilon
  or epsilon_pareto_improved(candidate, previous)
)
```

`epsilon_pareto_improved(candidate, previous)` means at least one tracked objective improves beyond its epsilon threshold and no tracked objective regresses beyond its epsilon threshold. The default thresholds are:

- `score_epsilon = 0.0005`
- `ratio_epsilon = 0.005` for score components such as accuracy, performance, and stability
- `metric_epsilon = max(25ms, previous_metric * 0.03)` for raw timing metrics

This avoids rejecting a real case/rank improvement because a timing metric moved inside normal noise, and it avoids accepting a candidate that only wins through noise while silently regressing a protected objective. Accuracy, case, gate, and metric regressions are recorded as degradation feedback for the next Codex prompt. Positive score, case, gate, and metric improvements are also recorded and passed to the next Codex prompt so later iterations know what to preserve. Accepted optimization plans are also stored in each run record as `optimization_plan` and passed to the next prompt under `Recent adopted optimization plans to build on`.

The `chart` command writes:

- `.git/relay-knowledge-self-iteration/score.csv`
- `.git/relay-knowledge-self-iteration/score.svg`

## Evaluation data

`cases.json` defines the benchmark targets:

- `/opt/workspace/relay-teams` full `scope=all` indexing and Python service, connector, eval checkpoint, and re-export queries.
- `/opt/workspace/linux` full `scope=all` indexing in the default profile, covering functions, syscall-style macros, exported symbols, includes, callers, callees, mmap flow, and epoll/eventfd retrieval.
- `/opt/workspace/linux` repeated full-repository initial indexing measurement in the `exhaustive` profile through the `linux_full` target.
- `/opt/workspace/leveldb` full `scope=all` C/C++ indexing and queries for class methods, free functions, headers, table cache, recovery, callers, hybrid lookup, and filters.
- `/opt/workspace/kubernetes` full `scope=all` Go indexing and queries for command constructors, kubelet flow, API types, clientset/generic clients, authorizers, informer imports, callers, hybrid lookup, and filters.
- `/opt/workspace/spring-framework` full `scope=all` Java indexing and queries for context, bean factory, WebMVC servlet/handler mapping, imports, and filtered lookup.

All repository targets must use `scope=all`. The evaluator rejects non-full scopes, and full-scope registration does not pass path or language filters to `repo register`; case-level filters remain available to test query filtering. Use `--profile smoke` for launcher validation without repository evaluation. Use `--profile exhaustive` when long-cycle Linux full initial indexing time should be repeated.
