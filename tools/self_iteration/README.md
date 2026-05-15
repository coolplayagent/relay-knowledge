# relay-knowledge self-iteration

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
7. Commits the candidate net change as one squash commit only when the strict improvement policy accepts it.
8. Restores the iteration start commit when the candidate is rejected.

If the worktree is dirty at startup, the loop exits immediately instead of
retrying the same non-retryable precondition failure.

## Scoring

The score is:

```text
accuracy * 0.55 + performance * 0.30 + stability * 0.15
```

Strict acceptance requires all quality gates to pass, a score above the best accepted historical score, no accuracy regression, and no key performance metric regression beyond 5%.

The `chart` command writes:

- `.git/relay-knowledge-self-iteration/score.csv`
- `.git/relay-knowledge-self-iteration/score.svg`

## Evaluation data

`cases.json` defines the default benchmark targets:

- `/opt/workspace/relay-teams` full repository indexing and representative code graph queries.
- `/opt/workspace/linux` full repository indexing and covering functions, macros, includes, callers, and callees.

Use `--profile smoke` for launcher validation without full repository evaluation. Use `--profile exhaustive` for future large-scope Linux experiments.
