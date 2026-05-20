# Relay Knowledge CLI Workflows

## Installation and Upgrade Checks

Use the skill's bundled binary when it is the newest matching candidate for the
current platform. Released skill packages include `assets/linux-x86_64/relay-knowledge`
and `assets/windows-x86_64/relay-knowledge.exe`. Compare each usable candidate
with `relay-knowledge version --format json`; choose the newest semver version,
and prefer the `PATH` binary when versions match.

Use a GitHub Release archive when the bundled asset is absent, unusable, or
older than the requested published version. Before downloading, tell the user to
configure proxy settings if their network needs them:

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

- Resolve the executable before running workflow commands with the active
  shell's executable lookup command: `command -v relay-knowledge` on POSIX,
  `Get-Command relay-knowledge` in PowerShell, or `where.exe relay-knowledge`
  in cmd.exe. Also check the matching bundled asset:
  `assets/linux-x86_64/relay-knowledge` on Linux x64 or
  `assets/windows-x86_64/relay-knowledge.exe` on Windows x64. Then run
  `version --format json` for each candidate and select the newest semver
  version; if the versions are equal, prefer `PATH`.
  Use only published installs on `PATH`: a verified GitHub Release archive, or
  `cargo install relay-knowledge` from crates.io when Cargo is the selected
  published package channel. Do not use source-checkout build artifacts or
  source builds as the installation path for this published skill.
- Prefer `--format json` for commands whose output will be parsed.
- Inspect `relay-knowledge help --format json` and command-specific help before
  exposing or automating a command.
- Treat `status`, `health`, `setup doctor`, `setup profile`, `provider probe`,
  `version check`, `repo report`, and `audit query` as diagnostics.
- Treat `ingest`, `repo index`, `repo update`, `index refresh`,
  `worker run-once`, proposal state changes, and `service definition write` as
  commands that may write runtime state.
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
  --language rust \
  --format json
```

Preview and index:

```bash
relay-knowledge repo scope preview core --ref HEAD --format json
relay-knowledge repo index core --ref HEAD --format json
relay-knowledge repo status core --format json
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

- `hybrid`: first pass when intent is broad or ambiguous.
- `symbol`: find declarations by identifier.
- `definition`: locate API definitions or type/function declarations.
- `references`: find uses of a symbol or concept.
- `callers`: find who calls a function-like symbol.
- `callees`: find calls made by a function-like symbol.
- `imports`: find import/include/module dependency edges.

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
