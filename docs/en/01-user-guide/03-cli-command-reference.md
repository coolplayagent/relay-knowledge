# Chapter 3: CLI Command Reference

[English](../../en/01-user-guide/03-cli-command-reference.md) | [中文](../../zh/01-user-guide/03-cli-command-reference.md)

This chapter is an executable command index. Workflow details live in later chapters; use this page to find entry points and diagnostics quickly.

## 3.1 Common Status Commands

Project status:

```bash
relay-knowledge status --format json
```

Health check:

```bash
relay-knowledge health --format json
```

Service diagnostics:

```bash
relay-knowledge service status --format json
relay-knowledge service doctor --format json
```

`service status` and `service doctor` currently share the same unified API output, covering service mode, background update state, service definition path, agent protocol status, and refresh queue diagnostics.

## 3.2 Provider Diagnostics

```bash
relay-knowledge provider probe --format json
```

`provider probe` reads remote embedding provider configuration through the environment boundary and performs a lightweight probe. The JSON response includes `ok`, `provider`, `model`, `dimension`, optional `latency_ms`, and on failure `error_code`, `error_message`, and `retryable`. It does not print raw API keys or bypass the `env` module.

The OpenAI-compatible embedding base URL may be a host root, a versioned API root such as `/v1` or `/v4`, or a full `/embeddings` endpoint. Non-version path prefixes keep resolving as `<prefix>/v1/embeddings`, and query or fragment suffixes are ignored during endpoint construction.

Endpoint host, batch, timeout, concurrency, and cursor metadata belong to runtime diagnostics in `status`, `health`, or the Web Providers panel.

## 3.3 Setup Doctor and Profiles

`setup doctor` is a storage-free read-only diagnostic:

```bash
relay-knowledge setup doctor --format json
```

It reads only parsed runtime configuration. It does not open or migrate SQLite and does not refresh indexes. `configuration_ready=true` only means configuration checks passed; `live_health_checked=false` means graph storage, index freshness, and worker/service live health still need `health` or `service doctor`.

`setup profile` writes no files and installs no service. It prints recommended environment variables, commands, and notes:

```bash
relay-knowledge setup profile local --format json
relay-knowledge setup profile agent-readonly --format json
relay-knowledge setup profile service --format json
relay-knowledge setup profile external-embedding --format json
```

The profiles cover zero-config local use, read-only MCP agent access, platform service-manager preview, and external embedding provider metadata. Persisting those suggestions into a shell, service manager, or deployment tool is always explicit caller work.

## 3.4 Command Overview

```bash
relay-knowledge status
relay-knowledge help [command...] [--format text|json]
relay-knowledge ingest --source <scope> --content <text> [--entity <label>]
relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness allow-stale|wait-until-fresh|graph-only]
relay-knowledge repo register <path> --alias <name> [--path <filter>] [--language <id>]
relay-knowledge repo index <alias> [--ref <ref>] [--dry-run]
relay-knowledge repo scope preview <alias> [--ref <ref>]
relay-knowledge repo update <alias> --base <ref> --head <ref>
relay-knowledge repo query <alias> --query <text> [--kind hybrid|symbol|definition|references|callers|callees|imports]
relay-knowledge repo impact <alias> --base <ref> --head <ref>
relay-knowledge repo report <alias> [--format markdown|json]
relay-knowledge repo status <alias>
relay-knowledge graph inspect
relay-knowledge index refresh [--kind bm25|semantic|vector]
relay-knowledge worker status|run-once [--kind embedding|ocr|vision|extractor]
relay-knowledge proposal list [--state proposed|accepted|rejected|superseded] [--limit <n>]
relay-knowledge proposal show <proposal-id>
relay-knowledge proposal accept|reject|supersede <proposal-id> --by <actor> [--reason <text>]
relay-knowledge audit query [--operation <name>] [--limit <n>]
relay-knowledge provider probe
relay-knowledge health
relay-knowledge service status
relay-knowledge service doctor
relay-knowledge service plan install|uninstall
relay-knowledge service definition write
relay-knowledge service operator status|pause|resume
relay-knowledge service run [--web] [--mcp streamable-http]
relay-knowledge setup doctor
relay-knowledge setup profile local|agent-readonly|service|external-embedding
relay-knowledge version
```

## 3.5 Read and Write Impact

Status, health, help, setup doctor/profile, provider probe, report, and audit query are diagnostic entry points and should not mutate graph facts. `ingest`, `repo index`, `repo update`, `index refresh`, `worker run-once`, proposal state changes, and service definition write can write runtime state, derived indexes, proposals/audit, or service definitions.

Automated callers should read operation and read/write metadata from `help --format json` before exposing a command in CI, agents, or the Web operation surface.
