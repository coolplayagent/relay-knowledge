---
name: relay-knowledge-cli
description: "Use relay-knowledge local CLI for repository knowledge bootstrap and GraphRAG: initialize/validate .knowledge/knowledge-map.yaml plus code and authored business maps; read snapshot-bound repo business/software/view/context models for specs and coding; run durable update/status/impact loops after commits. Use for 知识地图初始化, 业务术语与技术映射, git commit知识库增量更新, 用户代码查询kind/查询类型, 图关系, 调用关系, 导入依赖, SDK/API, 代码地图, definitions, references, usage, impact. Recover durable single-writer index tasks through repo status, a managed service, or bounded local index-worker attempts. Pin immutable refs. Prefer graph CLI before grep/rg unless unavailable, unindexable, inexpressible, or raw regex is required. Do not use for MCP/ACP setup or protocol access."
metadata:
  version: 1.1.14
  openclaw:
    skillKey: relay-knowledge-cli
    homepage: https://github.com/coolplayagent/relay-knowledge
---

# Relay Knowledge CLI

## Workflow

Use the compiled `relay-knowledge` binary as the control surface. Resolve the
executable before the first operation. Prefer JSON output for automation and
read command metadata before issuing unfamiliar commands.

Treat cold and incremental repository indexing as status-driven workflows, not
single long foreground waits. `repo index` and `repo update` submit durable
single-writer tasks; a local CLI invocation may also run one bounded worker
attempt before rendering its response. If the command runner times out before
JSON is captured, recover through
`repo status <alias> --format json` and inspect `active_task`, checkpoint counters, and
freshness before retrying. If a managed platform service is already draining
the code-index queue, poll status and do not start a competing worker. If no
managed service is draining work and status shows a running task after the
command runner killed a foreground attempt, inspect `lease_expires_at_ms` and
checkpoint timestamps and wait for lease recovery before retrying. When
`repo status` or the index/update response shows a queued or retrying code-index
task, let the remote managed service drain it; only a local/service-host client
may run bounded single-shot `repo index-worker --task-id <task-id> --format
json` attempts before re-checking status. Do not replace leases with loops.

Prefer the bundled `assets` binary for the current operating system, CPU, and
active command runner whenever it exists and `version --format json` succeeds.
GitHub Release skill archives include Linux x64 and Windows x64 binaries at
`assets/linux-x86_64/relay-knowledge` and
`assets/windows-x86_64/relay-knowledge.exe`. Registry distributions such as
ClawHub omit these binaries to satisfy per-file limits. Use the published `PATH`
install when the bundled asset is missing, not executable, fails its version
check, has no matching OS or CPU architecture, has no matching shell boundary,
the Linux host is older than the glibc 2.31 baseline, or the user explicitly
asks for the system-installed binary. Version comparisons are diagnostic only;
do not choose a newer `PATH` binary over a working bundled asset by default.

The command examples below use `relay-knowledge` as readable shorthand for the
resolved executable. When the bundled asset is selected, substitute that asset
path for `relay-knowledge` while keeping the same arguments.

Use the command form that matches the active shell. Do not run the Windows
bundled asset from POSIX shells. That includes bash, sh, zsh, fish, and WSL bash
unless the command intentionally crosses into a Windows shell boundary. On
POSIX, check only the POSIX asset first and fall back to `PATH` only when that
asset is unusable:

```bash
/absolute/path/to/relay-knowledge-cli/assets/linux-x86_64/relay-knowledge version --format json
command -v relay-knowledge
relay-knowledge version --format json
```

If the Linux asset fails before printing JSON with an error that mentions
`GLIBC_`, treat the bundled asset as incompatible with that host and use a
published install path built for the host instead of retrying the same asset.

```powershell
$relayKnowledge = "C:\absolute\path\to\relay-knowledge-cli\assets\windows-x86_64\relay-knowledge.exe"
& $relayKnowledge version --format json
Get-Command relay-knowledge
relay-knowledge version --format json
```

```cmd
set "RELAY_KNOWLEDGE=C:\absolute\path\to\relay-knowledge-cli\assets\windows-x86_64\relay-knowledge.exe"
"%RELAY_KNOWLEDGE%" version --format json
where.exe relay-knowledge
relay-knowledge version --format json
```

```bash
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

Do not use source-checkout build artifacts or source builds as an installation
path. This skill is intended to operate published installs only. If the binary
is missing, install it from a published channel first: prefer a verified GitHub
Release archive, or use `cargo install relay-knowledge` from crates.io when
Cargo is the selected published package channel.

Before downloading a binary from GitHub Releases or crates.io, tell the user to
configure a proxy when their network requires one. Prefer standard
`HTTPS_PROXY`, `HTTP_PROXY`, and `NO_PROXY` environment variables, and preserve
those settings for checksum verification and follow-up diagnostics.

Do not use this skill for MCP setup, and do not start or configure MCP from this
skill. If a task asks for MCP, Streamable HTTP, resources, prompts, sessions, or
protocol tools, use the project MCP documentation or a separate MCP skill
instead.

For repository knowledge navigation contracts, use
`references/knowledge-map-workflows.md`. Prefer `relay-knowledge map` commands
to create, read, update, delete, validate, and route the shared
`.knowledge/knowledge-map.yaml` contract. Read that reference for repository
bootstrap, spec work, coding work, commit refresh, or source reconciliation.
Do not hand-edit the YAML unless the CLI is unavailable and the user explicitly
asks for manual repair.

When the user asks for a test, smoke check, or reproduction that should not
touch existing runtime state, set an explicit temporary `RELAY_KNOWLEDGE_HOME`
and clean it up after the scenario. Prefer local deterministic retrieval
backends for isolated tests so smoke checks do not depend on external embedding
services.

POSIX shells:

```bash
export RELAY_KNOWLEDGE_HOME="$(mktemp -d /tmp/relay-knowledge-skill.XXXXXX)"
export RELAY_KNOWLEDGE_SEMANTIC_BACKEND=local
export RELAY_KNOWLEDGE_VECTOR_BACKEND=local
```

PowerShell:

```powershell
$env:RELAY_KNOWLEDGE_HOME = Join-Path $env:TEMP ("relay-knowledge-skill-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $env:RELAY_KNOWLEDGE_HOME | Out-Null
$env:RELAY_KNOWLEDGE_SEMANTIC_BACKEND = "local"
$env:RELAY_KNOWLEDGE_VECTOR_BACKEND = "local"
```

cmd.exe:

```cmd
set "RELAY_KNOWLEDGE_HOME=%TEMP%\relay-knowledge-skill-%RANDOM%-%RANDOM%"
mkdir "%RELAY_KNOWLEDGE_HOME%"
set "RELAY_KNOWLEDGE_SEMANTIC_BACKEND=local"
set "RELAY_KNOWLEDGE_VECTOR_BACKEND=local"
```

If each command runs in a fresh shell or tool call, pass these environment
variables inline on every `relay-knowledge` invocation rather than relying on a
previous `export` to persist. Prefer the tool's environment map when it is
available. Otherwise choose one temporary absolute path for the scenario,
substitute it into every command, and include the shell-specific assignments in
the same command invocation.

POSIX per-command invocation:

```bash
mkdir -p /tmp/relay-knowledge-skill-example && \
  RELAY_KNOWLEDGE_HOME=/tmp/relay-knowledge-skill-example \
  RELAY_KNOWLEDGE_SEMANTIC_BACKEND=local \
  RELAY_KNOWLEDGE_VECTOR_BACKEND=local \
  relay-knowledge status --format json
```

PowerShell per-command invocation:

```powershell
$relayKnowledgeHome = Join-Path $env:TEMP "relay-knowledge-skill-example"; New-Item -ItemType Directory -Force -Path $relayKnowledgeHome | Out-Null; $env:RELAY_KNOWLEDGE_HOME = $relayKnowledgeHome; $env:RELAY_KNOWLEDGE_SEMANTIC_BACKEND = "local"; $env:RELAY_KNOWLEDGE_VECTOR_BACKEND = "local"; relay-knowledge status --format json
```

cmd.exe per-command invocation:

```cmd
if not exist "%TEMP%\relay-knowledge-skill-example" mkdir "%TEMP%\relay-knowledge-skill-example" && set "RELAY_KNOWLEDGE_HOME=%TEMP%\relay-knowledge-skill-example" && set "RELAY_KNOWLEDGE_SEMANTIC_BACKEND=local" && set "RELAY_KNOWLEDGE_VECTOR_BACKEND=local" && relay-knowledge status --format json
```

Remove the temporary directory after capturing the test result.

## Readiness

Check whether the resolved CLI works, then inspect runtime configuration and
live health:

```bash
relay-knowledge version
relay-knowledge setup doctor --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

On Windows, run the same diagnostics through the resolved executable.

Run live diagnostics with a command timeout when the host shell supports one,
and report timeout as a diagnostic finding instead of waiting indefinitely. On
Linux or hosts with GNU coreutils, `timeout` is acceptable:

```bash
timeout 20s relay-knowledge health --format json
timeout 20s relay-knowledge service doctor --format json
timeout 20s relay-knowledge audit query --limit 50 --format json
```

On default macOS shells where GNU `timeout` is not installed, use the command
runner's timeout setting if available. If only shell text is available, use a
short POSIX watchdog for each diagnostic:

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

For online install or upgrades, prefer the official release path first and
Cargo second:

```bash
cargo install relay-knowledge
relay-knowledge version check --format json
```

`version check` only reports available stable versions. It must not replace the
binary automatically. Follow installer or package-manager policy for the actual
upgrade.

## Code Repository Graph

For repository questions, make the index state explicit before querying. Use a
short alias and narrow scope when the user provides relevant paths or languages.
Start with `repo list --format json` when the runtime may already contain
registered repositories; reuse a matching completed alias instead of creating
duplicate registrations. The command is read-only and omits registrations
that have never completed an indexed scope.
For code-structure, code-query-kind, or repository relationship prompts, use the
graph-backed CLI surfaces before raw text search:

- `repo query --kind ...` for code graph retrieval tied to symbols, definitions,
  references, calls, imports, or SBOM dependency facts.
- `repo context` for one bounded coding-agent context pack over the committed
  graph at an immutable ref.
- `repo graph` for an OKF v0.2 Markdown-bundle neighborhood rooted at an
  explicit `--focus` path and `--path` boundary. It is not a call graph.
- `repo software --kind ...` for repository-wide software graph projections:
  dependencies, SDK/API usage, files, topics, relationships, build, IaC, design,
  or all slices together.
- `repo business --kind ...` for authored domains, canonical terms, aliases,
  semantics, conflicts, and business-to-technical mappings.
- `repo feature-flags` for configuration-driven feature flags and guarded-code
  relationships.

Treat kind values as command-local. Do not pass `repo software` kinds to
`repo query`, do not pass `repo query` kinds to `repo software`, and do not
pass `repo business` kinds (`terms`, `mappings`, `all`) to either command. Do
not use
`index refresh` kinds (`bm25`, `semantic`, `vector`), worker kinds
(`embedding`, `ocr`, `vision`, `extractor`), or knowledge-map source kinds
(`repo`, `file`, `doc`, `config`, `db`, `ci`, `runtime`, `wiki`,
`monitoring`) as repository query kinds.

Use `--path` only where the CLI supports a path filter: `repo register` stores
the indexed scope, while `repo query` and `repo feature-flags` narrow reads
inside an already indexed scope. Do not pass `--path` to `repo index`,
`repo scope preview`, `repo update`, `repo impact`, or `repo software`; those
commands use the registered scope plus their ref arguments. For non-Git source
directories, use `--ref HEAD` for the normal moving filesystem snapshot. The
`worktree` selector is for Git worktree overlays only.

### Repository Knowledge Bootstrap

For repository initialization, prepare the knowledge map and code map as one
recoverable workflow. Validate first; create or upgrade the map with `map init`;
reuse a matching completed alias or register the root; index a clean `HEAD`
baseline; add a `worktree` overlay only when authorized uncommitted state must
be modeled. Wait for the exact target before reading the software model. Finish
with the same-ref architecture view and another map validation:

```bash
relay-knowledge map validate --format json
relay-knowledge map init --format json
relay-knowledge map route business-knowledge --format json
relay-knowledge repo list --format json
relay-knowledge repo register . --format json
relay-knowledge repo index <alias> --ref HEAD --format json
relay-knowledge repo status <alias> --format json
relay-knowledge repo business <alias> --kind all --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo software <alias> --kind all --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind architecture-layers --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge repo view <alias> --kind business-domains --ref <pinned-ref> --freshness wait-until-fresh --format json
relay-knowledge map validate --format json
```

`map init` idempotently ensures the `software-model` route and the
`business-knowledge` route whose repository-scoped source points to
`.knowledge/business-glossary.yaml`; it creates the minimal valid glossary only
when that file is absent. Edit the glossary as the version-controlled authored
business surface, but never copy derived architecture, build, IaC, resolved
mapping ids, or commit facts into the Knowledge Map. Follow
`references/knowledge-map-workflows.md` for
missing/invalid-map handling, conditional register/worktree steps, durable task
recovery, source reconciliation, and shell-specific examples.

### `repo query --kind` Code Retrieval

Choose the command-local query kind from the user's intent:

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

Use `references/cli-workflows.md` for deeper recipes, but keep this selection
rule: if the user names a supported kind, use it directly. If the intent is
unclear, start with `--kind hybrid`, then narrow to `symbol`, `definition`,
`references`, `callers`, `callees`, `imports`, or `sbom` based on the returned
evidence. For call-chain questions, expand callers or callees step by step from
the known symbol and report when the CLI exposes only bounded one-hop call
edges.

Large-repository budgets are elastic by default. The effective cold-index
budget scales from the authorized Git file count and throughput baseline, then
is capped explicitly; the historical 180-second value is not a universal hard
timeout. Preserve durable staging, leases, checkpoints, single-writer
behavior, and freshness checks while a long task progresses. Only an explicit
fixed/strict benchmark mode opts out of elastic calculation.

Non-Git source directories use the same registered path filter and `HEAD`
selector flow. The resulting indexed commit is a `filesystem:<hash>` snapshot;
query `HEAD` after indexing unless you intentionally copy an explicit
`filesystem:<hash>` from `repo status`.

Query only an indexed ref:

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

### `repo software --kind` Software Graph

Use `repo software` when the user asks for graph relationships or a repository
overview beyond one symbol-level code query:

- `dependencies`: package and manifest dependency facts.
- `sdks`: SDK/API usage and unresolved external target metadata.
- `files`: file roles and indexed source/document surfaces.
- `topics`: documentation and source topics discovered from indexed evidence.
- `relationships`: cross-domain relationships between files, topics, configs,
  dependencies, SDK/API usages, build targets, IaC resources, and design facts.
- `build`: build target and build-manifest facts.
- `iac`: infrastructure-as-code resource facts.
- `design`: design documentation and design element facts.
- `all`: all software graph slices for repository overviews or when the user
  asks for "everything" about the software graph.

For prompts like "show graph relationships", "what relates these modules",
"dependency paths", "software architecture map", or "代码图关系", prefer
`--kind relationships` first. Use `--kind all` when the user asks for a broad
inventory that should include relationship context and all supporting slices.

```bash
relay-knowledge repo software core \
  --kind relationships \
  --ref HEAD \
  --freshness wait-until-fresh \
  --limit 100 \
  --format json
```

### `repo business --kind` Authored Business Graph

Use `repo business` when the user asks for a domain term, synonym or acronym,
definition conflict, calculation semantics, or a business-to-technical link.
Use `terms` for the glossary surface, `mappings` for technical links, and `all`
when both are required. Pin `--ref`; add `--domain` when homonyms exist. Treat
an `ambiguous` response as a request for domain evidence, never as permission
to guess. Preserve unresolved `target_hint` metadata and use it as a bounded
`repo context` or `repo query` seed rather than declaring repository damage.

```bash
relay-knowledge map route business-knowledge --format json
relay-knowledge repo business core \
  --kind all \
  --query "conversion rate" \
  --domain sales \
  --ref HEAD \
  --freshness wait-until-fresh \
  --limit 20 \
  --format json
```

### Feature Flags

For feature flag, config gate, environment-variable gate, settings gate,
gray-release switch, or guarded-code prompts, use the separate
`repo feature-flags` command. Do not invent `repo query --kind feature_flag`;
feature flags are indexed graph facts, not a normal query kind.

```bash
relay-knowledge repo feature-flags core \
  --query checkout \
  --ref HEAD \
  --path src \
  --limit 20 \
  --format json
```

`repo feature-flags` reads feature flag facts and FTS documents from the indexed
scope. It must not recursively scan source at query time. After adding or
fixing feature flag extraction rules, run `repo index` or `repo update` before
expecting new facts in this command.

### Spec-Grounded Incremental Loop

Before writing a spec, read `map route business-knowledge` and combine relevant
route results with pinned `repo business --kind all`, `repo software --kind
all`, both architecture/business-domain views, and requirement-specific code
context. For a registered Git repository, omit both
refs for the normal commit event:

```bash
relay-knowledge repo update core --format json
relay-knowledge repo status core --format json
```

`--head` defaults to `HEAD`; `--base` defaults to the last successfully
published clean Git commit and unwraps a preceding `worktree:<commit>:<hash>`
identity. Use explicit refs for replay or audit, but never assume the branch is
named `main`. The update is a durable, idempotent, single-writer task. A local
CLI may drain one bounded attempt; a remote CLI may return it queued.

Do not run impact or context against moving refs. When a local update completes,
capture and validate `summary.base_resolved_commit_sha` and
`summary.resolved_commit_sha`. When an update is queued, treat
`.task.mode.incremental.base_ref` and `.task.resolved_commit_sha` as the
authoritative immutable pair. Let the managed service drain remote work, or use
bounded local `repo index-worker` attempts, then poll `repo status` until
`status.last_indexed_commit` equals the exact target and `status.stale` is
false. Use the pinned pair directly; do not reissue an update merely to obtain
a non-null summary:

```bash
relay-knowledge repo impact core --base <pinned-base> --head <pinned-head> --limit 100 --format json
relay-knowledge repo business core --kind all --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge repo context core --query "explain the affected implementation" --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge repo software core --kind all --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge repo view core --kind architecture-layers --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge repo view core --kind business-domains --ref <pinned-head> --freshness wait-until-fresh --format json
relay-knowledge map validate --format json
```

When changed evidence includes Markdown, specifications, or the knowledge map,
also inspect `repo software --kind topics`, `repo software --kind
relationships`, and the OKF neighborhood with `repo graph --focus
<changed-markdown> --path <bundle-root> --ref <pinned-head>`. `repo graph`
requires a fresh indexed Markdown snapshot and is distinct from code call
edges. Refresh the worktree overlay when an uncommitted map mutation must affect
the current decision; otherwise commit it and publish it in the next update.
See `references/knowledge-map-workflows.md` for the complete spec/coding
contract and `references/cli-workflows.md` for deeper CLI recipes.

When the managed service and watcher are enabled, Git HEAD/ref reconciliation
automatically submits the same durable incremental work after a commit. Treat
`repo update` as explicit recovery, replay, or manual event ingress; do not
replace the service with a shell polling loop.

Retention keeps active/recent/incremental/worktree/task/set-pinned scopes and retires older graph/index scopes through durable phased jobs. Without the service, repeat bounded index-worker calls until `maintenance_error` is absent and response/status report no maintenance. Under partitioning, preserve the counted catalog route through batched shard deletion; only the coordinator removes it immediately before final `scope_metadata`. Never bypass it to relieve capacity. SQLite reuses free pages; immediate file shrink needs separate bounded compaction. A truncated status scope listing is not exhaustive. A pruned ref is not a
valid incremental base: create a full snapshot with
`repo index <alias> --ref <desired-head>` and start a new update window instead
of bypassing retention.

Keep non-Git directories separate: rerun `repo index <alias> --ref HEAD` after
changes. They have no commit-ref event stream and do not participate in managed
Git reconciliation.

If a Git commit leaves other files dirty, recreate the worktree overlay with
`repo index <alias> --ref worktree`; the clean commit update does not claim that
uncommitted remainder.

Use `grep`, `ripgrep`, `rg`, or other plain text search only as a fallback after
the CLI is unavailable, the target scope cannot be indexed, the supported
query/software kinds cannot express the request, or the user explicitly needs
raw text or regex matching instead of graph semantics. Do not start with `grep`
or `rg` for code kind or graph relationship queries.

## Knowledge Graph

For non-code evidence, ingest scoped text, refresh derived indexes when needed,
and query with freshness metadata:

```bash
relay-knowledge ingest --source docs \
  --content "Rust async services isolate blocking SQLite work" \
  --entity Rust \
  --format json

relay-knowledge query SQLite \
  --source docs \
  --freshness wait-until-fresh \
  --limit 10 \
  --format json

relay-knowledge index refresh --kind bm25 --format json
relay-knowledge graph inspect --format json
```

## Troubleshooting

If a command fails, prefer its JSON error when present; otherwise read the
stderr or text error exactly and avoid guessing hidden state. Run diagnostics
in this order:

```bash
relay-knowledge status --format json
relay-knowledge setup doctor --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
relay-knowledge audit query --limit 50 --format json
```

For empty code results, verify `repo status`, the queried ref, path/language
filters, and `--kind`. Use `--kind hybrid` before narrowing. For stale graph
results, use `--freshness wait-until-fresh` or run `index refresh` explicitly.

For deeper command recipes, read `references/cli-workflows.md`.
