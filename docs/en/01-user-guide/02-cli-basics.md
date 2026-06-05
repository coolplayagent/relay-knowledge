# Chapter 2: CLI Basics

[English](../../en/01-user-guide/02-cli-basics.md) | [中文](../../zh/01-user-guide/02-cli-basics.md)

This chapter explains shared CLI syntax, outputs, freshness, and parser diagnostics. See [Chapter 3: CLI Command Reference](03-cli-command-reference.md) for the complete command index.

## 2.1 Command Structure

The CLI uses git-style subcommands. Global `--format` and `--remote <base-url>` can be placed before or after the command; command options are still parsed by the selected subcommand:

```bash
relay-knowledge [command] [command options] [--remote <base-url>] [--format text|json|markdown|streaming-json]
```

`--remote` or `RELAY_KNOWLEDGE_REMOTE_BASE_URL` sends supported code repository index, scope preview, status, query, feature-flag, impact, report, and software projection commands to the resident service HTTP API instead of opening local runtime storage. Remote mode does not run `repo index-worker`; index tasks are drained by the bounded worker pool in the remote `service run --web` process. Remote-selected `repo index --reset` and `repo index-worker` commands are rejected so maintenance cannot accidentally clear or drain local state.

Calling the binary without a subcommand is equivalent to `status`. Help examples:

```bash
relay-knowledge --help
relay-knowledge query --help
relay-knowledge repo query --help
relay-knowledge help repo query --format json
```

If query text starts with `-`, separate it with `--`:

```bash
relay-knowledge query -- "--help" --format json
```

## 2.2 Self-Describing Specification

For scripts, skills, and LLM tools, prefer the machine-readable CLI specification:

```bash
relay-knowledge help --format json
relay-knowledge help repo query --format json
```

The JSON specification includes command path, operation, read/write impact, argument semantics, required status, defaults, allowed values, repeatability, examples, and notes. Any new or changed CLI argument must update this self-description.

CLI input is parsed against this specification into an internal syntax tree before it maps to runtime commands. When parsing fails, automated callers should read `matched_path`, `expected`, `usage`, and `suggestion` from the diagnostic instead of guessing argument meaning.

## 2.3 Output Formats

Four output formats are supported:

- `text`: short terminal-oriented summary, used by default.
- `json`: single-line JSON for scripts, tests, and tools.
- `markdown`: human-readable Markdown, mainly for `repo report` and version output.
- `streaming-json`: `started`, `item`, `completed`, and related events for long operations and future streaming UI paths.

Examples:

```bash
relay-knowledge status --format text
relay-knowledge status --format json
relay-knowledge repo report core --format markdown
relay-knowledge status --format streaming-json
```

`version` and `--version` support `text` and `json`, but not `streaming-json`:

```bash
relay-knowledge version
relay-knowledge --version --format json
```

## 2.4 Freshness Policy

Query commands can use `--freshness` to control derived index freshness:

- `allow-stale`: allow older index results and mark stale or degraded metadata.
- `wait-until-fresh`: try to refresh lagging indexes before the query; return an error or degraded state when freshness cannot be met.
- `graph-only`: bypass BM25, semantic, and vector indexes, and read only graph facts.

General knowledge retrieval uses the current implementation default. Code repository queries default to `allow-stale`; pass `wait-until-fresh` when callers require the latest graph state.

## 2.5 Argument Boundaries

`--limit` must be a positive integer and is still capped by API-layer validation. `0` is rejected by retrieval, repository, audit, and proposal requests.

`--kind` has command-specific meanings:

- `index refresh`: `bm25`, `semantic`, `vector`.
- `worker`: `embedding`, `ocr`, `vision`, `extractor`.
- `repo query`: `hybrid`, `symbol`, `definition`, `references`, `callers`, `callees`, `imports`, `sbom`.
- `repo software`: `dependencies`, `sdks`, `files`, `topics`, `relationships`, `build`, `iac`, `design`, `all`.

When query text or a reason contains words beginning with `-`, use `--` or quoting so they are not parsed as options.

## 2.6 Syntax Diagnostics

CLI parse errors return the closest syntax-tree context. In text mode, errors are written to stderr and try to include `Try:` and `Usage:`.

Unknown command example:

```bash
relay-knowledge repo qurey core --query rust
```

The CLI reports unknown command `repo qurey` and suggests `repo query`.

Positional argument example:

```bash
relay-knowledge query --query SQLite
```

The CLI reports that `query` text is positional and suggests:

```bash
relay-knowledge query SQLite
```

When the request includes `--format json` and parsing fails, stderr contains a single-line JSON diagnostic:

```json
{"error":"...","matched_path":["query"],"unexpected_token":"--query","expected":["<text>","--source","--limit","--freshness"],"suggestion":"relay-knowledge query SQLite","usage":"relay-knowledge query <text> [--source <scope>] [--limit <n>] [--freshness <policy>]"}
```

That JSON describes only the parse error. Successful business responses are still written to stdout.
