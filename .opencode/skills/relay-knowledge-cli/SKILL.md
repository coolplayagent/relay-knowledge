---
name: relay-knowledge-cli
description: Use relay-knowledge through its local CLI for knowledge graph ingestion, hybrid GraphRAG queries, code repository registration, indexing, code graph search, impact analysis, setup diagnostics, installation checks, and upgrade checks. Use when an OpenCode agent should operate relay-knowledge by running CLI commands and parsing JSON output.
metadata:
  opencode:
    skillKey: relay-knowledge-cli
---

# Relay Knowledge CLI

This OpenCode project skill is auto-discovered from `.opencode/skills`, so it
remains available when OpenCode starts from the repository root or a nested
working directory. Before operating `relay-knowledge`, read and follow the
canonical published skill at `../../../skills/relay-knowledge-cli/SKILL.md` and
its deeper workflow recipes at
`../../../skills/relay-knowledge-cli/references/cli-workflows.md`.

Use only a published `relay-knowledge` binary on `PATH`. Do not use
source-checkout build artifacts or source builds as an installation path. If
the binary is missing, install it from a published channel first: prefer a
verified GitHub Release archive, or use `cargo install relay-knowledge` from
crates.io when Cargo is the selected published package channel.

Resolve the executable with the active shell's lookup command before running
workflow commands:

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

For isolated tests, smoke checks, or reproductions, set a temporary
`RELAY_KNOWLEDGE_HOME` and local deterministic retrieval backends, then remove
the temporary home after capturing the result.

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

Use `--format json` for automation, inspect `relay-knowledge help --format
json` before unfamiliar commands, and keep this skill scoped to CLI workflows.
Do not configure MCP, Streamable HTTP, ACP, resources, prompts, sessions, or
protocol tools from this OpenCode skill.

