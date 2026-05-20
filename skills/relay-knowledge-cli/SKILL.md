---
name: relay-knowledge-cli
description: Use relay-knowledge through a limited local CLI command set for readiness checks, repository registration, repository indexing status, repository search, impact reports, simple knowledge graph ingest/query, and troubleshooting JSON CLI errors. Do not use this skill for MCP setup, MCP tools, ACP adapters, protocol-level agent access, file upload workflows, or commands not listed in this skill.
allowed-tools:
  - functions.shell_command
metadata:
  version: 1.0.6
  openclaw:
    skillKey: relay-knowledge-cli
    homepage: https://github.com/coolplayagent/relay-knowledge
---

# Relay Knowledge CLI

## Tool Boundary

Use only `functions.shell_command` to run the CLI commands listed in this file.
Do not use web, GitHub, MCP, file upload, browser, long-running service, or
unlisted helper tools for this skill.

Only run command shapes explicitly shown below. Example values may change for
repository paths, aliases, refs, queries, limits, paths, languages, sources,
entities, and output formats. Do not invent unlisted subcommands, modes, flags,
shell wrappers, polling loops, watchdogs, or path-computing helper commands.
Treat `help --format json` as command metadata for the listed commands only.

Use the command runner timeout field instead of shell timeout programs. Suggested
maximums:

- Discovery, help, version, status, and diagnostics: 30 seconds.
- Repo query, impact, report, ingest, graph inspect, and index refresh: 60 seconds.
- Repo index and repo update: 5 minutes.

If a command times out, report the timeout and run at most one listed status or
diagnostic command. Do not keep retrying.

## Command Discovery

Use the lookup command for the active shell, then check the CLI version.

```bash
command -v relay-knowledge
relay-knowledge version --format json
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

```powershell
Get-Command relay-knowledge
relay-knowledge version --format json
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

```cmd
where.exe relay-knowledge
relay-knowledge version --format json
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

## Readiness

Inspect runtime configuration and health:

```bash
relay-knowledge setup doctor --format json
relay-knowledge health --format json
relay-knowledge service doctor --format json
relay-knowledge audit query --limit 50 --format json
```

Use `relay-knowledge version check --format json` only when the user asks to
check for available releases. Do not install, upgrade, or replace binaries from
this skill unless the user explicitly requests installation work.

## Code Repository Graph

For repository questions, make the index state explicit before querying. Use a
short alias and narrow scope when the user provides relevant paths or languages.
Show `repo register` as a final CLI command with a concrete simulated
repository path. Do not wrap it in tool JSON, and do not show helper commands
for computing the path. Use a native-looking absolute path for the active
platform.

```bash
relay-knowledge repo register "C:/workspaces/example/sample-codebase" --alias sample --language python --format json
```

```bash
relay-knowledge repo register "C:/workspaces/example/sample-codebase" --alias sample --path src --language python --format json
```

```bash
relay-knowledge repo register "/home/example/repos/sample-codebase" --alias sample --format json
```

Use only these code repository commands. Keep command paths and option names
unchanged; substitute only values.

Preview and index:

```bash
relay-knowledge repo scope preview sample --ref HEAD --format json
relay-knowledge repo index sample --ref HEAD --format json
relay-knowledge repo status sample --format json
```

Run `repo scope preview` before `repo index`. Run `repo index` only when the
user asks to build or refresh a repository index, or when a query explicitly
requires an indexed ref. Use `repo status` after indexing because initial
indexing may return a background task handle. Query only an indexed ref:

```bash
relay-knowledge repo query sample \
  --query retry_policy \
  --kind hybrid \
  --ref HEAD \
  --freshness wait-until-fresh \
  --limit 10 \
  --format json
```

Choose `--kind hybrid` for broad discovery, `symbol` or `definition` for API
locations, `references` for uses, `callers` or `callees` for call relations,
and `imports` for import edges.

For diff-aware work, index the head snapshot first and then run:

```bash
relay-knowledge repo update sample --base main --head HEAD --format json
relay-knowledge repo impact sample --base main --head HEAD --limit 100 --format json
relay-knowledge repo report sample --format markdown
```

If `repo update` reports that the base ref is not indexed, run the listed base
index command once before retrying the update:

```bash
relay-knowledge repo index sample --ref main --format json
relay-knowledge repo update sample --base main --head HEAD --format json
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
stderr or text error exactly and avoid guessing hidden state. Run at most these
diagnostics, in this order:

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
