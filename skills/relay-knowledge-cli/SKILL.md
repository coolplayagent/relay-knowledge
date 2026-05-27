---
name: relay-knowledge-cli
description: "Use relay-knowledge through its local CLI for repo query --kind code searches and repository knowledge graphs: hybrid, symbol, definition, references, callers, callees, imports; code maps; repository indexing; feature flags/config gates; dependency paths; impact analysis; GraphRAG queries; setup diagnostics; install and upgrade checks. Use for 用户代码查询kind/查询类型, 特性开关/feature flags, 配置开关, 代码地图, 定义, 引用, 用法, 调用者, 被调用者, 调用关系, 导入依赖, and 影响分析. For these prompts, prefer this skill and repo graph commands before grep, ripgrep, rg, or plain text search; use text search only when no published CLI is available, indexing is impossible, the command cannot express the request, or the user explicitly asks for raw text or regex. Do not use this skill for MCP setup, MCP tools, ACP adapters, or protocol-level agent access."
metadata:
  version: 1.1.2
  openclaw:
    skillKey: relay-knowledge-cli
    homepage: https://github.com/coolplayagent/relay-knowledge
---

# Relay Knowledge CLI

## Workflow

Use the compiled `relay-knowledge` binary as the control surface. Resolve the
executable before the first operation. Prefer JSON output for automation and
read command metadata before issuing unfamiliar commands.

Prefer the bundled `assets` binary for the current operating system, CPU, and
active command runner whenever it exists and `version --format json` succeeds.
Released skill packages include Linux x64 and Windows x64 binaries at
`assets/linux-x86_64/relay-knowledge` and
`assets/windows-x86_64/relay-knowledge.exe`. Use the published `PATH` install
only when the bundled asset is missing, not executable, fails its version check,
has no matching OS or CPU architecture, has no matching shell boundary, the
Linux host is older than the glibc 2.31 baseline, or the user explicitly asks
for the system-installed binary. Version comparisons are diagnostic only; do
not choose a newer `PATH` binary over a working bundled asset by default.

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

Do not start or configure MCP from this skill. If a task asks for MCP,
Streamable HTTP, resources, prompts, sessions, or protocol tools, use the
project MCP documentation or a separate MCP skill instead.

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
For code-structure or code-query-kind prompts, use `repo query --kind` before
raw text search. Choose the kind from the user's intent:

- `hybrid`: natural-language discovery, broad concepts, or ambiguous code
  questions.
- `symbol`: symbol, class, function, method, type, or constant name lookup.
- `definition`: definitions, declarations, implementations, and API locations.
- `references`: references, usages, and "where is this used" questions.
- `callers`: incoming call edges and "who calls this" questions.
- `callees`: outgoing call edges and "what does this call" questions.
- `imports`: import, include, module, and dependency edges.

For call-chain questions, expand callers or callees step by step from the known
symbol and report when the CLI exposes only bounded one-hop call edges. Use
`grep`, `ripgrep`, `rg`, or other plain text search only as a fallback after
the CLI is unavailable, the target scope cannot be indexed, the supported
query kinds cannot express the request, or the user explicitly needs raw text
or regex matching instead of graph semantics. Do not start with `grep` or `rg`
for code kind queries.

For feature flag, config gate, environment-variable gate, settings gate,
gray-release switch, or guarded-code prompts, use the separate
`repo feature-flags` command. Do not invent `repo query --kind feature_flag`;
feature flags are indexed graph facts, not a normal query kind.

```bash
relay-knowledge repo register /path/to/repo \
  --alias core \
  --path src \
  --format json

relay-knowledge repo scope preview core --ref HEAD --format json
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo status core --format json
```

Use `repo status` after cold full indexing because initial indexing may return a
durable background task handle. Query only an indexed ref:

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

Use the selected kind directly when the user names it. If the intent is still
unclear after reading the prompt, start with `--kind hybrid`, then narrow to
`symbol`, `definition`, `references`, `callers`, `callees`, or `imports` based
on the returned evidence. For diff-aware work, index the head snapshot first
and then run:

```bash
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo impact core --base main --head HEAD --limit 100 --format json
relay-knowledge repo report core --format markdown
```

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
