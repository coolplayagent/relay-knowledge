# Relay Knowledge CLI Workflows

## Installation and Upgrade Checks

Use a GitHub Release archive when the user wants prebuilt binaries. Verify the
archive with `checksums.txt`, then place the binary on `PATH`.

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
  in cmd.exe. Then run `relay-knowledge version --format json`.
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
  include the isolated environment variables inline on every command. Do not
  assume `export` from one tool call persists into the next one.
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

On Unix-like shells, use bounded diagnostics:

```bash
timeout 20s relay-knowledge health --format json
timeout 20s relay-knowledge service doctor --format json
timeout 20s relay-knowledge audit query --limit 50 --format json
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
