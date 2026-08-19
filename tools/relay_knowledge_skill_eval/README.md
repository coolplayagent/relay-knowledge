# relay-knowledge CLI skill A/B evaluator

This standalone harness measures whether the published `relay-knowledge-cli`
skill improves software-engineering outcomes. Pi is only the controlled agent
runner. The evaluated variable is whether Pi receives the skill:

- baseline: `--no-skills`
- treatment: `--no-skills --skill /opt/pi-eval/skill/SKILL.md`

Everything else is fixed: Pi `0.80.3`, DeepSeek's official
`deepseek-v4-flash`, `high` thinking, the same problem statement, base commit,
tool set, timeout, runtime image, and pristine SWE-bench container. A stable
SHA-256 decision alternates which condition runs first for each instance.

Each condition has a one-hour overall agent deadline. A network, rate-limit, or
process interruption preserves the container, working tree, and Pi session, then
sends a continuation instruction up to three times inside that same deadline.
Ten minutes without output triggers the same recovery path. Continuations are
recorded in the trace and report.

An explicit intervention mode is also available. `--require-skill-use` adds a
treatment-only instruction requiring Pi to follow the loaded skill and execute
its bundled CLI. `--parallel-conditions` runs both conditions concurrently.
Together with `--concurrency 2`, this permits two baseline and two treatment
agents—and their isolated official scorer processes—to run at once. This mode
measures “skill plus mandatory-use instruction,” so its checkpoint must remain
separate from the default controlled protocol.

## Prerequisites

- Windows with PowerShell 7 and a running Linux Docker Desktop engine
- `uv`
- network access to Hugging Face, GitHub Releases, npm, Docker registries, and
  the official DeepSeek API
- a DeepSeek API key in the current process environment

The harness downloads the official `SWE-bench/SWE-bench_Verified` test split at
immutable revision `91aa3ed51b709be6457e12d00300a6a596d4c6a3`, requires exactly
500 unique rows, and verifies both downloaded and cached normalized JSONL against
SHA-256 `de1e478b9b64b2d69a46bfe329273f3dc56f201307cd6dd0055f8d9a4de98841`.
It also verifies the published
`relay-knowledge-cli-skill-v1.1.13.tar.gz` against the release checksum. It does
not build the skill binary from source.

## Commands

Run these commands from `tools/relay_knowledge_skill_eval`.

```powershell
uv sync
uv run relay-knowledge-skill-eval prepare --suite smoke-10
$env:DEEPSEEK_API_KEY = Read-Host -MaskInput "DeepSeek API key"
uv run relay-knowledge-skill-eval run --suite smoke-10 --concurrency 1
uv run relay-knowledge-skill-eval report
```

Run the four-way mandatory-use smoke variant:

```powershell
uv run relay-knowledge-skill-eval run `
  --suite smoke-10 `
  --concurrency 2 `
  --parallel-conditions `
  --require-skill-use `
  --agent-timeout 3600 `
  --max-continuations 3 `
  --stall-timeout 600 `
  --output-dir ..\..\.evals\relay-knowledge-skill\runs\1.1.13-forced-skill-parallel
```

After the 10-instance smoke run has produced 20 condition records, continue the
same checkpoint through the first 100 rows of the official Verified split:

```powershell
uv run relay-knowledge-skill-eval run `
  --suite verified-first-100 `
  --resume `
  --concurrency 2 `
  --parallel-conditions `
  --require-skill-use `
  --agent-timeout 3600 `
  --output-dir ..\..\.evals\relay-knowledge-skill\runs\1.1.13-forced-skill-parallel
```

To stop at the first 100 rows in the official Verified split while preserving the
same checkpoint and resume semantics, use `--suite verified-first-100`. The target
is 200 final condition results (100 baseline and 100 Skill).

The command skips existing records and stops at 200 final condition results.
Infrastructure failures remain retryable on resume; completed, timed-out, and
agent-error records are not silently rerun. Recoverable transport or process
failures that outlast the bounded in-session continuations are infrastructure
results; provider credential and model-configuration failures use the same
retryable classification so a corrected resume can replace them. A scorer outage
after an already-final Agent outcome does not change that outcome or cause the
Agent to rerun. When a hard timeout truncates the gzip footer before the capture
summary is written, report generation recovers token and tool counts from complete
bounded trace events while preserving the timed-out outcome. A run or explicit report
rebuild exits nonzero and keeps the report non-final while any retryable
infrastructure result remains. Use `verified-full` only when a 500-row run is
intentionally required.
Resume also verifies that every existing checkpoint row belongs to the selected
suite and that the new target is not smaller than the recorded result set. A
shrunk or switched suite is rejected instead of relabeling old results under new
run metadata.

Instance-image builds pin the Python build toolchain used by legacy benchmark
repositories so upstream packaging releases cannot break an unchanged task.
The same clone and package-install transformations are applied whether or not an
official build script starts with a shebang.
Both SWE-bench modules that cache image-build globals are redirected to the
evaluation cache, keeping setup scripts and logs out of the process CWD.
An image-build failure is checkpointed as retryable infrastructure failure for
that pair and does not terminate preparation or execution of unrelated tasks.
Official test-suite timeouts caused by a candidate are recorded as normal
unresolved results; scorer/container launch failures remain retryable infrastructure.
If a Windows scorer worker exceeds its outer deadline, the parent removes the
known run-scoped SWE-bench container before returning the retryable failure.

## DeepSWE A/B run

The DeepSWE runner consumes the 113 official Pier tasks from the pinned
`datacurve-ai/deep-swe` checkout. It runs one task at a time and starts that
task's baseline and Skill conditions together, so there is one active agent per
condition. A task's two official verifiers finish before the next task starts.
Both conditions use Pi 0.80.3, `deepseek-v4-flash`, high thinking, and a 3600
second agent timeout. Pier's outer timeout includes an additional five-minute
cleanup window so the inner Agent deadline can stop Pi and preserve partial work.
The Pier setup watchdog likewise exceeds the 900-second treatment pre-index
deadline by two minutes, covering both preceding 30-second container probes and
an additional minute of setup margin.
Treatment indexing is completed before agent timing begins.
DeepSWE resolves the same official release skill and full-SHA runtime image as
the SWE-bench runner; it builds that exact image when absent and passes the
content-addressed identity into every Pier environment.
The shared Pi runtime image is explicitly built for `linux/amd64`, matching the
x86-64 CLI asset and official task-container platform even on ARM64 Docker hosts.
Every Docker build receives a UUID-scoped runtime context that is removed in a
`finally` block, so concurrent cold-cache runs cannot overwrite one another.
Both conditions receive the same explicit engineering workflow: inspect the
repository, establish the target behavior, reproduce the issue when practical,
implement a minimal general solution, iterate on focused tests, check edge cases,
and review the final Git diff. The Skill condition adds only the mandatory
relay-knowledge investigation paragraph. This follows the workflow shape used by
the official DeepSWE `mini-swe-agent` runner while retaining Pi as the fixed agent
runtime.

```powershell
$env:DEEPSEEK_API_KEY = Read-Host -MaskInput "DeepSeek API key"
uv run relay-knowledge-skill-eval deep-swe-run `
  --output-dir ..\..\.evals\relay-knowledge-skill\runs\deepswe-113-pi-v4-flash-ab-1h
```

When `--tasks-dir` is omitted, the command creates or reuses the official
`datacurve-ai/deep-swe` checkout under the evaluation cache and verifies the
pinned commit `435ee89ec2f2e2289f33b0da4f992f0b7b7266b9` plus all 113 task
directories before starting. The dedicated cached checkout is reset and cleaned
before every reuse, preventing modified or untracked task inputs from being
reported as the pinned official corpus. An explicit `--tasks-dir` remains
available for a pre-provisioned official checkout, but it must point to the
root-level `tasks` directory of a clean checkout with the same official origin
and pinned commit. The harness validates those invariants without modifying the
caller-provided checkout.
Official HTTPS remotes are normalized with or without the conventional `.git`
suffix before comparison.

Each task has its own resumable Pier job containing exactly two trials. Pier
retries transient infrastructure failures, and the outer coordinator archives
still-failing infrastructure trials before a bounded retry. If all three
attempts still leave an infrastructure failure, the command exits nonzero
instead of publishing an incomplete run as successful. Pier retries only the
explicit DeepSWE transport exception; Agent timeouts, output limits, and
nonrecoverable Agent errors are final on their first attempt. Invalid provider
credentials or model configuration are infrastructure results for a corrected
resume, but are excluded from Pier's immediate retry loop. When Pier's host-side
exec deadline expires,
the harness terminates Pi's isolated in-container process group before committing
and collecting partial work, so a timed-out agent cannot keep editing during
grading. Missing or unreadable Pier trial results are
archived and retried instead of being mistaken for a completed pair. Pi traces,
prompts, treatment index logs,
patches, verifier output, reward files, token usage, cost, tool calls, and phase
timings remain under the run directory.
Before rewriting report provenance, a resumed run archives corrupt or already
classified retryable infrastructure state, then validates every surviving Pier
job against the requested agents and runtime image. A stale or incompatible job
therefore fails without replacing the provenance of the data already on disk,
while a truncated Pier result cannot prevent its own bounded recovery.
Validation-only Pier jobs close their log handlers immediately, before the real
job opens or archives the same files.
Transport classification is based on the current final attempt; a network error
from an earlier continued attempt cannot turn a later deterministic Agent error
into retryable infrastructure failure. An empty stderr artifact alone is not a
transport signal: only explicit transport markers or known transient process
exit codes consume continuations and become retryable infrastructure.
Persisted DeepSWE Pi trace and stderr output is redacted against both the exact
provider key and generic key-shaped values, then bounded to 64 MiB
per stream; exceeding either budget terminates that Agent condition as a final
agent error instead of exhausting host or container storage. The stderr bound
is enforced on raw input, including data arriving after an exact newline-aligned
boundary even when redaction makes the persisted artifact smaller. Token, tool, cost,
Agent-time, and pre-index metadata are populated from any completed summary
artifacts even when the Agent ends with a timeout, output limit, or error.
Successful repository queries remain valid across transport continuations in
the same Skill trial. Pier setup failures that occur before Agent execution is
created are classified as retryable infrastructure rather than model failures.
DeepSWE reports record the resolved release skill version and full SHA-256 used
to build the content-addressed runtime image; report rebuilds preserve that
recorded provenance.
The dependency lock pins Pier commit
`0daf53d3599e58c4506cf0bcff5e12c77dc282d2`, which includes native
`[[verifier.collect]]` support required by DeepSWE v1.1.

Report files use per-writer temporary files followed by atomic replacement, so
the runner and live watcher can refresh the same output directory concurrently
without sharing or deleting one another's partial temporary file.
Hosted-site upload failures do not advance the watcher's source signature; the
same snapshot is retried, including the final completed report. Reaching the
expected record count does not stop the watcher while any result is retryable
infrastructure; it stays alive to publish the same-key replacement from resume.
DeepSWE reports count infrastructure rows as recorded but not completed and do
not enable the final paired bootstrap until every expected row is infra-free.
Dashboard progress honors an explicit completed count even when it is zero,
instead of falling back to recorded condition rows.
Live report refreshes use a lightweight paired normal confidence interval.
The explicit final report rebuild and automatically completed runs compute the
10,000-sample paired bootstrap interval once, avoiding repeated bootstrap work
for every intermediate checkpoint.

Before a Pier job starts, the runner copies the pinned official task into the run
directory and restores LF endings for shell scripts and patch files. This
preserves the official content while preventing a Windows checkout from producing
an invalid Linux shebang such as `/bin/bash\r` or a patch that Linux Git cannot
apply inside the verifier container. Pier executes the official declarative
`[[verifier.collect]]` commands after the Agent exits. A missing `model.patch`
handoff is a retryable infrastructure failure, including when a verifier
otherwise returns a normal zero reward; a present zero-byte patch remains a
valid empty candidate.
An unexecutable verifier script is likewise retryable infrastructure failure; an
official test-patch conflict after applying candidate work is a final candidate
failure.

The official task's `tests` and `solution` directories remain on the host and are
not mounted into the agent container. `/logs/verifier` is an initially empty result
directory; the separate verifier writes it only after the agent has exited. The
task image also removes future Git history, remote refs, reflogs, and unreachable
objects before the agent starts.

Benchmark selection and outcome handling do not contain repository- or task-specific
branches. The fixed smoke selection is loaded from the packaged
`src/relay_knowledge_skill_eval/data/smoke-10.txt` manifest rather than duplicated
in Python. A held-out `test.patch` conflict is a final candidate failure and is not
retried as infrastructure; only environment failures such as an unexecutable
verifier remain retryable.

The combined dashboard reads the SWE-bench and DeepSWE reports without exposing
their artifacts. It only rewrites the page when one source report changes. The
browser checks the small response headers once per minute while visible and
downloads the HTML only after a change. Live progress excludes retryable
infrastructure rows until a final replacement is recorded.

```powershell
uv run relay-knowledge-skill-eval combined-dashboard `
  --swe-report <swe-run>\report.json `
  --deep-swe-report <deep-swe-run>\report.json `
  --output-dir <dashboard-directory> `
  --watch
```

The dashboard keeps benchmark totals and same-task averages visible together.
The potentially long per-task tables share a fixed-height area and are selected
with SWE-bench/DeepSWE buttons, so the page height does not grow with the dataset.

## Isolation and evaluation contract

Each instance and condition starts from a new official SWE-bench image and is
hard-reset to the dataset base commit. Pi receives only a JSON document containing
the `problem_statement` and generic work instructions. Reference patches, test
patches, hints, gold answers, and official grading metadata are never included in
the prompt.

Pinned dataset downloads use per-invocation staging files and an atomic publish,
so concurrent prepare or run processes cannot share a partially written cache.
DeepSWE trace capture enforces its byte budget while consuming bounded chunks;
an oversized single JSON or stderr line cannot be buffered without limit.
DeepSWE successful Skill trials enforce the same observed repository-query rule
as SWE-bench. Cold official task checkouts are staged uniquely and atomically
published so concurrent runners cannot observe a partial clone.

The treatment container is registered and indexed before Pi starts. Its durable
index lives only inside that condition's container, and Pi receives the same
`RELAY_KNOWLEDGE_HOME` so skill queries reuse that exact index. Pre-index time is recorded
separately and excluded from primary A/B agent time. Any indexing Pi chooses to
repeat after its timer starts remains part of agent time. Both benchmark runners
drain the durable index task with bounded status/worker attempts before launching
Pi; queued or retrying work cannot be mistaken for a completed pre-index. A
retrying task's durable `next_retry_at_ms` is honored before another worker claim,
so the bounded loop cannot exhaust itself by spinning ahead of backoff. Local
semantic and vector backends avoid a second hosted-model variable.
When mandatory Skill use is enabled, a completed treatment must contain a
successfully completed repository query from the bundled CLI. Starting a query
that later fails does not satisfy the contract. A treatment that only loads the
skill, ignores the instruction, or has only failed queries is recorded as an
Agent error and is not sent to the scorer. DeepSWE applies the same rule.

Pi's final working-tree `git diff --binary` against the immutable task base commit
is passed unchanged to the official SWE-bench harness, including changes already
committed by the Agent. Only the persisted patch artifact is redacted; scoring
never receives mutated diff bytes. The official harness determines patch
application, FAIL_TO_PASS, PASS_TO_PASS, and `resolved`; the agent cannot
self-report a pass. Patch collection uses a five-minute boundary and a 64 MiB
artifact budget before loading the diff into the host or scorer. A verifier pass
counts in pass-rate and paired statistics only when the Agent outcome completed;
timed-out or Agent-error candidates remain failures even if their partial patch
passes tests. Docker SDK clients used for image preparation and direct scoring
are closed after every boundary call.

Global Pi config, sessions, extensions, prompt templates, themes, and unrelated
skills are disabled or redirected into isolated container paths. The DeepSeek key
is forwarded to Docker by environment-variable name, never placed in a command
argument. Exact values and `sk-...` patterns are redacted from persisted output.

## Metrics and artifacts

The report contains paired pass rates, absolute delta, skill-only and
baseline-only passes, McNemar exact probability, and a deterministic paired
bootstrap interval. It also summarizes:

- input, output, reasoning, cache-read/cache-write and total tokens
- Pi-reported cost and request count
- image preparation, container startup, treatment pre-index, agent, scorer, and
  end-to-end time
- tool calls, tool errors, cumulative tool time, relay query command categories,
  automatic API retries, agent timeouts, infrastructure failures, and retry history
- empty patches, patch-application failures, FAIL_TO_PASS, and PASS_TO_PASS buckets

The dashboard's knowledge-query count uses the same accepted repository-query
kind set as mandatory-use enforcement, including software, feature-flag, and
impact queries.

Generated files are under `.evals/relay-knowledge-skill/`:

- `cache/`: immutable dataset, content-addressed local/release skill extraction,
  runtime build context, and SWE-bench image build inputs
- `runs/<version>/checkpoint.*`: append-only resumable records and immutable run
  signature
- `runs/<version>/artifacts/<instance>/<condition>/`: prompt, Pi JSONL trace,
  generated patch, and treatment index log
- `runs/<version>/official-scorer/`: official test output and instance reports
- `runs/<version>/report.{json,jsonl,csv,html}`: regenerated summaries

When a process is interrupted during the final JSONL write, resume discards the
incomplete trailing record before appending new results. A malformed record in
the middle still fails closed instead of silently skipping data.
SWE-bench continuation is limited to stalls, detected transport failures, and
explicit transient process exit codes; an ordinary nonzero Pi exit is a final
Agent error instead of retryable infrastructure.
Transport detection scans provider stderr plus explicit structured error events
from Pi stdout. Ordinary task and test stdout containing text such as `timeout`,
`429`, or `rate limit` cannot turn a deterministic Agent failure into retryable
infrastructure. A bounded-output breach remains a final Agent error even if Pi
exits with status zero while the harness is terminating the process. Live report
metadata publishes all checkpoint rows as `recorded_results` but counts only
non-infrastructure outcomes as `completed_results`.
The Pi runtime image tag includes the full skill SHA-256, so changing local skill
content cannot silently reuse an image built from older bytes.
Local skill caches are copied through a temporary directory and rehashed before
reuse, so interrupted or manually changed cache trees are rebuilt.
Release downloads and extraction also use per-invocation UUID staging paths and
atomically publish a validated skill tree, preventing cold-cache races between
parallel prepare or run processes.
Agent containers mount the runtime skill and CLI volume read-only; per-task index
state remains writable only in the task container's isolated `/tmp` directory.
SWE-bench Agent containers use an internal Docker network with no default egress.
A fixed-purpose TCP sidecar forwards only `api.deepseek.com:443`; DeepSWE declares
the same domain through Pier's network allowlist.
Prompts are transferred over stdin to avoid host command-line limits, then the
Linux wrapper passes the complete text as Pi's final message argument, matching
Pi's documented JSON-mode interface.
DeepSWE injects the API key when the task container is created through a Compose
environment placeholder. Later `docker compose exec` commands do not contain the
credential in their process arguments.

`live.html` is refreshed from the checkpoint while a run is active. To mirror the
dashboard to a hosted Sites instance, run the watcher with `--site-url` and expose
the write-only token through `EVAL_SITE_INGEST_TOKEN`. The uploaded snapshot is
recursively allowlisted: prompts, traces, patches, scorer log paths, local paths,
repository commit, and errors are not sent even inside nested result objects.

```powershell
$env:EVAL_SITE_INGEST_TOKEN = Read-Host -MaskInput "Sites ingest token"
uv run python -m relay_knowledge_skill_eval.live_dashboard `
  --output-dir <run-directory> `
  --expected-results 20 `
  --site-url https://<site-hostname>
```

The watcher exits after all expected executions are present. Full Pi JSONL traces
remain local as `pi-trace.jsonl.gz`; the gzip stream is flushed after every event
so an active run is observable without waiting for process exit.

The checkpoint signature records the dataset hash, skill hash/version, repository
commit, Pi/model/thinking settings, runtime image, image prefix, and timeouts. A
resume with a different signature fails instead of mixing incomparable records.

## Recorded evaluation snapshot

The completed 2026-08-13 mandatory-use run used Pi `0.80.3`,
`deepseek-v4-flash`, high thinking, and a 3600-second agent deadline. The
SWE-bench Verified first-100 result was 78/100 for baseline and 82/100 for
Skill (+4 percentage points; 95% CI -3 to +11 points; McNemar p=0.424).
The full 113-task DeepSWE result was 46/113 for both conditions (40.7%; 95% CI
-9.7 to +9.7 points; McNemar p=1.000). Both reports reached their expected
result counts with zero infrastructure failures. DeepSWE Skill used 5.0% fewer
total tokens, 3.8% less reported cost, and 4.9% less agent time; SWE-bench Skill
used 23.6% more total tokens, 20.9% more cost, and 17.1% more agent time.

The full tables and interpretation are recorded in
`docs/zh/05-benchmarks/13-cli-skill-swebench-ab-evaluation.md`. These numbers
measure the explicit `--require-skill-use` protocol, not the isolated effect of
merely making a skill available. Raw traces and scorer artifacts remain in the
gitignored evaluation archive rather than Git.

## Development checks

```powershell
uv run ruff format --check .
uv run ruff check .
uv run pytest
```

Tests use fake Docker, Pi, relay indexer, and SWE-bench boundaries to cover trace
parsing, timing segmentation, A/B command equality, redaction, checkpoint recovery,
timeouts, incomplete JSONL, empty and over-budget patches, successful mandatory
queries, Docker client cleanup, scorer failure, reporting, and smoke-to-full
resume behavior without consuming API credit. DeepSWE Agent behavior and Pier
recovery coverage live in separate test modules so each remains below the
repository's 1,000-line hard cap.
