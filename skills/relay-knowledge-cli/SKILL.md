---
name: relay-knowledge-cli
description: Use relay-knowledge through its local CLI when a user asks for a code map, codebase map, code knowledge graph, repository knowledge graph, multi-repository map, 代码地图, 代码知识图谱, 代码仓库地图, 多代码仓库地图, codebase exploration, repository indexing, code graph search, impact analysis, knowledge graph ingestion, or hybrid GraphRAG queries. The skill operates relay-knowledge by running CLI commands and parsing JSON output for repo registration, full and incremental indexing, repo-set cross-repo queries, setup diagnostics, installation checks, and upgrade checks. Do not use this skill for MCP server setup, MCP tools, ACP adapters, or protocol-level agent access.
metadata:
  version: 1.0.6
  openclaw:
    skillKey: relay-knowledge-cli
    homepage: https://github.com/coolplayagent/relay-knowledge
---

# Relay Knowledge CLI

## Workflow

Use the compiled `relay-knowledge` binary as the control surface. Prefer JSON
output for automation and read command metadata before issuing unfamiliar
commands:

```bash
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

Resolve the executable before the first operation by looking for the published
`relay-knowledge` binary on `PATH` and for this skill's bundled asset binary
for the current platform. Released skill packages include Linux x64 and Windows
x64 binaries at `assets/linux-x86_64/relay-knowledge` and
`assets/windows-x86_64/relay-knowledge.exe`.

Use the command form that matches the active shell:

```bash
command -v relay-knowledge
relay-knowledge version --format json
```

```powershell
Get-Command relay-knowledge
relay-knowledge version --format json
```

```cmd
where.exe relay-knowledge
relay-knowledge version --format json
```

When both `PATH` and the bundled `assets` binary are available, run
`version --format json` for each candidate and use the newest semver version.
If the versions are equal, prefer the `PATH` binary so user-managed installs are
respected. If the current OS or CPU architecture has no bundled asset, use only
the published `PATH` install or install from a published channel.

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

Check whether the CLI exists, then inspect runtime configuration and live
health:

```bash
command -v relay-knowledge
relay-knowledge version
relay-knowledge setup doctor --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
```

On Windows, use `Get-Command relay-knowledge` in PowerShell or
`where.exe relay-knowledge` in cmd.exe before running the same diagnostics.

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

```bash
relay-knowledge repo register /path/to/repo \
  --alias core \
  --path src \
  --language rust \
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
  --freshness wait-until-fresh \
  --limit 10 \
  --format json
```

Choose `--kind hybrid` for broad discovery, `symbol` or `definition` for API
locations, `references` for uses, `callers` or `callees` for call relations,
and `imports` for import edges. For diff-aware work, index the head snapshot
first and then run:

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
