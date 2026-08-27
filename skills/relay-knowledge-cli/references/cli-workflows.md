# Relay Knowledge CLI Workflows

## Contents

- [Installation and upgrade checks](#installation-and-upgrade-checks)
- [Safe agent defaults](#safe-agent-defaults)
- [Code repository index/query flow](#code-repository-index-query-flow)
- [Commit-driven Git update loop](#commit-driven-git-update-loop)
- [Knowledge graph query flow](#knowledge-graph-query-flow)
- [Diagnostics](#diagnostics)
- [Out of scope](#out-of-scope)

## Installation and Upgrade Checks

Use the skill's bundled binary first for the current operating system, CPU, and
active command runner. GitHub Release skill archives include
`assets/linux-x86_64/relay-knowledge` and
`assets/windows-x86_64/relay-knowledge.exe`; ClawHub packages omit them to stay
within the registry's per-file limit. The Linux x64 asset is built and checked
against a glibc 2.31 baseline. If that asset exists, is executable, and `version
--format json` succeeds, run the workflow commands through that resolved
executable. The examples below keep the command as `relay-knowledge` for
readability; when executing them, substitute the bundled asset path if it was
selected. Do not run the Windows bundled asset from POSIX shells; use PowerShell
or cmd.exe for Windows `.exe` examples. Use `PATH` only when the asset is absent,
unusable, unsupported on the current OS, CPU, or shell boundary, incompatible
with the host Linux glibc version, or explicitly requested by the user. Treat
version comparisons as diagnostics, not as the default selection rule.

Use a GitHub Release archive when the bundled asset is absent, unusable, or the
user requested a specific published version that is not available in the skill
assets. Before downloading, tell the user to configure proxy settings if their
network needs them:

```bash
export HTTPS_PROXY=http://proxy.example:8080
export HTTP_PROXY=http://proxy.example:8080
export NO_PROXY=localhost,127.0.0.1
```

```powershell
$env:HTTPS_PROXY = "http://proxy.example:8080"
$env:HTTP_PROXY = "http://proxy.example:8080"
$env:NO_PROXY = "localhost,127.0.0.1"
```

Verify the archive with `checksums.txt`, then place the binary on `PATH`.

Use Cargo when Rust is available:

```bash
cargo install relay-knowledge
relay-knowledge --version
relay-knowledge service doctor --format json
```

Check for new versions without upgrading automatically:

```bash
relay-knowledge version
relay-knowledge version check --format json
```

`version` is local only. `version check` may contact GitHub Releases and
crates.io through relay-knowledge network configuration and cache the result in
the runtime cache directory.

## Safe Agent Defaults

- Resolve the executable before running workflow commands. Check the matching
  bundled asset for the active OS, CPU, and shell boundary first:
  `assets/linux-x86_64/relay-knowledge` on Linux x64 or
  `assets/windows-x86_64/relay-knowledge.exe` on Windows x64. If the bundled
  asset passes `version --format json`, use it even when `PATH` has another
  version. If Linux reports a missing `GLIBC_` symbol before JSON is printed,
  treat the bundled asset as incompatible rather than retrying it. Fall back to
  `PATH` only when the asset cannot be used or the user explicitly chooses the
  system install. Use only published installs on `PATH`: a verified GitHub
  Release archive, or `cargo install relay-knowledge` from crates.io when Cargo
  is the selected published package channel. Do not use source-checkout build
  artifacts or source builds as the installation path for this published skill.
  Command examples use `relay-knowledge` as shorthand for the resolved
  executable, and Windows `.exe` commands must stay in PowerShell or cmd.exe
  command blocks rather than bash/POSIX command blocks.
- Prefer `--format json` for commands whose output will be parsed.
- Inspect `relay-knowledge help --format json` and command-specific help before
  exposing or automating a command.
- Treat `status`, `health`, `setup doctor`, `setup profile`, `provider probe`,
  `version check`, `repo report`, and `audit query` as diagnostics.
- Treat `ingest`, `repo index`, `repo update`, `index refresh`,
  `worker run-once`, proposal state changes, and `service definition write` as
  commands that may write runtime state.
- Treat cold and incremental repository indexing as status-driven code-index
  workflows. `repo index` and `repo update` submit durable single-writer tasks;
  either command may return a task id or time out while its bounded
  foreground worker attempt is still making durable progress. Recover through
  `repo status <alias> --format json`, inspect `active_task`, checkpoint
  counters, and lease expiry, and let a managed service drain the queue when
  one is running. Without a managed service, a killed foreground attempt can
  leave a running lease behind; wait for lease recovery before retrying, then
  use bounded `repo index-worker --task-id <task-id>` attempts only on the
  local service host when the response/status shows queued or retrying work;
  a remote client must let the managed service drain it.
- Keep runtime state in the platform directories managed by relay-knowledge.
  Do not redirect databases, logs, or caches into arbitrary repository folders
  unless the user explicitly asks for an isolated test home.
- For isolated smoke tests, set `RELAY_KNOWLEDGE_HOME` to a temporary absolute
  directory, set `RELAY_KNOWLEDGE_SEMANTIC_BACKEND=local` and
  `RELAY_KNOWLEDGE_VECTOR_BACKEND=local`, and remove the temporary home after
  capturing the result. Use `mktemp -d` on POSIX, `Join-Path $env:TEMP` plus
  `New-Item -ItemType Directory` in PowerShell, or `%TEMP%` plus `mkdir` in
  cmd.exe.
- If the agent runtime invokes commands through separate shell/tool calls,
  pass the isolated environment variables through the tool's environment map
  when possible. If only shell text is available, include the active shell's
  assignment form in the same command invocation and reuse the same temporary
  absolute home path for every command in the scenario. POSIX can use
  `RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-skill-example
  RELAY_KNOWLEDGE_SEMANTIC_BACKEND=local
  RELAY_KNOWLEDGE_VECTOR_BACKEND=local relay-knowledge status --format json`.
  PowerShell can set a scenario home with
  `Join-Path $env:TEMP "relay-knowledge-skill-example"`, assign
  `$env:RELAY_KNOWLEDGE_HOME`,
  `$env:RELAY_KNOWLEDGE_SEMANTIC_BACKEND`, and
  `$env:RELAY_KNOWLEDGE_VECTOR_BACKEND` before `relay-knowledge` in the same
  command string. cmd.exe can use `%TEMP%\relay-knowledge-skill-example` with
  chained `set "NAME=value" && relay-knowledge ...` commands. Do not assume
  `export` from one tool call persists into the next one.
- Wrap live diagnostics in a short command timeout when the shell supports one.
  Treat a timeout as diagnostic evidence and continue with narrower commands
  instead of waiting indefinitely.

## Code Repository Index Query Flow

Inspect existing completed repository scopes before adding a registration:

```bash
relay-knowledge repo list --format json
```

The list is read-only and omits repositories that have never completed an
indexed scope. Reuse a matching alias instead of creating a duplicate.

Register a Git worktree or non-Git source directory:

```bash
relay-knowledge repo register /path/to/repo \
  --alias core \
  --path src \
  --format json
```

Registration keeps the full language surface of the selected paths. Apply
language filters at query time instead of passing `--language` to
`repo register`. The `--path` flag is the CLI spelling for a path filter:
`repo register --path` stores the indexed scope, while query-time `--path`
narrows reads inside that indexed scope. Do not pass `--path` to `repo index`;
indexing uses the registered scope plus `--ref`.

Preview and index:

```bash
relay-knowledge repo scope preview core --ref HEAD --format json
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo status core --format json
```

Large-repository indexing uses elastic budgets by default. The 180-second value
is a historical baseline only; the effective budget scales from authorized Git
file count and throughput baseline and is bounded by a configured maximum. Do
not treat a caller timeout as indexing failure: inspect checkpoint progress,
lease state, and freshness, then let the managed worker or a bounded
`repo index-worker` attempt continue the durable task. Fixed/strict behavior is
only an explicit benchmark override.

For non-Git source directories, keep the normal selector as `HEAD`. Indexing
resolves it into a `filesystem:<hash>` snapshot, and queries should use `HEAD`
after indexing unless an explicit stored `filesystem:<hash>` from
`repo status` is required for audit or diff work.

```powershell
relay-knowledge repo register "D:/workspace/hello" --alias hello --path "云存储服务开发部" --format json
relay-knowledge repo index hello --ref HEAD --format json
relay-knowledge repo query hello --query "关键词" --kind hybrid --ref HEAD --format json
```

When `repo index` returns a durable task handle and no managed service is
already draining background work, non-interactive agents should run bounded
single-shot worker attempts instead of waiting for an unmanaged loop:

```bash
relay-knowledge repo index-worker --task-id <task-id> --format json
relay-knowledge repo status core --format json
```

The idle worker case is still machine-readable: every invocation also advances
one bounded retention pass, and JSON reports `maintenance_active` plus optional
`maintenance_error`. Repeat while it is active or status says maintenance is
pending. If the error is present, report it and treat a false activity value as
inconclusive until the fault is resolved; `claimed=false` and `task=null` only
mean no index task ran. For event consumers, use streaming JSON and
read the worker result from the `item.payload` event:

```bash
relay-knowledge repo index-worker --task-id <task-id> --format streaming-json
```

Query:

```bash
relay-knowledge repo query core \
  --query retry_policy \
  --kind hybrid \
  --ref HEAD \
  --path src \
  --language rust \
  --freshness wait-until-fresh \
  --limit 10 \
  --format json
```

Kind selection:

For user prompts about supported code query kinds, use graph-backed commands
before plain text search. Select the command and command-local `--kind` from the
user's intent.

Each `--kind` set belongs to a specific command family. `repo query`,
`repo software`, `index refresh`, `worker`, and `map source` kinds are not
interchangeable. Do not map feature flags or impact analysis into
`repo query --kind`; use `repo feature-flags` and `repo impact` instead.

### `repo query --kind`

Use `relay-knowledge repo query --kind ...` for code graph retrieval tied to one
query string, symbol surface, or code edge:

- `hybrid`: natural-language discovery, broad concepts, or ambiguous code
  questions.
- `symbol`: symbol, class, function, method, type, or constant name lookup.
- `definition`: definitions, declarations, implementations, and API locations.
- `references`: references, usages, and "where is this used" questions.
- `callers`: incoming call edges and "who calls this" questions.
- `callees`: outgoing call edges and "what does this call" questions.
- `imports`: import, include, module, and dependency edges.
- `sbom`: package-manager dependency inventory from indexed manifests and
  lockfiles.

Use the selected kind directly when the user names it. If intent is ambiguous,
start with `--kind hybrid`, then narrow based on the returned evidence. For
call-chain prompts, expand `callers` or `callees` step by step and state that
the CLI returns bounded one-hop edges when that limit matters.

### `repo software --kind`

Use `relay-knowledge repo software --kind ...` when the user asks for
repository-wide graph relationships, architecture maps, dependency paths,
software inventory, or "代码图关系":

- `dependencies`: package and manifest dependency facts.
- `sdks`: SDK/API usage and unresolved external target metadata.
- `files`: file roles and indexed source/document surfaces.
- `topics`: documentation and source topics discovered from indexed evidence.
- `relationships`: cross-domain relationships between files, topics, configs,
  dependencies, SDK/API usages, build targets, IaC resources, and design facts.
- `build`: build target and build-manifest facts.
- `iac`: infrastructure-as-code resource facts.
- `design`: design documentation and design element facts.
- `all`: all software graph slices for broad repository overviews.

Prefer `--kind relationships` for prompts that explicitly ask for graph
relationships. Prefer `--kind all` when the user asks for an inventory or
overview that should include the relationship slice plus supporting facts.

```bash
relay-knowledge repo software core \
  --kind relationships \
  --ref HEAD \
  --freshness wait-until-fresh \
  --limit 100 \
  --format json
```

### `repo business --kind`

Read `map route business-knowledge --format json` first, then use `repo
business` for authored domain terms, aliases/acronyms, semantics, conflicting
definitions, and technical mappings. The command-local kinds are `terms`,
`mappings`, and `all`. Pin the same immutable `--ref` used by context and
software queries. Supply `--domain` for homonyms; do not guess when resolution
is `ambiguous`. Preserve unresolved `target_hint` values as bounded follow-up
seeds rather than treating them as parser or repository degradation.

```bash
relay-knowledge repo business core \
  --kind all \
  --query "conversion rate" \
  --domain sales \
  --ref "$pinned_head" \
  --freshness wait-until-fresh \
  --limit 20 \
  --format json
```

### `repo context` and OKF `repo graph`

Use `repo context` to build one bounded coding-agent context pack from a fresh,
committed snapshot. Pin `--ref` to an immutable commit after an update; the
command reads but never starts indexing:

```bash
relay-knowledge repo context core \
  --query "trace the retry policy change" \
  --ref "$pinned_head" \
  --freshness wait-until-fresh \
  --max-context-bytes 65536 \
  --format json
```

Use `repo graph` for a versioned OKF v0.2 neighborhood over parseable
YAML-frontmatter Markdown. Supply both a focus file and an authorized bundle
root; traversal is bounded and never reads the live worktree. This is a
documentation/concept graph, not the callers/callees code graph:

```bash
relay-knowledge repo graph core \
  --focus docs/architecture/commit-loop.md \
  --path docs \
  --ref "$pinned_head" \
  --depth 2 \
  --format json
```

Use `grep`, `ripgrep`, `rg`, or other text search only as a fallback after the
CLI is unavailable, the target repository cannot be indexed, the supported
query or software kinds cannot express the request, or the user explicitly asks
for raw text or regular-expression matching. When falling back, say that text
search is a fallback rather than the preferred code graph path.

### Feature Flag Query Flow

For prompts about feature flags, config keys, environment-variable gates,
settings gates, gray-release switches, or code guarded by runtime configuration,
use `repo feature-flags` instead of `repo query --kind`. Feature flags are a
separate indexed graph surface; do not pass `feature_flag` or `feature-flags` as
a query kind.

```bash
relay-knowledge repo feature-flags core \
  --query checkout \
  --ref HEAD \
  --path src \
  --limit 20 \
  --format json
```

Without `--query`, the command enumerates feature flag groups for the selected
indexed scope. With `--query`, it filters indexed feature flag names, config
sources, paths, and excerpts. It does not recursively grep the repository at
query time; after adding flags or changing extraction rules, refresh the scope
with `repo index` or `repo update`.

Use `grep`, `ripgrep`, `rg`, or another raw text search for feature flag prompts
only when the CLI is unavailable, the target repository cannot be indexed, or
the user explicitly asks for raw text or regular-expression matching.

## Commit-Driven Git Update Loop

### Managed commit events

When `RELAY_KNOWLEDGE_WATCHER_ENABLED=true` and the resident service is running,
the managed watcher reconciles Git HEAD/ref changes and submits durable
commit-to-commit index tasks. Startup and bounded reconciliation recover missed
notifications. The queue preserves leases, checkpoints, retry backoff, and at
most one active writer for each repository. Do not add a shell loop, kill a
competing process, or bypass the task lease.

Use `repo update` as explicit recovery, replay, CI/hook ingress, or a manual
commit event. It shares the durable task path with managed reconciliation. A
local invocation may drain one bounded worker attempt; remote mode may return a
queued task for the service to drain.

### Ref resolution contract

The normal form is:

```bash
relay-knowledge repo update core --format json
```

`--head` defaults to `HEAD`. `--base` defaults to the last successfully
published clean Git commit. If the last publication was a worktree overlay, the
CLI unwraps `worktree:<base-commit>:<content-hash>` and uses its clean base.
Never assume a branch is named `main`. For audit or replay, either or both refs
can be explicit:

```bash
relay-knowledge repo update core --base <base-commit> --head <head-commit> --format json
```

The queued task pins moving refs before work starts. A local completed response
contains both immutable identities in `summary.base_resolved_commit_sha` and
`summary.resolved_commit_sha`; validate and use them when present. For a queued
response, `.task.mode.incremental.base_ref` and `.task.resolved_commit_sha` are
the authoritative pair. Do not use `HEAD`, a branch, or the original spelling
for a downstream comparison, and do not reissue update merely to obtain a
summary.

### POSIX completion and immutable-ref flow

The parsing example below uses `jq`. Keep polling bounded and issue each status
check explicitly; do not turn it into an unmanaged daemon loop.

```bash
update_json="$(relay-knowledge repo update core --format json)"
printf '%s\n' "$update_json"

task_id="$(printf '%s' "$update_json" | jq -r '.task.task_id // empty')"
pinned_base="$(printf '%s' "$update_json" | jq -er '.summary.base_resolved_commit_sha // .task.mode.incremental.base_ref')"
pinned_head="$(printf '%s' "$update_json" | jq -er '.summary.resolved_commit_sha // .task.resolved_commit_sha')"
printf '%s' "$update_json" | jq -e '
  .summary == null or .task == null or
  (.summary.base_resolved_commit_sha == .task.mode.incremental.base_ref and
   .summary.resolved_commit_sha == .task.resolved_commit_sha)'
```

If `.summary` is null, let the managed service drain the task. A remote client
cannot run `repo index-worker`; only on the local service host, when no managed
service is draining it, run a bounded single-shot attempt. Then inspect status:

```bash
relay-knowledge repo index-worker --task-id "$task_id" --format json
status_json="$(relay-knowledge repo status core --format json)"
printf '%s\n' "$status_json"
printf '%s' "$status_json" | jq -e --arg head "$pinned_head" \
  '.status.last_indexed_commit == $head and (.status.stale == false)'
```

Repeat the bounded status/worker sequence only while the task is queued or
retrying. Stop and diagnose failed/dead-letter state. Once the exact target is
fresh, use the already pinned pair directly. Run impact first, then build coding
context at the immutable head:

```bash
relay-knowledge repo impact core \
  --base "$pinned_base" \
  --head "$pinned_head" \
  --limit 100 \
  --format json
relay-knowledge repo context core \
  --query "explain the affected implementation and tests" \
  --ref "$pinned_head" \
  --freshness wait-until-fresh \
  --format json
```

When Markdown, specifications, or `.knowledge/knowledge-map.yaml` changed,
include topic/relationship projections and the focused OKF neighborhood:

```bash
relay-knowledge repo software core --kind topics --ref "$pinned_head" --freshness wait-until-fresh --format json
relay-knowledge repo software core --kind relationships --ref "$pinned_head" --freshness wait-until-fresh --format json
relay-knowledge repo graph core --focus docs/architecture/commit-loop.md --path docs --ref "$pinned_head" --depth 2 --format json
```

### PowerShell completion and immutable-ref flow

```powershell
$update = relay-knowledge repo update core --format json | ConvertFrom-Json
$taskId = $update.task.task_id
$pinnedBase = if ($null -ne $update.summary) { $update.summary.base_resolved_commit_sha } else { $update.task.mode.incremental.base_ref }
$pinnedHead = if ($null -ne $update.summary) { $update.summary.resolved_commit_sha } else { $update.task.resolved_commit_sha }
if ($null -ne $update.summary -and $null -ne $update.task -and
    ($update.summary.base_resolved_commit_sha -ne $update.task.mode.incremental.base_ref -or
     $update.summary.resolved_commit_sha -ne $update.task.resolved_commit_sha)) { throw "resolved commit pair changed" }
```

If `$update.summary` is null, let the managed service drain it. A remote client
cannot run `repo index-worker`; only on the local service host, when no managed
service is draining the task, run one bounded attempt. Re-check exact freshness:

```powershell
relay-knowledge repo index-worker --task-id $taskId --format json
$status = relay-knowledge repo status core --format json | ConvertFrom-Json
if ($status.status.last_indexed_commit -ne $pinnedHead -or $status.status.stale) { throw "queued commit is not fresh" }
```

Use the immutable values for downstream reads:

```powershell
relay-knowledge repo impact core --base $pinnedBase --head $pinnedHead --limit 100 --format json
relay-knowledge repo context core --query "explain the affected implementation and tests" --ref $pinnedHead --freshness wait-until-fresh --format json
relay-knowledge repo software core --kind topics --ref $pinnedHead --freshness wait-until-fresh --format json
relay-knowledge repo software core --kind relationships --ref $pinnedHead --freshness wait-until-fresh --format json
relay-knowledge repo graph core --focus "docs/architecture/commit-loop.md" --path "docs" --ref $pinnedHead --depth 2 --format json
```

### Scope and index retention

Every successful publication runs bounded scope retention so a long commit
history does not grow the graph store indefinitely. The policy retains the
active scope, a small rollback window (currently the two latest successful
scopes), the latest successful incremental predecessor, the clean base of an
active worktree overlay, base and target scopes required by unfinished tasks,
and scopes pinned by repository-set members.
Same-tree commits reuse content and keep a bounded 256-row commit-alias window.
Retention first marks one unprotected scope `retiring` atomically, excluding it
from reads and incremental-base selection, and records a durable GC job. Each
later maintenance transaction advances one scope-GC phase whose physical
deletion is capped at 512 rows in aggregate across affected application tables.
Separate succeeded-audit, failure-class-audit, and commit-alias quotas are each
512 rows, capping primary cleanup at 2,048 physical rows plus at most one
terminal GC-job bookkeeping row per pass. The managed worker retries persistent
work while idle and removes unpinned code facts and their
derived search/index rows. This bounds live generations and lets SQLite reuse
free pages; it does not promise immediate OS-visible file shrink, which needs
a separate explicit bounded compaction. Without the managed service, repeat
bounded `repo index-worker --format json` calls until `maintenance_active=false`
with no `maintenance_error` and status reports no pending maintenance. Inspect `retention.maintenance_pending`,
`retention.retiring_jobs`, `retention.scope_listing_truncated`, `active_task`, and `checkpoint` in
`repo status <alias> --format json` rather than counting database files.
When `scope_listing_truncated` is true, treat the retained/prunable arrays and
displayed counts as bounded diagnostic lower bounds, never as an exhaustive protection set.
Under partitioned SQLite, preserve the control catalog route as a counted slot
throughout batched shard fact deletion. Only the retention coordinator removes
it immediately before the final `scope_metadata` shard transaction; do not
delete or bypass the route to relieve capacity. A crash in that final gap
replays the deterministic shard job without restoring a stale route.

Never use a pruned scope as an incremental base. If the requested base has
expired, run `repo index <alias> --ref <desired-head>` to publish a new full
snapshot and begin a new comparison window. Do not weaken retention, expand the
window without a bound, or retry an update in a way that bypasses freshness.

### Worktree and non-Git flows

For uncommitted Git worktree analysis, use the explicit overlay selector:

```bash
relay-knowledge repo index core --ref worktree --format json
relay-knowledge repo query core --query retry_policy --ref worktree --format json
```

An overlay is not a commit event. After the next commit, the default update
base unwraps the clean commit from the overlay identity and the managed watcher
reconciles the new HEAD. If other files remain dirty after that partial commit,
run `repo index core --ref worktree` again; the clean commit update does not
silently fold the uncommitted remainder into its snapshot.

Do not use `--ref worktree` for non-Git source directories. They have no Git
HEAD/ref event stream, so managed commit reconciliation does not apply. After a
change, rerun a full moving-filesystem snapshot:

```bash
relay-knowledge repo index source-tree --ref HEAD --format json
```

For an explicitly requested non-Git diff, copy the previous
`filesystem:<hash>` from `repo status`, pass it as `--base`, and use
`--head HEAD`. If that filesystem scope was pruned, publish a full snapshot
instead.

## Knowledge Graph Query Flow

For the repository knowledge navigation contract, use
`references/knowledge-map-workflows.md`. The contract lives at
`.knowledge/knowledge-map.yaml` and should be maintained through
`relay-knowledge map` commands rather than direct YAML edits.

Ingest scoped evidence:

```bash
relay-knowledge ingest --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust \
  --format json
```

Query with freshness:

```bash
relay-knowledge query SQLite \
  --source docs \
  --freshness wait-until-fresh \
  --limit 10 \
  --format json
```

Inspect graph and refresh indexes:

```bash
relay-knowledge graph inspect --format json
relay-knowledge index refresh --kind bm25 --format json
relay-knowledge index refresh --kind semantic --format json
relay-knowledge index refresh --kind vector --format json
```

## Diagnostics

Use this order when runtime behavior is unclear:

```bash
relay-knowledge status --format json
relay-knowledge setup doctor --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
relay-knowledge audit query --limit 50 --format json
```

If a failing command prints a text error even though `--format json` was used,
treat the text as the authoritative failure message and then run the diagnostic
sequence above.

On Linux or hosts with GNU coreutils, use bounded diagnostics with `timeout`:

```bash
timeout 20s relay-knowledge health --format json
timeout 20s relay-knowledge service doctor --format json
timeout 20s relay-knowledge audit query --limit 50 --format json
```

On default macOS shells where GNU `timeout` is not installed, prefer the
command runner's timeout setting. If only shell text is available, run each
diagnostic behind a short POSIX watchdog:

```bash
relay-knowledge health --format json &
relay_knowledge_pid=$!
( sleep 20; kill "$relay_knowledge_pid" 2>/dev/null ) &
relay_knowledge_watchdog=$!
wait "$relay_knowledge_pid"
relay_knowledge_status=$?
kill "$relay_knowledge_watchdog" 2>/dev/null
exit "$relay_knowledge_status"
```

For provider setup:

```bash
relay-knowledge provider probe --format json
```

For local service operation:

```bash
relay-knowledge service plan install --format json
relay-knowledge service lifecycle install --dry-run --format json
relay-knowledge service definition write --format json
relay-knowledge service operator status --format json
relay-knowledge service operator pause --format json
relay-knowledge service operator resume --format json
```

Use platform service managers for long-running operation. Do not replace them
with unmanaged CLI loops.

## Out of Scope

This skill does not configure MCP, launch MCP Streamable HTTP, call MCP tools,
or manage ACP sessions. Use relay-knowledge CLI commands only.
