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
| `--profile fast` | You want the default local loop or pre-PR check | Runs formatting, a release product-binary build, harness check, key product gates including hierarchical BM25 and bounded code-index persistence invariants, the default repository subset, repo-set guards, and a semantic/vector guardrail. |
| `--profile full` | You need complete product and harness rails | Restores release builds, clippy, tests, the named hierarchical BM25 gate, local file fixtures, full repository evaluation, semantic/vector fixtures, and the research judge. |
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

### Source Ownership

The evaluator root is a declaration-only facade for `evaluate_candidate` and `EvaluationRun`; all behavior and tests live with their owners. Runtime separates contracts, bounded concurrency, reporting, finish serialization, and top-level orchestration. Workloads depend on the lower runtime services, while orchestration composes them without a reverse dependency through the evaluator facade. Evaluator quality-gate contracts live at the `quality` domain root, while policy and execution are explicit owner modules with direct tests. The research judge is likewise a real module tree: its shared input contract is rooted in `judge`, and evaluation composes independently tested settings, prompt, backend, and outcome owners. Workload execution is split into explicit agent, CLI, file, repository, repository-set, selection, and semantic-vector modules; shared case scoring has its own owner, and each behavioral source attaches its sibling tests directly. Fixture source families, repository assembly, and file writing are also real owner modules with direct tests; generated agent-workflow source constants live there rather than in workload execution. Config, scoring, evaluator, workflow, nested unattended stages, cases, process adapters, history, and progressive memory use real Rust modules with no production or test `include!` assembly. Unattended operation is nested under `workflow` so it consumes workflow services without a top-level module cycle.

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
| `--model MODEL` | `gpt-5.6-sol` | Codex model for candidate generation. |
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
codex -a never exec --dangerously-bypass-approvals-and-sandbox -s danger-full-access -C /opt/workspace/relay-knowledge -m gpt-5.6-sol -c 'model_reasoning_effort="xhigh"' -
```

Use it only in an externally trusted workspace. Candidate generation defaults to `gpt-5.6-sol` with `model_reasoning_effort="xhigh"`; override with `--model` and `--codex-reasoning-effort low|medium|high|xhigh` when a run needs a cheaper or different generation mode.

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
| Basic gates | Product and harness `fmt --check`, Linux GNU glibc 2.28 baseline policy gate, `cargo build --release --bin relay-knowledge`, and harness `cargo check`. |
| Product gates | `skill_metadata_policy_cases`, `business_knowledge_regression_cases`, `code_index_recovery_cases`, `code_index_health_isolation_cases`, `code_index_sqlite_lock_cases`, and CLI contract cases for index-worker plus typed CodeSpec/Knowledge maps. |
| Default repositories | `index_performance_many_files`, `index_performance_c_fragment`, `c_syntax_fixture`, `cpp_syntax_fixture`, `cross_language_syntax_fixture`, `typescript_syntax_fixture`, `nonstandard_layout_fixture`, `software_global_fixture`, `project_alias_fixture`, `relay_teams`, `leveldb_cpp`, `temporal_samples_go`, and `temporal_sdk_go`. |
| Default sampling | First 8 normal query cases per repository, while always preserving explicit `guardrail=true` cases. |
| Repository sets | 2 cross-repository threshold cases from `temporal_go_workspace`. |
| Semantic/vector | 1 guardrail query. |
| Coding-agent workflows | Skipped by default in `fast`; run with `--categories agent_workflows` or by the PR benchmark workflow. |
| Runtime state | Every evaluation uses a fresh `.git/relay-knowledge-self-iteration/work-v2/<run-id>/home/`; mutable database and generated-fixture state is never reused across runs. `fast` still reuses compiled Cargo artifacts plus history/baselines, but its repository latency is a cold measurement and remains subject to the declared key budgets. |

Every non-`smoke` profile runs repository workloads with `target/release/relay-knowledge`; a debug harness may still orchestrate that release product binary. The evaluation report records `product_binary_profile` and `product_binary_path`, and workload previous/best history plus the profile-wide hard acceptance floor are compared only within that product-binary profile. Legacy records without the field retain their historical meaning (`fast=debug`, other profiles `release`), so earlier debug-fast scores and timings cannot reject or become a baseline for a release-fast candidate. Comparison metadata labels this hard floor `evaluation_profile_and_product_binary_profile_acceptance_floor`; it also reports the best score across product-binary profiles as `evaluation_profile_diagnostic_only`, which never participates in acceptance. `smoke` runs formatting only and neither builds nor executes a product workload.

`fast` does not run full clippy, full tests, local file fixtures, the research judge, or a release build of the harness itself by default. `full` and `exhaustive` restore those rails and run complete repository evaluation, repository-set cases, local file fixtures, semantic/vector fixtures, and the research judge.

Key fast guardrail responsibilities:

| Guardrail | Protects |
| --- | --- |
| `skill_metadata_policy_cases` | Rejects Windows commands or asset examples in bash/POSIX code fences so agent-facing instructions stay shell-specific. |
| CLI contract cases | Verify agent-visible help exposes `repo index-worker`, idle worker plus streaming worker output parseable JSON, and typed CodeSpec/Knowledge map help, validation, directory filtering, and business routing. |
| `code_index_recovery_cases` | Cover expired task lease recovery, stale worker completion rejection, attempt-budget dead-lettering, checkpoint-batch lease renewal, renew-before/after boundaries for every durable finalization step, bounded finalization-step derivation, query-index subphase resume at the next unit, and rejection of stale caller-observed renewal time after writer-lock acquisition. Its `code_index_task_` structural cases also freeze the v3 17-slot plan and grouped reference-search v2: cleanup/discover/build page counts, occurrence-to-group aggregation, exact manifests, rollback/reopen replay, fair full occurrence expansion, and leased v1 restart with v2-clamped budgets. Retired query-index unit 1 is not recreated or dropped, existing same-name shape stays strict, v1/v2 cursors cannot skip physical unit 1 and retain their token versions across writer quanta, and every fresh Restart precreates only chunk units 13/14 on an empty owner while deferring all other heavy indexes even for a one-path session. This gate runs for every non-smoke profile, including fast and performance-focused evaluations. |
| `code_index_persistence_performance_suite` | Runs as an isolated `fast` stage with a 120-second timeout and a 30,000-ms key budget. Direct owner and SQLite trace tests require 1,025 references, symbols, and chunks to use two bounded base statements apiece at the default 1,024-row ceiling; runtime variable-limit tests enforce each owner's exact one-row floor, and rollback/replay tests retain checkpoint, staging, FTS, and fence ownership. Search-document trace and EQP checks cross the runtime-clamped 1,024-document flush boundary above a high raw orphan, require exactly two main FTS inserts and one equality-constrained `INT64_MAX` point probe per flush, enforce 12/6/5-variable two-row/one-row/reject boundaries, and reject any constructor or flush `max(rowid)` aggregate while preserving exact post-insert FTS/metadata intervals. Grouped reference-search tests prohibit nullable-range SQL, require indexed first/continuation keysets, prove that lazy length-only scans reject an oversized cursor before payload fetch, require each admitted page to point-fetch only its final durable cursor, and use SQLite VM-step measurement to prove that returning UPSERT removes the repeated discovery-page grouped scan without changing page caps. Its build-page trace also requires 1,025 admitted groups to use one ordered main FTS `INSERT ... SELECT` and one metadata insert, while canonical blank-field content and prewrite `INT64_MAX` rejection remain protected. Ordinary `finalizing:resolve_references:v1` tests exercise multi-row keyset pages, two-control-row/full-record byte accounting, length-only owner probes with a per-page name/path cache, exact budget boundaries, rollback/reopen/fence replay, and VM work that stays independent of a hot symbol tail. A 1,025-row call-only page must perform no payload point-fetch or owner update, advance its exact count/cursor, and point-fetch only the final cursor; dedicated call-target tests retain stale-binding validation. |
| `code_index_health_isolation_cases` | Verify health queries stay bounded during no-language-filter repository updates, and `repo query --freshness allow-stale` can read the latest committed scope. |
| `code_index_sqlite_lock_cases` | Protect duplicate-process SQLite lock avoidance, active-task reuse, and concurrent claims for distinct task fingerprints. |
| `bm25_hierarchy_build` | Compiles and links the exact `--lib --all-features` test target with `cargo test --no-run` in a dedicated stage. Its 1,200-second hard timeout matches the existing root Rust-gate ceiling and covers a clean cold build without prewarming or unbounded waiting. This preparation gate has no latency budget and contributes no BM25 performance claim. |
| `bm25_hierarchy_suite` | Runs in the next dedicated stage, after the Cargo build lock is released, with its existing 120-second execution timeout and 30-second non-key whole-suite diagnostic budget. The metric therefore covers Cargo freshness validation plus the 50 deterministic product tests, not a cold compile/link; exceeding 30 seconds affects diagnostics/scoring but is not itself a hard gate failure. Those tests protect the `simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4` contract: same-v4 routed/flat score parity; a synthetic 4,096-document production-write/query-path fixture with Recall@10 of at least 0.9 and a planned-MATCH result-domain reduction from 768 to 448 rows; selected-document/coarse-score bounds; one `graph_bm25 MATCH` intersecting business terms, a zero-weight scope64 token, and scope-qualified group tokens while SQL scope remains authoritative; hidden rank with rowid-sidecar hydrate; bounded persisted-DF probes; version-leading unscoped historical indexes; observable oversized-label degradation and fuzzy-posting bounds; and a resumable shadow rebuild with durable owner/expiry, phase/cursor, semantic/vector plan, 128-document/4-MiB/8,192-label/8,192-link transaction budgets, isolated oversized-document warnings, companion-read pause, fencing, swap, and rollback. Removing an invariant fails without wall-clock timing. The 448/768 result-domain invariant is not a posting-scan, VM-step, or query-latency measurement; the gate does not establish natural-corpus recall/performance, deterministic equal-score cutoff membership, or end-to-end bounds for the whole hybrid pipeline. |
| Syntax and layout fixtures | Protect external-import unresolved metadata, C/C++ recoverable parser errors, non-top-level `src/` layouts, project aliases reusing one indexed scope, and source/text fallback guardrails. |
| `software_global_fixture` | Ensures `repo software` projections come from indexed evidence, not package caches, cloud APIs, SDK directories, or unindexed external source; its legacy inline Knowledge map must also retain required timestamp/history metadata so compatibility indexing reaches the topic projection. |
| `business_knowledge_regression_cases` | Runs in every fast evaluation and protects acronym/alias resolution, cross-domain homonym ambiguity, competing-definition retention, mapping resolution/unresolved hints, route authorization, and the business publication barrier. |
| `agent_workflow_fixture` | Replays coding-agent issue-analysis tasks over generated Rust, TypeScript, Python, YAML, and Markdown evidence, with budgets for tool calls, source reads, output/context size, evidence count, fallback ratio, and total latency. |

Software lifecycle projection filters ordinary source chunks in SQLite before Rust materialization by using a semantic superset of the supported manifest, CI/IaC, and Markdown paths. It preflights fixed ceilings of 32,768 candidate documents, 262,144 chunks, and 256 MiB, streams one path-ordered document at a time through the build/IaC/design collectors, and reports candidate document, chunk, and byte counts. Component, dependency-usage, SDK, build, IaC, and design writes reuse prepared statements. For a fenced single-SQLite full or incremental index, the new code scope remains stale and the checkpoint remains in `finalizing:software_projection` until software facts are ready; after revalidating the fence, software status, code-scope/repository freshness, checkpoint completion, and the publication receipt become visible in one SQLite transaction. A partitioned store does not claim cross-database atomicity: code and software facts complete in the target shard while the catalog route remains task-owned and `staged`; one fenced control-database transaction then validates that owner, activates the repository and scope routes, mirrors fresh status, and records the receipt. Public checkpoint state stays pre-publication until that active control route exists. Both pre-control and post-control crashes converge idempotently without re-parsing an already durable target. Task `succeeded` is a subsequent fenced completion transaction that requires the receipt and matching fresh target, and the external worker response still waits for that terminal task state.

The general library-test rail separately verifies the 256-label-per-document, 1,024-byte-per-label, and 8,192-gram-per-document fuzzy-index limits and that request-level disabling skips every graph-search source family. Those tests are not part of the name-filtered `bm25_hierarchy_suite` fast gate.

Override the default subset with:

```bash
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=index_performance_many_files,index_performance_c_fragment,c_syntax_fixture,cpp_syntax_fixture,cross_language_syntax_fixture,typescript_syntax_fixture,nonstandard_layout_fixture,software_global_fixture,project_alias_fixture,relay_teams,leveldb_cpp,temporal_samples_go,temporal_sdk_go
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT=12
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS=temporal_go_workspace
RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT=2
```

`full` and `exhaustive` also run `index_performance_wide_mixed_files`, which generates 2048 Rust target files and cross-shard bridge queries. Its post-finalize guardrail includes a real `references` lookup for `rk_wide_target_2047` on the bridge path; the fixture contains two occurrences with the same grouped identity, while focused storage tests require complete deterministic expansion. Every repository index is followed by a read-only `repo scope preview` at the pinned ref. The preview's exact `selected_file_count`, repository/alias, requested and resolved refs, tree hash, and path/language filters must match the task, summary, checkpoint, scope, and fresh status returned by indexing; `task.state=succeeded`, `checkpoint.state=completed`, and exact committed/status counts are mandatory for a cold full index. The harness also records the pinned parent Git tree count as independent diagnostics, but does not treat it as an upper bound because the authorized product scope can expand available gitlink/submodule contents. That Git observation runs through the evaluation's global command limiter with a 120-second ceiling; a plain filesystem source records `source_kind=filesystem` and no raw Git count instead of failing. A declared `expected_file_count` is preserved and conflicts with the selected count fail instead of being overwritten. Generated performance repositories then create a second Git commit with modified, added, and deleted files and run `repo update`; incremental completion requires a succeeded task plus exact summary/scope/status count and identity while retaining its changed-path/blob-read/parse budgets. A completed incremental response may legitimately omit `checkpoint`; if the fenced durable-clone path returns one, it must be completed, belong to the exact target scope, and prove the full selected target count. Its task-bound `incremental_summary` receipt, rather than the scope-wide checkpoint counters, must exactly preserve the base identity and changed-path/blob-read/parse delta metrics. Checkpoint payloads expose repository and scope identity but no commit or tree fields, so commit/tree identity is enforced across scope, task, summary, and status. Reports record `*_cold_index_ms`, `*_cold_register_index_ms`, `*_incremental_index_ms`, and query p50/p95/max metrics.

All fenced clean incremental runs use the durable clone protocol, so the generated performance repositories exercise the same recovery path even when their base would fit a direct transaction. The task-bound receipt must preserve the reported delta metrics across clone-to-`indexing` finalization and response loss, while a later task adopting the same content returns the established neutral no-work summary. Benchmark CI keeps the `index_performance_many_files` rail at 3,000 ms and requires exactly three changed paths, two blob reads, two parsed files, succeeded task/completed checkpoint, and the named persistence gate. Full/exhaustive keeps `index_performance_wide_mixed_files` at 5,000 ms with the same two-read/two-parse ceiling. A missing or zero legacy fact proof may use typed full staging, but it cannot be counted as an incremental pass or target write.

For a declared `index_only_performance_target`, a successful `<repository>_cold_index_completion` validation adds `cold_index_result` to that repository report. It is the exact raw cold `repo index` JSON and retains `scope`, `task`, `summary`, `checkpoint`, and `status`, so a zero-retrieval-case target still carries independently auditable completion, freshness, counts, and identity evidence. Ordinary repository reports keep the existing `index_summary` schema and omit `cold_index_result`; an index-only target also omits it when strict cold-terminal validation fails. The bilingual elastic-budget benchmark contract contains the final `jq` acceptance assertion.

Isolation is a within-run measurement and disk-lifetime boundary, not a substitute for shared-state coverage. A repository configured with `isolated_index_home=true` receives a child home under the unique run home and is removed after its commands, cases, metrics, and in-memory report are collected unless `--keep-workdirs` was requested; cleanup also runs on evaluation errors. Creation and recursive cleanup require every run/isolation/home component to be a non-symlink directory with canonical direct-parent containment. Repository-set members are rejected if they request isolation because their overlay must read all members from the evaluation's common home. The small LevelDB and OpenTelemetry set workloads remain shared within one fresh run to retain ordering and overlay coverage without carrying state into another run.

`.github/workflows/benchmark-checks.yml` runs the 1024-file performance fixture on pull requests and first asserts that the JSON report selected the release product binary at `target/release/relay-knowledge`. It then verifies the completed cold task/checkpoint, the three-path incremental delta, the two-file blob/parse budget, completion commands, and all three latency budgets directly from that report.

### Coding-Agent Workflow Gate

`--categories agent_workflows` runs deterministic end-to-end coding-agent scenarios from `cases/agent_workflow_targets.json`. The fixture covers definition lookup, one-call `repo context` packing, cross-language impact tracing, configuration-to-documentation tracing, and freshness policy checks. Each scenario executes bounded `repo query` or `repo context` steps and fails when expected evidence is missing, context/output grows beyond the case budget, too many unique source files must be read, text fallback dominates the evidence pack, too many tool calls are needed, or total query latency exceeds the threshold.

The PR benchmark workflow runs this category as `agent-workflow-regression` with the generated fixture isolated through `RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS=agent_workflow_fixture`. After the evaluation run it requires the exact four `(repository, case_id)` observations without duplicates, verifies that the category was selected rather than skipped, requires agent metrics to be present, and fails when any gate, case, or agent workflow metric budget fails. Empty observations therefore cannot pass vacuously; the score-vs-history adoption decision is not used for this CI gate. This keeps the CI cost bounded while still exercising the agent-facing behavior.

### Category Focus

`--categories` evaluates explicit guardrail cases plus selected category cases; guardrail failures become quality-gate failures and reject the candidate even when the focused score improves. `--categories semantic_vector` runs the full semantic/vector suite while preserving repository and repo-set bottom-line cases. `--categories performance` keeps repository, repo-set, semantic/vector, and file-fixture workloads that emit performance metrics instead of reducing the run to guardrails only. Score history is isolated by profile and category focus, and acceptance also checks the best committed run for the same profile across category focuses so a new category cannot be accepted below the established profile bar.

### Parallelism Boundaries

Parallelism defaults to `--jobs auto`, `--repo-jobs auto`, and `--query-jobs auto`. `auto` uses the available CPU count for the global command limiter and query pool, and half the available CPU count for repository jobs. All repository register/index and repository-set create/add/refresh writer commands in one evaluation share its writer lock, including commands that use isolated homes. Separate harness processes operate on separate run-scoped homes, so they do not need a cross-process mutable-database lock. This bounds per-run disk and I/O pressure and keeps cold latency from being distorted by concurrent writers; query subprocesses can run concurrently after writer boundaries. Command completion uses the operating system's child-exit notification with the remaining timeout/progress deadline, so reported query latency is not rounded up by the former 20 ms polling interval.

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

`performance` uses `budget_relative_v2`. If no compatible previous run exists, metrics use their budget-normalized score. For a lower-is-better metric, every non-negative value at or below its budget receives full budget-fit credit, including zero and fractional ratios such as `text_fallback_ratio`; only an over-budget value uses the bounded `budget / value` ratio. Higher-is-better budget fit uses `value / budget`. With a compatible previous run, relative progress uses `previous / current` for lower-is-better and `current / previous` for higher-is-better, bounded to `1.25`; equal values, including `0 == 0`, are neutral at `1.0`, while a positive-to-zero improvement (or zero-to-positive higher-is-better improvement) receives the upper bound. The final metric score blends 70% budget fit and 30% relative progress.

### Acceptance Policy

Acceptance uses an epsilon-Pareto policy with hard constraints and a weighted-score tie-breaker. Build/test gates, candidate diff existence, and every current key metric's declared budget are hard constraints. A key lower-is-better metric fails only above its budget; a key higher-is-better metric fails below its budget. Non-key metrics remain scoring and diagnostic signals rather than hard constraints. Foundational_capability, competitive_capability, semantic_vector, stability, and latency observations are protected objectives; epsilon thresholds suppress measurement noise; the weighted score breaks ties rather than acting as the only decision rule.

A candidate is accepted when:

```text
hard_constraints_pass
and no_current_key_metric_budget_failure
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

`bug_fix_priority_improved` means the candidate fixes an observed program failure by turning a previously failed quality gate into a passing gate or a previously failing evaluation case into a passing case. It can override the weighted-score tie-breaker, the profile-level best committed score bar, and raw timing degradation, but it cannot override missing diffs, current gate failures, current key metric budget failures, or protected-objective regressions. Each key metric rejection lists the metric name, observed value, and declared budget in `reject_reasons`.

Default epsilons:

| Threshold | Default | Used for |
| --- | --- | --- |
| `score_epsilon` | `0.0005` | Overall score comparison. |
| `ratio_epsilon` | `0.005` | Score components such as foundational, competitive, semantic_vector, performance, and stability. |
| `metric_epsilon` | `max(1e-9, 0.03 * max(abs(previous), abs(current)), min(25, 0.03 * budget))`; omit the budget term when no budget exists | Symmetric raw-metric change detection. The declared budget supplies a unit-aware noise floor, the two observations supply a continuous relative scale, and crossing `1.0` never changes formulas. |

Regressions are recorded as degradation feedback for the next Codex prompt; positive improvements are also passed forward so later iterations know what to preserve. Accepted optimization plans are stored in each run record as `optimization_plan` and passed to the next prompt under `Recent adopted optimization plans to build on`.

## Evaluation Data

`cases.json` and its `include_files` define the self-improvement workload. The root file owns the bounded manifest and global suites; base repository query targets are split into descriptive project-alias, relay-teams, Linux, LevelDB, Spring Framework, and Kubernetes include files. They are not merely a list of capabilities that already work; new cases may represent competitive targets that future candidates must complete. Candidates should improve general parser, graph-edge, candidate-pruning, ranking, service workflow, or observability behavior instead of deleting, weakening, or enumerating cases.

### Generated and Local Fixtures

| Group | Coverage |
| --- | --- |
| Local file-index fixtures | Generate deterministic temporary roots for user documents, Linux `/opt`-style paths, Windows `D:`-style paths, deep directories, and high-noise file sets; run `files index/query`; record `file_index_ms`, `file_query_p50_ms`, and `file_query_p95_ms`. |
| C/C++ syntax fixtures | Generate temporary git repositories and run `repo register/index/query`; cover function pointer typedefs, operation tables, initializers, macros, local includes, callback dispatch, namespaces, templates, overrides, operators, lambdas, aliases, and header/source split. Design notes live in `docs/en/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md`. |
| Cross-language syntax fixture | Covers C calling C++, C++ calling C, Go cgo calling C, and Rust FFI calling C so default fast runs can validate multi-language call graph retrieval without another large checkout. |
| Additional multilingual fixtures | Cover Python, JavaScript, TypeScript/TSX, Go, Java, Rust, Bash, C#, Kotlin, PHP, Ruby, Scala, and Swift; the matrix is documented in `docs/en/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md`. |
| Repository-set targets | Register each member as a full `scope=all` repository, create an explicit `repo-set`, refresh cross-repository overlays, and run `repo-set query`; cases can require member, source scope, path, line, and excerpt evidence. |
| Cold and incremental index performance targets | `repository_index_performance_targets.json` sets cold `index_budget_ms`/`register_index_budget_ms`, incremental `incremental_index_budget_ms`, completion evidence, and delta read/parse caps; default fast includes a 1024-file fixture, while `full` and `exhaustive` also include a 2048-file wide fixture. |
| Hierarchical BM25 algorithm gate | `fast`, `full`, and `exhaustive` first run the non-budgeted `bm25_hierarchy_build` preparation gate with a bounded 1,200-second cold-build timeout, then run `bm25_hierarchy_suite` alone with its unchanged 120-second timeout and 30-second non-key diagnostic budget. Its fixed SQLite fixtures assert the v4 fingerprint and scope partition, same-schema flat parity, the synthetic production-write/query-path Recall@10 >= 0.9 floor, planned-MATCH result-domain reduction, hard SQL authorization, single-FTS hidden-rank/rowid-hydrate shape, persisted-DF and 65,536-posting admission bounds, route-document `fts_rowid`/version/label-state invariants, version-leading global fallback indexes, observable oversized-label degradation and 8,192-posting exhaustion, durable checkpoint takeover, all four rebuild work budgets, oversized-document isolation and bounded warning identity, current-writer fencing, companion-read pause, complete-reader activation, and swap rollback. Reports retain build preparation separately from the whole-suite duration and captured `BM25_WORK`; none is query latency or FTS posting/VM-step work, and equal-score cutoff membership plus natural-corpus or whole-pipeline claims remain outside this synthetic gate. |
| Software global projection targets | `repository_software_global_targets.json` runs `repo software` for dependencies, sdks, files, topics, relationships, build, iac, design, and all projection kinds, with facts derived only from indexed evidence. |
| Framework graph targets | `repository_framework_targets.json` runs the independent `repo framework` surface against pinned official Angular and Vue repositories. Cases score both graph nodes and edges and enforce the declared cold-index, p50, and p95 budgets. |
| CLI contract cases | Run product CLI commands without indexing a large repository; default fast covers `repo index-worker` help/idle/streaming JSON plus typed CodeSpec/Knowledge map help, validation, directory filtering, and business routing. |
| Semantic/vector suite | Writes a small evidence fixture, refreshes semantic/vector indexes, and verifies `retriever_sources`, `backend_statuses`, and relevant ranking; external providers are inherited only from the runtime environment. |
| Research judge suite | Sends candidate diff, deterministic evaluation summary, documentation excerpts, competitive targets, and implementation guardrails to an LLM or coding-agent judge; it does not replace deterministic gates. |

Multi-language repository retrieval targets are split by language under `cases/repository_*_targets.json` so each language can evolve independently. Language cases cover real `symbol`, `definition`, `references`, `callers`, `callees`, `imports`, and `hybrid` scenarios for functions, methods, classes, exported values, macros, includes/imports, callback or trait relationships, and execution flows. Relationship targets are split into regression and challenge groups; challenge cases use `expected_all` or `expected_sequence` to keep ranking and coverage improvement room even after they pass. A broad high-fanout relationship query must assert the edge kind, resolution state, target hint, retrieval layer, or evidence surface shared by every valid equivalent result; it must not require one arbitrary importer path unless the request itself supplies path or importer context. A context-bearing challenge may require that importer, edge, and evidence properties occur on the same hit, while a separate scoped regression case can lock the direct filtered lookup. The contextual importer term must remain after removing the imported target and local-binding identity; a target-only FQN cannot claim importer context.

### Real Repository Targets

Full-scope external repositories set `isolated_index_home=true` when their size or cold-index contract makes ordering material. `relay_teams`, `opencode_typescript`, and the exhaustive repositories below are isolated and cleaned per repository. LevelDB is the bounded shared-order regression. Temporal and OpenTelemetry members must remain non-isolated because each repository-set overlay depends on a common runtime home. Isolated cold-index latency and shared preload/order behavior are separate signals; neither result can stand in for the other.

| Repository | Profile | Target |
| --- | --- | --- |
| `/opt/workspace/relay-teams` | default | Python service, connector, eval checkpoint, and re-export queries. |
| `/opt/workspace/opencode` | default | TypeScript/TSX monorepo queries for symbols, references, overloads, exported constants, TSX components, caller/callee edges, relative imports, `@/` and `~/` aliases, HTTP recorder redaction flow, LLM protocol streaming flow, and negative symbol lookup. |
| `/opt/workspace/leveldb` | default | C/C++ classes, free functions, headers, table cache, recovery, callers, hybrid lookup, and filters. |
| `/opt/workspace/temporal-samples-go`, `/opt/workspace/temporal-sdk-go` | default | Full-scope Go indexing plus repository-set API usage from Temporal samples to the SDK. |
| `/opt/workspace/opentelemetry-collector-contrib`, `/opt/workspace/opentelemetry-collector` | default | Full-scope Go indexing plus contrib-to-core receiver factory and component type usage. |
| `/opt/workspace/angular`, `/opt/workspace/vue` | default | Pinned official Angular layout and Vue SFC playground scopes covering components, rendered selectors, props, and template variables through the framework graph. |
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

Prepare every fixed-ref repository from a clean destination with the exact
commit recorded in `cases.json`. This Bash recipe fetches only that commit,
checks it out in detached-HEAD state, and fails if `HEAD` does not match the
configured SHA:

```bash
set -eu

clone_pinned_repository() {
    repository_url=$1
    destination=$2
    commit=$3

    test ! -e "$destination"
    git init --quiet "$destination"
    git -C "$destination" remote add origin "$repository_url"
    git -C "$destination" fetch --quiet --depth 1 origin "$commit"
    git -C "$destination" checkout --quiet --detach "$commit"
    test "$(git -C "$destination" rev-parse HEAD)" = "$commit"
}

# Default-profile multi-repository fixtures.
clone_pinned_repository https://github.com/temporalio/samples-go.git /opt/workspace/temporal-samples-go 231564bebe0be78e78233ef14992158c623d1e86
clone_pinned_repository https://github.com/temporalio/sdk-go.git /opt/workspace/temporal-sdk-go ff47f19909ac85aacff89645360de0dba6f6f898
clone_pinned_repository https://github.com/open-telemetry/opentelemetry-collector-contrib.git /opt/workspace/opentelemetry-collector-contrib 84fe8df16c34efbb7e929310c955df8f4861d2f4
clone_pinned_repository https://github.com/open-telemetry/opentelemetry-collector.git /opt/workspace/opentelemetry-collector 31e51520f30fc5c4362949e41307ea57b7b45a9d
clone_pinned_repository https://github.com/angular/angular.git /opt/workspace/angular 133cafda42028fbd8efd7840d6ff3fea25223166
clone_pinned_repository https://github.com/vuejs/core.git /opt/workspace/vue d63616ca17de965ed32dcb449a4c5cd9982f15d2

# Exhaustive-profile tree-sitter language repositories.
clone_pinned_repository https://github.com/nvm-sh/nvm.git /opt/workspace/nvm 53855417eb66b9c35b732ac39358f1aae3ee1977
clone_pinned_repository https://github.com/dotnet/runtime.git /opt/workspace/dotnet-runtime 86db03a9c145cefc46fbe9e0f0dc646f739c606c
clone_pinned_repository https://github.com/square/okhttp.git /opt/workspace/okhttp 1d9a8ba6c335355da9c71586abf82c9516e1bac5
clone_pinned_repository https://github.com/laravel/framework.git /opt/workspace/laravel-framework f05ef246c22eac49c7c7e9b2815449873ccd8a22
clone_pinned_repository https://github.com/rails/rails.git /opt/workspace/rails a78f8bcaac1d6f10a515aeccfb6553b895f126c3
clone_pinned_repository https://github.com/scala/scala3.git /opt/workspace/scala3 c101b01b41f8780122caffcc03e0f395edc8016e
clone_pinned_repository https://github.com/Alamofire/Alamofire.git /opt/workspace/alamofire 7595cbcf59809f9977c5f6378500de2ad73b7ddb
```

All repository targets must use `scope=all`, and the evaluator rejects other values. Ordinary full-scope registration does not pass repository `path_filters` or `language_filters` to `repo register`, and a default guardrail verifies that product registration rejects `--language`; case-level filters remain available to test query filtering. The two official framework targets use the separate `registration_path_filters` field to authorize only the locked Angular layout and Vue SFC playground source ranges, while still running every indexing stage inside those scopes. Missing external dependency source is not parser, index, file, scope, or response degradation. It must surface as unresolved edge metadata such as `resolution_state` and `target_hint`, and source/text fallback must not mask authorization gaps, dependency coverage gaps, or parser recovery problems.
