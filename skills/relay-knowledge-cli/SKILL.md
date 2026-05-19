---
name: relay-knowledge-cli
description: Use relay-knowledge through its local CLI for knowledge graph ingestion, hybrid GraphRAG queries, code repository registration, indexing, code graph search, impact analysis, setup diagnostics, installation checks, and upgrade checks. Use when an agent should operate relay-knowledge by running CLI commands and parsing JSON output. Do not use this skill for MCP server setup, MCP tools, ACP adapters, or protocol-level agent access.
metadata:
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

Do not start or configure MCP from this skill. If a task asks for MCP,
Streamable HTTP, resources, prompts, sessions, or protocol tools, use the
project MCP documentation or a separate MCP skill instead.

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

If a command fails, read its JSON error and avoid guessing hidden state. Run
diagnostics in this order:

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
