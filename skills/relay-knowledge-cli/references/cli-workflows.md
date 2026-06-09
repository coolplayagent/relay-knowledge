# Relay Knowledge CLI Workflows

## Installation and Upgrade Checks

Use the skill's bundled binary first for the current operating system, CPU, and
active command runner. Released skill packages include
`assets/linux-x86_64/relay-knowledge` and
`assets/windows-x86_64/relay-knowledge.exe`. The Linux x64 asset is built and
checked against a glibc 2.31 baseline. If that asset exists, is executable, and
`version --format json` succeeds, run the workflow commands through that
resolved executable. The examples below keep the command as `relay-knowledge`
for readability; when executing them, substitute the bundled asset path if it
was selected. Do not run the Windows bundled asset from POSIX shells; use
PowerShell or cmd.exe for Windows `.exe` examples. Use `PATH` only when the
asset is absent, unusable, unsupported on the current OS, CPU, or shell boundary,
incompatible with the host Linux glibc version, or explicitly requested by the
user. Treat version comparisons as diagnostics, not as the default selection
rule.

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
- Treat large cold repository indexing as a status-driven code-index workflow.
  `repo index` may return a task id or may time out while its bounded
  foreground worker attempt is still making durable progress. Recover through
  `repo status <alias> --format json`, inspect `active_task`, checkpoint
  counters, and lease expiry, and let a managed service drain the queue when
  one is running. Without a managed service, a killed foreground attempt can
  leave a running lease behind; wait for lease recovery before retrying, then
  use bounded `repo index-worker --task-id <task-id>` attempts only when the
  `repo index` response or status shows a queued or retrying task id. Use
  `repo update` and `index refresh` completion results or status diagnostics
  instead of assuming they expose task ids.
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

Register a Git worktree:

```bash
relay-knowledge repo register /path/to/repo \
  --alias core \
  --path src \
  --format json
```

Registration keeps the full language surface of the selected paths. Apply
language filters at query time instead of passing `--language` to
`repo register`.

Preview and index:

```bash
relay-knowledge repo scope preview core --ref HEAD --format json
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo status core --format json
```

When `repo index` returns a durable task handle and no managed service is
already draining background work, non-interactive agents should run bounded
single-shot worker attempts instead of waiting for an unmanaged loop:

```bash
relay-knowledge repo index-worker --task-id <task-id> --format json
relay-knowledge repo status core --format json
```

The idle worker case is still machine-readable: JSON output reports
`claimed=false` and `task=null`. For event consumers, use streaming JSON and
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

Incremental update and impact:

```bash
relay-knowledge repo update core --base main --head HEAD --format json
relay-knowledge repo impact core --base main --head HEAD --limit 100 --format json
relay-knowledge repo report core --format markdown
```

If `repo update` cannot find an indexed base, first run:

```bash
relay-knowledge repo index core --ref main --format json
```

For uncommitted worktree analysis:

```bash
relay-knowledge repo index core --ref worktree --format json
relay-knowledge repo query core --query retry_policy --ref worktree --format json
```

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
